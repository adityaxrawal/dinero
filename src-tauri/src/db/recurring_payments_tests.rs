use anyhow::Result;
use chrono::{NaiveDate, Utc};
use uuid::Uuid;

use crate::db::recurring_payments::{self, RecurringPaymentsRow};
use rusqlite::Connection;

fn setup_db() -> Connection {
    crate::db::test_helpers::setup_test_db()
}

/// Mandate-tracking tests need real FK-satisfying instrument/merchant rows
/// (recurring_payments.instrument_id/merchant_entity_id are real foreign
/// keys) -- same seeding pattern as extraction/recurring_detector.rs's own
/// tests.
fn setup_db_with_instrument_and_merchant() -> Connection {
    let conn = setup_db();
    conn.execute(
        "INSERT INTO instruments (id, type, issuer_name, masked_identifier, status) VALUES ('instr-1', 'credit_card', 'SBI Card', 'XXXX7603', 'active')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO merchants (id, name, normalized_name, source) VALUES ('merchant-1', 'ScribdInc', 'SCRIBDINC', 'system')",
        [],
    )
    .unwrap();
    conn
}

#[test]
fn test_recurring_payments_crud() -> Result<()> {
    let conn = setup_db();

    let id = Uuid::new_v4().to_string();
    let row = RecurringPaymentsRow {
        id: id.clone(),
        merchant_entity_id: None,
        instrument_id: None,
        amount_minor: Some(1500),
        currency: Some("USD".to_string()),
        cadence: Some("monthly".to_string()),
        next_billing_date: NaiveDate::from_ymd_opt(2026, 7, 10),
        next_predicted_date: None,
        next_predicted_amount: None,
        confidence: Some(0.95),
        status: Some("active".to_string()),
        source: "inferred".to_string(),
        external_mandate_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    // Insert
    recurring_payments::insert(&conn, &row)?;

    // Get
    let fetched = recurring_payments::get(&conn, &id)?.expect("Row should exist");
    assert_eq!(fetched.amount_minor, Some(1500));
    assert_eq!(fetched.cadence.as_deref(), Some("monthly"));

    // Update
    std::thread::sleep(std::time::Duration::from_secs(1));
    let mut updated_row = fetched.clone();
    updated_row.amount_minor = Some(2000);
    updated_row.status = Some("paused".to_string());
    recurring_payments::update(&conn, &updated_row)?;

    let fetched_updated = recurring_payments::get(&conn, &id)?.unwrap();
    assert_eq!(fetched_updated.amount_minor, Some(2000));
    assert_eq!(fetched_updated.status.as_deref(), Some("paused"));

    // updated_at trigger check
    assert!(fetched_updated.updated_at > fetched.updated_at);

    // Delete
    recurring_payments::delete(&conn, &id)?;
    assert!(recurring_payments::get(&conn, &id)?.is_none());

    Ok(())
}

#[test]
fn test_upsert_explicit_inserts_new_row_when_none_exists() {
    let conn = setup_db_with_instrument_and_merchant();
    let id = recurring_payments::upsert_explicit(
        &conn,
        "instr-1",
        "merchant-1",
        Some(0),
        "INR",
        Some("monthly"),
        Some("SIHUB123"),
    )
    .unwrap();
    let row = recurring_payments::get(&conn, &id).unwrap().unwrap();
    assert_eq!(row.status, Some("active".to_string()));
    assert_eq!(row.source, "explicit");
    assert_eq!(row.external_mandate_id, Some("SIHUB123".to_string()));
}

#[test]
fn test_upsert_explicit_updates_existing_explicit_row_not_duplicate() {
    let conn = setup_db_with_instrument_and_merchant();
    let id1 = recurring_payments::upsert_explicit(
        &conn, "instr-1", "merchant-1", Some(0), "INR", Some("monthly"), Some("SIHUB123"),
    )
    .unwrap();
    let id2 = recurring_payments::upsert_explicit(
        &conn, "instr-1", "merchant-1", Some(0), "INR", Some("monthly"), Some("SIHUB123"),
    )
    .unwrap();
    assert_eq!(
        id1, id2,
        "second registration for the same instrument+merchant must update, not duplicate"
    );
}

#[test]
fn test_find_active_candidates_matches_by_external_mandate_id_first() {
    let conn = setup_db_with_instrument_and_merchant();
    recurring_payments::upsert_explicit(
        &conn, "instr-1", "merchant-1", Some(0), "INR", Some("monthly"), Some("SIHUB123"),
    )
    .unwrap();
    let candidates = recurring_payments::find_active_candidates_for_cancellation(
        &conn,
        Some("instr-1"),
        Some("merchant-1"),
        Some("SIHUB123"),
    )
    .unwrap();
    assert_eq!(candidates.len(), 1);
}

#[test]
fn test_find_active_candidates_zero_when_no_active_row() {
    let conn = setup_db_with_instrument_and_merchant();
    let candidates = recurring_payments::find_active_candidates_for_cancellation(
        &conn,
        Some("instr-1"),
        Some("merchant-1"),
        None,
    )
    .unwrap();
    assert_eq!(candidates.len(), 0);
}

#[test]
fn test_mark_cancelled_sets_status() {
    let conn = setup_db_with_instrument_and_merchant();
    let id = recurring_payments::upsert_explicit(
        &conn, "instr-1", "merchant-1", Some(0), "INR", Some("monthly"), None,
    )
    .unwrap();
    recurring_payments::mark_cancelled(&conn, &id).unwrap();
    let row = recurring_payments::get(&conn, &id).unwrap().unwrap();
    assert_eq!(row.status, Some("cancelled".to_string()));
}
