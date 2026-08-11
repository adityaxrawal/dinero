//! Feeds user reconciliation decisions back into matching.
//!
//! Each confirmation or rejection is evidence about this user's actual data, so
//! the matcher improves against the banks and merchants they really use rather
//! than only against general heuristics.
use crate::reconciliation::audit::DecisionType;
use anyhow::Result;
use rusqlite::Connection;

/// Feeds a user's reconciliation decision back into the matcher.
pub fn process_reconciliation_feedback(
    conn: &Connection,
    decision: &DecisionType,
    _observation_id: &str,
    rule_id_opt: Option<&str>,
) -> Result<()> {
    if let Some(rule_id) = rule_id_opt {
        match decision {
            DecisionType::AutoMatchedExact
            | DecisionType::AutoMatchedScored
            | DecisionType::ManuallyConfirmed => {
                crate::db::field_rules::record_success(conn, rule_id)?;
            }
            DecisionType::ManuallyCorrected | DecisionType::RejectedMatch => {
                crate::db::field_rules::record_failure(conn, rule_id)?;
            }
            _ => {}
        }
    }

    Ok(())
}
