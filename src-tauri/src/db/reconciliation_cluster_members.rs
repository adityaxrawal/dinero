//! Membership rows linking transactions into a reconciliation cluster.
//!
//! Each row carries the member's role -- the incoming observation versus the
//! existing candidates it might match -- which is what the comparison UI uses to
//! decide what is being matched against what.
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
    pub match_score: Option<f64>,
}

/// Adds a transaction to a cluster with its role.
pub fn insert(conn: &Connection, member: &ReconciliationClusterMembersRow) -> Result<()> {
    conn.execute(
        "INSERT INTO reconciliation_cluster_members (
            id, cluster_id, observation_id, canonical_transaction_id, member_role, added_at, match_score
         ) VALUES (?1, ?2, ?3, ?4, ?5, COALESCE(?6, CURRENT_TIMESTAMP), ?7)",
        params![
            member.id,
            member.cluster_id,
            member.observation_id,
            member.canonical_transaction_id,
            member.member_role,
            member.added_at,
            member.match_score,
        ],
    )?;
    Ok(())
}

/// Members of a cluster, for the comparison view.
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

/// Removes all members when a cluster is resolved.
pub fn delete_by_cluster_id(conn: &Connection, cluster_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM reconciliation_cluster_members WHERE cluster_id = ?1",
        params![cluster_id],
    )?;
    Ok(())
}

/// Maps a result row onto a cluster member.
fn row_to_member(row: &Row) -> rusqlite::Result<ReconciliationClusterMembersRow> {
    Ok(ReconciliationClusterMembersRow {
        id: row.get("id")?,
        cluster_id: row.get("cluster_id")?,
        observation_id: row.get("observation_id")?,
        canonical_transaction_id: row.get("canonical_transaction_id")?,
        member_role: row.get("member_role")?,
        added_at: row.get("added_at")?,
        match_score: row.get("match_score")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_score_round_trips() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        conn.execute(
            "INSERT INTO reconciliation_clusters (id, cluster_status, reason) VALUES ('c1', 'open', 'mid_range_score')",
            [],
        )
        .unwrap();
        let member = ReconciliationClusterMembersRow {
            id: "m1".to_string(),
            cluster_id: "c1".to_string(),
            observation_id: None,
            canonical_transaction_id: Some("txn1".to_string()),
            member_role: "candidate_a".to_string(),
            added_at: None,
            match_score: Some(0.71),
        };
        insert(&conn, &member).unwrap();

        let members = select_by_cluster_id(&conn, "c1").unwrap();
        assert_eq!(members[0].match_score, Some(0.71));
    }

    #[test]
    fn test_incoming_member_has_no_match_score() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        conn.execute(
            "INSERT INTO reconciliation_clusters (id, cluster_status, reason) VALUES ('c1', 'open', 'mid_range_score')",
            [],
        )
        .unwrap();
        let member = ReconciliationClusterMembersRow {
            id: "m1".to_string(),
            cluster_id: "c1".to_string(),
            observation_id: Some("obs1".to_string()),
            canonical_transaction_id: None,
            member_role: "incoming".to_string(),
            added_at: None,
            match_score: None,
        };
        insert(&conn, &member).unwrap();

        let members = select_by_cluster_id(&conn, "c1").unwrap();
        assert_eq!(members[0].match_score, None);
    }
}
