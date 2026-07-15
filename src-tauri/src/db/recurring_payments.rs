use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecurringPaymentsRow {
    pub id: String,
    pub merchant_entity_id: Option<String>,
    pub instrument_id: Option<String>,
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub cadence: Option<String>,
    pub next_billing_date: Option<NaiveDate>,
    pub next_predicted_date: Option<NaiveDate>,
    pub next_predicted_amount: Option<f64>,
    pub confidence: Option<f64>,
    pub status: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub fn insert(conn: &Connection, row: &RecurringPaymentsRow) -> Result<()> {
    conn.execute(
        "INSERT INTO recurring_payments (
            id, merchant_entity_id, instrument_id, amount_minor, currency, cadence,
            next_billing_date, next_predicted_date, next_predicted_amount, confidence, status
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            row.id,
            row.merchant_entity_id,
            row.instrument_id,
            row.amount_minor,
            row.currency,
            row.cadence,
            row.next_billing_date,
            row.next_predicted_date,
            row.next_predicted_amount,
            row.confidence,
            row.status,
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<RecurringPaymentsRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, merchant_entity_id, instrument_id, amount_minor, currency, cadence,
                next_billing_date, next_predicted_date, next_predicted_amount, confidence, status, created_at, updated_at
         FROM recurring_payments WHERE id = ?1"
    )?;
    let row = stmt
        .query_row(params![id], |r| {
            Ok(RecurringPaymentsRow {
                id: r.get(0)?,
                merchant_entity_id: r.get(1)?,
                instrument_id: r.get(2)?,
                amount_minor: r.get(3)?,
                currency: r.get(4)?,
                cadence: r.get(5)?,
                next_billing_date: r.get(6)?,
                next_predicted_date: r.get(7)?,
                next_predicted_amount: r.get(8)?,
                confidence: r.get(9)?,
                status: r.get(10)?,
                created_at: r.get(11)?,
                updated_at: r.get(12)?,
            })
        })
        .optional()?;
    Ok(row)
}

/// Doc 30 TASK-TXN-011: looks up an existing recurring-payment row for this
/// (instrument, merchant) pair so re-detection after a new occurrence
/// updates it in place rather than creating a duplicate row every time.
pub fn find_by_instrument_and_merchant(
    conn: &Connection,
    instrument_id: &str,
    merchant_entity_id: &str,
) -> Result<Option<RecurringPaymentsRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, merchant_entity_id, instrument_id, amount_minor, currency, cadence,
                next_billing_date, next_predicted_date, next_predicted_amount, confidence, status, created_at, updated_at
         FROM recurring_payments WHERE instrument_id = ?1 AND merchant_entity_id = ?2"
    )?;
    let row = stmt
        .query_row(params![instrument_id, merchant_entity_id], |r| {
            Ok(RecurringPaymentsRow {
                id: r.get(0)?,
                merchant_entity_id: r.get(1)?,
                instrument_id: r.get(2)?,
                amount_minor: r.get(3)?,
                currency: r.get(4)?,
                cadence: r.get(5)?,
                next_billing_date: r.get(6)?,
                next_predicted_date: r.get(7)?,
                next_predicted_amount: r.get(8)?,
                confidence: r.get(9)?,
                status: r.get(10)?,
                created_at: r.get(11)?,
                updated_at: r.get(12)?,
            })
        })
        .optional()?;
    Ok(row)
}

pub fn update(conn: &Connection, row: &RecurringPaymentsRow) -> Result<()> {
    conn.execute(
        "UPDATE recurring_payments SET
            merchant_entity_id = ?2,
            instrument_id = ?3,
            amount_minor = ?4,
            currency = ?5,
            cadence = ?6,
            next_billing_date = ?7,
            next_predicted_date = ?8,
            next_predicted_amount = ?9,
            confidence = ?10,
            status = ?11
         WHERE id = ?1",
        params![
            row.id,
            row.merchant_entity_id,
            row.instrument_id,
            row.amount_minor,
            row.currency,
            row.cadence,
            row.next_billing_date,
            row.next_predicted_date,
            row.next_predicted_amount,
            row.confidence,
            row.status,
        ],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM recurring_payments WHERE id = ?1", params![id])?;
    Ok(())
}
