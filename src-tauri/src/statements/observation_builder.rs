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
        // FLAGGED, not fixed (Doc 30 TASK-DEDUP-001 architectural gap, out
        // of this task's scope): Doc 30 TASK-TXN-008's fingerprint formula
        // requires connected_accounts.id as a hash input, but statement rows
        // have no connected-account concept at all (Doc 18 §4.7's
        // `statements` table carries only instrument_id) -- so a statement
        // observation can never produce a fingerprint that would collide
        // with its corresponding email observation for the same real-world
        // transaction, even though that is the exact scenario the
        // fingerprint pre-filter's own doc comment (TASK-TXN-008) names as
        // its purpose. Left `None` here rather than guessing a substitute
        // input: the windowed candidate search + scoring engine (Doc 30
        // TASK-DEDUP-003/004, which already scores cross-pipeline
        // complementarity) still correctly reconciles this case -- just
        // without the fingerprint performance shortcut.
        fingerprint: None,
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
