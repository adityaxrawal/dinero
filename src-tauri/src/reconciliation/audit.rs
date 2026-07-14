use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

/// Immutable match decision types (Doc 11 §7).
#[derive(Debug, Clone, PartialEq)]
pub enum DecisionType {
    AutoMatchedExact,
    AutoMatchedScored,
    AmbiguousPending(String),
    NewCanonical,
    ManuallyConfirmed,
    ManuallyCorrected,
    RejectedMatch,
}

impl DecisionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DecisionType::AutoMatchedExact => "auto_matched_exact",
            DecisionType::AutoMatchedScored => "auto_matched_scored",
            DecisionType::AmbiguousPending(_) => "ambiguous_pending",
            DecisionType::NewCanonical => "new_canonical",
            DecisionType::ManuallyConfirmed => "manually_confirmed",
            DecisionType::ManuallyCorrected => "manually_corrected",
            DecisionType::RejectedMatch => "rejected_match",
        }
    }
}

/// Appends an immutable row to `match_decisions`.
/// Every reconciliation action — auto or manual — must create one of these rows (Doc 11 §7).
/// This table is the auditable, append-only trail of all reconciliation outcomes.
pub fn append_match_decision(
    conn: &Connection,
    observation_id: &str,
    matched_canonical_id: Option<&str>,
    score: f64,
    decision: DecisionType,
) -> Result<()> {
    let id = Uuid::new_v4().to_string();
    // TASK-DB-011 fix: Document 18 §4.5's authoritative review_status enum is
    // not_required/pending_review/reviewed -- was "unreviewed"/"needs_review",
    // which never matched it. Zero other code read these exact strings
    // (checked before changing), so this is a pure internal-consistency fix.
    let review_status = if let DecisionType::AmbiguousPending(_) = decision {
        "pending_review"
    } else {
        "not_required"
    };

    conn.execute(
        "INSERT INTO match_decisions (id, observation_id, matched_transaction_id, score, decision, review_status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, CURRENT_TIMESTAMP)",
        params![id, observation_id, matched_canonical_id, score, decision.as_str(), review_status],
    )?;

    Ok(())
}

/// Creates a corrected match_decisions row when a user corrects an auto-matched transaction.
/// The original auto-match row is preserved for full traceability (Doc 11 §9.1).
pub fn append_correction_decision(
    conn: &Connection,
    observation_id: &str,
    new_canonical_id: Option<&str>,
    original_decision_id: &str,
) -> Result<()> {
    let id = Uuid::new_v4().to_string();

    conn.execute(
        "INSERT INTO match_decisions (id, observation_id, matched_transaction_id, score, decision, review_status, created_at)
         VALUES (?1, ?2, ?3, 1.0, 'manually_corrected', 'reviewed', CURRENT_TIMESTAMP)",
        params![id, observation_id, new_canonical_id],
    )?;

    // We do NOT update the original_decision_id here, to preserve immutability.

    let audit_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO audit_log (id, actor_type, actor_id, action, resource_type, resource_id, created_at)
         VALUES (?1, 'user', 'user_id', 'manual_correction', 'match_decision', ?2, CURRENT_TIMESTAMP)",
        params![audit_id, original_decision_id],
    )?;

    Ok(())
}

/// Detect when a user corrects an extracted field.
/// Stores the correction in `feedback_log` and synthesizes/increments a `pattern_rule`.
pub fn log_user_correction(
    conn: &Connection,
    tx_id: &str,
    field_name: &str,
    old_value: &str,
    new_value: &str,
) -> Result<()> {
    // Find the observation ID and source pipeline for this transaction
    let obs_info: rusqlite::Result<(String, Option<String>, Option<String>)> = conn.query_row(
        "SELECT id, source_pipeline, source_record_id FROM transaction_observations WHERE canonical_transaction_id = ?1 LIMIT 1",
        rusqlite::params![tx_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    );

    let (obs_id, source_pipeline, source_record_id) = match obs_info {
        Ok(info) => info,
        Err(_) => return Ok(()), // No observation found
    };

    let feedback_id = Uuid::new_v4().to_string();

    let source_context_json = serde_json::json!({
        "source_record_id": source_record_id.clone().unwrap_or_default()
    });

    conn.execute(
        "INSERT INTO feedback_log (id, transaction_id, observation_id, source_pipeline, field_name, old_value, new_value, source_context_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            feedback_id,
            tx_id,
            obs_id,
            source_pipeline.unwrap_or_else(|| "manual".to_string()),
            field_name,
            old_value,
            new_value,
            serde_json::to_string(&source_context_json).unwrap()
        ]
    )?;

    // For gmail pipelines, we synthesize a candidate pattern rule to learn from this correction
    if let Some(record_id) = source_record_id {
        // We do a mock template hash using the source_record_id for now as we don't have the original body in memory
        let template_hash = crate::extraction::ladder::compute_template_hash(&format!(
            "mock body for {}",
            record_id
        ));

        let existing_rule: rusqlite::Result<String> = conn.query_row(
            "SELECT id FROM pattern_rules WHERE template_hash = ?1 AND field_name = ?2 LIMIT 1",
            rusqlite::params![template_hash, field_name],
            |row| row.get(0),
        );

        if let Ok(rule_id) = existing_rule {
            let _ = crate::db::pattern_rules::record_rule_success(conn, &rule_id);
        } else {
            let rule = crate::db::pattern_rules::PatternRulesRow {
                id: Uuid::new_v4().to_string(),
                bank_name: "Unknown".to_string(),
                template_hash: template_hash.clone(),
                field_name: field_name.to_string(),
                rule_payload_json: serde_json::json!({"regex": format!("learned regex for {}", new_value)}),
                status: "pending".to_string(),
                success_count: 1, // Start with 1 success since it's user-driven
                failure_count: 0,
                confidence: 1.0,
                created_at: None,
                updated_at: None,
            };
            let _ = crate::db::pattern_rules::insert(conn, &rule);
        }
    }

    Ok(())
}
