//! Payment instruments -- the cards and accounts transactions belong to.
//!
//! `find_instrument_by_key` is the attribution path used during ingestion:
//! extraction recovers an issuer and a masked number, and this resolves that
//! pair to an existing instrument or reports that none matches, which is what
//! raises the instrument-confirmation prompt in the UI.
use crate::extraction::normalization::clean_masked_identifier;
use anyhow::Result;
use chrono::{NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InstrumentsRow {
    pub id: String,
    pub r#type: String,
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

/// Insert a new payment instrument.
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

/// Update an instrument's editable fields.
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

/// Fetch one instrument by id.
pub fn get_instrument(conn: &Connection, id: &str) -> Result<Option<InstrumentsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM instruments WHERE id = ?1 AND is_deleted = 0")?;
    let mut rows = stmt.query([id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_instrument(row)?))
    } else {
        Ok(None)
    }
}

/// Looks up an instrument by its natural key: type, issuer and masked identifier.
///
/// This is the attribution path used during ingestion. All three parts are
/// required because none is unique alone -- a user may hold two cards from the
/// same issuer, and the same last-four digits can recur across issuers.
pub fn find_instrument_by_key(
    conn: &Connection,
    instrument_type: &str,
    issuer_name: &str,
    masked_identifier: &str,
) -> Result<Option<String>> {
    let issuer_name = canonical_issuer(issuer_name);
    let masked_identifier = clean_masked_identifier(masked_identifier);
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

/// All live instruments.
pub fn get_all_instruments(conn: &Connection) -> Result<Vec<InstrumentsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM instruments WHERE is_deleted = 0")?;
    let rows = stmt.query_map([], row_to_instrument)?;

    let mut instruments = Vec::new();
    for row in rows {
        instruments.push(row?);
    }
    Ok(instruments)
}

/// One page of instruments.
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

/// Instruments with a billing date falling on or near the given day.
///
/// Drives both the dashboard's upcoming-bills widget and the daily reminder
/// notification.
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

/// Soft-delete an instrument.
///
/// Soft because transactions reference it: hard deletion would orphan those rows
/// and erase the account history they belong to.
pub fn delete_instrument(conn: &Connection, id: &str) -> Result<()> {
    let count = conn.execute(
        "UPDATE instruments SET is_deleted = 1 WHERE id = ?1 AND is_deleted = 0",
        params![id],
    )?;
    if count == 0 {
        return Err(anyhow::anyhow!("Instrument not found"));
    }
    Ok(())
}

/// Removes instrument rows created from malformed UPI identifiers.
///
/// Repairs damage from an earlier extraction bug that produced instruments keyed
/// on fragments of a VPA rather than the identifier itself.
pub fn cleanup_corrupted_vpa_instruments(conn: &Connection) -> Result<usize> {
    // Both steps or neither: soft-deleting the instruments without clearing
    // the transactions that point at them would leave dangling references.
    let tx = conn.unchecked_transaction()?;
    let conn: &Connection = &tx;
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
        conn.execute(
            "UPDATE transactions \
             SET instrument_id = NULL \
             WHERE instrument_id IN (SELECT id FROM instruments WHERE is_deleted = 1 AND (type = 'upi_vpa' OR upi_vpa IS NOT NULL))",
            [],
        )?;
    }
    tx.commit()?;

    Ok(count)
}

/// Maps a result row onto InstrumentsRow.
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

/// Canonical issuer key for an instrument (audit A-3).
///
/// Emails name the bank as the sender registry does ("HDFC Bank") and
/// statements as their header parser does ("HDFC"). Both go through this, so
/// one card resolves to one instrument whichever source sees it first. Exact
/// (case-insensitive) aliases only: an unknown name is kept as written rather
/// than guessed at.
pub fn canonical_issuer(name: &str) -> String {
    const ALIASES: &[(&str, &str)] = &[
        ("hdfc", "HDFC"),
        ("hdfc bank", "HDFC"),
        ("icici", "ICICI"),
        ("icici bank", "ICICI"),
        ("axis", "Axis"),
        ("axis bank", "Axis"),
        ("sbi", "SBI"),
        ("sbi card", "SBI"),
        ("state bank of india", "SBI"),
        ("kotak", "Kotak"),
        ("kotak mahindra", "Kotak"),
        ("kotak mahindra bank", "Kotak"),
        ("indusind", "IndusInd"),
        ("indusind bank", "IndusInd"),
        ("yes bank", "Yes Bank"),
        ("idfc", "IDFC"),
        ("idfc first", "IDFC"),
        ("idfc first bank", "IDFC"),
        ("amex", "AMEX"),
        ("american express", "AMEX"),
    ];
    let trimmed = name.trim();
    let lower = trimmed.to_lowercase();
    ALIASES
        .iter()
        .find(|(alias, _)| *alias == lower)
        .map(|(_, canonical)| canonical.to_string())
        .unwrap_or_else(|| trimmed.to_string())
}

/// The instrument for this key exists but the user archived it. New activity
/// is not booked against it, nor does it create a duplicate (audit A-2).
#[derive(Debug)]
pub struct InstrumentArchived {
    pub instrument_id: String,
}

impl std::fmt::Display for InstrumentArchived {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "instrument {} is archived", self.instrument_id)
    }
}

impl std::error::Error for InstrumentArchived {}

/// Returns the id of a matching instrument, creating one if none exists. The
/// single instrument resolver for email, mandate, Layer 6 and statements.
///
/// The issuer and identifier are canonicalised first, so the same account
/// written differently by two sources converges on one row. An archived match
/// is an `InstrumentArchived` error, never a silent link or a duplicate.
///
/// INSERT OR IGNORE followed by a SELECT makes this safe under concurrency: two
/// ingestion workers seeing the same new instrument both end up with the same id
/// instead of one failing on the unique constraint.
pub fn get_or_create_instrument(
    conn: &Connection,
    instrument_type: &str,
    issuer_name: &str,
    masked_identifier: &str,
    network: Option<&str>,
) -> Result<String> {
    let issuer_name = canonical_issuer(issuer_name);
    let masked_identifier = clean_masked_identifier(masked_identifier);
    let select = |conn: &Connection| {
        conn.query_row(
            "SELECT id, is_deleted FROM instruments
             WHERE type = ?1 AND issuer_name = ?2 AND masked_identifier = ?3",
            params![instrument_type, issuer_name, masked_identifier],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
        )
        .optional()
    };

    let found = match select(conn)? {
        Some(found) => found,
        None => {
            conn.execute(
                "INSERT OR IGNORE INTO instruments (
                     id, type, issuer_name, masked_identifier, network,
                     status, created_at, updated_at, is_deleted
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, 'active',
                         CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, 0)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    instrument_type,
                    issuer_name,
                    masked_identifier,
                    network,
                ],
            )?;
            // Another worker may have inserted between the SELECT and here.
            select(conn)?.ok_or_else(|| anyhow::anyhow!("instrument insert left no row"))?
        }
    };
    match found {
        (id, false) => Ok(id),
        (instrument_id, true) => Err(InstrumentArchived { instrument_id }.into()),
    }
}

/// Instrument resolution for an extracted observation.
///
/// Needs the full key; with any part missing there is no instrument and the
/// observation goes to review. Resolving from the issuer alone was removed:
/// it guessed (audit A-3, owner decision).
pub fn resolve_instrument(
    conn: &Connection,
    instrument_type: Option<&str>,
    issuer_name: Option<&str>,
    masked_identifier: Option<&str>,
    network: Option<&str>,
) -> Result<Option<String>> {
    match (instrument_type, issuer_name, masked_identifier) {
        (Some(itype), Some(iname), Some(masked)) => {
            get_or_create_instrument(conn, itype, iname, masked, network).map(Some)
        }
        _ => Ok(None),
    }
}

/// Tables whose `instrument_id` references `instruments.id`.
const INSTRUMENT_REFERENCES: &[&str] = &[
    "transactions",
    "transaction_observations",
    "statements",
    "statement_drafts",
    "pdf_passwords",
    "recurring_payments",
];

/// Rows whose stored key differs from the canonical one, with that key.
fn non_canonical_instruments(
    conn: &Connection,
) -> Result<Vec<(String, String, String, String, bool, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, type, issuer_name, masked_identifier, is_deleted, IFNULL(created_at, '')
         FROM instruments",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, bool>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Whether any instrument is stored under a non-canonical key.
pub fn needs_key_canonicalization(conn: &Connection) -> Result<bool> {
    Ok(non_canonical_instruments(conn)?.iter().any(|(_, _, issuer, masked, _, _)| {
        canonical_issuer(issuer) != *issuer || clean_masked_identifier(masked) != *masked
    }))
}

/// One-time repair for the split instrument keys (audit A-3, owner decision).
///
/// Rewrites every instrument to its canonical key and merges rows that then
/// collide: references move to one keeper (live before archived, then oldest),
/// fields the keeper lacks are copied from the duplicates, and the duplicates
/// are deleted. Already-canonical data is left untouched. Returns the number
/// of instruments merged away.
pub fn canonicalize_instrument_keys(conn: &Connection) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let rows = non_canonical_instruments(&tx)?;
    let mut groups: std::collections::BTreeMap<(String, String, String), Vec<(String, bool, String, bool)>> =
        Default::default();
    for (id, itype, issuer, masked, deleted, created) in rows {
        let key = (itype, canonical_issuer(&issuer), clean_masked_identifier(&masked));
        let canonical = key.1 == issuer && key.2 == masked;
        groups.entry(key).or_default().push((id, deleted, created, canonical));
    }

    let mut merged = 0;
    for ((_, issuer, masked), mut members) in groups {
        if members.len() == 1 && members[0].3 {
            continue;
        }
        // Live first, then the oldest; id breaks ties deterministically.
        members.sort_by(|a, b| a.1.cmp(&b.1).then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0)));
        let keeper = members[0].0.clone();
        for (dup, _, _, _) in &members[1..] {
            for table in INSTRUMENT_REFERENCES {
                tx.execute(
                    &format!("UPDATE {table} SET instrument_id = ?1 WHERE instrument_id = ?2"),
                    params![keeper, dup],
                )?;
            }
            tx.execute(
                "UPDATE instruments SET
                     network = COALESCE(network, (SELECT network FROM instruments WHERE id = ?2)),
                     credit_limit = COALESCE(credit_limit, (SELECT credit_limit FROM instruments WHERE id = ?2)),
                     current_balance = COALESCE(current_balance, (SELECT current_balance FROM instruments WHERE id = ?2)),
                     statement_due_date = COALESCE(statement_due_date, (SELECT statement_due_date FROM instruments WHERE id = ?2)),
                     minimum_due = COALESCE(minimum_due, (SELECT minimum_due FROM instruments WHERE id = ?2)),
                     bank_ifsc = COALESCE(bank_ifsc, (SELECT bank_ifsc FROM instruments WHERE id = ?2)),
                     account_type = COALESCE(account_type, (SELECT account_type FROM instruments WHERE id = ?2)),
                     upi_vpa = COALESCE(upi_vpa, (SELECT upi_vpa FROM instruments WHERE id = ?2)),
                     nickname = COALESCE(nickname, (SELECT nickname FROM instruments WHERE id = ?2)),
                     rewards_summary = COALESCE(rewards_summary, (SELECT rewards_summary FROM instruments WHERE id = ?2)),
                     full_identifier = COALESCE(full_identifier, (SELECT full_identifier FROM instruments WHERE id = ?2)),
                     billing_cycle_day = COALESCE(billing_cycle_day, (SELECT billing_cycle_day FROM instruments WHERE id = ?2))
                 WHERE id = ?1",
                params![keeper, dup],
            )?;
            tx.execute("DELETE FROM instruments WHERE id = ?1", params![dup])?;
            merged += 1;
        }
        tx.execute(
            "UPDATE instruments SET issuer_name = ?2, masked_identifier = ?3 WHERE id = ?1",
            params![keeper, issuer, masked],
        )?;
    }
    tx.commit()?;
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_instrument_linked_from_existing_record() {
        let conn = setup_test_db();

        let existing_id = "aaaa-bbbb-cccc-dddd";
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier, status, created_at, updated_at, is_deleted)
             VALUES (?1, 'credit_card', 'HDFC', '1234', 'active', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, 0)",
            rusqlite::params![existing_id],
        )
        .unwrap();

        let returned_id =
            get_or_create_instrument(&conn, "credit_card", "HDFC Bank", "XXXX1234", None).unwrap();

        assert_eq!(
            returned_id, existing_id,
            "Expected existing instrument id to be returned"
        );

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "No duplicate should have been created");
    }

    #[test]
    fn test_instrument_auto_created_for_new_card() {
        let conn = setup_test_db();

        let id =
            get_or_create_instrument(&conn, "credit_card", "ICICI Bank", "XXXX5678", Some("Visa"))
                .unwrap();

        assert!(!id.is_empty(), "Returned id should not be empty");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM instruments WHERE type = 'credit_card' AND issuer_name = 'ICICI' AND masked_identifier = '5678'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 1,
            "Exactly one instrument row should have been created"
        );

        let network: Option<String> = conn
            .query_row(
                "SELECT network FROM instruments WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(network, Some("Visa".to_string()));
    }

    #[test]
    fn test_instrument_unique_constraint_enforced() {
        let conn = setup_test_db();

        let id1 =
            get_or_create_instrument(&conn, "bank_account", "Axis Bank", "XXXX9999", None).unwrap();

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

    /// Audit A-3: an email alert ("HDFC Bank", "XXXX1234") and a statement
    /// ("HDFC", "1234") for the same card resolve to one instrument.
    #[test]
    fn email_and_statement_spellings_share_one_instrument() {
        let conn = setup_test_db();
        let from_email =
            get_or_create_instrument(&conn, "credit_card", "HDFC Bank", "XXXX1234", None).unwrap();
        let from_statement =
            get_or_create_instrument(&conn, "credit_card", "HDFC", "1234", None).unwrap();
        assert_eq!(from_email, from_statement);
        assert_eq!(canonical_issuer("State Bank of India"), "SBI");
        assert_eq!(canonical_issuer("SBI Card"), "SBI");
        assert_eq!(canonical_issuer(" Citibank India "), "Citibank India", "unknown names are kept");
        assert_eq!(
            find_instrument_by_key(&conn, "credit_card", "HDFC Bank", "xx1234").unwrap(),
            Some(from_email)
        );
    }

    /// Audit A-2/A-3: an archived instrument is reported, never linked to
    /// silently and never duplicated.
    #[test]
    fn archived_instrument_is_reported_not_duplicated() {
        let conn = setup_test_db();
        let id = get_or_create_instrument(&conn, "credit_card", "HDFC", "1234", None).unwrap();
        delete_instrument(&conn, &id).unwrap();
        let err = get_or_create_instrument(&conn, "credit_card", "HDFC Bank", "XXXX1234", None)
            .unwrap_err();
        let archived = err.downcast_ref::<InstrumentArchived>().expect("typed archived error");
        assert_eq!(archived.instrument_id, id);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
    }

    /// Owner decision on A-3: the issuer alone never resolves an instrument.
    #[test]
    fn issuer_alone_does_not_resolve_an_instrument() {
        let conn = setup_test_db();
        get_or_create_instrument(&conn, "credit_card", "HDFC", "1234", None).unwrap();
        assert_eq!(resolve_instrument(&conn, None, Some("HDFC Bank"), None, None).unwrap(), None);
    }

    /// Audit A-3: the one-time repair merges split instruments and moves
    /// every reference to the keeper.
    #[tokio::test]
    async fn split_instruments_are_merged_with_their_references() {
        let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = crate::db::init_db(dir.join("t.db")).await.unwrap();
        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            c.execute_batch(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier, created_at, nickname)
                     VALUES ('email', 'credit_card', 'HDFC Bank', '1234', '2026-01-01', NULL),
                            ('stmt', 'credit_card', 'HDFC', 'XXXX1234', '2026-02-01', 'Regalia'),
                            ('solo', 'bank_account', 'Axis Bank', '9999', '2026-01-01', NULL),
                            ('other', 'credit_card', 'Citibank India', '5555', '2026-01-01', NULL);
                 INSERT INTO pdf_passwords (id, instrument_id, password_ciphertext) VALUES ('s1', 'stmt', x'00');",
            )?;
            assert!(needs_key_canonicalization(c).unwrap());
            assert_eq!(canonicalize_instrument_keys(c).unwrap(), 1);
            assert!(!needs_key_canonicalization(c).unwrap());
            assert_eq!(canonicalize_instrument_keys(c).unwrap(), 0, "idempotent");
            let rows: Vec<(String, String, String, Option<String>)> = c
                .prepare("SELECT id, issuer_name, masked_identifier, nickname FROM instruments ORDER BY id")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect::<rusqlite::Result<_>>()?;
            assert_eq!(
                rows,
                vec![
                    ("email".into(), "HDFC".into(), "1234".into(), Some("Regalia".into())),
                    ("other".into(), "Citibank India".into(), "5555".into(), None),
                    ("solo".into(), "Axis".into(), "9999".into(), None),
                ]
            );
            let moved: String =
                c.query_row("SELECT instrument_id FROM pdf_passwords WHERE id = 's1'", [], |r| r.get(0))?;
            assert_eq!(moved, "email");
            Ok::<_, rusqlite::Error>(())
        })
        .await
        .unwrap()
        .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn insert_with_due_date(conn: &Connection, id: &str, due_date: Option<&str>) {
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier, statement_due_date, is_deleted) \
             VALUES (?1, 'credit_card', 'Test Bank', ?1, ?2, 0)",
            params![id, due_date],
        )
        .unwrap();
    }

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

    #[test]
    fn test_instruments_update_rejects_identity_field_changes() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier, billing_cycle_day) \
             VALUES ('inst_1', 'credit_card', 'HDFC Bank', '1234', 5)",
            [],
        )
        .unwrap();

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
