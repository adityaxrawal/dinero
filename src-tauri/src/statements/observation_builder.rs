//! Converts extracted statement rows into transaction observations.
//!
//! The join between statement ingestion and reconciliation: once rows become
//! observations they flow through the same matching pipeline as email-derived
//! ones, so a payment seen in both a statement and an alert is recognised as one.
use crate::reconciliation::engine::IncomingObservation;
use crate::statements::row_extractor::StatementRow;
use anyhow::Result;

/// Converts one statement row into a transaction observation.
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
        event_time: row.transaction_date.clone(),
        reference_id: row.reference_id.clone(),
        merchant_raw: Some(row.merchant_raw.clone()),
        source_pipeline: "statement_pdf".to_string(),
        source_record_id: statement_entry_id.to_string(),
        emi_total_installments: None,
        emi_original_amount_minor: None,
        fingerprint: Some(crate::extraction::fingerprint::compute_fingerprint(
            instrument_id,
            &row.direction,
            row.amount_minor,
            &format!("{}T00:00", row.transaction_date),
            instrument_id,
        )),
        confidence_score: None,
        event_time_confidence: None,
        channel: None,
    })
}

/// Converts every row of a statement into observations.
///
/// Once they are observations they flow through the same reconciliation pipeline
/// as email-derived ones, so a payment seen in both a statement and an alert is
/// recognised as a single transaction.
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
                    None
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

    #[test]
    fn test_statement_observation_has_fingerprint() {
        let obs = build_observation("stmt_1", "entry_1", "inst_1", &base_row()).unwrap();
        assert!(obs.fingerprint.is_some());
    }

    #[test]
    fn test_statement_fingerprint_deterministic_for_same_row() {
        let obs1 = build_observation("stmt_1", "entry_1", "inst_1", &base_row()).unwrap();
        let obs2 = build_observation("stmt_2", "entry_2", "inst_1", &base_row()).unwrap();
        assert_eq!(obs1.fingerprint, obs2.fingerprint);
    }

    #[test]
    fn test_statement_fingerprint_differs_across_instruments() {
        let obs1 = build_observation("stmt_1", "entry_1", "inst_1", &base_row()).unwrap();
        let obs2 = build_observation("stmt_1", "entry_1", "inst_2", &base_row()).unwrap();
        assert_ne!(obs1.fingerprint, obs2.fingerprint);
    }
}
