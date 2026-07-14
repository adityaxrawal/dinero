use crate::db::transaction_observations::{insert_observation, TransactionObservationsRow};
use crate::db::unassigned_transactions::{
    insert as insert_unassigned, update_status, UnassignedTransactionRow,
};
use chrono::Utc;
use rusqlite::Connection;

fn setup_db() -> Connection {
    crate::db::test_helpers::setup_test_db()
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
