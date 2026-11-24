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

/// The payload of a job that is still live (not dead), if it exists.
pub fn select_live_payload(conn: &Connection, id: &str) -> Result<Option<String>> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row(
            "SELECT payload_json FROM pending_ingest_jobs WHERE id = ?1 AND dead_at IS NULL",
            params![id],
            |r| r.get(0),
        )
        .optional()?)
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
///
/// Paged (audit D-3): returns at most `limit` jobs ordered by
/// `(created_at, id)`, starting after `after` (the last row of the previous
/// page), so a large backlog is never held in memory at once.
pub fn select_before(
    conn: &Connection,
    cutoff: &str,
    after: Option<&(String, String)>,
    limit: usize,
) -> Result<Vec<PendingJob>> {
    let (after_at, after_id) = after.map_or(("", ""), |(at, id)| (at.as_str(), id.as_str()));
    let mut stmt = conn.prepare(
        "SELECT id, kind, payload_json, created_at FROM pending_ingest_jobs
         WHERE created_at < ?1 AND dead_at IS NULL
           AND (created_at > ?2 OR (created_at = ?2 AND id > ?3))
         ORDER BY created_at, id
         LIMIT ?4",
    )?;
    let rows = stmt
        .query_map(params![cutoff, after_at, after_id, limit as i64], |r| {
            Ok(PendingJob { id: r.get(0)?, kind: r.get(1)?, payload_json: r.get(2)?, created_at: r.get(3)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// A persisted job as read back for replay.
pub struct PendingJob {
    pub id: String,
    pub kind: String,
    pub payload_json: String,
    pub created_at: String,
}
