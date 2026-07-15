use crate::reconciliation::audit::DecisionType;
use anyhow::Result;
use rusqlite::Connection;

/// Intercepts reconciliation decisions and provides feedback to the extraction pattern rules.
/// As per Doc 11 §7, every reconciliation decision acts as a supervised feedback loop:
/// - AutoMatchedExact / AutoMatchedScored -> Success for the LLM pattern
/// - ManuallyConfirmed / ManuallyCorrected -> Updates or corrections
pub fn process_reconciliation_feedback(
    conn: &Connection,
    decision: &DecisionType,
    _observation_id: &str,
    rule_id_opt: Option<&str>,
) -> Result<()> {
    // In a full implementation, we would look up the observation and find the pattern_rule_id
    // that generated it (perhaps stored in raw_payload_json or a dedicated column).
    // For now, we take rule_id_opt as an explicit parameter if available.

    if let Some(rule_id) = rule_id_opt {
        match decision {
            DecisionType::AutoMatchedExact
            | DecisionType::AutoMatchedScored
            | DecisionType::ManuallyConfirmed => {
                crate::db::pattern_rules::record_rule_success(conn, rule_id)?;
            }
            DecisionType::ManuallyCorrected | DecisionType::RejectedMatch => {
                crate::db::pattern_rules::record_rule_failure(conn, rule_id)?;
            }
            _ => {}
        }
    }

    Ok(())
}
