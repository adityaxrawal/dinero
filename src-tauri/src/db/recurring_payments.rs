//! Detected recurring payments and subscriptions.
//!
//! Identified by instrument and merchant, which is what allows an upcoming
//! charge to be predicted and surfaced as an upcoming bill before it arrives.
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
    pub source: String,
    pub external_mandate_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Record a detected recurring payment.
pub fn insert(conn: &Connection, row: &RecurringPaymentsRow) -> Result<()> {
    conn.execute(
        "INSERT INTO recurring_payments (
            id, merchant_entity_id, instrument_id, amount_minor, currency, cadence,
            next_billing_date, next_predicted_date, next_predicted_amount, confidence, status,
            source, external_mandate_id
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
            row.source,
            row.external_mandate_id,
        ],
    )?;
    Ok(())
}

/// Fetch one recurring payment.
pub fn get(conn: &Connection, id: &str) -> Result<Option<RecurringPaymentsRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, merchant_entity_id, instrument_id, amount_minor, currency, cadence,
                next_billing_date, next_predicted_date, next_predicted_amount, confidence, status,
                source, external_mandate_id, created_at, updated_at
         FROM recurring_payments WHERE id = ?1",
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
                source: r.get(11)?,
                external_mandate_id: r.get(12)?,
                created_at: r.get(13)?,
                updated_at: r.get(14)?,
            })
        })
        .optional()?;
    Ok(row)
}

/// Active recurring payments, used to predict upcoming charges.
pub fn select_active(conn: &Connection) -> Result<Vec<RecurringPaymentsRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, merchant_entity_id, instrument_id, amount_minor, currency, cadence,
                next_billing_date, next_predicted_date, next_predicted_amount, confidence, status,
                source, external_mandate_id, created_at, updated_at
         FROM recurring_payments WHERE status = 'active' ORDER BY next_predicted_date DESC",
    )?;
    let rows = stmt.query_map([], |r| {
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
            source: r.get(11)?,
            external_mandate_id: r.get(12)?,
            created_at: r.get(13)?,
            updated_at: r.get(14)?,
        })
    })?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Finds a recurring payment by the pair that identifies it.
///
/// Instrument and merchant together, because the same subscription billed to a
/// different card is a distinct arrangement from the user's point of view.
pub fn find_by_instrument_and_merchant(
    conn: &Connection,
    instrument_id: &str,
    merchant_entity_id: &str,
) -> Result<Option<RecurringPaymentsRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, merchant_entity_id, instrument_id, amount_minor, currency, cadence,
                next_billing_date, next_predicted_date, next_predicted_amount, confidence, status,
                source, external_mandate_id, created_at, updated_at
         FROM recurring_payments WHERE instrument_id = ?1 AND merchant_entity_id = ?2",
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
                source: r.get(11)?,
                external_mandate_id: r.get(12)?,
                created_at: r.get(13)?,
                updated_at: r.get(14)?,
            })
        })
        .optional()?;
    Ok(row)
}

/// Update a recurring payment's schedule or amount.
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
            status = ?11,
            source = ?12,
            external_mandate_id = ?13
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
            row.source,
            row.external_mandate_id,
        ],
    )?;
    Ok(())
}

/// Remove a recurring payment.
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM recurring_payments WHERE id = ?1", params![id])?;
    Ok(())
}

/// Inserts or updates a user-declared recurring payment.
///
/// Kept distinct from detection: a user's explicit statement about their own
/// subscriptions outranks anything inferred from transaction history.
pub fn upsert_explicit(
    conn: &Connection,
    instrument_id: &str,
    merchant_entity_id: &str,
    amount_minor: Option<i64>,
    currency: &str,
    cadence: Option<&str>,
    external_mandate_id: Option<&str>,
) -> Result<String> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM recurring_payments
             WHERE instrument_id = ?1 AND merchant_entity_id = ?2 AND source = 'explicit'",
            params![instrument_id, merchant_entity_id],
            |r| r.get(0),
        )
        .optional()?;

    if let Some(id) = existing {
        conn.execute(
            "UPDATE recurring_payments SET
                amount_minor = ?2, currency = ?3, cadence = ?4, status = 'active',
                external_mandate_id = ?5, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?1",
            params![id, amount_minor, currency, cadence, external_mandate_id],
        )?;
        Ok(id)
    } else {
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO recurring_payments (
                id, merchant_entity_id, instrument_id, amount_minor, currency, cadence,
                status, source, external_mandate_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', 'explicit', ?7)",
            params![
                id,
                merchant_entity_id,
                instrument_id,
                amount_minor,
                currency,
                cadence,
                external_mandate_id
            ],
        )?;
        Ok(id)
    }
}

/// Active payments that a cancellation notice might refer to.
///
/// A mandate cancellation names the arrangement loosely, so candidates are
/// narrowed here and matched afterwards.
pub fn find_active_candidates_for_cancellation(
    conn: &Connection,
    instrument_id: Option<&str>,
    merchant_entity_id: Option<&str>,
    external_mandate_id: Option<&str>,
) -> Result<Vec<RecurringPaymentsRow>> {
    if let Some(mandate_id) = external_mandate_id {
        let mut stmt = conn.prepare(
            "SELECT id, merchant_entity_id, instrument_id, amount_minor, currency, cadence,
                    next_billing_date, next_predicted_date, next_predicted_amount, confidence, status,
                    source, external_mandate_id, created_at, updated_at
             FROM recurring_payments WHERE external_mandate_id = ?1 AND status = 'active'",
        )?;
        let rows = stmt
            .query_map(params![mandate_id], row_from_sql)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if !rows.is_empty() {
            return Ok(rows);
        }
    }

    let (instrument_id, merchant_entity_id) = match (instrument_id, merchant_entity_id) {
        (Some(i), Some(m)) => (i, m),
        _ => return Ok(vec![]),
    };
    let mut stmt = conn.prepare(
        "SELECT id, merchant_entity_id, instrument_id, amount_minor, currency, cadence,
                next_billing_date, next_predicted_date, next_predicted_amount, confidence, status,
                source, external_mandate_id, created_at, updated_at
         FROM recurring_payments
         WHERE instrument_id = ?1 AND merchant_entity_id = ?2 AND status = 'active'",
    )?;
    let rows = stmt
        .query_map(params![instrument_id, merchant_entity_id], row_from_sql)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Maps a result row onto a recurring payment.
fn row_from_sql(r: &rusqlite::Row) -> rusqlite::Result<RecurringPaymentsRow> {
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
        source: r.get(11)?,
        external_mandate_id: r.get(12)?,
        created_at: r.get(13)?,
        updated_at: r.get(14)?,
    })
}

/// Marks a recurring payment cancelled, stopping future predictions for it.
pub fn mark_cancelled(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE recurring_payments SET status = 'cancelled', updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}
