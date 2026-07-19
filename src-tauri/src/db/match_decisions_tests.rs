use anyhow::Result;
use chrono::Utc;
use rusqlite::params;
use uuid::Uuid;

use crate::db::match_decisions::{self, MatchDecisionsRow};
use rusqlite::Connection;

fn setup_db() -> Connection {
    crate::db::test_helpers::setup_test_db()
}

#[test]
fn test_match_decisions_no_update_path() -> Result<()> {
    let conn = setup_db();

    // Insert dummy rows to satisfy FK constraints
    conn.execute("INSERT INTO instruments (id, type, issuer_name, masked_identifier) VALUES ('inst_1', 'credit_card', 'Test Bank', '1234')", params![]).unwrap();
    conn.execute(
        "INSERT INTO transaction_observations (id, instrument_id) VALUES ('obs_123', 'inst_1')",
        params![],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO transactions (id, instrument_id, amount_minor) VALUES ('tx_456', 'inst_1', 0)",
        params![],
    )
    .unwrap();

    let id = Uuid::new_v4().to_string();
    let row = MatchDecisionsRow {
        id: id.clone(),
        observation_id: Some("obs_123".to_string()),
        matched_transaction_id: Some("tx_456".to_string()),
        decision: Some("auto_matched_exact".to_string()),
        score: Some(0.95),
        rules_triggered_json: Some("[]".to_string()),
        review_status: Some("not_required".to_string()),
        reviewed_by: None,
        created_at: Some(Utc::now().naive_utc()),
    };

    // Insert
    match_decisions::insert(&conn, &row)?;

    // Get
    let fetched = match_decisions::select_by_id(&conn, &id)?.expect("Decision should exist");
    assert_eq!(fetched.decision.as_deref(), Some("auto_matched_exact"));
    assert_eq!(fetched.score, Some(0.95));

    // Immutability test (Attempt Update)
    let update_result = conn.execute(
        "UPDATE match_decisions SET decision = ?1 WHERE id = ?2",
        params!["reject", id],
    );

    assert!(
        update_result.is_err(),
        "Update should have failed due to immutability trigger"
    );
    let err_str = update_result.unwrap_err().to_string();
    assert!(
        err_str.contains("match_decisions is immutable"),
        "Unexpected error: {}",
        err_str
    );

    Ok(())
}
