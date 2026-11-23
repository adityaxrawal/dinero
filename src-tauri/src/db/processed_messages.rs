//! Final per-message decisions that dedupe must honour (review M2).
use anyhow::Result;
use rusqlite::{params, Connection};

/// Records a final decision for a message (rejection reason, mandate event).
pub fn record(conn: &Connection, message_id: &str, decision: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO processed_messages (message_id, decision) VALUES (?1, ?2)
         ON CONFLICT(message_id) DO UPDATE SET decision = excluded.decision",
        params![message_id, decision],
    )?;
    Ok(())
}
