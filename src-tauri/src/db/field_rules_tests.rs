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

// ── Review P2: a newer user correction serves at once; the old rule is kept ──
#[test]
fn recorrection_supersedes_the_serving_rule_and_keeps_it() {
    let conn = setup_db();
    let first = make_variant("r1", "HDFC", "merchant", "hash_r", "active");
    upsert_variant(&conn, &first, None).unwrap();

    let mut second = make_variant("r2", "HDFC", "merchant", "hash_r", "active");
    second.rule_payload_json = serde_json::json!({"regex": "NEW (.+)", "capture_group": 1});
    let id2 = upsert_variant(&conn, &second, Some("fb_1")).unwrap();

    assert_eq!(id2, "r2", "a correction is stored beside the old rule, not over it");
    assert_eq!(select_by_id(&conn, "r2").unwrap().unwrap().status, "active");
    assert_eq!(select_by_id(&conn, "r1").unwrap().unwrap().status, "inactive");
    let live = select_live_by_bank(&conn, "HDFC", "email").unwrap();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].rule_payload_json["regex"], "NEW (.+)");

    let (old, fb): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT old_payload_json, triggering_feedback_log_id FROM rule_change_log
             WHERE field_rule_variant_id = 'r2' AND action = 'created'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(old.unwrap().contains(r"Rs (\\d+)"), "the displaced payload is logged");
    assert_eq!(fb.as_deref(), Some("fb_1"));
}

/// A rule the way an automatic source (drift LLM, merchant cleanup) writes it.
fn automatic(id: &str, hash: &str, learned_from: &str, regex: &str) -> FieldRuleVariant {
    let mut v = make_variant(id, "HDFC", "merchant", hash, "active");
    v.authored_by = "llm".to_string();
    v.learned_from = learned_from.to_string();
    v.success_count = 1;
    v.rule_payload_json = serde_json::json!({"regex": regex, "capture_group": 1});
    v
}

// ── Review P2: an automatic rule never demotes or replaces a proven one ──────
#[test]
fn drift_rule_shadows_a_trusted_rule_instead_of_demoting_it() {
    let conn = setup_db();
    let mut proven = automatic("proven", "h", "batch_cleanup", "at (.+)");
    proven.status = "trusted".to_string();
    proven.success_count = 50;
    upsert_variant(&conn, &proven, None).unwrap();

    let mut drift = automatic("drift", "h", "drift_llm", "on (.+)");
    drift.status = "pending".to_string();
    drift.success_count = 0;
    upsert_variant(&conn, &drift, None).unwrap();

    let kept = select_by_id(&conn, "proven").unwrap().unwrap();
    assert_eq!((kept.status.as_str(), kept.success_count), ("trusted", 50));
    assert_eq!(select_by_id(&conn, "drift").unwrap().unwrap().status, "pending");
    let live = select_live_by_bank(&conn, "HDFC", "email").unwrap();
    assert_eq!(live.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(), ["proven"]);
}

#[test]
fn cleanup_rule_becomes_a_challenger_not_the_serving_rule() {
    let conn = setup_db();
    upsert_variant(&conn, &make_variant("user", "HDFC", "merchant", "h", "active"), None).unwrap();
    upsert_variant(&conn, &automatic("cleanup", "h", "batch_cleanup", "at (.+)"), None).unwrap();

    let c = select_by_id(&conn, "cleanup").unwrap().unwrap();
    assert_eq!((c.status.as_str(), c.success_count, c.failure_count), ("pending", 0, 0));
    assert_eq!(select_by_id(&conn, "user").unwrap().unwrap().status, "active");
}

#[test]
fn a_user_correction_rule_is_never_displaced_automatically() {
    let conn = setup_db();
    let mut user = make_variant("user", "HDFC", "merchant", "h", "active");
    user.success_count = 4;
    user.failure_count = 2;
    user.confidence = 4.0 / 6.0;
    upsert_variant(&conn, &user, None).unwrap();
    upsert_variant(&conn, &automatic("cleanup", "h", "batch_cleanup", "at (.+)"), None).unwrap();
    for _ in 0..5 {
        record_success(&conn, "cleanup").unwrap();
    }
    assert_eq!(select_by_id(&conn, "cleanup").unwrap().unwrap().status, "pending");
    assert_eq!(select_by_id(&conn, "user").unwrap().unwrap().status, "active");
}

#[test]
fn a_challenger_takes_over_only_by_beating_the_serving_rule() {
    let conn = setup_db();
    // A perfect serving rule is never displaced, however well a challenger does.
    let mut perfect = automatic("perfect", "h1", "batch_cleanup", "at (.+)");
    perfect.success_count = 20;
    upsert_variant(&conn, &perfect, None).unwrap();
    upsert_variant(&conn, &automatic("c1", "h1", "drift_llm", "on (.+)"), None).unwrap();
    for _ in 0..5 {
        record_success(&conn, "c1").unwrap();
    }
    assert_eq!(select_by_id(&conn, "c1").unwrap().unwrap().status, "pending");
    assert_eq!(select_by_id(&conn, "perfect").unwrap().unwrap().status, "active");

    // A serving rule that has started failing loses to a challenger with a
    // better hit rate after three shadow successes.
    let mut slipping = automatic("slipping", "h2", "batch_cleanup", "at (.+)");
    slipping.success_count = 8;
    slipping.failure_count = 2;
    slipping.confidence = 0.8;
    upsert_variant(&conn, &slipping, None).unwrap();
    upsert_variant(&conn, &automatic("c2", "h2", "drift_llm", "on (.+)"), None).unwrap();
    record_success(&conn, "c2").unwrap();
    record_success(&conn, "c2").unwrap();
    assert_eq!(select_by_id(&conn, "c2").unwrap().unwrap().status, "pending");
    record_success(&conn, "c2").unwrap();
    assert_eq!(select_by_id(&conn, "c2").unwrap().unwrap().status, "active");
    assert_eq!(select_by_id(&conn, "slipping").unwrap().unwrap().status, "inactive");
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
         VALUES ('inst_1', 'credit_card', 'HDFC', '1234')",
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

// ── Review M10: pending rules earn promotion by shadow evaluation ──────────
#[test]
fn pending_rule_is_promoted_by_agreeing_confirmations_and_not_by_disagreeing() {
    let conn = setup_db();
    let body = "Rs 250 debited from your account";
    let hash = crate::extraction::ladder::compute_template_hash(body);
    let id = upsert_variant(&conn, &make_variant("v_shadow", "HDFC", "amount", &hash, "pending"), None)
        .unwrap();
    let agree = crate::extraction::ladder::ExtractionResult {
        amount_minor: Some(25000),
        ..Default::default()
    };
    for _ in 0..3 {
        assert_eq!(shadow_evaluate_pending(&conn, "HDFC", "email", body, &agree).unwrap(), 1);
    }
    assert_eq!(select_by_id(&conn, &id).unwrap().unwrap().status, "active");

    // Disagreement counts against a pending rule; with no successes behind it
    // the existing failure policy retires it.
    let mut wrong = make_variant("v_wrong", "HDFC", "amount", &hash, "pending");
    wrong.learned_from = "drift_llm".to_string();
    wrong.rule_payload_json = serde_json::json!({"regex": r"Rs (\d)", "capture_group": 1});
    let id2 = upsert_variant(&conn, &wrong, None).unwrap();
    let disagree = crate::extraction::ladder::ExtractionResult {
        amount_minor: Some(99900),
        ..Default::default()
    };
    shadow_evaluate_pending(&conn, "HDFC", "email", body, &disagree).unwrap();
    let rule = select_by_id(&conn, &id2).unwrap().unwrap();
    assert_eq!(rule.failure_count, 1);
    assert_eq!(rule.status, "inactive");

    // Other banks and templates are untouched.
    assert_eq!(shadow_evaluate_pending(&conn, "ICICI", "email", body, &agree).unwrap(), 0);
}

// ── Review P2: a drift challenger is measured against the serving rule on the
// same messages ─────────────────────────────────────────────────────────────
#[test]
fn a_drift_challenger_beats_a_serving_rule_that_stopped_matching() {
    let conn = setup_db();
    let body = "Spent Rs 250 on STARBUCKS today";
    let hash = crate::extraction::ladder::compute_template_hash(body);

    // A proven rule that no longer captures anything in this template.
    let mut proven = automatic("proven", &hash, "batch_cleanup", r"at (\w+)");
    proven.status = "trusted".to_string();
    proven.success_count = 50;
    upsert_variant(&conn, &proven, None).unwrap();
    let mut drift = automatic("drift", &hash, "drift_llm", r"on (\w+)");
    drift.status = "pending".to_string();
    upsert_variant(&conn, &drift, None).unwrap();

    let confirmed = crate::extraction::ladder::ExtractionResult {
        merchant_raw: Some("STARBUCKS".to_string()),
        ..Default::default()
    };
    for _ in 0..3 {
        shadow_evaluate_pending(&conn, "HDFC", "email", body, &confirmed).unwrap();
    }
    let proven = select_by_id(&conn, "proven").unwrap().unwrap();
    assert_eq!((proven.status.as_str(), proven.failure_count), ("inactive", 3));
    assert_eq!(select_by_id(&conn, "drift").unwrap().unwrap().status, "active");

    // A user-confirmed serving rule is neither scored nor displaced.
    let body2 = "Paid Rs 99 on ZOMATO now";
    let hash2 = crate::extraction::ladder::compute_template_hash(body2);
    let mut user = make_variant("user", "HDFC", "merchant", &hash2, "active");
    user.rule_payload_json = serde_json::json!({"regex": r"at (\w+)", "capture_group": 1});
    upsert_variant(&conn, &user, None).unwrap();
    let mut drift2 = automatic("drift2", &hash2, "drift_llm", r"on (\w+)");
    drift2.status = "pending".to_string();
    upsert_variant(&conn, &drift2, None).unwrap();
    let confirmed2 = crate::extraction::ladder::ExtractionResult {
        merchant_raw: Some("ZOMATO".to_string()),
        ..Default::default()
    };
    for _ in 0..5 {
        shadow_evaluate_pending(&conn, "HDFC", "email", body2, &confirmed2).unwrap();
    }
    let user = select_by_id(&conn, "user").unwrap().unwrap();
    assert_eq!((user.status.as_str(), user.failure_count), ("active", 0));
    assert_eq!(select_by_id(&conn, "drift2").unwrap().unwrap().status, "pending");
}
