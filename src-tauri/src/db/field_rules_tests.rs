#![cfg(test)]

use super::field_rules::*;
use rusqlite::Connection;

fn setup_db() -> Connection {
    crate::db::test_helpers::setup_test_db()
}

fn make_variant(id: &str, bank: &str, field: &str, hash: &str, status: &str) -> FieldRuleVariant {
    let now = chrono::Utc::now().naive_utc();
    FieldRuleVariant {
        id: id.to_string(),
        bank_name: bank.to_string(),
        field_name: field.to_string(),
        source_type: "email".to_string(),
        template_hash: hash.to_string(),
        rule_payload_json: serde_json::json!({"regex": r"Rs (\d+)", "capture_group": 1}),
        status: status.to_string(),
        success_count: 0,
        failure_count: 0,
        confidence: 1.0,
        authored_by: "deterministic".to_string(),
        learned_from: "user_edit".to_string(),
        created_at: Some(now),
        updated_at: Some(now),
    }
}

// ── Lifecycle: pending → active after 3 successes ────────────────────────────
#[test]
fn pending_becomes_active_after_three_successes() {
    let conn = setup_db();
    let id = upsert_variant(
        &conn,
        &make_variant("v1", "HDFC", "amount", "h1", "pending"),
        None,
    )
    .unwrap();

    record_success(&conn, &id).unwrap();
    record_success(&conn, &id).unwrap();
    assert_eq!(select_by_id(&conn, &id).unwrap().unwrap().status, "pending");

    record_success(&conn, &id).unwrap();
    let r = select_by_id(&conn, &id).unwrap().unwrap();
    assert_eq!(r.status, "active");
    assert_eq!(r.success_count, 3);
    assert!(r.confidence > 0.99);
}

// ── Lifecycle: active → trusted at 10 cumulative successes ───────────────────
#[test]
fn active_becomes_trusted_after_ten_successes() {
    let conn = setup_db();
    let mut v = make_variant("v2", "HDFC", "amount", "h2", "active");
    v.success_count = 3;
    let id = upsert_variant(&conn, &v, None).unwrap();
    for _ in 0..7 {
        record_success(&conn, &id).unwrap();
    }
    assert_eq!(select_by_id(&conn, &id).unwrap().unwrap().status, "trusted");
}

// ── Lifecycle: 3 failures → inactive ─────────────────────────────────────────
#[test]
fn three_failures_deactivates() {
    let conn = setup_db();
    let mut v = make_variant("v3", "HDFC", "amount", "h3", "active");
    v.success_count = 5;
    let id = upsert_variant(&conn, &v, None).unwrap();
    record_failure(&conn, &id).unwrap();
    record_failure(&conn, &id).unwrap();
    assert_ne!(
        select_by_id(&conn, &id).unwrap().unwrap().status,
        "inactive"
    );
    record_failure(&conn, &id).unwrap();
    assert_eq!(
        select_by_id(&conn, &id).unwrap().unwrap().status,
        "inactive"
    );
}

// ── Confidence decay below 70% deactivates before 3 failures ─────────────────
#[test]
fn confidence_decay_deactivates() {
    let conn = setup_db();
    let mut v = make_variant("v4", "SBI", "amount", "h4", "active");
    v.success_count = 1;
    let id = upsert_variant(&conn, &v, None).unwrap();
    record_failure(&conn, &id).unwrap();
    record_failure(&conn, &id).unwrap();
    assert_eq!(
        select_by_id(&conn, &id).unwrap().unwrap().status,
        "inactive"
    );
}

// ── Cross-bank isolation: the guarantee the whole design rests on ────────────
#[test]
fn rules_never_leak_across_banks() {
    let conn = setup_db();
    upsert_variant(
        &conn,
        &make_variant("h", "HDFC", "amount", "hash_a", "active"),
        None,
    )
    .unwrap();
    upsert_variant(
        &conn,
        &make_variant("i", "ICICI", "amount", "hash_a", "active"),
        None,
    )
    .unwrap();

    let hdfc = select_live_by_bank(&conn, "HDFC", "email").unwrap();
    assert_eq!(hdfc.len(), 1);
    assert_eq!(hdfc[0].bank_name, "HDFC");

    assert_eq!(
        count_live_by_bank_and_hash(&conn, "HDFC", "hash_a", "email").unwrap(),
        1
    );
    assert_eq!(
        count_live_by_bank_and_hash(&conn, "ICICI", "hash_b", "email").unwrap(),
        0
    );
}

// ── source_type isolation: an email rule must not apply to PDF extraction ────
#[test]
fn rules_never_leak_across_source_types() {
    let conn = setup_db();
    let mut pdf = make_variant("p", "HDFC", "merchant", "hash_x", "active");
    pdf.source_type = "statement_pdf".to_string();
    upsert_variant(
        &conn,
        &make_variant("e", "HDFC", "merchant", "hash_x", "active"),
        None,
    )
    .unwrap();
    upsert_variant(&conn, &pdf, None).unwrap();

    assert_eq!(
        select_live_by_bank(&conn, "HDFC", "email").unwrap().len(),
        1
    );
    assert_eq!(
        select_live_by_bank(&conn, "HDFC", "statement_pdf")
            .unwrap()
            .len(),
        1
    );
}

// ── Two templates for one (bank, field) share a parent and coexist ───────────
#[test]
fn second_template_becomes_a_sibling_variant() {
    let conn = setup_db();
    upsert_variant(
        &conn,
        &make_variant("t1", "HDFC", "amount", "old", "active"),
        None,
    )
    .unwrap();
    upsert_variant(
        &conn,
        &make_variant("t2", "HDFC", "amount", "new", "active"),
        None,
    )
    .unwrap();

    let parents: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM field_rules WHERE bank_name = 'HDFC' AND field_name = 'amount'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(parents, 1, "both variants must share exactly one parent");
    assert_eq!(
        select_live_by_bank(&conn, "HDFC", "email").unwrap().len(),
        2
    );
}

// ── A newer correction replaces the live rule and preserves the old payload ──
#[test]
fn recorrection_replaces_payload_and_logs_the_old_one() {
    let conn = setup_db();
    let first = make_variant("r1", "HDFC", "merchant", "hash_r", "active");
    let id = upsert_variant(&conn, &first, None).unwrap();

    let mut second = make_variant("r2", "HDFC", "merchant", "hash_r", "active");
    second.rule_payload_json = serde_json::json!({"regex": "NEW (.+)", "capture_group": 1});
    let id2 = upsert_variant(&conn, &second, Some("fb_1")).unwrap();

    assert_eq!(
        id2, id,
        "replacing a live variant must reuse its row, not add a second"
    );
    let live = select_by_id(&conn, &id).unwrap().unwrap();
    assert_eq!(live.rule_payload_json["regex"], "NEW (.+)");

    // Both `created` rows land in the same second, so `created_at` cannot
    // distinguish them -- the replacement is identified by being the one that
    // displaced a payload.
    let (old, new, fb): (Option<String>, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT old_payload_json, new_payload_json, triggering_feedback_log_id
             FROM rule_change_log
             WHERE field_rule_variant_id = ?1 AND action = 'created'
               AND old_payload_json IS NOT NULL",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert!(
        old.unwrap().contains(r"Rs (\\d+)"),
        "the replaced payload must be recoverable"
    );
    assert!(new.unwrap().contains("NEW (.+)"));
    assert_eq!(fb.unwrap_or_default(), "fb_1");
}

// ── An inactive variant does not block relearning the same template ──────────
#[test]
fn a_reverted_rule_can_be_relearned() {
    let conn = setup_db();
    let id = upsert_variant(
        &conn,
        &make_variant("g1", "HDFC", "amount", "hash_g", "active"),
        None,
    )
    .unwrap();
    revert(&conn, &id, "user reverted").unwrap();
    assert_eq!(
        select_by_id(&conn, &id).unwrap().unwrap().status,
        "inactive"
    );

    let fresh = upsert_variant(
        &conn,
        &make_variant("g2", "HDFC", "amount", "hash_g", "active"),
        None,
    )
    .unwrap();
    assert_ne!(
        fresh, id,
        "a fresh attempt must insert a new row, not revive the reverted one"
    );
    assert_eq!(
        select_live_by_bank(&conn, "HDFC", "email").unwrap().len(),
        1
    );
}

// ── Revert is auditable ──────────────────────────────────────────────────────
#[test]
fn revert_writes_a_change_log_row() {
    let conn = setup_db();
    let id = upsert_variant(
        &conn,
        &make_variant("rv", "HDFC", "amount", "hash_v", "active"),
        None,
    )
    .unwrap();
    revert(&conn, &id, "misbehaving").unwrap();

    let (action, reason): (String, Option<String>) = conn
        .query_row(
            "SELECT action, reason FROM rule_change_log
             WHERE field_rule_variant_id = ?1 AND action = 'reverted'",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(action, "reverted");
    assert_eq!(reason.unwrap(), "misbehaving");
}

// ── A rejected candidate is logged without writing a rule ────────────────────
#[test]
fn rejection_logs_without_creating_a_variant() {
    let conn = setup_db();
    log_rejection(
        &conn,
        Some("fb_9"),
        Some(&serde_json::json!({"regex": "broken("})),
        "self-check failed",
    )
    .unwrap();

    let variants: i64 = conn
        .query_row("SELECT COUNT(*) FROM field_rule_variants", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        variants, 0,
        "a rejected candidate must write no rule at all"
    );

    let logged: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM rule_change_log WHERE action = 'rejected'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(logged, 1);
}

// ── Only live variants are read by extraction ────────────────────────────────
#[test]
fn inactive_and_pending_variants_are_not_returned_to_extraction() {
    let conn = setup_db();
    upsert_variant(
        &conn,
        &make_variant("a", "Axis", "amount", "h_a", "active"),
        None,
    )
    .unwrap();
    upsert_variant(
        &conn,
        &make_variant("t", "Axis", "merchant", "h_t", "trusted"),
        None,
    )
    .unwrap();
    upsert_variant(
        &conn,
        &make_variant("p", "Axis", "balance", "h_p", "pending"),
        None,
    )
    .unwrap();
    let inactive_id = upsert_variant(
        &conn,
        &make_variant("x", "Axis", "reference_id", "h_x", "active"),
        None,
    )
    .unwrap();
    revert(&conn, &inactive_id, "test").unwrap();

    let live = select_live_by_bank(&conn, "Axis", "email").unwrap();
    let fields: std::collections::HashSet<&str> =
        live.iter().map(|r| r.field_name.as_str()).collect();
    assert_eq!(fields, ["amount", "merchant"].into_iter().collect());
}

// ── historical_samples returns bodies + the currently accepted value ─────────
#[test]
fn historical_samples_pairs_body_with_accepted_value() {
    let conn = setup_db();
    conn.execute(
        "INSERT INTO instruments (id, type, issuer_name, masked_identifier)
         VALUES ('inst_1', 'credit_card', 'HDFC Bank', '1234')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO transactions (id, instrument_id, amount_minor, merchant_display_name)
         VALUES ('tx_1', 'inst_1', 50000, 'Amazon')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO transaction_observations
            (id, canonical_transaction_id, source_pipeline, instrument_id, merchant_raw, raw_payload_json)
         VALUES ('obs_1', 'tx_1', 'gmail_transaction', 'inst_1', 'Amazon',
                 '{\"body\":\"Rs 500 spent at Amazon\"}')",
        [],
    )
    .unwrap();

    let samples = historical_samples(&conn, "HDFC Bank", "merchant", "email", None, 20).unwrap();
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].0, "Rs 500 spent at Amazon");
    assert_eq!(samples[0].1.as_deref(), Some("Amazon"));

    let excluded =
        historical_samples(&conn, "HDFC Bank", "merchant", "email", Some("obs_1"), 20).unwrap();
    assert!(
        excluded.is_empty(),
        "the training example must be excludable"
    );
}
