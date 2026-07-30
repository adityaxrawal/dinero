//! Doc 30 TASK-QA-001: Build Zero-False-Positive Canonical Transaction Test Corpus.
//!
//! Drives the real Gate 2 (`ContentClassifier::classify`), full extraction
//! ladder (`run_extraction_ladder`), Gate 3 (mandatory-field check),
//! instrument resolution, fingerprinting, and reconciliation
//! (`reconcile_transactionally`) -- the same functions
//! `ingestion::message_processor::MessageProcessor::process_message` and
//! `ingestion::queues::process_transaction_job` call in production -- against
//! a labeled gold-standard corpus (`tests/fixtures/transaction_goldens.jsonl`)
//! covering the major Indian bank/fintech transaction patterns, plus the
//! negative examples that must never become canonical (Document 01 PG-02's
//! false-positive success metric).
//!
//! `MessageProcessor::process_message` itself is not called directly: it
//! needs a live `GmailClient`/`tauri::AppHandle`, and its own gate helpers
//! are `pub(crate)` (verified against `evaluate_mandatory_field_gate`'s
//! visibility), unreachable from this external integration-test binary.
//! Gate 2/3 and everything downstream are exercised via the same real, fully
//! `pub` functions that code path calls internally -- only Gate 3's
//! one-line predicate is duplicated verbatim below, since it is itself the
//! acceptance criterion Doc 30 TASK-GMAIL-006 defines.

use dinero_app_lib::db;
use dinero_app_lib::db::instruments::get_or_create_instrument;
use dinero_app_lib::db::transaction_observations::{
    insert_observation_idempotent, InsertObservationOutcome,
};
use dinero_app_lib::extraction::fingerprint::compute_fingerprint;
use dinero_app_lib::extraction::ladder::{run_extraction_ladder, ExtractionResult};
use dinero_app_lib::extraction::normalization::normalize_observation;
use dinero_app_lib::ingestion::content_classifier::{ContentClass, ContentClassifier};
use dinero_app_lib::ingestion::verified_senders::{SenderValidator, SenderVerificationResult};
use dinero_app_lib::reconciliation::audit::DecisionType;
use dinero_app_lib::reconciliation::engine::{reconcile_transactionally, IncomingObservation};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
struct GoldenRecord {
    id: String,
    category: String,
    is_positive: bool,
    #[serde(default)]
    bank_name: Option<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    sender_email: Option<String>,
    #[serde(default)]
    sender_display_name: Option<String>,
    #[serde(default)]
    expect_amount_minor: Option<i64>,
    #[serde(default)]
    expect_direction: Option<String>,
}

const REQUIRED_POSITIVE_CATEGORIES: &[&str] = &[
    "credit_card",
    "debit_card",
    "upi",
    "imps",
    "neft",
    "rtgs",
    "wallet",
    "pos",
    "atm",
    "refund",
    "reversal",
    "fee",
    "declined",
    "foreign_currency",
];

const REQUIRED_NEGATIVE_CATEGORIES: &[&str] = &[
    "otp",
    "kyc",
    "marketing",
    "reminder",
    "statement_like",
    "spoofed_domain",
    "missing_amount",
    "missing_counterparty",
];

fn load_corpus() -> Vec<GoldenRecord> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/transaction_goldens.jsonl");
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("bad corpus line: {l} ({e})")))
        .collect()
}

async fn migrated_pool(label: &str) -> deadpool_sqlite::Pool {
    let dir = std::env::temp_dir().join(format!(
        "dinero_false_positive_suite_{label}_{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("test.db");
    db::init_db(db_path.clone()).await.expect("DB init failed")
}

fn transactions_row_count(conn: &mut rusqlite::Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap()
}

/// Mirrors `MessageProcessor::process_message`'s Gate 2 match arms exactly:
/// every one of these classes returns `Ok(None)` (or, for `StatementEmail`,
/// routes to the Statement Queue) before the extraction ladder ever runs.
fn is_non_transaction_class(class: &ContentClass) -> bool {
    matches!(
        class,
        ContentClass::Otp
            | ContentClass::Kyc
            | ContentClass::Marketing
            | ContentClass::Reminder
            | ContentClass::Noise
            | ContentClass::Unknown
            | ContentClass::StatementEmail
            | ContentClass::MandateRegistration
            | ContentClass::MandateCancellation
    )
}

/// Doc 30 TASK-GMAIL-006's Gate 3 predicate, verbatim. The real
/// implementation (`MessageProcessor::evaluate_mandatory_field_gate`) is
/// `pub(crate)` and so unreachable from this external `tests/` binary --
/// duplicated here only as the exact 1-line boolean the doc itself defines
/// as the acceptance criterion, not as independently-invented logic.
fn passes_mandatory_field_gate(obs: &ExtractionResult) -> bool {
    (obs.amount_minor.is_some() && obs.merchant_raw.is_some()) || obs.balance_after.is_some()
}

#[derive(Debug)]
enum PipelineOutcome {
    /// Rejected before an observation was ever persisted; `reason` is a short tag.
    Rejected(&'static str),
    Reconciled {
        decision: DecisionType,
        amount_minor: i64,
        direction: String,
    },
}

/// Drives one corpus record through the real pipeline pieces described in
/// this file's module doc comment.
async fn process_record(pool: &deadpool_sqlite::Pool, rec: &GoldenRecord) -> PipelineOutcome {
    // Spoofed-domain negatives are a Gate 1 (sender verification) concern --
    // never reach content classification or the ladder at all.
    if rec.category == "spoofed_domain" {
        let validator = SenderValidator::new();
        let result = validator.verify_sender(
            rec.sender_email
                .as_deref()
                .unwrap_or_else(|| panic!("{}: spoofed_domain record needs sender_email", rec.id)),
            rec.sender_display_name.as_deref(),
        );
        return match result {
            SenderVerificationResult::UnverifiedReject(_) | SenderVerificationResult::SpoofReject(_) => {
                PipelineOutcome::Rejected("gate1_reject")
            }
            other => panic!("{}: expected Gate 1 to reject, got {:?}", rec.id, other),
        };
    }

    let subject = rec.subject.as_deref().unwrap_or_default();
    let body = rec.body.as_deref().unwrap_or_default();
    let bank_name = rec.bank_name.as_deref().unwrap_or("Unknown Bank");

    // GATE 2: Content Classification.
    let class = ContentClassifier::classify(subject, body);
    if is_non_transaction_class(&class) {
        return PipelineOutcome::Rejected("gate2_reject");
    }

    // Full extraction ladder (Layers 1-4).
    let mut layer6_timed_out = false;
    let extracted = run_extraction_ladder(pool, bank_name, body, None, false, None, &mut layer6_timed_out, None)
        .await
        .expect("run_extraction_ladder returned an Err, not a rejection");
    let Some(obs) = extracted else {
        return PipelineOutcome::Rejected("extraction_failed");
    };

    // GATE 3: Mandatory Field Gate.
    if !passes_mandatory_field_gate(&obs) {
        return PipelineOutcome::Rejected("gate3_failed");
    }

    let amount_minor = obs
        .amount_minor
        .expect("gate 3 already checked amount_minor is Some");
    let direction = obs.direction.clone().unwrap_or_else(|| "debit".to_string());
    let instrument_type = obs.instrument_type.clone();
    let issuer_name = obs.issuer_name.clone();
    let masked_identifier = obs.masked_identifier.clone();
    let network = obs.network.clone();

    let source_record_id = format!("rec_{}", rec.id);
    let mut row = normalize_observation(obs, "gmail_transaction", &source_record_id, Some(body), None);

    let connected_account_id = "acct_test".to_string();
    let conn = pool.get().await.expect("pool.get");
    let decision = conn
        .interact(move |c| -> anyhow::Result<DecisionType> {
            if let (Some(itype), Some(iname), Some(masked)) = (
                instrument_type.as_deref(),
                issuer_name.as_deref(),
                masked_identifier.as_deref(),
            ) {
                let instr_id = get_or_create_instrument(c, itype, iname, masked, network.as_deref())?;
                row.instrument_id = Some(instr_id);
            }

            if let (Some(instrument_id), Some(dir), Some(amt)) =
                (row.instrument_id.clone(), row.direction.clone(), row.amount_minor)
            {
                let event_bucket = row
                    .event_time
                    .map(|dt| dt.format("%Y-%m-%dT%H:%M").to_string())
                    .unwrap_or_default();
                row.fingerprint = Some(compute_fingerprint(
                    &instrument_id,
                    &dir,
                    amt,
                    &event_bucket,
                    &connected_account_id,
                ));
            }

            match insert_observation_idempotent(c, &row)? {
                InsertObservationOutcome::DuplicateSkipped => {
                    anyhow::bail!("unexpected duplicate for a freshly-generated observation id")
                }
                InsertObservationOutcome::Inserted => {}
            }

            let incoming = IncomingObservation {
                id: row.id.clone(),
                instrument_id: row.instrument_id.clone().unwrap_or_else(|| "unknown".to_string()),
                amount_minor: row.amount_minor.unwrap_or(0),
                currency: row.currency.clone().unwrap_or_else(|| "INR".to_string()),
                direction: row.direction.clone().unwrap_or_else(|| "debit".to_string()),
                event_time: row
                    .event_time
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_default(),
                reference_id: row.reference_id.clone(),
                merchant_raw: row.merchant_raw.clone(),
                source_pipeline: row.source_pipeline.clone().unwrap_or_else(|| "unknown".to_string()),
                source_record_id: row.source_record_id.clone().unwrap_or_default(),
                emi_total_installments: row.emi_total_installments,
                emi_original_amount_minor: row.emi_original_amount_minor,
                fingerprint: row.fingerprint.clone(),
                confidence_score: row.confidence_score,
                event_time_confidence: row.event_time_confidence.clone(),
            };

            reconcile_transactionally(c, &incoming)
        })
        .await
        .expect("interact")
        .expect("reconcile_transactionally failed");

    PipelineOutcome::Reconciled {
        decision,
        amount_minor,
        direction,
    }
}

/// Doc 30 TASK-QA-001 acceptance: `test_corpus_contains_positive_and_negative_examples`.
#[test]
fn test_corpus_contains_positive_and_negative_examples() {
    let corpus = load_corpus();

    let positives: Vec<&GoldenRecord> = corpus.iter().filter(|r| r.is_positive).collect();
    let negatives: Vec<&GoldenRecord> = corpus.iter().filter(|r| !r.is_positive).collect();

    assert!(!positives.is_empty(), "corpus must contain positive examples");
    assert!(!negatives.is_empty(), "corpus must contain negative examples");

    for category in REQUIRED_POSITIVE_CATEGORIES {
        assert!(
            positives.iter().any(|r| r.category == *category),
            "corpus missing a positive example for category {category:?}"
        );
    }
    for category in REQUIRED_NEGATIVE_CATEGORIES {
        assert!(
            negatives.iter().any(|r| r.category == *category),
            "corpus missing a negative example for category {category:?}"
        );
    }

    // Every record must be well-formed enough to actually drive the pipeline.
    for rec in &corpus {
        if rec.category == "spoofed_domain" {
            assert!(rec.sender_email.is_some(), "{}: missing sender_email", rec.id);
        } else {
            assert!(rec.body.is_some(), "{}: missing body", rec.id);
        }
    }
}

/// Doc 30 TASK-QA-001 acceptance: `test_zero_false_positive_integration_suite`.
///
/// Runs the full ladder against every corpus record and asserts
/// `false_positive_count == 0`: no negative example may ever produce a
/// canonical transaction. Also asserts every positive example DOES produce
/// one with the expected amount/direction, so the suite can't trivially pass
/// by having a ladder that rejects everything.
#[tokio::test]
async fn test_zero_false_positive_integration_suite() {
    let pool = migrated_pool("zero_fp").await;
    let corpus = load_corpus();

    let mut false_positive_count = 0usize;
    let mut positive_success_count = 0usize;

    for rec in &corpus {
        let outcome = process_record(&pool, rec).await;
        match (rec.is_positive, &outcome) {
            (true, PipelineOutcome::Reconciled { amount_minor, direction, .. }) => {
                positive_success_count += 1;
                if let Some(expected) = rec.expect_amount_minor {
                    assert_eq!(
                        *amount_minor, expected,
                        "{}: expected amount_minor {expected}, got {amount_minor}",
                        rec.id
                    );
                }
                if let Some(expected_dir) = &rec.expect_direction {
                    assert_eq!(
                        direction, expected_dir,
                        "{}: expected direction {expected_dir:?}, got {direction:?}",
                        rec.id
                    );
                }
            }
            (true, PipelineOutcome::Rejected(reason)) => {
                panic!("{}: positive example was rejected ({reason})", rec.id);
            }
            (false, PipelineOutcome::Reconciled { decision, .. }) => {
                false_positive_count += 1;
                eprintln!(
                    "FALSE POSITIVE: {} ({:?}) reached canonical creation: {:?}",
                    rec.id, rec.category, decision
                );
            }
            (false, PipelineOutcome::Rejected(_)) => {
                // Correctly rejected -- not a false positive.
            }
        }
    }

    assert_eq!(
        false_positive_count, 0,
        "false_positive_count must be zero (Document 01 PG-02)"
    );
    assert_eq!(
        positive_success_count,
        REQUIRED_POSITIVE_CATEGORIES.len(),
        "every positive example must reach canonical creation"
    );

    // Doc 15 Core Principle 6 / Data Flow Rule 3: exactly one canonical row
    // per distinct real-world positive example -- no ingestion path may
    // create more (a duplicate) or fewer (a silently dropped) rows than that.
    let conn = pool.get().await.expect("pool.get");
    let count = conn.interact(transactions_row_count).await.expect("interact");
    assert_eq!(
        count, positive_success_count as i64,
        "transactions table must contain exactly one row per positive example, no more, no fewer"
    );
}

/// Doc 30 TASK-QA-001 acceptance: `test_no_canonical_row_created_for_rejected_examples`.
///
/// A dedicated, independent pass over only the negative examples, asserting
/// the `transactions` table never gains a single row -- checked after every
/// record, not just once at the end, so an intermediate leak can't hide
/// behind a later rejection.
#[tokio::test]
async fn test_no_canonical_row_created_for_rejected_examples() {
    let pool = migrated_pool("no_canonical_for_rejected").await;
    let corpus = load_corpus();
    let negatives: Vec<&GoldenRecord> = corpus.iter().filter(|r| !r.is_positive).collect();
    assert!(!negatives.is_empty());

    for rec in negatives {
        let outcome = process_record(&pool, rec).await;
        assert!(
            matches!(outcome, PipelineOutcome::Rejected(_)),
            "{}: negative example must be rejected, got {:?}",
            rec.id,
            outcome
        );

        let conn = pool.get().await.expect("pool.get");
        let count = conn.interact(transactions_row_count).await.expect("interact");
        assert_eq!(
            count, 0,
            "{}: no canonical row may ever be created for a rejected example",
            rec.id
        );
    }
}
