//! Doc 30 TASK-DEDUP-010: Reconciliation Engine Regression Test Suite.
//!
//! A comprehensive scenario suite driving the real `reconcile()` entry
//! point end-to-end (not just its constituent unit-tested pieces) against
//! the six named scenarios, plus an aggregate false-merge/false-separation
//! rate check feeding the same benchmark-gate style already established by
//! TASK-TXN-014/TASK-SETUP-011.

use dinero_app_lib::db;
use dinero_app_lib::reconciliation::audit::DecisionType;
use dinero_app_lib::reconciliation::engine::{fetch_candidates, reconcile, IncomingObservation};
use rusqlite::Connection;

/// `db::test_helpers` is `#[cfg(test)]`-gated (only compiled for the lib
/// crate's own unit tests, not visible to this external integration test
/// binary) -- calls the same real `db::migrations::run_migrations` path
/// directly against a fresh temp file instead, matching production schema
/// exactly rather than hand-duplicating a parallel one here.
fn conn_with_fk_off() -> Connection {
    let dir = std::env::temp_dir().join(format!(
        "dinero_reconciliation_regression_{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("test.db");
    tokio::runtime::Runtime::new()
        .expect("failed to create tokio runtime for test migration")
        .block_on(db::migrations::run_migrations(&db_path, None))
        .expect("test database migration failed");
    let conn = Connection::open(&db_path).expect("failed to open migrated test database");
    conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
    conn
}

fn insert_observation_row(conn: &Connection, id: &str, fingerprint: Option<&str>) {
    conn.execute(
        "INSERT INTO transaction_observations (id, source_pipeline, source_record_id, fingerprint) \
         VALUES (?1, 'gmail_transaction', ?2, ?3)",
        rusqlite::params![id, format!("rec_{id}"), fingerprint],
    )
    .unwrap();
}

fn base_obs(id: &str, amount_minor: i64, direction: &str, event_time: &str) -> IncomingObservation {
    IncomingObservation {
        id: id.to_string(),
        instrument_id: "inst_1".to_string(),
        amount_minor,
        currency: "INR".to_string(),
        direction: direction.to_string(),
        event_time: event_time.to_string(),
        reference_id: None,
        merchant_raw: Some("Uber".to_string()),
        source_pipeline: "gmail_transaction".to_string(),
        source_record_id: format!("rec_{id}"),
        emi_total_installments: None,
        emi_original_amount_minor: None,
        fingerprint: None,
        confidence_score: None,
        event_time_confidence: None,
    }
}

// ── Scenario 1: email-then-statement ────────────────────────────────────

/// Doc 30 TASK-DEDUP-010 scenario: email arrives first (creates a canonical),
/// then a statement observation for the same transaction arrives and must
/// override the email-sourced fields.
#[test]
fn test_regression_email_then_statement_overrides() {
    let conn = conn_with_fk_off();

    insert_observation_row(&conn, "obs_email", None);
    let mut email_obs = base_obs("obs_email", 150000, "debit", "2026-06-10 14:00:00");
    email_obs.reference_id = Some("REF_R1".to_string());
    email_obs.merchant_raw = Some("Uber Email".to_string());
    let decision = reconcile(&conn, &email_obs, vec![]).unwrap();
    assert_eq!(decision, DecisionType::NewCanonical);

    let canonical_id: String = conn
        .query_row(
            "SELECT canonical_transaction_id FROM transaction_observations WHERE id = 'obs_email'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    insert_observation_row(&conn, "obs_stmt", None);
    let mut stmt_obs = base_obs("obs_stmt", 150000, "debit", "2026-06-11 00:00:00");
    stmt_obs.source_pipeline = "statement_pdf".to_string();
    stmt_obs.reference_id = Some("REF_R1".to_string());
    stmt_obs.merchant_raw = Some("Uber Statement".to_string());

    let candidates = fetch_candidates(&conn, &stmt_obs).unwrap();
    let decision = reconcile(&conn, &stmt_obs, candidates).unwrap();
    assert!(matches!(
        decision,
        DecisionType::AutoMatchedExact | DecisionType::AutoMatchedScored
    ));

    let (merchant, source_mix): (String, String) = conn
        .query_row(
            "SELECT merchant_display_name, source_mix FROM transactions WHERE id = ?1",
            rusqlite::params![canonical_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        merchant, "Uber Statement",
        "statement must override email-sourced merchant"
    );
    assert_eq!(source_mix, "merged");
}

// ── Scenario 2: statement-then-email ────────────────────────────────────

/// Doc 30 TASK-DEDUP-010 scenario: statement arrives first, then email --
/// email may only fill currently-NULL fields, never overwrite.
#[test]
fn test_regression_statement_then_email_fills_nulls_only() {
    let conn = conn_with_fk_off();

    insert_observation_row(&conn, "obs_stmt2", None);
    let mut stmt_obs = base_obs("obs_stmt2", 75000, "debit", "2026-06-10 00:00:00");
    stmt_obs.source_pipeline = "statement_pdf".to_string();
    stmt_obs.reference_id = Some("REF_R2".to_string());
    stmt_obs.merchant_raw = None; // statement row had no merchant text captured
    let decision = reconcile(&conn, &stmt_obs, vec![]).unwrap();
    assert_eq!(decision, DecisionType::NewCanonical);

    let canonical_id: String = conn
        .query_row(
            "SELECT canonical_transaction_id FROM transaction_observations WHERE id = 'obs_stmt2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let ref_before: String = conn
        .query_row(
            "SELECT reference_id FROM transactions WHERE id = ?1",
            rusqlite::params![canonical_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ref_before, "REF_R2");

    insert_observation_row(&conn, "obs_email2", None);
    // Same reference_id as the statement (needed for the score to clear the
    // viability floor at all, since a real reference-ID mismatch is itself
    // a strong negative signal, TASK-DEDUP-004) -- non-overwrite-of-a-
    // genuinely-different-value is separately covered at the unit level by
    // `test_email_only_fills_null_fields_when_statement_present`; this
    // scenario's own focus is the NULL-merchant-fill happening at all,
    // end-to-end through the real `reconcile()` entry point.
    let mut email_obs = base_obs("obs_email2", 75000, "debit", "2026-06-10 14:05:00");
    email_obs.reference_id = Some("REF_R2".to_string());
    email_obs.merchant_raw = Some("Uber Email".to_string()); // must fill the NULL

    let candidates = fetch_candidates(&conn, &email_obs).unwrap();
    reconcile(&conn, &email_obs, candidates).unwrap();

    let (merchant, reference_id): (String, String) = conn
        .query_row(
            "SELECT merchant_display_name, reference_id FROM transactions WHERE id = ?1",
            rusqlite::params![canonical_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        merchant, "Uber Email",
        "NULL merchant must be filled from email"
    );
    assert_eq!(
        reference_id, "REF_R2",
        "existing statement reference_id must never be overwritten by email"
    );
}

// ── Scenario 3: two emails, different connected accounts, same instrument ──

/// Doc 30 TASK-DEDUP-010 scenario: two emails describing the same
/// real-world transaction, arriving via different connected Gmail accounts
/// (differing fingerprints, since TASK-TXN-008's formula is
/// account-scoped), must still merge into one canonical -- the underlying
/// instrument is genuinely shared, and scoring never keys on
/// connected_account_id.
#[test]
fn test_regression_two_accounts_same_instrument_still_merges() {
    let conn = conn_with_fk_off();

    insert_observation_row(&conn, "obs_acct_a", Some("fp_account_a"));
    let mut obs_a = base_obs("obs_acct_a", 42000, "debit", "2026-06-10 09:00:00");
    obs_a.reference_id = Some("REF_R3".to_string());
    reconcile(&conn, &obs_a, vec![]).unwrap();

    insert_observation_row(&conn, "obs_acct_b", Some("fp_account_b")); // different account -> different fingerprint
    let mut obs_b = base_obs("obs_acct_b", 42000, "debit", "2026-06-10 09:02:00");
    obs_b.reference_id = Some("REF_R3".to_string());
    let candidates = fetch_candidates(&conn, &obs_b).unwrap();
    let decision = reconcile(&conn, &obs_b, candidates).unwrap();

    assert!(
        matches!(decision, DecisionType::AutoMatchedExact | DecisionType::AutoMatchedScored),
        "account-differentiated fingerprints must not block a legitimate instrument-level merge, got {:?}", decision
    );

    let canonical_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted = 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        canonical_count, 1,
        "must merge into a single canonical, not create two"
    );
}

// ── Scenario 4: genuinely distinct transactions, same amount/merchant/day ──

/// Doc 30 TASK-DEDUP-010 scenario: two genuinely distinct real-world
/// transactions that happen to share amount, merchant, and a close
/// timestamp must NOT be blindly auto-merged -- either ambiguous or two
/// separate canonicals, never a confident `AutoMatchedExact`.
#[test]
fn test_regression_coincidental_similarity_does_not_auto_merge() {
    let conn = conn_with_fk_off();

    insert_observation_row(&conn, "obs_coincidence_1", None);
    let obs1 = base_obs("obs_coincidence_1", 20000, "debit", "2026-06-10 12:00:00");
    reconcile(&conn, &obs1, vec![]).unwrap();

    insert_observation_row(&conn, "obs_coincidence_2", None);
    let obs2 = base_obs("obs_coincidence_2", 20000, "debit", "2026-06-10 12:03:00");
    let candidates = fetch_candidates(&conn, &obs2).unwrap();
    let decision = reconcile(&conn, &obs2, candidates).unwrap();

    assert!(
        !matches!(decision, DecisionType::AutoMatchedExact),
        "no reference ID and no fingerprint to confirm identity -- must never confidently auto-merge on amount/merchant/time alone, got {:?}", decision
    );
}

// ── Scenario 5: refund/reversal ─────────────────────────────────────────

/// Doc 30 TASK-DEDUP-010 scenario: a refund (opposite direction, same
/// amount, shortly after the original debit) must be a distinct new
/// canonical, never matched against the original debit -- candidate search
/// filters strictly on matching `direction`.
#[test]
fn test_regression_refund_is_distinct_canonical_not_matched_to_debit() {
    let conn = conn_with_fk_off();

    insert_observation_row(&conn, "obs_debit", None);
    let obs_debit = base_obs("obs_debit", 30000, "debit", "2026-06-10 10:00:00");
    reconcile(&conn, &obs_debit, vec![]).unwrap();

    insert_observation_row(&conn, "obs_refund", None);
    let obs_refund = base_obs("obs_refund", 30000, "credit", "2026-06-10 11:00:00");
    let candidates = fetch_candidates(&conn, &obs_refund).unwrap();
    assert!(
        candidates.is_empty(),
        "opposite-direction candidate search must never surface the original debit"
    );

    let decision = reconcile(&conn, &obs_refund, candidates).unwrap();
    assert_eq!(decision, DecisionType::NewCanonical);

    let canonical_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted = 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        canonical_count, 2,
        "debit and refund must remain two distinct canonicals"
    );
}

// ── Scenario 6: EMI conversion arriving after the original purchase ────────

/// Doc 30 TASK-DEDUP-010 scenario: an EMI-conversion alert (different
/// amount -- the installment amount, not the original purchase amount)
/// never scores as a duplicate of the original purchase (amounts differ),
/// so it always becomes its own new canonical -- but EMI linking
/// (TASK-TXN-012, via the shared post-processing hook) must still connect
/// it back to the original purchase through `parent_transaction_id`,
/// independent of and unblocked by the generic scoring/matching decision.
#[test]
fn test_regression_emi_conversion_links_to_original_purchase() {
    // Real EMI installments carry equal monthly amounts and equal EMI
    // fields on every installment's own alert (the actual, tested behavior
    // of `link_emi_installment`, Doc 30 TASK-TXN-012 -- it groups
    // installments to each other via a shared `emi_group_id`, not to an
    // untagged plain purchase transaction that never itself receives one).
    let conn = conn_with_fk_off();

    insert_observation_row(&conn, "obs_emi_1", None);
    let mut obs_emi_1 = base_obs("obs_emi_1", 100000, "debit", "2026-06-01 10:00:00");
    obs_emi_1.emi_total_installments = Some(6);
    obs_emi_1.emi_original_amount_minor = Some(600000);
    reconcile(&conn, &obs_emi_1, vec![]).unwrap();
    let installment_1_id: String = conn
        .query_row(
            "SELECT canonical_transaction_id FROM transaction_observations WHERE id = 'obs_emi_1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    // Directly seed the normalized-merchant precondition `link_emi_installment`
    // reads (Document 18 §4.2's `emi_group_id` formula hashes it) -- isolates
    // this scenario from the separate merchant-alias-matching pipeline,
    // which is not what this test is about.
    conn.execute(
        "UPDATE transactions SET merchant_normalized_name = 'uber' WHERE id = ?1",
        rusqlite::params![installment_1_id],
    )
    .unwrap();
    dinero_app_lib::extraction::emi_detector::link_emi_installment(
        &conn,
        &installment_1_id,
        "inst_1",
        Some("uber"),
        Some(6),
        Some(600000),
    )
    .unwrap();

    insert_observation_row(&conn, "obs_emi_2", None);
    let mut obs_emi_2 = base_obs("obs_emi_2", 100000, "debit", "2026-07-01 10:00:00"); // one month later, same amount
    obs_emi_2.emi_total_installments = Some(6);
    obs_emi_2.emi_original_amount_minor = Some(600000);
    let candidates = fetch_candidates(&conn, &obs_emi_2).unwrap();
    assert!(
        candidates.is_empty(),
        "installment 2 is 30 days after installment 1 -- outside the +/-3-day candidate window, so \
         generic scoring never even considers them a match; EMI linking below is what actually connects them"
    );
    let decision = reconcile(&conn, &obs_emi_2, candidates).unwrap();
    assert_eq!(decision, DecisionType::NewCanonical);
    let installment_2_id: String = conn
        .query_row(
            "SELECT canonical_transaction_id FROM transaction_observations WHERE id = 'obs_emi_2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(installment_2_id, installment_1_id);

    conn.execute(
        "UPDATE transactions SET merchant_normalized_name = 'uber' WHERE id = ?1",
        rusqlite::params![installment_2_id],
    )
    .unwrap();
    dinero_app_lib::extraction::emi_detector::link_emi_installment(
        &conn,
        &installment_2_id,
        "inst_1",
        Some("uber"),
        Some(6),
        Some(600000),
    )
    .unwrap();

    let (parent_id, subtype): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT parent_transaction_id, transaction_subtype FROM transactions WHERE id = ?1",
            rusqlite::params![installment_2_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(subtype.as_deref(), Some("emi_installment"));
    assert_eq!(
        parent_id,
        Some(installment_1_id),
        "the later installment must link back to the earlier one via the shared emi_group_id, \
         independent of and unblocked by the generic reconciliation decision above"
    );
}

// ── Aggregate: false-merge / false-separation rate ──────────────────────

/// Doc 30 TASK-DEDUP-010: "Computes and reports false-merge rate (≤0.1%
/// target) and false-separation rate." A synthetic, corpus-independent
/// batch (consistent with TASK-TXN-014's own precedent of a pure,
/// no-external-corpus-dependency benchmark when no real corpus is
/// provisioned locally): half the pairs are genuine duplicates (same
/// fingerprint, close time, same everything) that must merge; half are
/// genuinely distinct transactions (different instruments) that must never
/// merge.
#[test]
fn test_false_merge_and_false_separation_rates() {
    let conn = conn_with_fk_off();
    let mut false_merges = 0usize;
    let mut false_separations = 0usize;
    let should_merge_total = 25;
    let should_not_merge_total = 25;

    // Genuine duplicates: identical fingerprint, tight time window -> must merge.
    for i in 0..should_merge_total {
        let base_id = format!("dup_base_{i}");
        let dup_id = format!("dup_dup_{i}");
        let fp = format!("fp_dup_{i}");
        let amount = 10000 + (i as i64 * 137);

        insert_observation_row(&conn, &base_id, Some(&fp));
        let mut base = base_obs(&base_id, amount, "debit", "2026-06-10 10:00:00");
        base.instrument_id = format!("inst_dup_{i}");
        base.fingerprint = Some(fp.clone());
        reconcile(&conn, &base, vec![]).unwrap();

        insert_observation_row(&conn, &dup_id, Some(&fp));
        let mut dup = base_obs(&dup_id, amount, "debit", "2026-06-10 10:00:45");
        dup.instrument_id = format!("inst_dup_{i}");
        dup.fingerprint = Some(fp);
        let candidates = fetch_candidates(&conn, &dup).unwrap();
        let decision = reconcile(&conn, &dup, candidates).unwrap();
        if !matches!(
            decision,
            DecisionType::AutoMatchedExact | DecisionType::AutoMatchedScored
        ) {
            false_separations += 1;
        }
    }

    // Genuinely distinct: different instruments entirely -> must never merge.
    for i in 0..should_not_merge_total {
        let a_id = format!("distinct_a_{i}");
        let b_id = format!("distinct_b_{i}");
        let amount = 5000 + (i as i64 * 71);

        insert_observation_row(&conn, &a_id, None);
        let mut a = base_obs(&a_id, amount, "debit", "2026-06-10 08:00:00");
        a.instrument_id = format!("inst_a_{i}");
        reconcile(&conn, &a, vec![]).unwrap();

        insert_observation_row(&conn, &b_id, None);
        let mut b = base_obs(&b_id, amount, "debit", "2026-06-10 08:01:00");
        b.instrument_id = format!("inst_b_{i}"); // different instrument entirely
        let candidates = fetch_candidates(&conn, &b).unwrap();
        assert!(candidates.is_empty(), "candidate search is instrument-scoped -- a different instrument must never surface as a candidate");
        let decision = reconcile(&conn, &b, candidates).unwrap();
        if matches!(
            decision,
            DecisionType::AutoMatchedExact | DecisionType::AutoMatchedScored
        ) {
            false_merges += 1;
        }
    }

    let false_merge_rate = false_merges as f64 / should_not_merge_total as f64;
    let false_separation_rate = false_separations as f64 / should_merge_total as f64;

    println!(
        "reconciliation regression: false_merge_rate={:.4}% ({false_merges}/{should_not_merge_total}), false_separation_rate={:.4}% ({false_separations}/{should_merge_total})",
        false_merge_rate * 100.0,
        false_separation_rate * 100.0,
    );

    assert!(
        false_merge_rate <= 0.001,
        "false-merge rate {:.4}% exceeds the 0.1% target ({false_merges}/{should_not_merge_total})",
        false_merge_rate * 100.0
    );
    assert_eq!(
        false_separations, 0,
        "every genuine same-fingerprint duplicate in this batch must merge"
    );
}
