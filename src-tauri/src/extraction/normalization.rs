//! Normalises an extracted observation into storable form.
//!
//! Applies the conventions the rest of the system relies on -- amounts as
//! integer minor units, timestamps as epoch values, masked identifiers reduced
//! to a consistent shape so the same card matches itself across banks that print
//! it differently.
use crate::db::transaction_observations::TransactionObservationsRow;
use crate::extraction::ladder::ExtractionResult;
use chrono::{TimeZone, Utc};
use uuid::Uuid;

/// Normalises an extracted observation into storable form.
///
/// Applies the conventions the rest of the system depends on: integer minor units
/// for money, epoch timestamps, and canonicalised identifiers. Doing it once here
/// means downstream code never has to guess which representation it received.
pub fn normalize_observation(
    raw: ExtractionResult,
    source_pipeline: &str,
    source_message_id: &str,
    raw_body: Option<&str>,
    email_meta: Option<&crate::ingestion::message_processor::EmailMetadata>,
) -> TransactionObservationsRow {
    let amount_minor = raw.amount_minor;

    let currency = raw
        .currency
        .clone()
        .map(|c| c.to_uppercase())
        .unwrap_or_else(|| "INR".to_string());

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

    let event_time = raw.event_time.map(|ts| {
        let dt_utc = Utc.timestamp_opt(ts, 0).unwrap();
        let ist_offset = chrono::FixedOffset::east_opt(5 * 3600 + 30 * 60).unwrap();
        dt_utc.with_timezone(&ist_offset).naive_local()
    });

    let merchant_raw = raw.merchant_raw.clone();

    let raw_payload_json = raw_body.map(|b| {
        let mut payload = serde_json::json!({
            "body": b,
            "html": email_meta.and_then(|m| m.html.as_deref()),
        });
        if let Some(meta) = email_meta {
            payload["subject"] = serde_json::json!(meta.subject);
            payload["date"] = serde_json::json!(meta.date);
            payload["sender"] = serde_json::json!(meta.sender);
            payload["sender_email"] = serde_json::json!(meta.sender_email);
            payload["sender_domain"] = serde_json::json!(meta.sender_domain);
            payload["recipient"] = serde_json::json!(meta.recipient);
            payload["recipient_email"] = serde_json::json!(meta.recipient_email);
            payload["recipient_domain"] = serde_json::json!(meta.recipient_domain);
        }
        payload.to_string()
    });

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
        event_time_confidence: raw.date_cross_check_flag.clone(),
        posting_date: None,
        merchant_raw,
        merchant_normalized: None,
        reference_id: raw.reference_id,
        original_amount_minor: raw.original_amount_minor,
        original_currency: raw.original_currency,
        exchange_rate: raw.exchange_rate,
        balance_after_transaction: raw.balance_after.map(|a| a as f64 / 100.0),
        timezone_at_ingestion: None,
        fingerprint: None,
        extraction_method: Some(raw.extraction_method),
        confidence_score: raw.confidence_score,
        raw_payload_json,
        parser_version: None,
        emi_total_installments: raw.emi_total_installments,
        emi_installment_number: raw.emi_installment_number,
        emi_original_amount_minor: raw.emi_original_amount_minor,
        channel: raw.channel,
        is_deleted: false,
        created_at: Some(Utc::now().naive_utc()),
        updated_at: Some(Utc::now().naive_utc()),
    }
}

/// Reduces a masked account identifier to a consistent form.
///
/// Banks print the same card as `XX1234`, `****1234` and `...1234`. Without
/// canonicalisation the same account would key several distinct instruments, and
/// attribution would fragment across them.
pub fn clean_masked_identifier(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains('@') {
        return trimmed.to_string();
    }

    let after_mask: String = trimmed
        .rfind(['X', 'x', '*'])
        .map(|i| {
            trimmed[i + 1..]
                .chars()
                .filter(|c| c.is_ascii_digit())
                .collect()
        })
        .unwrap_or_default();
    if !after_mask.is_empty() {
        return after_mask;
    }

    let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return trimmed.to_string();
    }

    if digits.len() > 4 {
        digits[digits.len() - 4..].to_string()
    } else {
        digits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::ladder::ExtractionResult;

    #[test]
    fn test_clean_masked_identifier_all_edge_cases() {
        assert_eq!(clean_masked_identifier("XXXX1234"), "1234");
        assert_eq!(clean_masked_identifier("XXXXXX1234"), "1234");
        assert_eq!(clean_masked_identifier("1234"), "1234");
        assert_eq!(clean_masked_identifier("XXXX34"), "34");
        assert_eq!(clean_masked_identifier("XXXX 1234"), "1234");
        assert_eq!(clean_masked_identifier("XXXX XXXX 1234"), "1234");
        assert_eq!(clean_masked_identifier("**** **** **** 1234"), "1234");
        assert_eq!(clean_masked_identifier("XX-1234"), "1234");
        assert_eq!(clean_masked_identifier("4532 1234 5678 9012"), "9012");
        assert_eq!(clean_masked_identifier("user@upi"), "user@upi");
        assert_eq!(clean_masked_identifier("  XXXX 5678  "), "5678");
        assert_eq!(clean_masked_identifier(""), "");
        assert_eq!(clean_masked_identifier("5268XXXXXXXXXX64"), "64");
        assert_eq!(clean_masked_identifier("6529XXXXXXXXXX56"), "56");
        assert_eq!(clean_masked_identifier("1234XXXX"), "1234");
    }

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

        let obs = normalize_observation(raw, "test_pipeline", "msg_123", None, None);
        assert_eq!(obs.fingerprint, None);
    }

    #[test]
    fn test_direction_normalization() {
        let mut raw = ExtractionResult {
            direction: Some("Cr.".to_string()),
            ..Default::default()
        };
        let obs1 = normalize_observation(raw.clone(), "test_pipeline", "msg_123", None, None);
        assert_eq!(obs1.direction.unwrap(), "credit");

        raw.direction = Some("debited".to_string());
        let obs2 = normalize_observation(raw, "test_pipeline", "msg_123", None, None);
        assert_eq!(obs2.direction.unwrap(), "debit");
    }

    #[test]
    fn test_currency_normalization() {
        let mut raw = ExtractionResult {
            currency: Some("usd".to_string()),
            ..Default::default()
        };
        let obs1 = normalize_observation(raw.clone(), "test_pipeline", "msg_123", None, None);
        assert_eq!(obs1.currency.unwrap(), "USD");

        raw.currency = None;
        let obs2 = normalize_observation(raw, "test_pipeline", "msg_123", None, None);
        assert_eq!(obs2.currency.unwrap(), "INR");
    }

    #[test]
    fn test_extract_email_and_domain_edge_cases() {
        use crate::ingestion::message_processor::MessageProcessor;

        let (e1, d1) = MessageProcessor::extract_email_and_domain(
            "ASSPL Bangalore kaIN <assplbangalorekain@bank.com>",
        );
        assert_eq!(e1, Some("assplbangalorekain@bank.com".to_string()));
        assert_eq!(d1, Some("bank.com".to_string()));

        let (e2, d2) =
            MessageProcessor::extract_email_and_domain("\"User Name\" <user@sub.domain.co.in>");
        assert_eq!(e2, Some("user@sub.domain.co.in".to_string()));
        assert_eq!(d2, Some("sub.domain.co.in".to_string()));

        let (e3, d3) = MessageProcessor::extract_email_and_domain("support@jupiter.money");
        assert_eq!(e3, Some("support@jupiter.money".to_string()));
        assert_eq!(d3, Some("jupiter.money".to_string()));

        let (e4, d4) = MessageProcessor::extract_email_and_domain("To: user1@a.com, user2@b.com");
        assert_eq!(e4, Some("user1@a.com".to_string()));
        assert_eq!(d4, Some("a.com".to_string()));

        let (e5, d5) = MessageProcessor::extract_email_and_domain("");
        assert_eq!(e5, None);
        assert_eq!(d5, None);

        let (e6, d6) = MessageProcessor::extract_email_and_domain("Invalid Header String");
        assert_eq!(e6, None);
        assert_eq!(d6, None);
    }

    #[test]
    fn test_normalize_observation_populates_payload_metadata() {
        use crate::ingestion::message_processor::EmailMetadata;

        let raw = ExtractionResult {
            amount_minor: Some(55500),
            currency: Some("INR".to_string()),
            merchant_raw: Some("ASSPL".to_string()),
            ..Default::default()
        };

        let email_meta = EmailMetadata {
            sender: "ASSPL <asspl@bank.com>".to_string(),
            recipient: "me <aditya@example.com>".to_string(),
            subject: "Payment successful".to_string(),
            date: "Jan 3, 2026".to_string(),
            snippet: "Snippet text".to_string(),
            html: Some("<p>HTML</p>".to_string()),
            sender_email: Some("asspl@bank.com".to_string()),
            sender_domain: Some("bank.com".to_string()),
            recipient_email: Some("aditya@example.com".to_string()),
            recipient_domain: Some("example.com".to_string()),
        };

        let obs = normalize_observation(
            raw,
            "gmail_transaction",
            "msg_123",
            Some("Plain text body"),
            Some(&email_meta),
        );
        assert!(obs.raw_payload_json.is_some());
        let payload: serde_json::Value =
            serde_json::from_str(&obs.raw_payload_json.unwrap()).unwrap();

        assert_eq!(payload["body"], "Plain text body");
        assert_eq!(payload["html"], "<p>HTML</p>");
        assert_eq!(payload["subject"], "Payment successful");
        assert_eq!(payload["date"], "Jan 3, 2026");
        assert_eq!(payload["sender"], "ASSPL <asspl@bank.com>");
        assert_eq!(payload["sender_email"], "asspl@bank.com");
        assert_eq!(payload["sender_domain"], "bank.com");
        assert_eq!(payload["recipient"], "me <aditya@example.com>");
        assert_eq!(payload["recipient_email"], "aditya@example.com");
        assert_eq!(payload["recipient_domain"], "example.com");
    }

    #[test]
    fn test_timestamp_normalization_to_ist() {
        let raw = ExtractionResult {
            event_time: Some(1704103200),
            ..Default::default()
        };
        let obs = normalize_observation(raw, "test_pipeline", "msg_123", None, None);
        assert_eq!(
            obs.event_time
                .unwrap()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            "2024-01-01 15:30:00"
        );
    }
}
