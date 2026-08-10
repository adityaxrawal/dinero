//! Messages that failed during a scan, retained for retry and diagnosis.
//!
//! Recorded rather than dropped so a parse failure is visible and re-attemptable
//! instead of appearing as a silently missing transaction.
use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScanFailedMessageRow {
    pub id: String,
    pub account_id: String,
    pub msg_id: String,
    pub error: String,
    pub failed_at: Option<NaiveDateTime>,
}

/// Records a message that failed during a scan.
///
/// Recorded rather than dropped, so a parse failure is visible and retryable
/// instead of appearing as a silently missing transaction.
pub fn insert(conn: &Connection, row: &ScanFailedMessageRow) -> Result<()> {
    conn.execute(
        "INSERT INTO scan_failed_messages (id, account_id, msg_id, error) VALUES (?1, ?2, ?3, ?4)",
        params![row.id, row.account_id, row.msg_id, row.error],
    )?;
    Ok(())
}

/// Failed messages for an account.
pub fn select_by_account(conn: &Connection, account_id: &str) -> Result<Vec<ScanFailedMessageRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, account_id, msg_id, error, failed_at \
         FROM scan_failed_messages WHERE account_id = ?1 ORDER BY failed_at DESC",
    )?;
    let rows = stmt
        .query_map(params![account_id], |r| {
            Ok(ScanFailedMessageRow {
                id: r.get(0)?,
                account_id: r.get(1)?,
                msg_id: r.get(2)?,
                error: r.get(3)?,
                failed_at: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
#[path = "scan_failed_messages_tests.rs"]
mod tests;
