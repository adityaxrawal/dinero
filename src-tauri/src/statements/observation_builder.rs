use crate::reconciliation::engine::IncomingObservation;
use crate::statements::row_extractor::StatementRow;
use anyhow::Result;

/// Converts a statement row into a `transaction_observation` ready for the reconciliation engine.
///
/// Observation fields set from statement entries (Doc 10 §13.1):
///   source_pipeline       = "statement_pdf"
///   source_record_id      = statement_entries.id
///   statement_id          = statements.id
///   statement_entry_id    = statement_entries.id
///   instrument_id         = statements.instrument_id
///   amount_minor          = statement_entries.amount_minor
///   currency              = statement_entries.currency
///   direction             = statement_entries.direction
///   event_time            = statement_entries.transaction_date (UTC)
///   merchant_raw          = statement_entries.merchant_raw
///   reference_id          = statement_entries.reference_id
///   extraction_method     = "statement_row_parser" (or "llm_assist" if LLM-extracted)
///
/// Field precedence at canonical layer (Doc 10 §14 = Doc 11 §5):
///   Statement wins: reference_id, posting_date, merchant_display_name, settled amount_minor
///   Email wins: event_time (authorization timestamp), balance_after_transaction
pub fn build_observation(
    _statement_id: &str,
    statement_entry_id: &str,
    instrument_id: &str,
    row: &StatementRow,
) -> Result<IncomingObservation> {
    let extraction_method = if row.llm_extracted {
        "llm_assist"
    } else {
        "statement_row_parser"
    };

    println!(
        "Building observation from statement_entry_id='{}' extraction_method='{}'",
        statement_entry_id, extraction_method
    );

    Ok(IncomingObservation {
        id: uuid::Uuid::new_v4().to_string(),
        instrument_id: instrument_id.to_string(),
        amount_minor: row.amount_minor,
        currency: row.currency.clone(),
        direction: row.direction.clone(),
        // event_time = transaction_date from statement row (source timestamp, not wall-clock)
        event_time: row.transaction_date.clone(),
        reference_id: row.reference_id.clone(),
        merchant_raw: Some(row.merchant_raw.clone()),
        source_pipeline: "statement_pdf".to_string(),
        source_record_id: statement_entry_id.to_string(),
        // Doc 30 TASK-TXN-012 scopes EMI-language detection to Layers 2/3
        // (email body text) only -- no equivalent detection exists for
        // statement rows, so this is correctly left unpopulated here.
        emi_total_installments: None,
        emi_original_amount_minor: None,
        // Doc 30 TASK-TXN-008: the fingerprint formula's 5th input
        // (`connected_accounts.id` for email observations, "to prevent
        // cross-account false merges") has no equivalent concept for
        // statement rows (Doc 18 §4.7's `statements` table carries only
        // instrument_id). Substituting `instrument_id` itself for that slot
        // -- a statement is already instrument-scoped, so it's a stable,
        // always-available stand-in -- means a re-imported/duplicate
        // statement row now hits the fast fingerprint pre-filter instead of
        // always paying the full windowed-scoring cost. Statement rows also
        // carry only day precision (no time-of-day column), so the minute
        // bucket is fixed at midnight UTC for that date.
        fingerprint: Some(crate::extraction::fingerprint::compute_fingerprint(
            instrument_id,
            &row.direction,
            row.amount_minor,
            &format!("{}T00:00", row.transaction_date),
            instrument_id,
        )),
        // Statement rows always win precedence outright (Doc 30
        // TASK-DEDUP-008) regardless of confidence -- this field only
        // matters for the email-vs-email comparison, irrelevant here.
        confidence_score: None,
    })
}

/// Builds all observations from a statement's rows and returns the list.
/// Invalid rows are skipped with a log entry — they do NOT abort the batch (Doc 10 §16).
pub fn build_all_observations(
    statement_id: &str,
    instrument_id: &str,
    rows: &[StatementRow],
    statement_entries_ids: &[String],
) -> Vec<IncomingObservation> {
    rows.iter()
        .zip(statement_entries_ids.iter())
        .filter_map(|(row, entry_id)| {
            match build_observation(statement_id, entry_id, instrument_id, row) {
                Ok(obs) => Some(obs),
                Err(e) => {
                    println!(
                        "Row observation build failed for entry_id={}: {}",
                        entry_id, e
                    );
                    None // isolated failure — other rows continue
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_row() -> StatementRow {
        StatementRow {
            transaction_date: "2026-06-10".to_string(),
            merchant_raw: "AMAZON PAY".to_string(),
            amount_minor: 150000,
            currency: "INR".to_string(),
            direction: "debit".to_string(),
            reference_id: Some("REF123".to_string()),
            row_index: 0,
            llm_extracted: false,
        }
    }

    /// Doc 30 TASK-TXN-008: statement observations must produce a real
    /// fingerprint (not `None`) so the fast pre-filter can fire on a
    /// re-imported/duplicate statement row instead of always falling
    /// through to the full windowed scoring engine.
    #[test]
    fn test_statement_observation_has_fingerprint() {
        let obs = build_observation("stmt_1", "entry_1", "inst_1", &base_row()).unwrap();
        assert!(obs.fingerprint.is_some());
    }

    /// Same instrument/amount/direction/date must fingerprint identically
    /// across two separately-built observations (e.g. the same statement
    /// re-imported) -- the property the fast pre-filter actually relies on.
    #[test]
    fn test_statement_fingerprint_deterministic_for_same_row() {
        let obs1 = build_observation("stmt_1", "entry_1", "inst_1", &base_row()).unwrap();
        let obs2 = build_observation("stmt_2", "entry_2", "inst_1", &base_row()).unwrap();
        assert_eq!(obs1.fingerprint, obs2.fingerprint);
    }

    /// A different instrument must fingerprint differently even for an
    /// otherwise-identical row.
    #[test]
    fn test_statement_fingerprint_differs_across_instruments() {
        let obs1 = build_observation("stmt_1", "entry_1", "inst_1", &base_row()).unwrap();
        let obs2 = build_observation("stmt_1", "entry_1", "inst_2", &base_row()).unwrap();
        assert_ne!(obs1.fingerprint, obs2.fingerprint);
    }
}
