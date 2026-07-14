use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ReconciliationClusterMembersRow {
    pub id: String,
    pub cluster_id: String,
    pub observation_id: Option<String>,
    pub canonical_transaction_id: Option<String>,
    pub member_role: String,
    pub added_at: Option<NaiveDateTime>,
}

pub fn insert(conn: &Connection, member: &ReconciliationClusterMembersRow) -> Result<()> {
    conn.execute(
        "INSERT INTO reconciliation_cluster_members (
            id, cluster_id, observation_id, canonical_transaction_id, member_role, added_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, COALESCE(?6, CURRENT_TIMESTAMP))",
        params![
            member.id,
            member.cluster_id,
            member.observation_id,
            member.canonical_transaction_id,
            member.member_role,
            member.added_at,
        ],
    )?;
    Ok(())
}

pub fn select_by_cluster_id(
    conn: &Connection,
    cluster_id: &str,
) -> Result<Vec<ReconciliationClusterMembersRow>> {
    let mut stmt =
        conn.prepare("SELECT * FROM reconciliation_cluster_members WHERE cluster_id = ?1")?;
    let rows = stmt.query_map([cluster_id], row_to_member)?;

    let mut members = Vec::new();
    for row in rows {
        members.push(row?);
    }
    Ok(members)
}

pub fn delete_by_cluster_id(conn: &Connection, cluster_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM reconciliation_cluster_members WHERE cluster_id = ?1",
        params![cluster_id],
    )?;
    Ok(())
}

fn row_to_member(row: &Row) -> rusqlite::Result<ReconciliationClusterMembersRow> {
    Ok(ReconciliationClusterMembersRow {
        id: row.get("id")?,
        cluster_id: row.get("cluster_id")?,
        observation_id: row.get("observation_id")?,
        canonical_transaction_id: row.get("canonical_transaction_id")?,
        member_role: row.get("member_role")?,
        added_at: row.get("added_at")?,
    })
}
