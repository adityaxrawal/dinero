use crate::db::transaction_observations::TransactionObservationsRow;
use crate::extraction::ladder::ExtractionResult;
use chrono::{TimeZone, Utc};
use uuid::Uuid;

pub fn normalize_observation(
    raw: ExtractionResult,
    source_pipeline: &str,
    source_message_id: &str,
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
        exchange_rate: None,
        balance_after_transaction: raw.balance_after.map(|a| a as f64 / 100.0),
        timezone_at_ingestion: None,
        fingerprint: None,
        extraction_method: Some(raw.extraction_method),
        confidence_score: None,
        raw_payload_json: None,
        parser_version: None,
        emi_total_installments: None,
        emi_installment_number: None,
        emi_original_amount_minor: None,
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

        let obs = normalize_observation(raw, "test_pipeline", "msg_123");
        assert_eq!(obs.fingerprint, None);
    }

    #[test]
    fn test_direction_normalization() {
        let mut raw = ExtractionResult {
            direction: Some("Cr.".to_string()),
            ..Default::default()
        };
        let obs1 = normalize_observation(raw.clone(), "test_pipeline", "msg_123");
        assert_eq!(obs1.direction.unwrap(), "credit");

        raw.direction = Some("debited".to_string());
        let obs2 = normalize_observation(raw, "test_pipeline", "msg_123");
        assert_eq!(obs2.direction.unwrap(), "debit");
    }

    #[test]
    fn test_currency_normalization() {
        let mut raw = ExtractionResult {
            currency: Some("usd".to_string()),
            ..Default::default()
        };
        let obs1 = normalize_observation(raw.clone(), "test_pipeline", "msg_123");
        assert_eq!(obs1.currency.unwrap(), "USD");

        raw.currency = None;
        let obs2 = normalize_observation(raw, "test_pipeline", "msg_123");
        assert_eq!(obs2.currency.unwrap(), "INR");
    }

    #[test]
    fn test_timestamp_normalization_to_ist() {
        let raw = ExtractionResult {
            // UTC time: 2024-01-01 10:00:00 UTC
            event_time: Some(1704103200),
            ..Default::default()
        };
        let obs = normalize_observation(raw, "test_pipeline", "msg_123");
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
