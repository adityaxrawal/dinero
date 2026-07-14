use anyhow::Result;
use chrono::Utc;
use uuid::Uuid;

use crate::db::sessions::{self, SessionsRow};
use rusqlite::Connection;

fn setup_db() -> Connection {
    crate::db::test_helpers::setup_test_db()
}

#[test]
fn test_sessions_crud() -> Result<()> {
    let conn = setup_db();

    let id = Uuid::new_v4().to_string();
    let row = SessionsRow {
        id: id.clone(),
        device_name: Some("MacBook Pro".to_string()),
        device_fingerprint: Some("abcdef123456".to_string()),
        created_at: Utc::now(),
        revoked_at: None,
    };

    // Insert
    sessions::insert(&conn, &row)?;

    // Get
    let fetched = sessions::get(&conn, &id)?.expect("Session should exist");
    assert_eq!(fetched.device_name.as_deref(), Some("MacBook Pro"));
    assert!(fetched.revoked_at.is_none());

    // Revoke
    let revoked_at_time = Utc::now();
    sessions::revoke(&conn, &id, revoked_at_time)?;

    let fetched_revoked = sessions::get(&conn, &id)?.unwrap();
    assert!(fetched_revoked.revoked_at.is_some());
    assert_eq!(
        fetched_revoked.revoked_at.unwrap().timestamp_millis(),
        revoked_at_time.timestamp_millis()
    );

    Ok(())
}
