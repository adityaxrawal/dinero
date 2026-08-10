//! Imported statement documents and their processing state.
//!
//! Rows are inserted queued and progress through parsing, so a statement is
//! tracked from the moment it arrives -- including one that fails, which stays
//! visible and retryable rather than disappearing.
use anyhow::Result;
use chrono::{NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StatementsRow {
    pub id: String,
    pub instrument_id: Option<String>,
    pub statement_type: String,
    pub source_type: Option<String>,
    pub billing_period_start: NaiveDate,
    pub billing_period_end: NaiveDate,
    pub due_date: Option<NaiveDate>,
    pub statement_date: Option<NaiveDate>,
    pub current_balance: Option<i64>,
    pub minimum_due: Option<i64>,
    pub rewards_summary_json: Option<String>,
    pub source_message_id: Option<String>,
    pub parse_status: String,
    pub is_duplicate: bool,
    pub file_hash: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Records a statement as queued, before parsing begins.
///
/// Inserting up front means a statement is tracked from arrival, so one that
/// fails during parsing remains visible and retryable rather than vanishing.
pub fn insert_queued(
    conn: &Connection,
    id: &str,
    source_type: &str,
    source_message_id: Option<&str>,
    file_hash: Option<&str>,
) -> Result<()> {
    let today = chrono::Utc::now().date_naive();
    conn.execute(
        "INSERT INTO statements (
            id, instrument_id, statement_type, source_type, billing_period_start, billing_period_end,
            due_date, statement_date, current_balance, minimum_due, rewards_summary_json,
            source_message_id, parse_status, is_duplicate, file_hash, created_at, updated_at
         ) VALUES (
            ?1, NULL, 'credit_card_statement', ?2, ?3, ?3,
            NULL, NULL, NULL, NULL, NULL,
            ?4, 'queued', 0, ?5, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
         )",
        params![id, source_type, today, source_message_id, file_hash],
    )?;
    Ok(())
}

/// Insert a fully populated statement row.
///
/// Used where the statement's metadata is already known, as opposed to
/// `insert_queued`, which records one before parsing has established any of it.
pub fn insert(conn: &Connection, stmt: &StatementsRow) -> Result<()> {
    conn.execute(
        "INSERT INTO statements (
            id, instrument_id, statement_type, source_type, billing_period_start, billing_period_end,
            due_date, statement_date, current_balance, minimum_due, rewards_summary_json,
            source_message_id, parse_status, is_duplicate, file_hash, created_at, updated_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, COALESCE(?16, CURRENT_TIMESTAMP), COALESCE(?17, CURRENT_TIMESTAMP)
         )",
        params![
            stmt.id, stmt.instrument_id, stmt.statement_type, stmt.source_type, stmt.billing_period_start,
            stmt.billing_period_end, stmt.due_date, stmt.statement_date, stmt.current_balance,
            stmt.minimum_due, stmt.rewards_summary_json, stmt.source_message_id, stmt.parse_status,
            stmt.is_duplicate, stmt.file_hash, stmt.created_at, stmt.updated_at
        ],
    )?;
    Ok(())
}

/// Update a statement's metadata or processing state.
pub fn update(conn: &Connection, stmt: &StatementsRow) -> Result<()> {
    let count = conn.execute(
        "UPDATE statements SET
            instrument_id = ?2, statement_type = ?3, source_type = ?4, billing_period_start = ?5,
            billing_period_end = ?6, due_date = ?7, statement_date = ?8, current_balance = ?9,
            minimum_due = ?10, rewards_summary_json = ?11, source_message_id = ?12,
            parse_status = ?13, is_duplicate = ?14, file_hash = ?15, updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![
            stmt.id,
            stmt.instrument_id,
            stmt.statement_type,
            stmt.source_type,
            stmt.billing_period_start,
            stmt.billing_period_end,
            stmt.due_date,
            stmt.statement_date,
            stmt.current_balance,
            stmt.minimum_due,
            stmt.rewards_summary_json,
            stmt.source_message_id,
            stmt.parse_status,
            stmt.is_duplicate,
            stmt.file_hash,
        ],
    )?;
    if count == 0 {
        return Err(anyhow::anyhow!("Statement not found"));
    }
    Ok(())
}

/// Fetch one statement.
pub fn select_by_id(conn: &Connection, id: &str) -> Result<Option<StatementsRow>> {
    let mut db_stmt = conn.prepare("SELECT * FROM statements WHERE id = ?1")?;
    let mut rows = db_stmt.query([id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_statement(row)?))
    } else {
        Ok(None)
    }
}

/// One page of statement history.
pub fn select_all_paginated(
    conn: &Connection,
    limit: i64,
    offset: i64,
) -> Result<Vec<StatementsRow>> {
    let mut db_stmt =
        conn.prepare("SELECT * FROM statements ORDER BY created_at DESC LIMIT ?1 OFFSET ?2")?;
    let rows = db_stmt.query_map(params![limit, offset], row_to_statement)?;

    let mut statements = Vec::new();
    for row in rows {
        statements.push(row?);
    }
    Ok(statements)
}

/// Soft-delete a statement, preserving the entries derived from it.
pub fn soft_delete(conn: &Connection, id: &str) -> Result<()> {
    let count = conn.execute("DELETE FROM statements WHERE id = ?1", params![id])?;
    if count == 0 {
        return Err(anyhow::anyhow!("Statement not found"));
    }
    Ok(())
}

/// Maps a result row onto a statement record.
fn row_to_statement(row: &Row) -> rusqlite::Result<StatementsRow> {
    Ok(StatementsRow {
        id: row.get("id")?,
        instrument_id: row.get("instrument_id")?,
        statement_type: row.get("statement_type")?,
        source_type: row.get("source_type")?,
        billing_period_start: row.get("billing_period_start")?,
        billing_period_end: row.get("billing_period_end")?,
        due_date: row.get("due_date")?,
        statement_date: row.get("statement_date")?,
        current_balance: row.get("current_balance")?,
        minimum_due: row.get("minimum_due")?,
        rewards_summary_json: row.get("rewards_summary_json")?,
        source_message_id: row.get("source_message_id")?,
        parse_status: row.get("parse_status")?,
        is_duplicate: row.get("is_duplicate")?,
        file_hash: row.get("file_hash")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}
