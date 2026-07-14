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

#[test]
fn test_audit_log_hash_chain_intact_across_multiple_rows() -> Result<()> {
    let conn = setup_db();

    for i in 0..3 {
        audit_log::insert(
            &conn,
            &AuditLogRow {
                id: format!("row_{}", i),
                actor_type: Some("user".to_string()),
                actor_id: None,
                action: Some("login".to_string()),
                resource_type: None,
                resource_id: None,
                before_json: None,
                after_json: None,
                created_at: Utc::now(),
            },
        )?;
    }

    assert!(audit_log::verify_chain(&conn)?);
    Ok(())
}

#[test]
fn test_audit_log_hash_chain_detects_tampering() -> Result<()> {
    let conn = setup_db();

    audit_log::insert(
        &conn,
        &AuditLogRow {
            id: "row_a".to_string(),
            actor_type: Some("user".to_string()),
            actor_id: None,
            action: Some("login".to_string()),
            resource_type: None,
            resource_id: None,
            before_json: None,
            after_json: None,
            created_at: Utc::now(),
        },
    )?;
    audit_log::insert(
        &conn,
        &AuditLogRow {
            id: "row_b".to_string(),
            actor_type: Some("user".to_string()),
            actor_id: None,
            action: Some("logout".to_string()),
            resource_type: None,
            resource_id: None,
            before_json: None,
            after_json: None,
            created_at: Utc::now(),
        },
    )?;

    assert!(audit_log::verify_chain(&conn)?);

    // The immutability trigger blocks mutation through the normal app path
    // (already covered by test_audit_log_no_update_path above) but does
    // nothing against an actor with direct SQLite file access, who could
    // simply drop the trigger before editing. Dropping it here isolates
    // verify_chain()'s own detection logic from how the tampering happened.
    conn.execute("DROP TRIGGER immutable_audit_log", [])?;
    conn.execute(
        "UPDATE audit_log SET action = 'logout' WHERE id = 'row_a'",
        [],
    )?;

    assert!(
        !audit_log::verify_chain(&conn)?,
        "verify_chain must detect a row edited out-of-band"
    );

    Ok(())
}
