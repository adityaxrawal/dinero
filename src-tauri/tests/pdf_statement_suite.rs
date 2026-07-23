//! Doc 30 TASK-QA-003: PDF Statement Extraction Accuracy Suite.
//!
//! Drives the real two-phase statement pipeline (`stage_parse_pipeline` /
//! `commit_staged_draft`, Doc 15 §14a) against a labeled synthetic corpus
//! (`tests/fixtures/statements/`, generated with known ground truth so a
//! row-level exact-match evaluator has something authoritative to compare
//! against) covering a plain PDF, a password-protected PDF, a corrupted
//! file, and an empty file -- through the real `pdfium-render` sidecar
//! process, not a mocked parser.

use dinero_app_lib::commands::{
    commit_staged_draft, stage_parse_pipeline, ConfirmedInstrument, DraftMetadataUpdate,
    PipelineOutcome,
};
use dinero_app_lib::db;
use dinero_app_lib::db::statement_drafts;
use dinero_app_lib::reconciliation::audit::DecisionType;
use dinero_app_lib::reconciliation::engine::{reconcile_transactionally, IncomingObservation};
use dinero_app_lib::statements::metadata_extractor::resolve_or_create_instrument;
use dinero_app_lib::statements::row_extractor::StatementRow;
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::{AppHandle, Manager};

fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/statements").join(name)
}

fn mock_app() -> AppHandle<tauri::test::MockRuntime> {
    mock_builder()
        .build(mock_context(noop_assets()))
        .unwrap()
        .handle()
        .clone()
}

async fn migrated_pool(label: &str) -> deadpool_sqlite::Pool {
    let dir = std::env::temp_dir().join(format!(
        "dinero_pdf_statement_suite_{label}_{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    db::init_db(dir.join("test.db")).await.expect("DB init failed")
}

/// Ground truth for `tests/fixtures/statements/hdfc_plain.pdf` /
/// `hdfc_password_protected.pdf` -- mirrors the `ROWS` list in the fixture
/// generator script exactly (`(date DD/MM/YYYY, merchant, amount_minor, direction)`).
/// "SALARY CREDIT FROM EMPLOYER" (not the more obvious "payment received
/// thank you" wording real HDFC statements use) deliberately avoids
/// `row_extractor::is_excluded_row`'s "payment received"/"payment thank"
/// boilerplate filter -- a real accuracy corpus needs at least one
/// legitimate CR row that isn't itself a payment-received line.
fn ground_truth_rows() -> Vec<(&'static str, &'static str, i64, &'static str)> {
    vec![
        ("2025-12-01", "AMAZON RETAIL INDIA PVT LTD", 123450, "debit"),
        ("2025-12-02", "SWIGGY BANGALORE", 45000, "debit"),
        ("2025-12-03", "UBER INDIA SYSTEMS", 32075, "debit"),
        ("2025-12-04", "NETFLIX ENTERTAINMENT", 64900, "debit"),
        ("2025-12-05", "BIG BAZAAR RETAIL", 289900, "debit"),
        ("2025-12-06", "IRCTC RAIL TICKET", 156000, "debit"),
        ("2025-12-07", "SALARY CREDIT FROM EMPLOYER", 500000, "credit"),
        ("2025-12-08", "STARBUCKS COFFEE INDIA", 45000, "debit"),
        ("2025-12-09", "APOLLO PHARMACY", 89000, "debit"),
        ("2025-12-10", "REFUND FLIPKART INTERNET", 123450, "credit"),
        ("2025-12-11", "ZOMATO ONLINE ORDERING", 67800, "debit"),
        ("2025-12-12", "BOOKMYSHOW ENTERTAINMENT", 100000, "debit"),
        ("2025-12-13", "RELIANCE FRESH SUPERMARKET", 345600, "debit"),
        ("2025-12-14", "SPOTIFY INDIA MUSIC", 11900, "debit"),
        ("2025-12-15", "MAKEMYTRIP FLIGHT BOOKING", 890000, "debit"),
        ("2025-12-16", "PAYTM WALLET TOPUP", 200000, "debit"),
        ("2025-12-17", "URBAN COMPANY SERVICES", 78000, "debit"),
        ("2025-12-18", "AMAZON RETAIL INDIA PVT LTD", 234500, "debit"),
        ("2025-12-19", "CASHBACK REWARD CREDIT", 5000, "credit"),
        ("2025-12-20", "OLA CABS BANGALORE", 45600, "debit"),
    ]
}

/// Row-level exact match: every field (date, merchant, amount, direction)
/// must match one extracted row for that ground-truth row to count.
fn row_level_accuracy(rows: &[StatementRow], truth: &[(&str, &str, i64, &str)]) -> f64 {
    let matched = truth
        .iter()
        .filter(|(date, merchant, amount_minor, direction)| {
            rows.iter().any(|r| {
                r.transaction_date == *date
                    && r.merchant_raw == *merchant
                    && r.amount_minor == *amount_minor
                    && r.direction.eq_ignore_ascii_case(direction)
            })
        })
        .count();
    matched as f64 / truth.len() as f64
}

async fn draft_rows(pool: &deadpool_sqlite::Pool, draft_id: &str) -> (statement_drafts::StatementDraftRow, Vec<StatementRow>) {
    let conn = pool.get().await.unwrap();
    let id = draft_id.to_string();
    let draft = conn
        .interact(move |c| statement_drafts::select_by_id(c, &id))
        .await
        .unwrap()
        .unwrap()
        .expect("draft must exist after staging");
    let rows: Vec<StatementRow> = serde_json::from_str(&draft.rows_json).unwrap();
    (draft, rows)
}

/// Doc 30 TASK-QA-003 acceptance: `test_statement_row_level_exact_match_accuracy_target`.
#[tokio::test]
async fn test_statement_row_level_exact_match_accuracy_target() {
    let pool = migrated_pool("accuracy").await;
    let app = mock_app();
    let bytes = std::fs::read(fixture("hdfc_plain.pdf")).unwrap();

    let outcome = stage_parse_pipeline(
        &bytes,
        "hdfc_plain.pdf",
        "hash_accuracy",
        &pool,
        &app,
        None,
        None,
        "manual_upload",
        None,
    )
    .await
    .expect("stage_parse_pipeline failed on a well-formed statement");

    let draft_id = match outcome {
        PipelineOutcome::Staged(id) => id,
        other => panic!("expected Staged, got {other:?}"),
    };
    let (_, rows) = draft_rows(&pool, &draft_id).await;

    let truth = ground_truth_rows();
    let accuracy = row_level_accuracy(&rows, &truth);
    const TARGET: f64 = 0.90;
    assert!(
        accuracy >= TARGET,
        "row-level exact-match accuracy {:.1}% is below the {:.0}% target (extracted {} rows, expected {}): {:?}",
        accuracy * 100.0,
        TARGET * 100.0,
        rows.len(),
        truth.len(),
        rows
    );

    // Doc 30 TASK-QA-003's "intentionally corrupted/empty documents" case:
    // the pipeline must fail gracefully (an Err), never panic or silently
    // fabricate rows from garbage/empty input.
    for bad in ["corrupted.pdf", "empty.pdf"] {
        let bad_bytes = std::fs::read(fixture(bad)).unwrap();
        let result = stage_parse_pipeline(
            &bad_bytes,
            bad,
            &format!("hash_{bad}"),
            &pool,
            &app,
            None,
            None,
            "manual_upload",
            None,
        )
        .await;
        assert!(result.is_err(), "{bad} must fail to parse, not silently succeed");
    }
}

/// Doc 30 TASK-QA-003 acceptance: `test_password_protected_statement_flow`.
#[tokio::test]
async fn test_password_protected_statement_flow() {
    let pool = migrated_pool("password").await;
    let app = mock_app();
    let bytes = std::fs::read(fixture("hdfc_password_protected.pdf")).unwrap();

    // Without the password: must fail (never silently return zero rows as
    // if the statement were genuinely empty).
    let without_password = stage_parse_pipeline(
        &bytes,
        "hdfc_password_protected.pdf",
        "hash_nopw",
        &pool,
        &app,
        None,
        None,
        "manual_upload",
        None,
    )
    .await;
    assert!(
        without_password.is_err(),
        "a password-protected PDF must not parse without the password"
    );

    // With the correct password: succeeds, and extraction accuracy holds
    // exactly as it does for the unencrypted PDF -- the password/decrypt
    // step must not itself corrupt or drop row text.
    let outcome = stage_parse_pipeline(
        &bytes,
        "hdfc_password_protected.pdf",
        "hash_withpw",
        &pool,
        &app,
        None,
        Some("TEST1234"),
        "manual_upload",
        None,
    )
    .await
    .expect("stage_parse_pipeline with the correct password must succeed");

    let draft_id = match outcome {
        PipelineOutcome::Staged(id) => id,
        other => panic!("expected Staged, got {other:?}"),
    };
    let (_, rows) = draft_rows(&pool, &draft_id).await;
    let truth = ground_truth_rows();
    let accuracy = row_level_accuracy(&rows, &truth);
    assert!(
        accuracy >= 0.90,
        "password-unlocked statement accuracy {:.1}% below 90% target: {:?}",
        accuracy * 100.0,
        rows
    );
}

/// Doc 30 TASK-QA-003 acceptance: `test_statement_overrides_email_where_both_exist`.
///
/// Same precedence rule `reconciliation_regression.rs`'s
/// `test_regression_email_then_statement_overrides` proves against a
/// hand-built `IncomingObservation` -- this test proves it holds when the
/// statement side arrives through the real PDF pipeline instead.
#[tokio::test]
async fn test_statement_overrides_email_where_both_exist() {
    let pool = migrated_pool("override").await;
    let app = mock_app();

    // Resolve the instrument the statement will also resolve to (via
    // `ConfirmedInstrument` below) so both sides target one real-world
    // card by construction. Note: the email/transaction-ladder path keys
    // masked_identifier as "XXXX4321" (see `extraction::ladder::
    // extract_instrument_signals`) while the statement path here uses the
    // bare "4321" captured by `metadata_extractor`'s regex -- a real
    // formatting inconsistency between the two ingestion paths, out of
    // scope for this QA task to fix (flagged in this task's fix-log entry
    // for TASK-STMT-004/TXN-007 to reconcile); forcing both sides onto the
    // same resolved id here isolates this test to the precedence rule only.
    let instrument_id = resolve_or_create_instrument("credit_card", "HDFC", "4321", None, &pool)
        .await
        .expect("resolve_or_create_instrument failed");

    let conn = pool.get().await.unwrap();
    let email_obs = IncomingObservation {
        id: "obs_email_amazon".to_string(),
        instrument_id: instrument_id.clone(),
        amount_minor: 123450,
        currency: "INR".to_string(),
        direction: "debit".to_string(),
        event_time: "2025-12-01 00:00:00".to_string(),
        reference_id: None,
        merchant_raw: Some("Amazon Email Alert".to_string()),
        source_pipeline: "gmail_transaction".to_string(),
        source_record_id: "rec_email_amazon".to_string(),
        emi_total_installments: None,
        emi_original_amount_minor: None,
        fingerprint: None,
        confidence_score: None,
        event_time_confidence: None,
    };
    conn.interact(|c| {
        c.execute(
            "INSERT INTO transaction_observations (id, source_pipeline, source_record_id, fingerprint) \
             VALUES ('obs_email_amazon', 'gmail_transaction', 'rec_email_amazon', NULL)",
            [],
        )
    })
    .await
    .unwrap()
    .unwrap();

    let decision = conn
        .interact({
            let obs = email_obs.clone();
            move |c| reconcile_transactionally(c, &obs)
        })
        .await
        .unwrap()
        .expect("reconcile_transactionally failed for the email observation");
    assert_eq!(decision, DecisionType::NewCanonical);

    let canonical_id: String = conn
        .interact(|c| {
            c.query_row(
                "SELECT canonical_transaction_id FROM transaction_observations WHERE id = 'obs_email_amazon'",
                [],
                |r| r.get(0),
            )
        })
        .await
        .unwrap()
        .unwrap();

    // Now commit the real statement -- row 0 (2025-12-01, AMAZON RETAIL
    // INDIA PVT LTD, 1234.50 debit) describes the exact same real-world
    // event as the email observation above.
    let bytes = std::fs::read(fixture("hdfc_plain.pdf")).unwrap();
    let confirmed = ConfirmedInstrument {
        issuer_name: "HDFC".to_string(),
        masked_identifier: "4321".to_string(),
        instrument_type: "credit_card".to_string(),
    };
    let outcome = stage_parse_pipeline(
        &bytes,
        "hdfc_plain.pdf",
        "hash_override",
        &pool,
        &app,
        Some(confirmed),
        None,
        "manual_upload",
        None,
    )
    .await
    .expect("stage_parse_pipeline failed");
    let draft_id = match outcome {
        PipelineOutcome::Staged(id) => id,
        other => panic!("expected Staged, got {other:?}"),
    };
    let (draft, rows) = draft_rows(&pool, &draft_id).await;

    let edited_metadata = DraftMetadataUpdate {
        issuer_name: draft.issuer_name.clone().unwrap(),
        masked_identifier: draft.masked_identifier.clone().unwrap(),
        instrument_type: draft.instrument_type.clone().unwrap(),
        billing_period_start: draft.billing_period_start.clone(),
        billing_period_end: draft.billing_period_end.clone(),
        due_date: draft.due_date.clone(),
        statement_date: draft.statement_date.clone(),
        current_balance: draft.current_balance,
        minimum_due: draft.minimum_due,
    };
    let statement_row_count = rows.len() as i64;
    commit_staged_draft(&draft_id, edited_metadata, rows, &pool, &app)
        .await
        .expect("commit_staged_draft failed");

    let (count, merchant, source_mix): (i64, String, String) = conn
        .interact(move |c| {
            let count: i64 = c.query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))?;
            let (m, sm): (String, String) = c.query_row(
                "SELECT merchant_display_name, source_mix FROM transactions WHERE id = ?1",
                rusqlite::params![canonical_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            Ok::<_, rusqlite::Error>((count, m, sm))
        })
        .await
        .unwrap()
        .unwrap();

    // 1 pre-existing (email) canonical + one new canonical per *non-matching*
    // statement row. Row 0 (the Amazon row) describes the same real-world
    // event as the email observation and must merge into it rather than
    // creating its own 21st row -- so the total must be exactly
    // `statement_row_count` (1 fewer than `1 + statement_row_count`), not
    // one more for a duplicate.
    assert_eq!(
        count, statement_row_count,
        "the statement row for the same real-world event must match the existing canonical, not create a second one"
    );
    assert_eq!(
        merchant, "AMAZON RETAIL INDIA PVT LTD",
        "statement data must override the email-sourced merchant on the same event"
    );
    assert_eq!(source_mix, "merged");
}

/// Doc 30 TASK-QA-003 acceptance: `test_raw_pdf_not_persisted_after_parse`.
///
/// Doc 15 §14a (v1.3) updated the "never persisted" invariant to "never
/// persisted *unencrypted*" -- the PDF is now intentionally retained,
/// AES-256-GCM-encrypted, through review and a 30-day post-commit window.
/// This test asserts the stronger claim that still holds: no raw/plaintext
/// PDF bytes ever land in the DB, and if a file is written to disk at all,
/// it is genuinely ciphertext, never a plaintext copy.
#[tokio::test]
async fn test_raw_pdf_not_persisted_after_parse() {
    let pool = migrated_pool("pdf_persist").await;
    let app = mock_app();
    let bytes = std::fs::read(fixture("hdfc_plain.pdf")).unwrap();

    let outcome = stage_parse_pipeline(
        &bytes,
        "hdfc_plain.pdf",
        "hash_persist",
        &pool,
        &app,
        None,
        None,
        "manual_upload",
        None,
    )
    .await
    .expect("stage_parse_pipeline failed");
    let draft_id = match outcome {
        PipelineOutcome::Staged(id) => id,
        other => panic!("expected Staged, got {other:?}"),
    };

    let (draft, _rows) = draft_rows(&pool, &draft_id).await;
    assert!(
        !draft.rows_json.contains("%PDF"),
        "rows_json must contain only extracted StatementRow data, never embedded raw PDF bytes"
    );

    if let Ok(app_data_dir) = app.path().app_data_dir() {
        let path = app_data_dir.join("statements").join(format!("{draft_id}.pdf.enc"));
        if path.exists() {
            let on_disk = std::fs::read(&path).unwrap();
            assert_ne!(
                on_disk, bytes,
                "a retained PDF copy must be encrypted, never a byte-for-byte plaintext copy"
            );
            assert!(
                !on_disk.windows(5).any(|w| w == b"%PDF-"),
                "encrypted-at-rest storage must not leave a readable PDF header in the stored bytes"
            );
        }
    }
}
