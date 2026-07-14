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

pub fn update_status(
    conn: &Connection,
    id: &str,
    new_status: &str,
    resolved_statement_id: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE unprocessed_statements
         SET status = ?1, resolved_statement_id = ?2
         WHERE id = ?3",
        params![new_status, resolved_statement_id, id],
    )?;
    Ok(())
}

/// I9 fix: increments and returns the wrong-password attempt counter for a
/// blocked statement, so the caller can enforce a 3-attempt cap (previously
/// only the 2.5-minute timeout was enforced).
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

/// Doc 30 TASK-STMT-010: the 3 actionable buckets `statements_list_unprocessed`
/// groups by — `awaiting_instrument_confirmation` and `resolved` are
/// deliberately excluded (the former has its own dedicated UI flow, the
/// latter is done and doesn't belong in an "unprocessed" list at all).
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

/// Doc 30 TASK-STMT-010: `statements_discard(id)` — permanent removal.
pub fn delete(conn: &Connection, id: &str) -> Result<bool> {
    let count = conn.execute("DELETE FROM unprocessed_statements WHERE id = ?1", params![id])?;
    Ok(count > 0)
}

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
