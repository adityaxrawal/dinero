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

/// Forgets rejection decisions so the next scan evaluates those emails again.
/// Mandate events stay recorded. Returns how many were cleared.
pub fn forget_rejections(conn: &Connection) -> Result<usize> {
    Ok(conn.execute("DELETE FROM processed_messages WHERE decision LIKE 'rejected:%'", [])?)
}

#[cfg(test)]
mod tests {
    #[test]
    fn forgetting_rejections_keeps_mandate_events() {
        let conn = crate::db::test_helpers::setup_test_db();
        super::record(&conn, "r1", "rejected:gate2_not_financial").unwrap();
        super::record(&conn, "m1", "mandate_event").unwrap();
        assert_eq!(super::forget_rejections(&conn).unwrap(), 1);
        let left: String = conn
            .query_row("SELECT message_id FROM processed_messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, "m1");
    }
}
