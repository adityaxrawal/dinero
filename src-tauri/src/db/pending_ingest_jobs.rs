//! Durable mirror of the transaction and mandate queues (review H4).
//!
//! A job is written here before it enters its in-memory channel and deleted
//! once its worker has finished it, so work still queued when the app quits,
//! or dropped by a worker DB failure, is replayed at the next launch.
use anyhow::Result;
use rusqlite::{params, Connection};

/// Persists a job; re-enqueuing the same id replaces the payload and starts
/// its attempt count again (a user "Retry failed" re-enqueues a dead job).
pub fn upsert(conn: &Connection, id: &str, kind: &str, payload_json: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO pending_ingest_jobs (id, kind, payload_json) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET payload_json = excluded.payload_json,
                                       attempts = 0,
                                       dead_at = NULL",
        params![id, kind, payload_json],
    )?;
    Ok(())
}

/// Removes a job once its worker has finished it.
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM pending_ingest_jobs WHERE id = ?1", params![id])?;
    Ok(())
}

/// Failed attempts allowed before a job is marked dead.
pub const MAX_ATTEMPTS: i64 = 3;

/// Records a failed attempt; returns true once the job has been marked dead.
pub fn record_failed_attempt(conn: &Connection, id: &str) -> Result<bool> {
    conn.execute(
        "UPDATE pending_ingest_jobs
         SET attempts = attempts + 1,
             dead_at = CASE WHEN attempts + 1 >= ?2 THEN datetime('now') ELSE dead_at END
         WHERE id = ?1",
        params![id, MAX_ATTEMPTS],
    )?;
    Ok(conn
        .query_row(
            "SELECT dead_at IS NOT NULL FROM pending_ingest_jobs WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap_or(false))
}

/// Jobs persisted before `cutoff` (UTC, `YYYY-MM-DD HH:MM:SS`), oldest first.
pub fn select_before(conn: &Connection, cutoff: &str) -> Result<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, payload_json FROM pending_ingest_jobs
         WHERE created_at < ?1 AND dead_at IS NULL ORDER BY created_at",
    )?;
    let rows = stmt
        .query_map(params![cutoff], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
