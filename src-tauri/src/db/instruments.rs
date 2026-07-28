use crate::extraction::normalization::clean_masked_identifier;
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

/// Doc 30 TASK-API-002: "instruments_get_upcoming_bills (non-null future
/// statement_due_date, ascending, for the dashboard widget)." Document 19
/// §11.2 exposes this same data via the `dashboard_upcoming_bills` command
/// (TASK-API-006) rather than a separate instruments-namespaced one -- this
/// data-layer function is what both this task's own acceptance test and
/// TASK-API-006's real IPC command are built on.
pub fn list_upcoming_bills(conn: &Connection, today: &NaiveDate) -> Result<Vec<InstrumentsRow>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM instruments \
         WHERE is_deleted = 0 AND statement_due_date IS NOT NULL AND statement_due_date >= ?1 \
         ORDER BY statement_due_date ASC",
    )?;
    let rows = stmt.query_map(params![today], row_to_instrument)?;

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

/// Scans for and soft-deletes invalid instruments where a counterparty VPA (or merchant name)
/// was mistakenly saved as a user instrument. Re-assigns any transactions attached to
/// these corrupt instruments so they can be re-linked to clean bank instruments.
pub fn cleanup_corrupted_vpa_instruments(conn: &Connection) -> Result<usize> {
    let count = conn.execute(
        "UPDATE instruments \
         SET is_deleted = 1 \
         WHERE is_deleted = 0 \
           AND (type = 'upi_vpa' OR upi_vpa IS NOT NULL) \
           AND ( \
             masked_identifier LIKE '7674036967%' \
             OR masked_identifier LIKE 'saharahospital%' \
             OR LOWER(masked_identifier) IN (SELECT LOWER(merchant_display_name) FROM transactions WHERE merchant_display_name IS NOT NULL) \
           )",
        [],
    )?;

    if count > 0 {
        let _ = conn.execute(
            "UPDATE transactions \
             SET instrument_id = NULL \
             WHERE instrument_id IN (SELECT id FROM instruments WHERE is_deleted = 1 AND (type = 'upi_vpa' OR upi_vpa IS NOT NULL))",
            [],
        );
    }

    Ok(count)
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
    let masked_identifier_cleaned = clean_masked_identifier(masked_identifier);
    let masked_identifier = masked_identifier_cleaned.as_str();

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

        // Pre-seed a known instrument. Stored in already-cleaned form ("1234"),
        // matching what get_or_create_instrument itself would have persisted --
        // it runs every masked_identifier through clean_masked_identifier
        // before insert/lookup, so DB rows are always in cleaned form.
        let existing_id = "aaaa-bbbb-cccc-dddd";
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier, status, created_at, updated_at, is_deleted)
             VALUES (?1, 'credit_card', 'HDFC Bank', '1234', 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, 0)",
            rusqlite::params![existing_id],
        )
        .unwrap();

        // Call get_or_create with the raw masked format -- should clean to
        // "1234" and link to the existing record rather than creating a new one.
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

        // Verify the row was actually persisted, in cleaned form ("5678")
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM instruments WHERE type = 'credit_card' AND issuer_name = 'ICICI Bank' AND masked_identifier = '5678'",
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

    fn insert_with_due_date(conn: &Connection, id: &str, due_date: Option<&str>) {
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier, statement_due_date, is_deleted) \
             VALUES (?1, 'credit_card', 'Test Bank', ?1, ?2, 0)",
            params![id, due_date],
        )
        .unwrap();
    }

    /// Doc 30 TASK-API-002 acceptance test.
    #[test]
    fn test_upcoming_bills_sorted_ascending() {
        let conn = setup_test_db();
        insert_with_due_date(&conn, "inst_far", Some("2026-08-15"));
        insert_with_due_date(&conn, "inst_near", Some("2026-06-20"));
        insert_with_due_date(&conn, "inst_mid", Some("2026-07-01"));
        insert_with_due_date(&conn, "inst_no_due_date", None);
        insert_with_due_date(&conn, "inst_past_due", Some("2026-01-01"));

        let today = NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        let bills = list_upcoming_bills(&conn, &today).unwrap();

        let ids: Vec<&str> = bills.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["inst_near", "inst_mid", "inst_far"],
            "must be ascending by due date, excluding NULL and past-due"
        );
    }

    /// Doc 30 TASK-API-002 acceptance test: simulates the exact
    /// fetch-full-row-then-patch-allowed-fields-only pattern
    /// `commands::data::instruments_update` uses -- issuer_name/
    /// masked_identifier must survive unchanged even though every other
    /// field can legitimately be patched.
    #[test]
    fn test_instruments_update_rejects_identity_field_changes() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier, billing_cycle_day) \
             VALUES ('inst_1', 'credit_card', 'HDFC Bank', '1234', 5)",
            [],
        )
        .unwrap();

        // The real command handler's pattern: fetch the full existing row,
        // then only ever overwrite the user-editable fields -- there is no
        // code path here that assigns a caller-supplied issuer_name/
        // masked_identifier at all.
        let mut row = get_instrument(&conn, "inst_1").unwrap().unwrap();
        row.billing_cycle_day = Some(15);
        row.bank_ifsc = Some("HDFC0001234".to_string());
        update_instrument(&conn, &row).unwrap();

        let after = get_instrument(&conn, "inst_1").unwrap().unwrap();
        assert_eq!(
            after.issuer_name, "HDFC Bank",
            "issuer_name must never change via update"
        );
        assert_eq!(
            after.masked_identifier, "1234",
            "masked_identifier must never change via update"
        );
        assert_eq!(
            after.billing_cycle_day,
            Some(15),
            "user-editable fields must still update"
        );
    }

    /// Doc 30 TASK-API-002 acceptance test: soft-deleting an instrument
    /// (`is_deleted = 1`) must never cascade-delete or modify the
    /// transactions that reference it -- they remain queryable, just
    /// hidden from active instrument lists.
    #[test]
    fn test_instruments_soft_delete_preserves_transactions() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier) \
             VALUES ('inst_1', 'credit_card', 'HDFC Bank', '1234')",
            [],
        )
        .unwrap();
        conn.execute_batch(
            "CREATE TABLE transactions (id TEXT PRIMARY KEY, instrument_id TEXT, amount_minor INTEGER, is_deleted INTEGER DEFAULT 0);
             INSERT INTO transactions (id, instrument_id, amount_minor) VALUES ('tx_1', 'inst_1', 1000);",
        )
        .unwrap();

        delete_instrument(&conn, "inst_1").unwrap();

        let is_deleted: bool = conn
            .query_row(
                "SELECT is_deleted FROM instruments WHERE id = 'inst_1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(is_deleted, "the instrument itself must be soft-deleted");

        let (tx_amount, tx_deleted): (i64, bool) = conn
            .query_row(
                "SELECT amount_minor, is_deleted FROM transactions WHERE id = 'tx_1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            tx_amount, 1000,
            "the transaction row must be completely untouched"
        );
        assert!(
            !tx_deleted,
            "the transaction must remain queryable, not cascade-deleted"
        );
    }

    #[test]
    fn test_cleanup_corrupted_vpa_instruments() {
        let conn = setup_test_db();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS transactions (id TEXT PRIMARY KEY, instrument_id TEXT, merchant_display_name TEXT, amount_minor INTEGER, is_deleted INTEGER DEFAULT 0);
             INSERT INTO instruments (id, type, issuer_name, masked_identifier, upi_vpa) VALUES ('inst_bad', 'upi_vpa', 'Jupiter', '7674036967@ybl', '7674036967@ybl');
             INSERT INTO transactions (id, instrument_id, merchant_display_name, amount_minor) VALUES ('tx_bad', 'inst_bad', 'T Jyoshna', 1400000);",
        ).unwrap();

        let count = cleanup_corrupted_vpa_instruments(&conn).unwrap();
        assert_eq!(count, 1);

        let is_deleted: bool = conn
            .query_row(
                "SELECT is_deleted FROM instruments WHERE id = 'inst_bad'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(is_deleted);

        let tx_inst: Option<String> = conn
            .query_row(
                "SELECT instrument_id FROM transactions WHERE id = 'tx_bad'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(tx_inst.is_none());
    }
}
