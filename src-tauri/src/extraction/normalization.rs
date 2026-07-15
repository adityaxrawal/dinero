use crate::db::transaction_observations::TransactionObservationsRow;
use crate::extraction::ladder::ExtractionResult;
use chrono::{TimeZone, Utc};
use uuid::Uuid;

pub fn normalize_observation(
    raw: ExtractionResult,
    source_pipeline: &str,
    source_message_id: &str,
    raw_body: Option<&str>,
) -> TransactionObservationsRow {
    // 1. Amount Minor Normalization
    let amount_minor = raw.amount_minor;

    // 2. Currency Normalization
    let currency = raw
        .currency
        .clone()
        .map(|c| c.to_uppercase())
        .unwrap_or_else(|| "INR".to_string());

    // 3. Direction Normalization
    let direction = raw
        .direction
        .clone()
        .map(|d| d.to_lowercase())
        .map(|d| {
            if d.contains("cr") || d.contains("credit") || d.contains("received") {
                "credit".to_string()
            } else {
                "debit".to_string()
            }
        })
        .unwrap_or_else(|| "debit".to_string());

    // 4. Timestamp Normalization (UTC to IST storage per schema rule)
    // raw.event_time is an i64 Unix timestamp (UTC).
    // We convert it to a NaiveDateTime representing the local time (IST).
    let event_time = raw.event_time.map(|ts| {
        let dt_utc = Utc.timestamp_opt(ts, 0).unwrap();
        let ist_offset = chrono::FixedOffset::east_opt(5 * 3600 + 30 * 60).unwrap();
        dt_utc.with_timezone(&ist_offset).naive_local()
    });

    // 5. Merchant Raw
    let merchant_raw = raw.merchant_raw.clone();

    // Doc 30 TASK-TXN-008: fingerprint is deliberately NOT computed here.
    // It must be keyed on the *resolved* instrument_id (plus
    // connected_accounts.id), neither of which this function has access to
    // — instrument resolution happens downstream, after this row is built
    // (`ingestion::queues::process_transaction_job`), which is where
    // `extraction::fingerprint::compute_fingerprint` is actually called.

    TransactionObservationsRow {
        id: Uuid::new_v4().to_string(),
        canonical_transaction_id: None,
        source_pipeline: Some(source_pipeline.to_string()),
        source_record_id: Some(source_message_id.to_string()),
        source_message_id: Some(source_message_id.to_string()),
        source_thread_id: None,
        statement_id: None,
        statement_entry_id: None,
        instrument_id: None,
        direction: Some(direction),
        amount: None,
        amount_minor,
        currency: Some(currency),
        event_time,
        event_time_confidence: None,
        posting_date: None,
        merchant_raw,
        merchant_normalized: None,
        reference_id: raw.reference_id,
        original_amount_minor: raw.original_amount_minor,
        original_currency: raw.original_currency,
        // Doc 30 TASK-TXN-013: previously hardcoded None -- no layer ever
        // populated ExtractionResult.exchange_rate before this task added
        // the field and the currency_handler that fills it.
        exchange_rate: raw.exchange_rate,
        balance_after_transaction: raw.balance_after.map(|a| a as f64 / 100.0),
        timezone_at_ingestion: None,
        fingerprint: None,
        extraction_method: Some(raw.extraction_method),
        // Doc 30 TASK-TXN-001 added this field to ExtractionResult
        // (Layer 5/6 LLM sets 0.7 per FRS §6.3); TASK-TXN-009's "insert...
        // with all fields" means it must actually flow through, not be
        // silently dropped here.
        confidence_score: raw.confidence_score,
        // Doc 30 TASK-TXN-009: "raw_payload_json (the full sanitized body...
        // for auditability/reprocessing)".
        raw_payload_json: raw_body.map(|b| serde_json::json!({ "body": b }).to_string()),
        // No document anywhere defines a versioning scheme for bank-template
        // parsers (flagged, not guessed, at TASK-TXN-001 and TASK-TXN-003
        // already) -- still true here, left unpopulated.
        parser_version: None,
        // Doc 30 TASK-TXN-012: populated by the extraction ladder when EMI
        // language was detected in the source body -- previously always
        // silently dropped here (ExtractionResult didn't even carry these
        // fields until this task added them).
        emi_total_installments: raw.emi_total_installments,
        emi_installment_number: raw.emi_installment_number,
        emi_original_amount_minor: raw.emi_original_amount_minor,
        is_deleted: false,
        created_at: Some(Utc::now().naive_utc()),
        updated_at: Some(Utc::now().naive_utc()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::ladder::ExtractionResult;

    /// Doc 30 TASK-TXN-008: fingerprint computation moved out of this
    /// function entirely (it needs the resolved `instrument_id`, which
    /// isn't known yet at this point in the pipeline) -- see
    /// `extraction::fingerprint` for the real fingerprint tests
    /// (`test_fingerprint_deterministic_for_same_inputs`,
    /// `test_fingerprint_differs_across_accounts`,
    /// `test_fingerprint_time_bucketing`).
    #[test]
    fn test_normalize_observation_leaves_fingerprint_unset() {
        let raw = ExtractionResult {
            amount_minor: Some(150000),
            currency: Some("INR".to_string()),
            direction: Some("debit".to_string()),
            event_time: Some(1704067200),
            merchant_raw: Some("Amazon".to_string()),
            ..Default::default()
        };

        let obs = normalize_observation(raw, "test_pipeline", "msg_123", None);
        assert_eq!(obs.fingerprint, None);
    }

    #[test]
    fn test_direction_normalization() {
        let mut raw = ExtractionResult {
            direction: Some("Cr.".to_string()),
            ..Default::default()
        };
        let obs1 = normalize_observation(raw.clone(), "test_pipeline", "msg_123", None);
        assert_eq!(obs1.direction.unwrap(), "credit");

        raw.direction = Some("debited".to_string());
        let obs2 = normalize_observation(raw, "test_pipeline", "msg_123", None);
        assert_eq!(obs2.direction.unwrap(), "debit");
    }

    #[test]
    fn test_currency_normalization() {
        let mut raw = ExtractionResult {
            currency: Some("usd".to_string()),
            ..Default::default()
        };
        let obs1 = normalize_observation(raw.clone(), "test_pipeline", "msg_123", None);
        assert_eq!(obs1.currency.unwrap(), "USD");

        raw.currency = None;
        let obs2 = normalize_observation(raw, "test_pipeline", "msg_123", None);
        assert_eq!(obs2.currency.unwrap(), "INR");
    }

    #[test]
    fn test_timestamp_normalization_to_ist() {
        let raw = ExtractionResult {
            // UTC time: 2024-01-01 10:00:00 UTC
            event_time: Some(1704103200),
            ..Default::default()
        };
        let obs = normalize_observation(raw, "test_pipeline", "msg_123", None);
        // IST time should be UTC + 5:30 -> 2024-01-01 15:30:00
        assert_eq!(
            obs.event_time
                .unwrap()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            "2024-01-01 15:30:00"
        );
    }
}
