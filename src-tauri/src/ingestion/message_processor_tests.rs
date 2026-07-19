use super::message_processor::*;
use crate::ingestion::gmail_client::{GmailClient, Message, MessagePart, MessagePartHeader};
use crate::ingestion::verified_senders::SenderVerificationResult;

fn create_metadata_message(headers: Vec<(&str, &str)>) -> Message {
    let header_parts = headers
        .into_iter()
        .map(|(k, v)| MessagePartHeader {
            name: k.to_string(),
            value: v.to_string(),
        })
        .collect();

    Message {
        id: "msg123".to_string(),
        thread_id: "thread123".to_string(),
        history_id: None,
        payload: Some(MessagePart {
            part_id: None,
            mime_type: "multipart/mixed".to_string(),
            filename: None,
            headers: Some(header_parts),
            body: None,
            parts: None,
        }),
        internal_date: None,
        snippet: None,
    }
}

#[test]
fn test_metadata_gate_passes() {
    let msg = create_metadata_message(vec![
        ("From", "\"HDFC Bank\" <alerts@hdfcbank.net>"),
        ("Subject", "Your Transaction"),
        ("Date", "Mon, 1 Jan 2026 12:00:00 +0000"),
    ]);

    assert_eq!(
        MessageProcessor::evaluate_metadata_gate(&msg, false, &[]),
        SenderVerificationResult::VerifiedTransactionCandidate("HDFC Bank".to_string())
    );
}

#[test]
fn test_metadata_gate_fails_no_sender() {
    let msg = create_metadata_message(vec![("Subject", "Your Amazon.com order")]);

    match MessageProcessor::evaluate_metadata_gate(&msg, false, &[]) {
        SenderVerificationResult::UnverifiedReject(_) => {}
        _ => panic!("Expected UnverifiedReject"),
    }
}

#[test]
fn test_metadata_gate_fails_empty_sender() {
    let msg = create_metadata_message(vec![("From", "   "), ("Subject", "Receipt")]);

    match MessageProcessor::evaluate_metadata_gate(&msg, false, &[]) {
        SenderVerificationResult::UnverifiedReject(_) => {}
        _ => panic!("Expected UnverifiedReject"),
    }
}

#[test]
fn test_metadata_gate_spoof_detection() {
    let msg = create_metadata_message(vec![
        ("From", "\"HDFC Bank\" <alerts@hdfcbnk.net>"),
        ("Subject", "Urgent Action Required"),
    ]);

    match MessageProcessor::evaluate_metadata_gate(&msg, false, &[]) {
        SenderVerificationResult::SpoofReject(reason) => {
            assert!(reason.contains("Typo-squatted"));
        }
        _ => panic!("Expected SpoofReject"),
    }
}

/// The subject-based "Unknown Bank" rescue must not fire on a domain's
/// very first-ever sighting (`domain_previously_seen = false`) even when
/// the subject is transaction-shaped -- it should fall through to the
/// original rejection instead.
#[test]
fn test_metadata_gate_unknown_bank_rescue_requires_prior_sighting() {
    let msg = create_metadata_message(vec![
        ("From", "alerts@some-unregistered-bank.example"),
        ("Subject", "You spent Rs. 500 today"),
    ]);

    match MessageProcessor::evaluate_metadata_gate(&msg, false, &[]) {
        SenderVerificationResult::UnverifiedReject(_) => {}
        other => panic!(
            "Expected UnverifiedReject on first sighting, got {:?}",
            other
        ),
    }

    match MessageProcessor::evaluate_metadata_gate(&msg, true, &[]) {
        SenderVerificationResult::VerifiedTransactionCandidate(bank) => {
            assert_eq!(bank, "Unknown Bank");
        }
        other => panic!(
            "Expected the Unknown Bank rescue once previously seen, got {:?}",
            other
        ),
    }
}

/// A user-approved pending sender must resolve as verified even though its
/// domain would otherwise fail every string-based heuristic.
#[test]
fn test_metadata_gate_approved_pending_sender_verified() {
    let msg = create_metadata_message(vec![
        ("From", "alerts@newfintech.example"),
        ("Subject", "Transaction Alert"),
    ]);

    let approved = vec![crate::db::sender_reputation::PendingSenderRow {
        id: "id1".to_string(),
        domain: "newfintech.example".to_string(),
        bank_name: "New Fintech".to_string(),
        classification: "transaction_candidate".to_string(),
        status: "approved".to_string(),
        reject_count: 3,
    }];

    assert_eq!(
        MessageProcessor::evaluate_metadata_gate(&msg, false, &approved),
        SenderVerificationResult::VerifiedTransactionCandidate("New Fintech".to_string())
    );
}

use crate::extraction::ladder::ExtractionResult;

#[test]
fn test_gate3_passes_with_amount_and_merchant() {
    let obs = ExtractionResult {
        amount_minor: Some(1000),
        merchant_raw: Some("Amazon".to_string()),
        extraction_method: "test".to_string(),
        ..Default::default()
    };
    assert!(MessageProcessor::evaluate_mandatory_field_gate(&obs));
}

#[test]
fn test_gate3_fails_amount_only() {
    let obs = ExtractionResult {
        amount_minor: Some(1000),
        merchant_raw: None,
        extraction_method: "test".to_string(),
        ..Default::default()
    };
    assert!(!MessageProcessor::evaluate_mandatory_field_gate(&obs));
    assert_eq!(
        MessageProcessor::gate3_failure_reason(&obs),
        "gate3_failed:missing_counterparty"
    );
}

#[test]
fn test_gate3_fails_merchant_only() {
    let obs = ExtractionResult {
        amount_minor: None,
        merchant_raw: Some("Amazon".to_string()),
        extraction_method: "test".to_string(),
        ..Default::default()
    };
    assert!(!MessageProcessor::evaluate_mandatory_field_gate(&obs));
    assert_eq!(
        MessageProcessor::gate3_failure_reason(&obs),
        "gate3_failed:missing_amount"
    );
}

/// Doc 30 TASK-GMAIL-002: proves the metadata-first cost saving directly —
/// when Gate 1 rejects based on the metadata-only fetch, no full-body
/// (`format=full`) request is ever made. `full_mock.expect(0)` fails the
/// test if `process_message` fetched the body anyway.
#[tokio::test]
async fn test_metadata_fetch_before_full_fetch() {
    let mut server = mockito::Server::new_async().await;

    let metadata_mock = server
        .mock("GET", "/gmail/v1/users/me/messages/msg1")
        .match_query(mockito::Matcher::UrlEncoded(
            "format".into(),
            "metadata".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "id": "msg1",
                "threadId": "t1",
                "payload": {
                    "mimeType": "multipart/mixed",
                    "headers": [
                        {"name": "From", "value": "\"Newsletter\" <someone@example.com>"},
                        {"name": "Subject", "value": "Weekly Newsletter"}
                    ]
                }
            })
            .to_string(),
        )
        .create_async()
        .await;

    let full_mock = server
        .mock("GET", "/gmail/v1/users/me/messages/msg1")
        .match_query(mockito::Matcher::UrlEncoded("format".into(), "full".into()))
        .expect(0)
        .create_async()
        .await;

    let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

    let client =
        GmailClient::new_with_base_url("fake_token".into(), pool.clone(), server.url(), None);

    let result = MessageProcessor::process_message(&pool, &client, "msg1", None, false)
        .await
        .unwrap();
    assert!(
        result.is_none(),
        "unverified sender must be rejected at Gate 1"
    );

    metadata_mock.assert_async().await;
    full_mock.assert_async().await;

    let _ = std::fs::remove_dir_all(&temp_dir);
}
