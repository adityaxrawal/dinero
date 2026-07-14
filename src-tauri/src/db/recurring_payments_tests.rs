use anyhow::Result;
use chrono::{NaiveDate, Utc};
use uuid::Uuid;

use crate::db::recurring_payments::{self, RecurringPaymentsRow};
use rusqlite::Connection;

fn setup_db() -> Connection {
    crate::db::test_helpers::setup_test_db()
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
