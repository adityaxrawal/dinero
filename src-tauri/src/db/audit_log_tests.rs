use anyhow::Result;
use chrono::Utc;
use rusqlite::params;
use serde_json::json;
use uuid::Uuid;

use crate::db::audit_log::{self, AuditLogRow};
use rusqlite::Connection;

fn setup_db() -> Connection {
    crate::db::test_helpers::setup_test_db()
}

#[test]
fn test_audit_log_no_update_path() -> Result<()> {
    let conn = setup_db();

    let id = Uuid::new_v4().to_string();
    let row = AuditLogRow {
        id: id.clone(),
        actor_type: Some("user".to_string()),
        actor_id: Some("usr_123".to_string()),
        action: Some("login".to_string()),
        resource_type: None,
        resource_id: None,
        before_json: None,
        after_json: Some(json!({"ip": "127.0.0.1"})),
        created_at: Utc::now(),
    };

    // Insert
    audit_log::insert(&conn, &row)?;

    // Get
    let fetched = audit_log::get(&conn, &id)?.expect("Audit log should exist");
    assert_eq!(fetched.action.as_deref(), Some("login"));
    assert_eq!(fetched.after_json.unwrap()["ip"], "127.0.0.1");

    // Immutability test (Attempt Update)
    let update_result = conn.execute(
        "UPDATE audit_log SET action = ?1 WHERE id = ?2",
        params!["logout", id],
    );

    assert!(
        update_result.is_err(),
        "Update should have failed due to immutability trigger"
    );
    let err_str = update_result.unwrap_err().to_string();
    assert!(
        err_str.contains("audit_log is immutable"),
        "Unexpected error: {}",
        err_str
    );

    Ok(())
}
