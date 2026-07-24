use crate::db::transaction_observations::{insert_observation, TransactionObservationsRow};
use crate::db::unassigned_transactions::{
    insert as insert_unassigned, select_open_with_context, update_status, UnassignedTransactionRow,
};
use chrono::Utc;
use rusqlite::Connection;

fn setup_db() -> Connection {
    crate::db::test_helpers::setup_test_db()
}

fn base_observation(id: &str) -> TransactionObservationsRow {
    TransactionObservationsRow {
        id: id.to_string(),
        canonical_transaction_id: None,
        source_pipeline: Some("gmail_transaction".to_string()),
        source_record_id: Some("rec_1".to_string()),
        source_message_id: Some("msg_123".to_string()),
        source_thread_id: None,
        statement_id: None,
        statement_entry_id: None,
        instrument_id: None,
        direction: None,
        amount: None,
        amount_minor: None,
        currency: None,
        event_time: Some(Utc::now().naive_utc()),
        event_time_confidence: None,
        posting_date: None,
        merchant_raw: None,
        merchant_normalized: None,
        reference_id: None,
        original_amount_minor: None,
        original_currency: None,
        exchange_rate: None,
        balance_after_transaction: None,
        timezone_at_ingestion: None,
        fingerprint: None,
        extraction_method: Some("extraction_failed".to_string()),
        confidence_score: None,
        raw_payload_json: None,
        parser_version: None,
        emi_total_installments: None,
        emi_installment_number: None,
        emi_original_amount_minor: None,
        is_deleted: false,
        created_at: None,
        updated_at: None,
    }
}

#[test]
fn test_unassigned_transactions_lifecycle() {
    let conn = setup_db();

    // Need an observation to reference
    let obs = TransactionObservationsRow {
        id: "obs_1".to_string(),
        canonical_transaction_id: None,
        source_pipeline: Some("manual".to_string()),
        source_record_id: Some("rec_1".to_string()),
        source_message_id: None,
        source_thread_id: None,
        statement_id: None,
        statement_entry_id: None,
        instrument_id: None,
        direction: Some("DEBIT".to_string()),
        amount: Some(10.0),
        amount_minor: Some(1000),
        currency: Some("USD".to_string()),
        event_time: Some(Utc::now().naive_utc()),
        event_time_confidence: None,
        posting_date: None,
        merchant_raw: Some("Test Merchant".to_string()),
        merchant_normalized: None,
        reference_id: None,
        original_amount_minor: None,
        original_currency: None,
        exchange_rate: None,
        balance_after_transaction: None,
        timezone_at_ingestion: None,
        fingerprint: Some("fp_1".to_string()),
        extraction_method: None,
        confidence_score: None,
        raw_payload_json: None,
        parser_version: None,
        emi_total_installments: None,
        emi_installment_number: None,
        emi_original_amount_minor: None,
        is_deleted: false,
        created_at: None,
        updated_at: None,
    };
    insert_observation(&conn, &obs).unwrap();

    let unassigned = UnassignedTransactionRow {
        id: "ua_1".to_string(),
        observation_id: "obs_1".to_string(),
        reason: "No matched instrument".to_string(),
        status: "open".to_string(),
        created_at: None,
    };

    insert_unassigned(&conn, &unassigned).unwrap();

    update_status(&conn, "ua_1", "resolved").unwrap();

    // Test passed if no errors
}

/// Regression test: `select_open` gave the Reconciliation UI nothing to show
/// beyond a raw reason code and an opaque observation UUID.
/// `select_open_with_context` joins the linked observation for
/// merchant/amount/currency/source_message_id plus a whitespace-collapsed,
/// length-capped snippet of the original email body (stored as
/// `{"body": "..."}` in `raw_payload_json` for the `extraction_failed`
/// path).
#[test]
fn test_select_open_with_context_joins_observation_and_extracts_body_snippet() {
    let conn = setup_db();

    let long_body = format!(
        "Rs   500.00   debited\n\nat Amazon on 25-May-23. {}",
        "padding ".repeat(40)
    );
    let mut obs = base_observation("obs_ctx_1");
    obs.merchant_raw = Some("Amazon".to_string());
    obs.amount_minor = Some(50000);
    obs.currency = Some("INR".to_string());
    obs.raw_payload_json = Some(serde_json::json!({ "body": long_body }).to_string());
    insert_observation(&conn, &obs).unwrap();

    insert_unassigned(
        &conn,
        &UnassignedTransactionRow {
            id: "ua_ctx_1".to_string(),
            observation_id: "obs_ctx_1".to_string(),
            reason: "extraction_failed".to_string(),
            status: "open".to_string(),
            created_at: None,
        },
    )
    .unwrap();

    let results = select_open_with_context(&conn).unwrap();
    assert_eq!(results.len(), 1);
    let detail = &results[0];
    assert_eq!(detail.merchant_raw, Some("Amazon".to_string()));
    assert_eq!(detail.amount_minor, Some(50000));
    assert_eq!(detail.currency, Some("INR".to_string()));
    assert_eq!(detail.source_message_id, Some("msg_123".to_string()));

    let snippet = detail
        .body_snippet
        .as_ref()
        .expect("body_snippet must be present");
    assert!(
        !snippet.contains('\n'),
        "whitespace (including newlines) must be collapsed: {snippet:?}"
    );
    assert!(
        snippet.starts_with("Rs 500.00 debited at Amazon on 25-May-23."),
        "got {snippet:?}"
    );
    assert!(
        snippet.ends_with('…'),
        "a body longer than the cap must be truncated with an ellipsis: {snippet:?}"
    );
    assert!(
        snippet.chars().count() <= 241, // 240 chars + the ellipsis itself
        "snippet must respect the character cap, got {} chars",
        snippet.chars().count()
    );
}

/// A dismissed (Doc 18 §4.17's `status = 'ignored'`, or otherwise non-`open`)
/// row must not reappear in the enriched query either -- same status filter
/// as `select_open`.
#[test]
fn test_select_open_with_context_excludes_dismissed() {
    let conn = setup_db();
    insert_observation(&conn, &base_observation("obs_ctx_2")).unwrap();
    insert_unassigned(
        &conn,
        &UnassignedTransactionRow {
            id: "ua_ctx_2".to_string(),
            observation_id: "obs_ctx_2".to_string(),
            reason: "extraction_failed".to_string(),
            status: "open".to_string(),
            created_at: None,
        },
    )
    .unwrap();

    update_status(&conn, "ua_ctx_2", "ignored").unwrap();

    let results = select_open_with_context(&conn).unwrap();
    assert!(results.is_empty());
}

/// TASK-FE-013: `direction`/`event_time` are often already known even when
/// the item is unassigned (especially `issuer_name_not_found`, where only
/// the instrument lookup failed) -- the "Save as Transaction" form needs
/// them to prefill instead of asking the user to re-enter data the system
/// already extracted correctly.
#[test]
fn test_select_open_with_context_includes_direction_and_event_time() {
    let conn = setup_db();
    let mut obs = base_observation("obs_ctx_4");
    obs.direction = Some("debit".to_string());
    insert_observation(&conn, &obs).unwrap();
    insert_unassigned(
        &conn,
        &UnassignedTransactionRow {
            id: "ua_ctx_4".to_string(),
            observation_id: "obs_ctx_4".to_string(),
            reason: "issuer_name_not_found".to_string(),
            status: "open".to_string(),
            created_at: None,
        },
    )
    .unwrap();

    let results = select_open_with_context(&conn).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].direction, Some("debit".to_string()));
    assert!(results[0].event_time.is_some());
}

/// No `raw_payload_json` at all (e.g. a future non-extraction-failure
/// reason that never populated it) must not panic -- `body_snippet` is
/// simply absent, every other field still resolves normally.
#[test]
fn test_select_open_with_context_handles_missing_raw_payload() {
    let conn = setup_db();
    insert_observation(&conn, &base_observation("obs_ctx_3")).unwrap();
    insert_unassigned(
        &conn,
        &UnassignedTransactionRow {
            id: "ua_ctx_3".to_string(),
            observation_id: "obs_ctx_3".to_string(),
            reason: "extraction_failed".to_string(),
            status: "open".to_string(),
            created_at: None,
        },
    )
    .unwrap();

    let results = select_open_with_context(&conn).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].body_snippet, None);
}
