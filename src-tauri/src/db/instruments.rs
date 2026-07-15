use anyhow::Result;
use chrono::{NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InstrumentsRow {
    pub id: String,
    pub r#type: String, // reserved keyword
    pub issuer_name: String,
    pub masked_identifier: String,
    pub network: Option<String>,
    pub credit_limit: Option<i64>,
    pub current_balance: Option<i64>,
    pub statement_due_date: Option<NaiveDate>,
    pub minimum_due: Option<i64>,
    pub bank_ifsc: Option<String>,
    pub account_type: Option<String>,
    pub upi_vpa: Option<String>,
    pub nickname: Option<String>,
    pub rewards_summary: Option<String>,
    pub status: String,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
    pub is_deleted: bool,
    pub full_identifier: Option<String>,
    pub billing_cycle_day: Option<u8>,
}

pub fn insert_instrument(conn: &Connection, instrument: &InstrumentsRow) -> Result<()> {
    conn.execute(
        "INSERT INTO instruments (
            id, type, issuer_name, masked_identifier, network, credit_limit, current_balance,
            statement_due_date, minimum_due, bank_ifsc, account_type, upi_vpa, nickname,
            rewards_summary, status, created_at, updated_at, is_deleted, full_identifier, billing_cycle_day
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, COALESCE(?16, CURRENT_TIMESTAMP), COALESCE(?17, CURRENT_TIMESTAMP), ?18, ?19, ?20)",
        params![
            instrument.id,
            instrument.r#type,
            instrument.issuer_name,
            instrument.masked_identifier,
            instrument.network,
            instrument.credit_limit,
            instrument.current_balance,
            instrument.statement_due_date,
            instrument.minimum_due,
            instrument.bank_ifsc,
            instrument.account_type,
            instrument.upi_vpa,
            instrument.nickname,
            instrument.rewards_summary,
            instrument.status,
            instrument.created_at,
            instrument.updated_at,
            instrument.is_deleted,
            instrument.full_identifier,
            instrument.billing_cycle_day,
        ],
    )?;
    Ok(())
}

pub fn update_instrument(conn: &Connection, instrument: &InstrumentsRow) -> Result<()> {
    let count = conn.execute(
        "UPDATE instruments SET
            type = ?2,
            issuer_name = ?3,
            masked_identifier = ?4,
            network = ?5,
            credit_limit = ?6,
            current_balance = ?7,
            statement_due_date = ?8,
            minimum_due = ?9,
            bank_ifsc = ?10,
            account_type = ?11,
            upi_vpa = ?12,
            nickname = ?13,
            rewards_summary = ?14,
            status = ?15,
            is_deleted = ?16,
            full_identifier = ?17,
            billing_cycle_day = ?18
         WHERE id = ?1",
        params![
            instrument.id,
            instrument.r#type,
            instrument.issuer_name,
            instrument.masked_identifier,
            instrument.network,
            instrument.credit_limit,
            instrument.current_balance,
            instrument.statement_due_date,
            instrument.minimum_due,
            instrument.bank_ifsc,
            instrument.account_type,
            instrument.upi_vpa,
            instrument.nickname,
            instrument.rewards_summary,
            instrument.status,
            instrument.is_deleted,
            instrument.full_identifier,
            instrument.billing_cycle_day,
        ],
    )?;
    if count == 0 {
        return Err(anyhow::anyhow!("Instrument not found"));
    }
    Ok(())
}

pub fn get_instrument(conn: &Connection, id: &str) -> Result<Option<InstrumentsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM instruments WHERE id = ?1 AND is_deleted = 0")?;
    let mut rows = stmt.query([id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_instrument(row)?))
    } else {
        Ok(None)
    }
}

/// Doc 30 TASK-TXN-005: a read-only lookup by the same `(type, issuer_name,
/// masked_identifier)` key `get_or_create_instrument` uses, but never
/// creates a row. Layer 5 (statement cross-reference) needs to know whether
/// an instrument is already known *without* speculatively creating one from
/// a single ambiguous, still-unextracted email — that would violate Doc 15
/// §2 principle 8 (never guess/prematurely create an instrument). If no
/// statement has ever been processed for this instrument, none can exist to
/// cross-reference against either, so a clean `None` here is the correct,
/// safe outcome for the caller to fall through on.
pub fn find_instrument_by_key(
    conn: &Connection,
    instrument_type: &str,
    issuer_name: &str,
    masked_identifier: &str,
) -> Result<Option<String>> {
    let id: Option<String> = conn
        .query_row(
            "SELECT id FROM instruments \
             WHERE type = ?1 AND issuer_name = ?2 AND masked_identifier = ?3 AND is_deleted = 0 \
             LIMIT 1",
            params![instrument_type, issuer_name, masked_identifier],
            |row| row.get(0),
        )
        .optional()?;
    Ok(id)
}

pub fn get_all_instruments(conn: &Connection) -> Result<Vec<InstrumentsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM instruments WHERE is_deleted = 0")?;
    let rows = stmt.query_map([], row_to_instrument)?;

    let mut instruments = Vec::new();
    for row in rows {
        instruments.push(row?);
    }
    Ok(instruments)
}

pub fn get_paginated_instruments(
    conn: &Connection,
    limit: i64,
    offset: i64,
) -> Result<Vec<InstrumentsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM instruments WHERE is_deleted = 0 ORDER BY created_at DESC LIMIT ?1 OFFSET ?2")?;
    let rows = stmt.query_map(params![limit, offset], row_to_instrument)?;

    let mut instruments = Vec::new();
    for row in rows {
        instruments.push(row?);
    }
    Ok(instruments)
}

pub fn delete_instrument(conn: &Connection, id: &str) -> Result<()> {
    // Soft delete
    let count = conn.execute(
        "UPDATE instruments SET is_deleted = 1 WHERE id = ?1 AND is_deleted = 0",
        params![id],
    )?;
    if count == 0 {
        return Err(anyhow::anyhow!("Instrument not found"));
    }
    Ok(())
}

fn row_to_instrument(row: &Row) -> rusqlite::Result<InstrumentsRow> {
    Ok(InstrumentsRow {
        id: row.get("id")?,
        r#type: row.get("type")?,
        issuer_name: row.get("issuer_name")?,
        masked_identifier: row.get("masked_identifier")?,
        network: row.get("network")?,
        credit_limit: row.get("credit_limit")?,
        current_balance: row.get("current_balance")?,
        statement_due_date: row.get("statement_due_date")?,
        minimum_due: row.get("minimum_due")?,
        bank_ifsc: row.get("bank_ifsc")?,
        account_type: row.get("account_type")?,
        upi_vpa: row.get("upi_vpa")?,
        nickname: row.get("nickname")?,
        rewards_summary: row.get("rewards_summary")?,
        status: row.get("status")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        is_deleted: row.get("is_deleted")?,
        full_identifier: row.get("full_identifier")?,
        billing_cycle_day: row.get("billing_cycle_day")?,
    })
}

/// Finds an existing instrument by `(type, issuer_name, masked_identifier)` or auto-creates one.
///
/// Uses the `INSERT OR IGNORE` + subsequent `SELECT` pattern to ensure atomicity without
/// relying on application-level locking.
///
/// # Returns
/// The `id` (UUID string) of the matched or newly created instrument.
pub fn get_or_create_instrument(
    conn: &Connection,
    instrument_type: &str,
    issuer_name: &str,
    masked_identifier: &str,
    network: Option<&str>,
) -> Result<String> {
    // 1. Attempt to INSERT OR IGNORE a new row. If a row with the same
    //    (type, issuer_name, masked_identifier) already exists the UNIQUE constraint
    //    fires but the error is silently ignored due to `OR IGNORE`.
    let new_id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT OR IGNORE INTO instruments (
             id, type, issuer_name, masked_identifier, network,
             status, created_at, updated_at, is_deleted
         )
         VALUES (?1, ?2, ?3, ?4, ?5, 'active',
                 CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, 0)",
        rusqlite::params![
            new_id,
            instrument_type,
            issuer_name,
            masked_identifier,
            network,
        ],
    )?;

    // 2. Retrieve the id of the row that actually won the race — either our freshly
    //    inserted row or a pre-existing one.
    let id: String = conn.query_row(
        "SELECT id FROM instruments
         WHERE type = ?1
           AND issuer_name = ?2
           AND masked_identifier = ?3
           AND is_deleted = 0
         LIMIT 1",
        rusqlite::params![instrument_type, issuer_name, masked_identifier],
        |row| row.get(0),
    )?;

    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates an in-memory SQLite connection with the minimal `instruments` schema.
    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("Failed to open in-memory DB");
        conn.execute_batch(
            "CREATE TABLE instruments (
                id                  TEXT PRIMARY KEY,
                type                TEXT NOT NULL,
                issuer_name         TEXT NOT NULL,
                masked_identifier   TEXT NOT NULL,
                network             TEXT,
                credit_limit        INTEGER,
                current_balance     INTEGER,
                statement_due_date  TEXT,
                minimum_due         INTEGER,
                bank_ifsc           TEXT,
                account_type        TEXT,
                upi_vpa             TEXT,
                nickname            TEXT,
                rewards_summary     TEXT,
                status              TEXT NOT NULL DEFAULT 'active',
                created_at          TEXT,
                updated_at          TEXT,
                is_deleted          INTEGER NOT NULL DEFAULT 0,
                full_identifier     TEXT,
                billing_cycle_day   INTEGER,
                UNIQUE (type, issuer_name, masked_identifier)
            );",
        )
        .expect("Failed to create instruments table");
        conn
    }

    /// Pre-inserts an instrument record and verifies that `get_or_create_instrument`
    /// returns the existing id instead of creating a duplicate.
    #[test]
    fn test_instrument_linked_from_existing_record() {
        let conn = setup_test_db();

        // Pre-seed a known instrument
        let existing_id = "aaaa-bbbb-cccc-dddd";
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier, status, created_at, updated_at, is_deleted)
             VALUES (?1, 'credit_card', 'HDFC Bank', 'XXXX1234', 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, 0)",
            rusqlite::params![existing_id],
        )
        .unwrap();

        // Call get_or_create — should link to the existing record
        let returned_id =
            get_or_create_instrument(&conn, "credit_card", "HDFC Bank", "XXXX1234", None).unwrap();

        assert_eq!(
            returned_id, existing_id,
            "Expected existing instrument id to be returned"
        );

        // Confirm only one row exists
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "No duplicate should have been created");
    }

    /// Verifies that calling `get_or_create_instrument` for a brand-new card creates
    /// a fresh row and returns a valid UUID.
    #[test]
    fn test_instrument_auto_created_for_new_card() {
        let conn = setup_test_db();

        let id =
            get_or_create_instrument(&conn, "credit_card", "ICICI Bank", "XXXX5678", Some("Visa"))
                .unwrap();

        // Must be a non-empty string (UUID)
        assert!(!id.is_empty(), "Returned id should not be empty");

        // Verify the row was actually persisted
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM instruments WHERE type = 'credit_card' AND issuer_name = 'ICICI Bank' AND masked_identifier = 'XXXX5678'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 1,
            "Exactly one instrument row should have been created"
        );

        // Network should be stored
        let network: Option<String> = conn
            .query_row(
                "SELECT network FROM instruments WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(network, Some("Visa".to_string()));
    }

    /// Verifies that two calls with the same `(type, issuer_name, masked_identifier)` tuple
    /// produce exactly one row in the database (UNIQUE constraint enforced).
    #[test]
    fn test_instrument_unique_constraint_enforced() {
        let conn = setup_test_db();

        let id1 =
            get_or_create_instrument(&conn, "bank_account", "Axis Bank", "XXXX9999", None).unwrap();

        // Second call — must return same id, not create a new row
        let id2 =
            get_or_create_instrument(&conn, "bank_account", "Axis Bank", "XXXX9999", None).unwrap();

        assert_eq!(
            id1, id2,
            "Both calls must resolve to the same instrument id"
        );

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "Unique constraint must prevent duplicate rows");
    }
}
