use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UnassignedTransactionRow {
    pub id: String,
    pub observation_id: String,
    pub reason: String,
    pub status: String,
    pub created_at: Option<NaiveDateTime>,
}

pub fn insert(conn: &Connection, unassigned: &UnassignedTransactionRow) -> Result<()> {
    conn.execute(
        "INSERT INTO unassigned_transactions (
            id, observation_id, reason, status
        ) VALUES (
            ?1, ?2, ?3, ?4
        )",
        params![
            unassigned.id,
            unassigned.observation_id,
            unassigned.reason,
            unassigned.status,
        ],
    )?;
    Ok(())
}

pub fn update_status(conn: &Connection, id: &str, new_status: &str) -> Result<()> {
    conn.execute(
        "UPDATE unassigned_transactions SET status = ?1 WHERE id = ?2",
        params![new_status, id],
    )?;
    Ok(())
}

/// Doc 30 TASK-API-005: `reconciliation_get_unassigned_transactions` -- "a
/// distinct queue from ambiguous clusters: extraction failures vs. matching
/// ambiguity are surfaced separately in the UI." Did not exist at all
/// before this task (only `insert`/`update_status` existed).
pub fn select_open(conn: &Connection) -> Result<Vec<UnassignedTransactionRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, observation_id, reason, status, created_at \
         FROM unassigned_transactions WHERE status = 'open' ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(UnassignedTransactionRow {
            id: row.get(0)?,
            observation_id: row.get(1)?,
            reason: row.get(2)?,
            status: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}
