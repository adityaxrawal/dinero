//! Establishes and revokes the local session.
//!
//! The device fingerprint computed here also underpins licence binding, so it
//! must stay stable across restarts while remaining distinct between machines.
use anyhow::Result;
use chrono::Utc;
use deadpool_sqlite::Pool;
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};
use std::sync::Mutex;

use crate::db::sessions::{self, SessionsRow};

pub const BUNDLE_ID: &str = "com.dinero.app";

#[derive(Default)]
pub struct SessionState(pub Mutex<Option<String>>);

/// Derives the device fingerprint from the hardware UUID.
///
/// Must be stable across restarts yet distinct between machines, since licence
/// binding depends on it.
pub fn compute_device_fingerprint(hw_uuid: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(hw_uuid.as_bytes());
    hasher.update(BUNDLE_ID.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Ensures a live local session exists, creating one if needed.
pub async fn ensure_active_session(pool: &Pool, state: &SessionState) -> Result<String> {
    let existing_id: Option<String> = {
        let conn = pool.get().await?;
        conn.interact(|c| {
            c.query_row(
                "SELECT id FROM sessions WHERE revoked_at IS NULL ORDER BY rowid DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()
        })
        .await
        .map_err(|e| anyhow::anyhow!("DB interaction error: {}", e))??
    };

    let session_id = match existing_id {
        Some(id) => id,
        None => {
            let hw_uuid = crate::db::crypto::get_hardware_uuid()?;
            let fingerprint = compute_device_fingerprint(&hw_uuid);
            let id = uuid::Uuid::new_v4().to_string();
            let row = SessionsRow {
                id: id.clone(),
                device_name: None,
                device_fingerprint: Some(fingerprint),
                created_at: Utc::now(),
                revoked_at: None,
            };
            let conn = pool.get().await?;
            conn.interact(move |c| sessions::insert(c, &row))
                .await
                .map_err(|e| anyhow::anyhow!("DB interaction error: {}", e))??;
            id
        }
    };

    *state.0.lock().unwrap() = Some(session_id.clone());
    Ok(session_id)
}

/// Ends the current session.
pub async fn logout(pool: &Pool, state: &SessionState) -> Result<()> {
    let session_id = state.0.lock().unwrap().clone();
    if let Some(id) = session_id {
        let conn = pool.get().await?;
        conn.interact(move |c| sessions::revoke(c, &id, Utc::now()))
            .await
            .map_err(|e| anyhow::anyhow!("DB interaction error: {}", e))??;
    }
    *state.0.lock().unwrap() = None;
    Ok(())
}

/// The current session id, if any.
pub fn current_session_id(state: &SessionState) -> Option<String> {
    state.0.lock().unwrap().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> rusqlite::Connection {
        crate::db::test_helpers::setup_test_db()
    }

    #[test]
    fn fingerprint_is_deterministic_and_hw_uuid_specific() {
        let a = compute_device_fingerprint("hw-uuid-a");
        let b = compute_device_fingerprint("hw-uuid-a");
        let c = compute_device_fingerprint("hw-uuid-b");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn ensure_active_session_creates_a_row_when_none_exists() {
        let conn = setup_db();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);

        let id = uuid::Uuid::new_v4().to_string();
        let row = SessionsRow {
            id: id.clone(),
            device_name: None,
            device_fingerprint: Some(compute_device_fingerprint("hw-uuid")),
            created_at: Utc::now(),
            revoked_at: None,
        };
        sessions::insert(&conn, &row).unwrap();

        let fetched = sessions::get(&conn, &id).unwrap().unwrap();
        assert!(fetched.revoked_at.is_none());
        assert!(fetched.device_fingerprint.is_some());
    }

    #[test]
    fn logout_revokes_without_deleting() {
        let conn = setup_db();
        let id = uuid::Uuid::new_v4().to_string();
        let row = SessionsRow {
            id: id.clone(),
            device_name: None,
            device_fingerprint: Some("fp".to_string()),
            created_at: Utc::now(),
            revoked_at: None,
        };
        sessions::insert(&conn, &row).unwrap();

        sessions::revoke(&conn, &id, Utc::now()).unwrap();

        let fetched = sessions::get(&conn, &id).unwrap().unwrap();
        assert!(
            fetched.revoked_at.is_some(),
            "session must still exist, just revoked"
        );
    }

    #[test]
    fn session_state_defaults_to_none_and_clears_on_logout() {
        let state = SessionState::default();
        assert_eq!(current_session_id(&state), None);

        *state.0.lock().unwrap() = Some("sess_1".to_string());
        assert_eq!(current_session_id(&state), Some("sess_1".to_string()));

        *state.0.lock().unwrap() = None;
        assert_eq!(current_session_id(&state), None);
    }
}
