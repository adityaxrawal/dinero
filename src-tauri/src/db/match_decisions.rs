use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MatchDecisionsRow {
    pub id: String,
    pub observation_id: Option<String>,
    pub matched_transaction_id: Option<String>,
    pub decision: Option<String>,
    pub score: Option<f64>,
    pub rules_triggered_json: Option<String>,
    pub review_status: Option<String>,
    pub reviewed_by: Option<String>,
    pub created_at: Option<NaiveDateTime>,
}

pub fn insert(conn: &Connection, decision: &MatchDecisionsRow) -> Result<()> {
    conn.execute(
        "INSERT INTO match_decisions (
            id, observation_id, matched_transaction_id, decision, score,
            rules_triggered_json, review_status, reviewed_by, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            decision.id,
            decision.observation_id,
            decision.matched_transaction_id,
            decision.decision,
            decision.score,
            decision.rules_triggered_json,
            decision.review_status,
            decision.reviewed_by,
            decision.created_at,
        ],
    )?;
    Ok(())
}

pub fn select_by_id(conn: &Connection, id: &str) -> Result<Option<MatchDecisionsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM match_decisions WHERE id = ?1")?;
    let mut rows = stmt.query([id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_decision(row)?))
    } else {
        Ok(None)
    }
}

pub fn select_by_observation_id(
    conn: &Connection,
    observation_id: &str,
) -> Result<Vec<MatchDecisionsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM match_decisions WHERE observation_id = ?1")?;
    let rows = stmt.query_map([observation_id], row_to_decision)?;

    let mut decisions = Vec::new();
    for row in rows {
        decisions.push(row?);
    }
    Ok(decisions)
}

fn row_to_decision(row: &Row) -> rusqlite::Result<MatchDecisionsRow> {
    Ok(MatchDecisionsRow {
        id: row.get("id")?,
        observation_id: row.get("observation_id")?,
        matched_transaction_id: row.get("matched_transaction_id")?,
        decision: row.get("decision")?,
        score: row.get("score")?,
        rules_triggered_json: row.get("rules_triggered_json")?,
        review_status: row.get("review_status")?,
        reviewed_by: row.get("reviewed_by")?,
        created_at: row.get("created_at")?,
    })
}
