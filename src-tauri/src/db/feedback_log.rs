use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FeedbackLogRow {
    pub id: String,
    pub transaction_id: String,
    pub observation_id: Option<String>,
    pub source_pipeline: Option<String>,
    pub field_name: String,
    pub old_value: Option<String>,
    pub new_value: String,
    pub source_context_json: serde_json::Value,
    pub created_at: Option<NaiveDateTime>,
}

pub fn insert(conn: &Connection, feedback: &FeedbackLogRow) -> Result<()> {
    if let Some(ref source) = feedback.source_pipeline {
        if !["gmail_transaction", "statement_pdf", "manual"].contains(&source.as_str()) {
            return Err(anyhow::anyhow!("Invalid source_pipeline: {}", source));
        }
    }

    conn.execute(
        "INSERT INTO feedback_log (
            id, transaction_id, observation_id, source_pipeline, field_name, old_value, new_value, source_context_json, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            feedback.id,
            feedback.transaction_id,
            feedback.observation_id,
            feedback.source_pipeline,
            feedback.field_name,
            feedback.old_value,
            feedback.new_value,
            serde_json::to_string(&feedback.source_context_json)?,
            feedback.created_at,
        ],
    )?;
    Ok(())
}

/// Doc 30 TASK-TXN-009: "When a user manually edits a canonical
/// transaction's category/merchant/amount (Area 8/9), write a feedback_log
/// row (old_value/new_value/field_name)." A thin, purpose-named wrapper
/// around `insert` — `source_pipeline` is always `"manual"` here since this
/// is specifically the user-edit path, not the reconciliation-decision
/// feedback path (`reconciliation::feedback::process_reconciliation_feedback`,
/// a separate mechanism for auto-match/reject decisions, not field edits).
/// The actual call site (an IPC command letting a user edit a transaction)
/// is Area 8's job, not yet built — this is the writer that command will use.
pub fn record_manual_correction(
    conn: &Connection,
    transaction_id: &str,
    observation_id: Option<&str>,
    field_name: &str,
    old_value: Option<&str>,
    new_value: &str,
) -> Result<()> {
    let row = FeedbackLogRow {
        id: uuid::Uuid::new_v4().to_string(),
        transaction_id: transaction_id.to_string(),
        observation_id: observation_id.map(|s| s.to_string()),
        source_pipeline: Some("manual".to_string()),
        field_name: field_name.to_string(),
        old_value: old_value.map(|s| s.to_string()),
        new_value: new_value.to_string(),
        source_context_json: serde_json::json!({ "trigger": "user_manual_edit" }),
        created_at: Some(chrono::Utc::now().naive_utc()),
    };
    insert(conn, &row)
}

pub fn select_by_transaction(
    conn: &Connection,
    transaction_id: &str,
) -> Result<Vec<FeedbackLogRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, transaction_id, observation_id, source_pipeline, field_name, old_value, new_value, source_context_json, created_at
         FROM feedback_log
         WHERE transaction_id = ?1
         ORDER BY created_at DESC"
    )?;

    let rows = stmt.query_map([transaction_id], |row| {
        let payload_str: String = row.get(7)?;
        let payload = serde_json::from_str(&payload_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;

        Ok(FeedbackLogRow {
            id: row.get(0)?,
            transaction_id: row.get(1)?,
            observation_id: row.get(2)?,
            source_pipeline: row.get(3)?,
            field_name: row.get(4)?,
            old_value: row.get(5)?,
            new_value: row.get(6)?,
            source_context_json: payload,
            created_at: row.get(8)?,
        })
    })?;

    let mut feedback = Vec::new();
    for r in rows {
        feedback.push(r?);
    }

    Ok(feedback)
}
