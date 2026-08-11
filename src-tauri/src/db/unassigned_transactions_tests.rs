use crate::db::transaction_observations::{insert_observation, TransactionObservationsRow};
use crate::db::unassigned_transactions::{
    insert as insert_unassigned, is_open, select_open, select_open_with_context, settle_if_open,
    update_reason, update_status, UnassignedTransactionRow,
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
        // Unique per observation: (source_pipeline, source_record_id) is a
        // uniqueness key, so tests holding several observations at once collide.
        source_record_id: Some(format!("rec_{id}")),
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
        channel: None,
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
        channel: None,
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

fn open_entry(conn: &Connection, id: &str, obs_id: &str) {
    insert_observation(conn, &base_observation(obs_id)).unwrap();
    insert_unassigned(
        conn,
        &UnassignedTransactionRow {
            id: id.to_string(),
            observation_id: obs_id.to_string(),
            reason: "extraction_failed".to_string(),
            status: "open".to_string(),
            created_at: None,
        },
    )
    .unwrap();
}

fn status_of(conn: &Connection, id: &str) -> String {
    conn.query_row(
        "SELECT status FROM unassigned_transactions WHERE id = ?1",
        rusqlite::params![id],
        |row| row.get(0),
    )
    .unwrap()
}

/// Resolving books a real transaction *before* the status flips, so a status
/// write that lands on an entry someone else already took must fail loudly --
/// otherwise two racing resolvers each create a transaction and both report
/// success, double-counting the money. Same for a dismiss arriving after a
/// resolve: the resolution must not be silently un-done.
#[test]
fn test_update_status_refuses_to_overwrite_a_closed_entry() {
    let conn = setup_db();
    open_entry(&conn, "ua_race", "obs_race");

    update_status(&conn, "ua_race", "resolved").unwrap();

    let err = update_status(&conn, "ua_race", "ignored")
        .expect_err("dismissing an already-resolved entry must not silently win");
    assert!(err.to_string().contains("already 'resolved'"), "{err}");
    assert_eq!(status_of(&conn, "ua_race"), "resolved");

    // Replaying the same transition is a no-op, not an error.
    update_status(&conn, "ua_race", "resolved").unwrap();
    assert_eq!(status_of(&conn, "ua_race"), "resolved");
}

/// A Layer 6 job that finishes after the user dismissed the entry has nothing
/// left to do. `update_status` reports that as an error, which nobody sees and
/// which leaves the durable job row undeleted -- so the LLM run replayed at
/// every launch, forever. `settle_if_open` converges instead: it reports that it
/// did not move the entry, and leaves the user's decision standing.
#[test]
fn test_settle_if_open_converges_on_an_entry_the_user_already_closed() {
    let conn = setup_db();
    open_entry(&conn, "ua_settle", "obs_settle");

    assert!(is_open(&conn, "ua_settle").unwrap());
    assert!(settle_if_open(&conn, "ua_settle", "resolved").unwrap());
    assert_eq!(status_of(&conn, "ua_settle"), "resolved");
    assert!(!is_open(&conn, "ua_settle").unwrap());

    // A late job must neither error nor overwrite the closed entry.
    assert!(!settle_if_open(&conn, "ua_settle", "no_transaction_found").unwrap());
    assert_eq!(status_of(&conn, "ua_settle"), "resolved");

    // An entry deleted underneath the job (a cancelled scan sweeps its rows) is
    // the same story -- nothing to do, not a failure to retry.
    conn.execute("DELETE FROM unassigned_transactions", [])
        .unwrap();
    assert!(!settle_if_open(&conn, "ua_settle", "resolved").unwrap());
    assert!(!is_open(&conn, "ua_settle").unwrap());
    assert!(!is_open(&conn, "ua_never_existed").unwrap());
}

/// A write against an id that is not in the table changed nothing, so
/// reporting `Ok` left the caller announcing "dismissed"/"resolved" for an
/// entry that never moved.
#[test]
fn test_status_and_reason_writes_reject_unknown_ids() {
    let conn = setup_db();
    assert!(update_status(&conn, "ua_missing", "ignored").is_err());
    assert!(update_reason(&conn, "ua_missing", "gate3_failed").is_err());
}

/// A late Layer 6 reason refinement for an entry the user already dealt with
/// is dropped, not written back over the closed entry's history.
#[test]
fn test_update_reason_is_scoped_to_open_entries() {
    let conn = setup_db();
    open_entry(&conn, "ua_reason", "obs_reason");

    update_reason(&conn, "ua_reason", "gate3_failed:low_confidence").unwrap();
    update_status(&conn, "ua_reason", "ignored").unwrap();
    update_reason(&conn, "ua_reason", "late_refinement").unwrap();

    let reason: String = conn
        .query_row(
            "SELECT reason FROM unassigned_transactions WHERE id = ?1",
            rusqlite::params!["ua_reason"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(reason, "gate3_failed:low_confidence");
}

/// `created_at` defaults to `CURRENT_TIMESTAMP`, which is second-granular, so
/// an ingest burst writes ties -- without a tiebreak the queue reordered
/// itself between refreshes and rows moved under the user's cursor.
#[test]
fn test_open_queue_order_is_stable_across_created_at_ties() {
    let conn = setup_db();
    open_entry(&conn, "ua_a", "obs_a");
    open_entry(&conn, "ua_b", "obs_b");
    open_entry(&conn, "ua_c", "obs_c");
    conn.execute(
        "UPDATE unassigned_transactions SET created_at = '2026-01-01 00:00:00'",
        [],
    )
    .unwrap();

    let ids: Vec<String> = select_open(&conn)
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(ids, vec!["ua_c", "ua_b", "ua_a"]);
    let ctx_ids: Vec<String> = select_open_with_context(&conn)
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(ctx_ids, ids, "both queue reads must agree on order");
}

/// The snippet is capped at 240 chars, so neither a huge body nor a body with
/// no whitespace to split on may be materialised in full to produce it.
#[test]
fn test_body_snippet_is_bounded_for_pathological_bodies() {
    let conn = setup_db();

    let mut huge = base_observation("obs_huge");
    huge.raw_payload_json =
        Some(serde_json::json!({ "body": "word ".repeat(400_000) }).to_string());
    insert_observation(&conn, &huge).unwrap();
    insert_unassigned(
        &conn,
        &UnassignedTransactionRow {
            id: "ua_huge".to_string(),
            observation_id: "obs_huge".to_string(),
            reason: "extraction_failed".to_string(),
            status: "open".to_string(),
            created_at: None,
        },
    )
    .unwrap();

    let mut unbroken = base_observation("obs_unbroken");
    unbroken.raw_payload_json =
        Some(serde_json::json!({ "body": "x".repeat(2_000_000) }).to_string());
    insert_observation(&conn, &unbroken).unwrap();
    insert_unassigned(
        &conn,
        &UnassignedTransactionRow {
            id: "ua_unbroken".to_string(),
            observation_id: "obs_unbroken".to_string(),
            reason: "extraction_failed".to_string(),
            status: "open".to_string(),
            created_at: None,
        },
    )
    .unwrap();

    for detail in select_open_with_context(&conn).unwrap() {
        let snippet = detail.body_snippet.expect("snippet must be present");
        assert_eq!(
            snippet.chars().count(),
            241, // the 240-char cap plus the ellipsis
            "snippet for {} must respect the cap: {snippet:?}",
            detail.id
        );
        assert!(snippet.ends_with('…'));
    }
}
