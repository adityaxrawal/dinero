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
        MessageProcessor::evaluate_metadata_gate(&msg, &[], &[]),
        SenderVerificationResult::VerifiedTransactionCandidate("HDFC Bank".to_string())
    );
}

#[test]
fn test_metadata_gate_fails_no_sender() {
    let msg = create_metadata_message(vec![("Subject", "Your Amazon.com order")]);

    match MessageProcessor::evaluate_metadata_gate(&msg, &[], &[]) {
        SenderVerificationResult::UnverifiedReject(_) => {}
        _ => panic!("Expected UnverifiedReject"),
    }
}

#[test]
fn test_metadata_gate_fails_empty_sender() {
    let msg = create_metadata_message(vec![("From", "   "), ("Subject", "Receipt")]);

    match MessageProcessor::evaluate_metadata_gate(&msg, &[], &[]) {
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

    match MessageProcessor::evaluate_metadata_gate(&msg, &[], &[]) {
        SenderVerificationResult::SpoofReject(reason) => {
            assert!(reason.contains("Typo-squatted"));
        }
        _ => panic!("Expected SpoofReject"),
    }
}

/// The subject-based "Unknown Bank" auto-promotion has been removed
/// entirely (2026-07-30 gate hardening, following the
/// notification_jiofibre@jio.com false-positive): an unregistered domain
/// must be rejected by Gate 1 no matter how transaction-shaped its subject
/// looks or how many times it's been seen before. Only the pending-senders
/// manual-approval path (`test_metadata_gate_approved_pending_sender_verified`)
/// can promote it.
#[test]
fn test_metadata_gate_never_auto_promotes_unregistered_domain_by_subject() {
    let msg = create_metadata_message(vec![
        ("From", "notification_jiofibre@jio.com"),
        ("Subject", "Payment received for your Jio Fiber bill"),
    ]);

    match MessageProcessor::evaluate_metadata_gate(&msg, &[], &[]) {
        SenderVerificationResult::UnverifiedReject(_) => {}
        other => panic!(
            "Expected UnverifiedReject for an unregistered domain regardless of subject wording, got {:?}",
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
        MessageProcessor::evaluate_metadata_gate(&msg, &approved, &[]),
        SenderVerificationResult::VerifiedTransactionCandidate("New Fintech".to_string())
    );
}

/// A second `@` makes the trailing label not the sending domain. Reading it as
/// one would let `victim@evil.example@hdfcbank.net` match an approved domain and
/// collect a verified verdict, so it must be rejected instead.
#[test]
fn test_metadata_gate_rejects_address_with_two_at_signs() {
    let msg = create_metadata_message(vec![
        ("From", "<victim@evil.example@newfintech.example>"),
        ("Subject", "Transaction Alert"),
    ]);

    let approved = vec![crate::db::sender_reputation::PendingSenderRow {
        id: "id1".to_string(),
        domain: "newfintech.example".to_string(),
        bank_name: "New Fintech".to_string(),
        classification: "transaction_candidate".to_string(),
        status: "approved".to_string(),
        reject_count: 0,
    }];

    match MessageProcessor::evaluate_metadata_gate(&msg, &approved, &[]) {
        SenderVerificationResult::UnverifiedReject(_)
        | SenderVerificationResult::SpoofReject(_) => {}
        other => panic!("a two-`@` address must never verify, got {:?}", other),
    }
    assert_eq!(MessageProcessor::extract_sender_domain(&msg), None);
}

/// Two From headers must not verify against one domain while reputation is
/// recorded against the other -- both readers take the first.
#[test]
fn test_metadata_gate_and_sighting_agree_on_first_from_header() {
    let msg = create_metadata_message(vec![
        ("From", "\"HDFC Bank\" <alerts@hdfcbank.net>"),
        ("From", "<attacker@evil.example>"),
        ("Subject", "Your Transaction"),
    ]);

    assert_eq!(
        MessageProcessor::extract_sender_domain(&msg),
        Some("hdfcbank.net".to_string())
    );
    assert_eq!(
        MessageProcessor::evaluate_metadata_gate(&msg, &[], &[]),
        SenderVerificationResult::VerifiedTransactionCandidate("HDFC Bank".to_string())
    );
}

/// An override domain is whatever the user typed, so it must be normalised on
/// both sides or it silently never applies.
#[test]
fn test_bank_override_matches_despite_case_and_whitespace() {
    let overrides = vec![crate::db::sender_bank_overrides::SenderBankOverride {
        id: "o1".to_string(),
        domain: "  HDFCBank.NET ".to_string(),
        bank_name: "HDFC Bank Ltd".to_string(),
        display_name: None,
        status: "active".to_string(),
        created_at: None,
    }];

    assert_eq!(
        MessageProcessor::apply_bank_override(
            SenderVerificationResult::VerifiedTransactionCandidate("HDFC Bank".to_string()),
            "hdfcbank.net",
            &overrides,
        ),
        SenderVerificationResult::VerifiedTransactionCandidate("HDFC Bank Ltd".to_string())
    );
}

/// A comma inside a quoted display name must not be mistaken for an address
/// separator, and a multi-recipient header must yield its first address.
#[test]
fn test_extract_email_and_domain_handles_commas_and_multiple_addresses() {
    assert_eq!(
        MessageProcessor::extract_email_and_domain("\"Doe, John\" <j@d.example>"),
        (
            Some("j@d.example".to_string()),
            Some("d.example".to_string())
        )
    );
    assert_eq!(
        MessageProcessor::extract_email_and_domain("<a@x.example>, <b@y.example>"),
        (
            Some("a@x.example".to_string()),
            Some("x.example".to_string())
        )
    );
    // Malformed: an address, but no domain that can be trusted for matching.
    assert_eq!(
        MessageProcessor::extract_email_and_domain("a@b.example@c.example"),
        (Some("a@b.example@c.example".to_string()), None)
    );
}

use crate::extraction::ladder::ExtractionResult;

#[test]
fn test_gate3_passes_with_amount_merchant_and_instrument() {
    let obs = ExtractionResult {
        amount_minor: Some(1000),
        merchant_raw: Some("Amazon".to_string()),
        instrument_type: Some("credit_card".to_string()),
        issuer_name: Some("HDFC Bank".to_string()),
        masked_identifier: Some("1234".to_string()),
        extraction_method: "test".to_string(),
        ..Default::default()
    };
    assert!(MessageProcessor::evaluate_mandatory_field_gate(&obs));
}

#[test]
fn test_gate3_fails_without_instrument_signals() {
    let obs = ExtractionResult {
        amount_minor: Some(1000),
        merchant_raw: Some("Amazon".to_string()),
        instrument_type: None,
        issuer_name: None,
        masked_identifier: None,
        extraction_method: "test".to_string(),
        ..Default::default()
    };
    assert!(!MessageProcessor::evaluate_mandatory_field_gate(&obs));
    assert_eq!(
        MessageProcessor::gate3_failure_reason(&obs),
        "gate3_failed:missing_instrument"
    );
}

#[test]
fn test_gate3_balance_only_still_bypasses_instrument_requirement() {
    // Matches existing has_balance-bypass behavior: a balance-only email is
    // exempt from every other mandatory field, instrument included.
    let obs = ExtractionResult {
        balance_after: Some(677300),
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

use crate::ingestion::content_classifier::ContentClass;

/// Regression test for the HDFC "available balance" bug: a `BalanceUpdate`
/// email where extraction mis-fired a garbage merchant ("real", lifted from
/// "real-time balance updates") and treated the balance figure as a
/// transaction amount must have both discarded in favor of the safe
/// "Balance Update" / 0 placeholder, even though `merchant_raw` was `Some`.
#[test]
fn test_balance_update_placeholder_discards_misfired_merchant_and_amount() {
    let mut obs = ExtractionResult {
        merchant_raw: Some("real".to_string()),
        amount_minor: Some(677300),
        balance_after: Some(677300),
        extraction_method: "test".to_string(),
        ..Default::default()
    };
    MessageProcessor::apply_balance_update_placeholder(&ContentClass::BalanceUpdate, &mut obs);
    assert_eq!(obs.merchant_raw, Some("Balance Update".to_string()));
    assert_eq!(obs.amount_minor, Some(0));
}

/// A `TransactionAlert` email that genuinely extracted no merchant but did
/// resolve a balance keeps the narrower, additive fallback: it must not
/// clobber a real extracted amount just because a balance was also present.
#[test]
fn test_transaction_alert_balance_fallback_only_fills_missing_fields() {
    let mut obs = ExtractionResult {
        merchant_raw: None,
        amount_minor: Some(24543),
        balance_after: Some(677300),
        extraction_method: "test".to_string(),
        ..Default::default()
    };
    MessageProcessor::apply_balance_update_placeholder(&ContentClass::TransactionAlert, &mut obs);
    assert_eq!(obs.merchant_raw, Some("Balance Update".to_string()));
    assert_eq!(
        obs.amount_minor,
        Some(24543),
        "a real extracted amount must not be overwritten"
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

    let result = MessageProcessor::process_message(&pool, &client, "msg1", None, false, None, None)
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

/// Spec optimization #2: Gate 2a classifies off the metadata-only Subject +
/// snippet *before* any `format=full` fetch. A verified sender ("HDFC
/// Bank") sending an unambiguous non-transactional subject must never
/// trigger the full-body fetch at all -- `full_mock.expect(0)` fails the
/// test otherwise.
#[tokio::test]
async fn test_gate2a_rejects_marketing_subject_without_full_fetch() {
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
                "snippet": "We've updated our terms. No action needed from you.",
                "payload": {
                    "mimeType": "multipart/mixed",
                    "headers": [
                        {"name": "From", "value": "\"HDFC Bank\" <alerts@hdfcbank.net>"},
                        {"name": "Subject", "value": "Updated Privacy Policy"}
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

    let result = MessageProcessor::process_message(&pool, &client, "msg1", None, false, None, None)
        .await
        .unwrap();
    assert!(
        result.is_none(),
        "an unambiguous non-transactional subject must be rejected at Gate 2a"
    );

    metadata_mock.assert_async().await;
    full_mock.assert_async().await;

    let _ = std::fs::remove_dir_all(&temp_dir);
}

/// Spec optimization #5: a Gate 2a `Noise`/`Unknown` verdict (ambiguous, not
/// a confident rejection like Marketing/OTP/KYC/Reminder) must land in the
/// recoverable `ignored_messages` table, not just the audit log.
#[tokio::test]
async fn test_gate2a_noise_routes_to_ignored_table() {
    let mut server = mockito::Server::new_async().await;

    let _metadata_mock = server
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
                "snippet": "Please review the attached notice.",
                "payload": {
                    "mimeType": "multipart/mixed",
                    "headers": [
                        {"name": "From", "value": "\"HDFC Bank\" <alerts@hdfcbank.net>"},
                        {"name": "Subject", "value": "Important notice regarding your account"}
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

    let result = MessageProcessor::process_message(&pool, &client, "msg1", None, false, None, None)
        .await
        .unwrap();
    assert!(result.is_none());
    full_mock.assert_async().await;

    let conn = pool.get().await.unwrap();
    let ignored = conn
        .interact(|c| crate::db::ignored_messages::select_recent(c, 10))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        ignored.len(),
        1,
        "a Gate 2a Noise/Unknown verdict must be recorded in ignored_messages, not just discarded"
    );
    assert_eq!(ignored[0].message_id, "msg1");
    assert!(ignored[0].reason.starts_with("gate2a_reject_"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

/// Spec optimization #2's Gate 2b test: a metadata Subject that plausibly
/// looks transactional must still trigger the full-body fetch (Gate 2a must
/// not falsely reject real transaction candidates just to save bandwidth).
#[tokio::test]
async fn test_gate2a_proceeds_to_full_fetch_for_transactional_subject() {
    let mut server = mockito::Server::new_async().await;

    let _metadata_mock = server
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
                "snippet": "Rs 500.00 debited from your account.",
                "payload": {
                    "mimeType": "multipart/mixed",
                    "headers": [
                        {"name": "From", "value": "\"HDFC Bank\" <alerts@hdfcbank.net>"},
                        {"name": "Subject", "value": "Rs 500 debited from your account"}
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
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "id": "msg1",
                "threadId": "t1",
                "snippet": "Rs 500.00 debited from your account.",
                "payload": {
                    "mimeType": "text/plain",
                    "headers": [
                        {"name": "From", "value": "\"HDFC Bank\" <alerts@hdfcbank.net>"},
                        {"name": "Subject", "value": "Rs 500 debited from your account"}
                    ],
                    "body": {"data": ""}
                }
            })
            .to_string(),
        )
        .create_async()
        .await;

    let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

    let client =
        GmailClient::new_with_base_url("fake_token".into(), pool.clone(), server.url(), None);

    let result =
        MessageProcessor::process_message(&pool, &client, "msg1", None, false, None, None).await;
    assert!(result.is_ok(), "must not error: {:?}", result.err());

    full_mock.assert_async().await;

    let _ = std::fs::remove_dir_all(&temp_dir);
}

/// Doc 2026-07-26 mail scan performance: Layers 1-5 all fail on an empty
/// body, this machine is LLM-eligible, so a Layer6Job must be enqueued for
/// background processing instead of awaiting Layer 6 inline.
#[tokio::test]
async fn transaction_needing_layer6_enqueues_job_instead_of_blocking() {
    let mut server = mockito::Server::new_async().await;

    let _metadata_mock = server
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
                "snippet": "Rs 500.00 debited from your account.",
                "payload": {
                    "mimeType": "multipart/mixed",
                    "headers": [
                        {"name": "From", "value": "\"HDFC Bank\" <alerts@hdfcbank.net>"},
                        {"name": "Subject", "value": "Rs 500 debited from your account"}
                    ]
                }
            })
            .to_string(),
        )
        .create_async()
        .await;

    let _full_mock = server
        .mock("GET", "/gmail/v1/users/me/messages/msg1")
        .match_query(mockito::Matcher::UrlEncoded("format".into(), "full".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "id": "msg1",
                "threadId": "t1",
                "snippet": "Rs 500.00 debited from your account.",
                "payload": {
                    "mimeType": "text/plain",
                    "headers": [
                        {"name": "From", "value": "\"HDFC Bank\" <alerts@hdfcbank.net>"},
                        {"name": "Subject", "value": "Rs 500 debited from your account"}
                    ],
                    "body": {"data": ""}
                }
            })
            .to_string(),
        )
        .create_async()
        .await;

    let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();
    let client =
        GmailClient::new_with_base_url("fake_token".into(), pool.clone(), server.url(), None);

    let (layer6_tx, mut layer6_rx) = tokio::sync::mpsc::channel(4);

    let start = std::time::Instant::now();
    let result = MessageProcessor::process_message(
        &pool,
        &client,
        "msg1",
        Some(temp_dir.clone()),
        /* llm_eligible */ true,
        Some(layer6_tx),
        None,
    )
    .await
    .unwrap();

    // Must return promptly — no inline LLM wait. This test never spawns a
    // real llama-server, so any accidental inline await would hang/timeout
    // this test rather than merely being slow; 2s is generous headroom.
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    assert!(matches!(result, Some(ProcessResult::EnqueuedForEnrichment)));

    let job = layer6_rx
        .try_recv()
        .expect("a Layer6Job must have been enqueued");
    assert_eq!(job.bank_name, "HDFC Bank");
    assert!(!job.observation_id.is_empty());
    assert!(!job.unassigned_id.is_empty());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

/// Spec optimization #1: a Gate 3 mandatory-field-gate failure (partial
/// extraction -- merchant found, amount missing) must still land in
/// `unassigned_transactions` with the specific failure reason, and the
/// linked `transaction_observations` row must keep the fields that *were*
/// extracted, not just a raw-body stub.
#[tokio::test]
async fn test_gate3_partial_extraction_salvaged_to_unassigned_queue() {
    let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

    let partial_obs = ExtractionResult {
        merchant_raw: Some("Amazon".to_string()),
        currency: Some("INR".to_string()),
        direction: Some("debit".to_string()),
        event_time: Some(1704067200),
        extraction_method: "test_layer".to_string(),
        // amount_minor deliberately left None -- this is the missing field.
        ..Default::default()
    };
    assert!(!MessageProcessor::evaluate_mandatory_field_gate(
        &partial_obs
    ));
    let reason = MessageProcessor::gate3_failure_reason(&partial_obs);
    assert_eq!(reason, "gate3_failed:missing_amount");

    MessageProcessor::record_unassigned_transaction(
        &pool,
        "msg_partial",
        partial_obs,
        "You spent at Amazon.",
        None,
        reason,
    )
    .await
    .unwrap();

    let conn = pool.get().await.unwrap();
    let (obs_reason, obs_id): (String, String) = conn
        .interact(|c| {
            c.query_row(
                "SELECT reason, observation_id FROM unassigned_transactions",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(obs_reason, "gate3_failed:missing_amount");

    let merchant_raw: Option<String> = conn
        .interact(move |c| {
            c.query_row(
                "SELECT merchant_raw FROM transaction_observations WHERE id = ?1",
                [obs_id],
                |r| r.get(0),
            )
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        merchant_raw,
        Some("Amazon".to_string()),
        "partial extraction fields must survive into the salvaged observation row"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn record_unassigned_transaction_returns_ids_on_fresh_insert() {
    let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let pool = crate::db::init_db(temp_dir.join("test.db"))
        .await
        .expect("DB init failed");

    let obs = crate::extraction::ladder::ExtractionResult::default();
    let result = MessageProcessor::record_unassigned_transaction(
        &pool,
        "msg_fresh",
        obs,
        "body text",
        None,
        "pending_llm_enrichment",
    )
    .await
    .unwrap();

    assert!(
        result.is_some(),
        "a fresh insert must return Some((observation_id, unassigned_id))"
    );
    let (observation_id, unassigned_id) = result.unwrap();
    assert!(!observation_id.is_empty());
    assert!(!unassigned_id.is_empty());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn record_unassigned_transaction_returns_none_on_duplicate() {
    let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let pool = crate::db::init_db(temp_dir.join("test.db"))
        .await
        .expect("DB init failed");

    let obs = crate::extraction::ladder::ExtractionResult::default();
    MessageProcessor::record_unassigned_transaction(
        &pool,
        "msg_dup",
        obs.clone(),
        "body text",
        None,
        "pending_llm_enrichment",
    )
    .await
    .unwrap();

    // Same message_id + same default obs => same fingerprint => duplicate.
    let second = MessageProcessor::record_unassigned_transaction(
        &pool,
        "msg_dup",
        obs,
        "body text",
        None,
        "pending_llm_enrichment",
    )
    .await
    .unwrap();

    assert!(
        second.is_none(),
        "a duplicate insert must return None, not a fresh id pair"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}
