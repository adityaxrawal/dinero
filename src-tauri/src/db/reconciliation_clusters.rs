//! Clusters of transactions suspected to be the same real-world payment.
//!
//! Created when the matcher finds a plausible but not certain match. The cluster
//! holds that ambiguity explicitly until a user resolves it, rather than the
//! matcher guessing and silently merging or duplicating.
use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ReconciliationClustersRow {
    pub id: String,
    pub cluster_status: String,
    pub reason: Option<String>,
    pub resolution_notes: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub resolved_at: Option<NaiveDateTime>,
}

/// Create a cluster recording an unresolved match ambiguity.
pub fn insert(conn: &Connection, cluster: &ReconciliationClustersRow) -> Result<()> {
    conn.execute(
        "INSERT INTO reconciliation_clusters (
            id, cluster_status, reason, resolution_notes, created_at, resolved_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            cluster.id,
            cluster.cluster_status,
            cluster.reason,
            cluster.resolution_notes,
            cluster.created_at,
            cluster.resolved_at,
        ],
    )?;
    Ok(())
}

/// Record the outcome once the user resolves a cluster.
pub fn update_status(conn: &Connection, id: &str, cluster_status: &str) -> Result<()> {
    conn.execute(
        "UPDATE reconciliation_clusters SET cluster_status = ?2 WHERE id = ?1",
        params![id, cluster_status],
    )?;
    Ok(())
}

/// Remove a cluster and, by cascade, its membership rows.
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM reconciliation_clusters WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// Fetch one cluster.
pub fn select_by_id(conn: &Connection, id: &str) -> Result<Option<ReconciliationClustersRow>> {
    let mut stmt = conn.prepare("SELECT * FROM reconciliation_clusters WHERE id = ?1")?;
    let cluster = stmt.query_row([id], row_to_cluster).optional()?;
    Ok(cluster)
}

/// All clusters, for the reconciliation queue.
pub fn select_all(conn: &Connection) -> Result<Vec<ReconciliationClustersRow>> {
    let mut stmt =
        conn.prepare("SELECT * FROM reconciliation_clusters ORDER BY created_at DESC")?;
    let clusters = stmt.query_map([], row_to_cluster)?;
    let mut results = Vec::new();
    for c in clusters {
        results.push(c?);
    }
    Ok(results)
}

/// Maps a result row onto a cluster record.
fn row_to_cluster(row: &Row) -> rusqlite::Result<ReconciliationClustersRow> {
    Ok(ReconciliationClustersRow {
        id: row.get("id")?,
        cluster_status: row.get("cluster_status")?,
        reason: row.get("reason")?,
        resolution_notes: row.get("resolution_notes")?,
        created_at: row.get("created_at")?,
        resolved_at: row.get("resolved_at")?,
    })
}
