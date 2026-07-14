use crate::db::processing_checkpoints::{
    get_checkpoint, upsert_checkpoint, ProcessingCheckpointRow,
};
use rusqlite::Connection;

fn setup_db() -> Connection {
    crate::db::test_helpers::setup_test_db()
}

#[test]
fn test_upsert_and_get_checkpoint() {
    let conn = setup_db();

    let checkpoint = ProcessingCheckpointRow {
        id: "ckpt_123".to_string(),
        job_type: "historical_scan".to_string(),
        job_key: "user@example.com".to_string(),
        checkpoint_state_json: r#"{"progress": 50}"#.to_string(),
        last_processed_token: Some("token_abc".to_string()),
        status: "running".to_string(),
        updated_at: None,
    };

    upsert_checkpoint(&conn, &checkpoint).unwrap();

    let fetched = get_checkpoint(&conn, "historical_scan", "user@example.com")
        .unwrap()
        .unwrap();
    assert_eq!(fetched.id, "ckpt_123");
    assert_eq!(fetched.checkpoint_state_json, r#"{"progress": 50}"#);
    assert_eq!(fetched.last_processed_token.as_deref(), Some("token_abc"));
    assert_eq!(fetched.status, "running");

    // Idempotency: update existing
    let mut updated_checkpoint = checkpoint.clone();
    updated_checkpoint.checkpoint_state_json = r#"{"progress": 100}"#.to_string();
    updated_checkpoint.status = "complete".to_string();

    upsert_checkpoint(&conn, &updated_checkpoint).unwrap();

    let fetched_updated = get_checkpoint(&conn, "historical_scan", "user@example.com")
        .unwrap()
        .unwrap();
    assert_eq!(fetched_updated.id, "ckpt_123"); // ID should be the same
    assert_eq!(
        fetched_updated.checkpoint_state_json,
        r#"{"progress": 100}"#
    );
    assert_eq!(fetched_updated.status, "complete");
}
