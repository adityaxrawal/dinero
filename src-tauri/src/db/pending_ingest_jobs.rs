//! Durable mirror of the transaction and mandate queues (review H4).
//!
//! A job is written here before it enters its in-memory channel and deleted
//! once its worker has finished it, so work still queued when the app quits,
//! or dropped by a worker DB failure, is replayed at the next launch.
use anyhow::Result;
use rusqlite::{params, Connection};

/// Persists a job; re-enqueuing the same id replaces the payload.
pub fn upsert(conn: &Connection, id: &str, kind: &str, payload_json: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO pending_ingest_jobs (id, kind, payload_json) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET payload_json = excluded.payload_json",
        params![id, kind, payload_json],
    )?;
    Ok(())
}

/// Removes a job once its worker has finished it.
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM pending_ingest_jobs WHERE id = ?1", params![id])?;
    Ok(())
}

/// Jobs persisted before `cutoff` (UTC, `YYYY-MM-DD HH:MM:SS`), oldest first.
pub fn select_before(conn: &Connection, cutoff: &str) -> Result<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, payload_json FROM pending_ingest_jobs
         WHERE created_at < ?1 ORDER BY created_at",
    )?;
    let rows = stmt
        .query_map(params![cutoff], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
