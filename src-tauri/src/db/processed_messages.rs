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

/// Decision for a statement email with no downloadable PDF (audit N-F3): the
/// one outcome that used to leave no trace in the database.
pub const STATEMENT_NO_ATTACHMENT: &str = "statement_no_attachment";

/// Records a statement email that had no downloadable PDF, so it can be
/// traced and rescans skip it (owner decision, audit N-F3).
pub async fn record_statement_without_attachment(
    pool: &deadpool_sqlite::Pool,
    message_id: &str,
) -> Result<()> {
    let id = message_id.to_string();
    pool.get()
        .await
        .map_err(|e| anyhow::anyhow!("pool error: {e}"))?
        .interact(move |c| record(c, &id, STATEMENT_NO_ATTACHMENT))
        .await
        .map_err(|e| anyhow::anyhow!("interact error: {e}"))?
}

/// Forgets rejection decisions, and statement emails that had no attachment,
/// so the next scan evaluates those emails again. Mandate events stay
/// recorded. Returns how many were cleared.
pub fn forget_rejections(conn: &Connection) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM processed_messages WHERE decision LIKE 'rejected:%' OR decision = ?1",
        [STATEMENT_NO_ATTACHMENT],
    )?)
}

#[cfg(test)]
mod tests {
    #[test]
    fn forgetting_rejections_keeps_mandate_events() {
        let conn = crate::db::test_helpers::setup_test_db();
        super::record(&conn, "r1", "rejected:gate2_not_financial").unwrap();
        super::record(&conn, "m1", "mandate_event").unwrap();
        super::record(&conn, "s1", super::STATEMENT_NO_ATTACHMENT).unwrap();
        assert_eq!(super::forget_rejections(&conn).unwrap(), 2);
        let left: String = conn
            .query_row("SELECT message_id FROM processed_messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, "m1");
    }
}
