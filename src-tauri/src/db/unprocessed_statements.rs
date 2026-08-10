//! Statements that failed to parse, with the reason and retry state.
//!
//! Password attempts are counted so repeated failures against an encrypted PDF
//! are visible and bounded rather than retried indefinitely.
use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UnprocessedStatementRow {
    pub id: String,
    pub statement_source_json: String,
    pub failure_type: String,
    pub failure_reason: String,
    pub status: String,
    pub resolved_statement_id: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Records a statement that failed to process, with its reason.
pub fn insert_unprocessed_statement(
    conn: &Connection,
    stmt: &UnprocessedStatementRow,
) -> Result<()> {
    conn.execute(
        "INSERT INTO unprocessed_statements (
            id, statement_source_json, failure_type, failure_reason, status, resolved_statement_id
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6
        )",
        params![
            stmt.id,
            stmt.statement_source_json,
            stmt.failure_type,
            stmt.failure_reason,
            stmt.status,
            stmt.resolved_statement_id,
        ],
    )?;
    Ok(())
}

/// Update status as a retry succeeds or fails again.
pub fn update_status(
    conn: &Connection,
    id: &str,
    new_status: &str,
    resolved_statement_id: Option<&str>,
) -> Result<()> {
    if new_status == "resolved" || new_status == "failed" {
        conn.execute(
            "UPDATE unprocessed_statements
             SET status = ?1, resolved_statement_id = ?2, pdf_retained_until = datetime('now', '+30 days')
             WHERE id = ?3",
            params![new_status, resolved_statement_id, id],
        )?;
    } else {
        conn.execute(
            "UPDATE unprocessed_statements
             SET status = ?1, resolved_statement_id = ?2
             WHERE id = ?3",
            params![new_status, resolved_statement_id, id],
        )?;
    }
    Ok(())
}

/// Counts a failed password attempt.
///
/// Bounds automatic retries against an encrypted PDF, so a wrong stored password
/// is not tried indefinitely on every scan.
pub fn increment_password_attempts(conn: &Connection, id: &str) -> Result<i64> {
    conn.execute(
        "UPDATE unprocessed_statements SET password_attempts = password_attempts + 1 WHERE id = ?1",
        params![id],
    )?;
    let attempts: i64 = conn.query_row(
        "SELECT password_attempts FROM unprocessed_statements WHERE id = ?1",
        params![id],
        |row| row.get(0),
    )?;
    Ok(attempts)
}

/// Entries the user can still do something about.
pub fn select_actionable(conn: &Connection) -> Result<Vec<UnprocessedStatementRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, statement_source_json, failure_type, failure_reason, status, resolved_statement_id, created_at, updated_at
         FROM unprocessed_statements
         WHERE status IN ('awaiting_password', 'pending_retry', 'failed')
         ORDER BY created_at DESC",
    )?;

    let iter = stmt.query_map([], |row: &Row| {
        Ok(UnprocessedStatementRow {
            id: row.get(0)?,
            statement_source_json: row.get(1)?,
            failure_type: row.get(2)?,
            failure_reason: row.get(3)?,
            status: row.get(4)?,
            resolved_statement_id: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;

    let mut rows = Vec::new();
    for r in iter {
        rows.push(r?);
    }
    Ok(rows)
}

/// Fetch one unprocessed statement.
pub fn select_by_id(conn: &Connection, id: &str) -> Result<Option<UnprocessedStatementRow>> {
    conn.query_row(
        "SELECT id, statement_source_json, failure_type, failure_reason, status, resolved_statement_id, created_at, updated_at
         FROM unprocessed_statements WHERE id = ?1",
        params![id],
        |row: &Row| {
            Ok(UnprocessedStatementRow {
                id: row.get(0)?,
                statement_source_json: row.get(1)?,
                failure_type: row.get(2)?,
                failure_reason: row.get(3)?,
                status: row.get(4)?,
                resolved_statement_id: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        },
    )
    .optional()
    .map_err(anyhow::Error::from)
}

/// Remove an entry once resolved or dismissed.
pub fn delete(conn: &Connection, id: &str) -> Result<bool> {
    let count = conn.execute(
        "DELETE FROM unprocessed_statements WHERE id = ?1",
        params![id],
    )?;
    Ok(count > 0)
}

/// Entries awaiting a retry.
pub fn select_pending(conn: &Connection) -> Result<Vec<UnprocessedStatementRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, statement_source_json, failure_type, failure_reason, status, resolved_statement_id, created_at, updated_at
         FROM unprocessed_statements
         WHERE status = 'pending_retry' OR status = 'awaiting_password'",
    )?;

    let iter = stmt.query_map([], |row: &Row| {
        Ok(UnprocessedStatementRow {
            id: row.get(0)?,
            statement_source_json: row.get(1)?,
            failure_type: row.get(2)?,
            failure_reason: row.get(3)?,
            status: row.get(4)?,
            resolved_statement_id: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;

    let mut rows = Vec::new();
    for r in iter {
        rows.push(r?);
    }

    Ok(rows)
}
