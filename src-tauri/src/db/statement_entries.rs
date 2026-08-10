//! Individual line items extracted from a statement.
//!
//! `find_crossref_candidates` supports the cross-referencing pass: a statement
//! line and a bank alert email frequently describe the same payment, and this is
//! how the two are brought together instead of being recorded twice.
use anyhow::Result;
use chrono::{NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct StatementEntriesRow {
    pub id: String,
    pub statement_id: Option<String>,
    pub row_index: Option<i32>,
    pub transaction_date: Option<NaiveDate>,
    pub posting_date: Option<NaiveDate>,
    pub description_raw: Option<String>,
    pub merchant_raw: Option<String>,
    pub merchant_normalized: Option<String>,
    pub amount: Option<f64>,
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub direction: Option<String>,
    pub reference_id: Option<String>,
    pub location: Option<String>,
    pub raw_row_json: Option<serde_json::Value>,
    pub created_at: Option<NaiveDateTime>,
}

/// Insert one extracted statement line.
pub fn insert(conn: &Connection, entry: &StatementEntriesRow) -> Result<()> {
    conn.execute(
        "INSERT INTO statement_entries (
            id, statement_id, row_index, transaction_date, posting_date, description_raw,
            merchant_raw, merchant_normalized, amount, amount_minor, currency, direction,
            reference_id, location, raw_row_json, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            entry.id,
            entry.statement_id,
            entry.row_index,
            entry.transaction_date,
            entry.posting_date,
            entry.description_raw,
            entry.merchant_raw,
            entry.merchant_normalized,
            entry.amount,
            entry.amount_minor,
            entry.currency,
            entry.direction,
            entry.reference_id,
            entry.location,
            entry.raw_row_json,
            entry.created_at
        ],
    )?;
    Ok(())
}

/// All entries belonging to a statement, in extraction order.
pub fn select_by_statement_id(
    conn: &Connection,
    statement_id: &str,
) -> Result<Vec<StatementEntriesRow>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM statement_entries WHERE statement_id = ?1 ORDER BY row_index ASC",
    )?;
    let rows = stmt.query_map([statement_id], row_to_entry)?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }
    Ok(entries)
}

/// Update an entry, used when the user corrects a row during review.
pub fn update(conn: &Connection, entry: &StatementEntriesRow) -> Result<()> {
    conn.execute(
        "UPDATE statement_entries SET
            statement_id = ?2,
            row_index = ?3,
            transaction_date = ?4,
            posting_date = ?5,
            description_raw = ?6,
            merchant_raw = ?7,
            merchant_normalized = ?8,
            amount = ?9,
            amount_minor = ?10,
            currency = ?11,
            direction = ?12,
            reference_id = ?13,
            location = ?14,
            raw_row_json = ?15,
            created_at = ?16
         WHERE id = ?1",
        params![
            entry.id,
            entry.statement_id,
            entry.row_index,
            entry.transaction_date,
            entry.posting_date,
            entry.description_raw,
            entry.merchant_raw,
            entry.merchant_normalized,
            entry.amount,
            entry.amount_minor,
            entry.currency,
            entry.direction,
            entry.reference_id,
            entry.location,
            entry.raw_row_json,
            entry.created_at
        ],
    )?;
    Ok(())
}

/// Finds statement entries that may describe the same payment as an email alert.
///
/// Supports cross-referencing the two ingestion sources. A statement line and a
/// bank alert routinely describe one payment, and matching them prevents the same
/// transaction being recorded twice from different origins.
pub fn find_crossref_candidates(
    conn: &Connection,
    instrument_id: &str,
    anchor_date: NaiveDate,
    reference_id_fragment: Option<&str>,
    amount_minor: Option<i64>,
) -> Result<Vec<StatementEntriesRow>> {
    if reference_id_fragment.is_none() && amount_minor.is_none() {
        return Ok(Vec::new());
    }

    let window_start = anchor_date - chrono::Duration::days(3);
    let window_end = anchor_date + chrono::Duration::days(3);

    let mut stmt = conn.prepare(
        "SELECT se.* FROM statement_entries se \
         JOIN statements s ON s.id = se.statement_id \
         WHERE s.instrument_id = ?1 \
           AND se.transaction_date BETWEEN ?2 AND ?3 \
           AND ( \
             (?4 IS NOT NULL AND se.reference_id IS NOT NULL AND se.reference_id LIKE '%' || ?4 || '%') \
             OR (?5 IS NOT NULL AND se.amount_minor = ?5) \
           )",
    )?;

    let rows = stmt.query_map(
        params![
            instrument_id,
            window_start,
            window_end,
            reference_id_fragment,
            amount_minor
        ],
        row_to_entry,
    )?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }
    Ok(entries)
}

/// Maps a result row onto a statement entry.
fn row_to_entry(row: &Row) -> rusqlite::Result<StatementEntriesRow> {
    Ok(StatementEntriesRow {
        id: row.get("id")?,
        statement_id: row.get("statement_id")?,
        row_index: row.get("row_index")?,
        transaction_date: row.get("transaction_date")?,
        posting_date: row.get("posting_date")?,
        description_raw: row.get("description_raw")?,
        merchant_raw: row.get("merchant_raw")?,
        merchant_normalized: row.get("merchant_normalized")?,
        amount: row.get("amount")?,
        amount_minor: row.get("amount_minor")?,
        currency: row.get("currency")?,
        direction: row.get("direction")?,
        reference_id: row.get("reference_id")?,
        location: row.get("location")?,
        raw_row_json: row.get("raw_row_json")?,
        created_at: row.get("created_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = crate::db::test_helpers::setup_test_db();

        conn.execute("INSERT INTO local_profile (id) VALUES (1)", [])
            .unwrap_or_default();
        conn.execute("INSERT INTO instruments (id, type, issuer_name, masked_identifier, status) VALUES ('inst_1', 'credit_card', 'HDFC', '1234', 'active')", []).unwrap_or_default();
        conn.execute("INSERT INTO statements (id, instrument_id, statement_type, billing_period_start, billing_period_end, parse_status) VALUES ('stmt_1', 'inst_1', 'credit_card', '2023-01-01', '2023-01-31', 'parsed')", []).unwrap_or_default();

        conn
    }

    #[test]
    fn test_crud_statement_entries() {
        let conn = setup_db();

        let mut entry = StatementEntriesRow {
            id: "entry_1".into(),
            statement_id: Some("stmt_1".into()),
            row_index: Some(1),
            transaction_date: NaiveDate::from_ymd_opt(2023, 1, 15),
            posting_date: NaiveDate::from_ymd_opt(2023, 1, 16),
            description_raw: Some("Test Txn".into()),
            merchant_raw: Some("Test Merchant".into()),
            merchant_normalized: Some("Test Merchant Norm".into()),
            amount: Some(100.50),
            amount_minor: Some(10050),
            currency: Some("INR".into()),
            direction: Some("debit".into()),
            reference_id: Some("REF123".into()),
            location: Some("Mumbai".into()),
            raw_row_json: Some(serde_json::json!({"raw": "Test Txn"})),
            created_at: Some(chrono::Utc::now().naive_utc()),
        };

        insert(&conn, &entry).unwrap();

        let entries = select_by_statement_id(&conn, "stmt_1").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "entry_1");
        assert_eq!(entries[0].description_raw, Some("Test Txn".into()));

        entry.description_raw = Some("Updated Txn".into());
        entry.amount = Some(200.0);
        update(&conn, &entry).unwrap();

        let updated_entries = select_by_statement_id(&conn, "stmt_1").unwrap();
        assert_eq!(updated_entries.len(), 1);
        assert_eq!(
            updated_entries[0].description_raw,
            Some("Updated Txn".into())
        );
        assert_eq!(updated_entries[0].amount, Some(200.0));
    }
}
