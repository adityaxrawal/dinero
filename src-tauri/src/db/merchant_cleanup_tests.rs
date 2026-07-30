//! Issue #12 persistence tests.

use super::merchant_cleanup::*;
use crate::extraction::merchant_llm::MerchantResolution;
use rusqlite::{params, Connection};

const SBI_BODY: &str = "Dear Cardholder, Rs.245.43 spent on your SBI Credit Card \
                        ending 7603 at RAZ*SWIGGY LIMITE BANGALORE on 01/07/26. \
                        Not you? Call 18001234.";

fn setup() -> Connection {
    let conn = crate::db::test_helpers::setup_test_db();
    conn.execute(
        "INSERT INTO instruments (id, type, issuer_name, masked_identifier, status)
         VALUES ('inst_1', 'credit_card', 'SBI Card', '7603', 'active')",
        [],
    )
    .unwrap();
    conn
}

/// Seeds one transaction plus the observation carrying its email body.
#[allow(clippy::too_many_arguments)]
fn seed_txn(
    conn: &Connection,
    tx_id: &str,
    merchant: &str,
    extraction_method: &str,
    body: Option<&str>,
    entity_id: Option<&str>,
) {
    conn.execute(
        "INSERT INTO transactions
             (id, instrument_id, amount_minor, amount, currency, direction,
              merchant_display_name, merchant_normalized_name, merchant_entity_id, is_deleted)
         VALUES (?1, 'inst_1', 24543, 245.43, 'INR', 'debit', ?2, ?2, ?3, 0)",
        params![tx_id, merchant, entity_id],
    )
    .unwrap();
    let payload = body.map(|b| serde_json::json!({ "body": b }).to_string());
    conn.execute(
        "INSERT INTO transaction_observations
             (id, canonical_transaction_id, source_pipeline, source_record_id,
              instrument_id, extraction_method, raw_payload_json)
         VALUES (?1, ?2, 'gmail_transaction', ?3, 'inst_1', ?4, ?5)",
        params![
            format!("obs_{tx_id}"),
            tx_id,
            format!("rec_{tx_id}"),
            extraction_method,
            payload
        ],
    )
    .unwrap();
}

fn resolution() -> MerchantResolution {
    MerchantResolution {
        merchant_in_email: "RAZ*SWIGGY LIMITE BANGALORE".to_string(),
        merchant_name: "Swiggy".to_string(),
        category: "Food & Dining".to_string(),
        confidence: 0.95,
    }
}

#[test]
fn queue_contains_bad_merchants_and_excludes_established_ones() {
    let conn = setup();
    // Seeded `merch_swiggy` ships with three aliases, so it is established.
    seed_txn(
        &conn,
        "tx_good",
        "SWIGGY",
        "bank_templates",
        Some(SBI_BODY),
        Some("merch_swiggy"),
    );
    seed_txn(&conn, "tx_bad", "USING YOUR", "nlp", Some(SBI_BODY), None);

    let queue = select_candidates(&conn, 100).unwrap();
    let ids: Vec<&str> = queue.iter().map(|c| c.transaction_id.as_str()).collect();

    assert!(ids.contains(&"tx_bad"), "garbage merchant must be queued");
    assert!(
        !ids.contains(&"tx_good"),
        "an established merchant must not burn inference"
    );
}

/// The body has to reach the model, or it cannot read the real merchant.
#[test]
fn candidate_carries_the_email_body_and_bank() {
    let conn = setup();
    seed_txn(&conn, "tx_bad", "USING YOUR", "nlp", Some(SBI_BODY), None);
    let c = &select_candidates(&conn, 10).unwrap()[0];
    assert_eq!(c.body.as_deref(), Some(SBI_BODY));
    assert_eq!(c.bank_name, "SBI Card");
    assert_eq!(c.current_merchant, "USING YOUR");
}

#[test]
fn apply_correction_fixes_the_transaction_and_sets_a_category() {
    let conn = setup();
    seed_txn(&conn, "tx_bad", "USING YOUR", "nlp", Some(SBI_BODY), None);
    let c = select_candidates(&conn, 10).unwrap().remove(0);

    apply_correction(&conn, "run_1", &c, &resolution()).unwrap();

    let (name, cat): (String, Option<String>) = conn
        .query_row(
            "SELECT merchant_display_name, category_id FROM transactions WHERE id = 'tx_bad'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(name, "Swiggy");
    assert_eq!(
        cat.as_deref(),
        Some("cat_food"),
        "the category must resolve to a real categories.id, not a bare name"
    );
}

/// The learning half: after a correction, a rule must exist that actually
/// fires on the next email of the same shape. This is the thing the old
/// placeholder regex (`"learned regex for <value>"`) never achieved, and the
/// reason the cleanup pass now writes through the shared synthesis path.
#[test]
fn apply_correction_teaches_a_rule_that_really_matches() {
    let conn = setup();
    seed_txn(&conn, "tx_bad", "USING YOUR", "nlp", Some(SBI_BODY), None);
    let c = select_candidates(&conn, 10).unwrap().remove(0);
    apply_correction(&conn, "run_1", &c, &resolution()).unwrap();

    let rules =
        crate::db::field_rules::select_live_by_bank(&conn, "SBI Card", "email").unwrap();
    let merchant_rule = rules
        .iter()
        .find(|r| r.field_name == "merchant")
        .expect("an active merchant rule must have been synthesized");
    assert_eq!(
        merchant_rule.learned_from, "batch_cleanup",
        "provenance must record which trigger taught this rule"
    );
    assert_eq!(merchant_rule.authored_by, "llm");

    let pattern = merchant_rule.rule_payload_json["regex"].as_str().unwrap();
    let re = regex::Regex::new(pattern).unwrap();

    let next_email = "Dear Cardholder, Rs.99.00 spent on your SBI Credit Card \
                      ending 4412 at RAZ*YULU BIKES on 09/08/26. Not you? Call 18009999.";
    let caps = re
        .captures(next_email)
        .expect("the learned rule must fire on the next email of this shape");
    assert_eq!(caps.get(1).unwrap().as_str().trim(), "RAZ*YULU BIKES");
}

/// A truncation genuinely names the merchant, so it earns an alias and the
/// next scan resolves it with no LLM at all.
#[test]
fn a_truncated_name_becomes_an_alias() {
    let conn = setup();
    seed_txn(
        &conn,
        "tx_trunc",
        "SWIGGY LIMITE",
        "bank_templates",
        Some(SBI_BODY),
        None,
    );
    let c = select_candidates(&conn, 10).unwrap().remove(0);
    apply_correction(&conn, "run_1", &c, &resolution()).unwrap();

    let resolved = crate::db::merchants::select_by_alias(&conn, "SWIGGY LIMITE")
        .unwrap()
        .expect("the truncation must now resolve without the LLM");
    assert_eq!(resolved.name, "Swiggy");
}

/// The dangerous case, and the reason `safe_to_alias` exists: "USING YOUR" is
/// boilerplate that appears in every bank's emails. Aliasing it to Swiggy
/// would silently relabel unrelated transactions forever, because the alias
/// table is consulted before anything else.
#[test]
fn boilerplate_never_becomes_an_alias() {
    let conn = setup();
    seed_txn(&conn, "tx_bad", "USING YOUR", "nlp", Some(SBI_BODY), None);
    let c = select_candidates(&conn, 10).unwrap().remove(0);
    apply_correction(&conn, "run_1", &c, &resolution()).unwrap();

    assert!(
        crate::db::merchants::select_by_alias(&conn, "USING YOUR")
            .unwrap()
            .is_none(),
        "boilerplate must never be aliased to a real merchant"
    );
    // ...while the verbatim span still is.
    assert!(
        crate::db::merchants::select_by_alias(&conn, "SWIGGY LIMITE BANGALORE")
            .unwrap()
            .is_some(),
        "the real span must still be learned"
    );
}

/// Resumability is derived, not stored: a fixed transaction must fall out of
/// the queue by itself, or a second run would redo all the same work.
#[test]
fn a_corrected_transaction_leaves_the_queue() {
    let conn = setup();
    seed_txn(&conn, "tx_bad", "USING YOUR", "nlp", Some(SBI_BODY), None);
    let c = select_candidates(&conn, 10).unwrap().remove(0);
    apply_correction(&conn, "run_1", &c, &resolution()).unwrap();

    let second_pass = select_candidates(&conn, 10).unwrap();
    assert!(
        second_pass.is_empty(),
        "a corrected transaction must not be re-queued, got {:?}",
        second_pass.iter().map(|c| &c.transaction_id).collect::<Vec<_>>()
    );
}

#[test]
fn revert_restores_the_previous_values_and_retires_the_rule() {
    let conn = setup();
    seed_txn(&conn, "tx_bad", "USING YOUR", "nlp", Some(SBI_BODY), None);
    let c = select_candidates(&conn, 10).unwrap().remove(0);
    apply_correction(&conn, "run_1", &c, &resolution()).unwrap();

    assert_eq!(revert_run(&conn, "run_1").unwrap(), 1);

    let (name, cat, entity): (Option<String>, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT merchant_display_name, category_id, merchant_entity_id
             FROM transactions WHERE id = 'tx_bad'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(name.as_deref(), Some("USING YOUR"), "name must be restored");
    assert_eq!(cat, None, "category must be restored to unset");
    assert_eq!(entity, None, "entity link must be restored");

    let active = crate::db::field_rules::select_live_by_bank(&conn, "SBI Card", "email").unwrap();
    assert!(
        !active.iter().any(|r| r.field_name == "merchant"),
        "the rule the correction taught must no longer be active"
    );
}

#[test]
fn revert_is_idempotent() {
    let conn = setup();
    seed_txn(&conn, "tx_bad", "USING YOUR", "nlp", Some(SBI_BODY), None);
    let c = select_candidates(&conn, 10).unwrap().remove(0);
    apply_correction(&conn, "run_1", &c, &resolution()).unwrap();

    assert_eq!(revert_run(&conn, "run_1").unwrap(), 1);
    assert_eq!(
        revert_run(&conn, "run_1").unwrap(),
        0,
        "a second revert must be a no-op, not a double-restore"
    );
}

/// Retention may already have wiped the body. The transaction still deserves
/// a fix from its extracted fields; it just cannot teach a pattern rule,
/// since there is no email left to anchor one to.
#[test]
fn a_transaction_with_no_body_still_gets_corrected() {
    let conn = setup();
    seed_txn(&conn, "tx_nobody", "RAZ", "generic_regex", None, None);
    let c = select_candidates(&conn, 10).unwrap().remove(0);
    assert!(c.body.is_none());

    apply_correction(&conn, "run_1", &c, &resolution()).unwrap();

    let name: String = conn
        .query_row(
            "SELECT merchant_display_name FROM transactions WHERE id = 'tx_nobody'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(name, "Swiggy");

    let learned: Option<String> = conn
        .query_row(
            "SELECT learned_rule_id FROM merchant_llm_corrections WHERE transaction_id = 'tx_nobody'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(learned.is_none(), "no body means no rule to learn");
}

#[test]
fn category_list_comes_from_the_database() {
    let conn = setup();
    let cats = category_names(&conn).unwrap();
    assert!(cats.contains(&"Food & Dining".to_string()));
    assert!(cats.contains(&"Others".to_string()));
    assert!(
        !cats.is_empty() && cats.len() > 5,
        "the seeded category tree must be offered to the LLM, got {cats:?}"
    );
}

#[test]
fn run_summary_counts_applied_and_reverted() {
    let conn = setup();
    seed_txn(&conn, "tx_a", "USING YOUR", "nlp", Some(SBI_BODY), None);
    seed_txn(&conn, "tx_b", "YOUR POT", "nlp", Some(SBI_BODY), None);
    for c in select_candidates(&conn, 10).unwrap() {
        apply_correction(&conn, "run_1", &c, &resolution()).unwrap();
    }
    assert_eq!(run_summary(&conn, "run_1").unwrap().applied, 2);

    revert_run(&conn, "run_1").unwrap();
    let s = run_summary(&conn, "run_1").unwrap();
    assert_eq!(s.applied, 0);
    assert_eq!(s.reverted, 2);
}

/// Worst-first ordering means an interrupted run has still fixed the most
/// damaged rows.
#[test]
fn queue_is_ordered_worst_first() {
    let conn = setup();
    seed_txn(&conn, "tx_mild", "SWIGGY LIMITE", "bank_templates", None, None);
    seed_txn(&conn, "tx_awful", "NK", "nlp", None, None);

    let queue = select_candidates(&conn, 10).unwrap();
    assert_eq!(queue[0].transaction_id, "tx_awful");
    assert!(queue[0].confidence < queue[1].confidence);
}
