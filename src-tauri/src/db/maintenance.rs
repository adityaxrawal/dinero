//! TASK-DB-019: Database Integrity Check and Retention Background Task.
//!
//! Document 30's TASK-DB-019 names 5 steps. Steps 2 (raw-JSON retention) and
//! 5 (5-year archival) already exist in `db::retention` (`sweep_raw_payloads`,
//! `archive_old_transactions`), wired into the daily background loop in
//! `lib.rs`. This module implements the 3 steps that were still missing:
//! 1. `PRAGMA integrity_check` — corruption detection.
//! 3. `PRAGMA incremental_vacuum(500)` — bounded reclaim, no full-file lock.
//! 4. A size warning once `finance.db` exceeds 2 GB.
//!
//! Following the pattern established in `startup.rs`'s RAM check: filesystem/
//! event-emission side effects are kept thin so the decision logic itself is
//! unit-testable without a real multi-GB file or a running Tauri app handle.

use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use std::path::Path;
use tauri::{AppHandle, Emitter};

/// `db_size_warning` fires once `finance.db` exceeds this size.
pub const SIZE_WARNING_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Step 1: `PRAGMA integrity_check`. Returns `Ok(true)` iff SQLite reports
/// `"ok"` — any other string (a list of corruption findings) means `Ok(false)`.
pub fn run_integrity_check(conn: &Connection) -> Result<bool> {
    let result: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    Ok(result.eq_ignore_ascii_case("ok"))
}

/// Runs the integrity check and, on failure, emits `db_corrupted` and writes
/// an `audit_log` entry (Document 30 TASK-DB-019 step 1's "on failure"
/// behavior). Never panics — a maintenance-task failure must not crash the
/// background job; the caller decides how the app reacts to `Ok(false)`.
pub fn check_integrity_and_report(conn: &Connection, app_handle: &AppHandle) -> Result<bool> {
    let ok = run_integrity_check(conn)?;
    if !ok {
        let _ = app_handle.emit(crate::ipc::events::AppEvent::DbCorrupted.as_str(), serde_json::json!({}));
        crate::db::audit_log::insert(
            conn,
            &crate::db::audit_log::AuditLogRow {
                id: uuid::Uuid::new_v4().to_string(),
                actor_type: Some("system".to_string()),
                actor_id: None,
                action: Some("db_corrupted".to_string()),
                resource_type: Some("database".to_string()),
                resource_id: None,
                before_json: None,
                after_json: None,
                created_at: Utc::now(),
            },
        )?;
    }
    Ok(ok)
}

/// Step 3: `PRAGMA incremental_vacuum(500)` — reclaims up to 500 free pages
/// without the full-file lock a plain `VACUUM` would take. Only effective
/// because TASK-DB-001 already set `auto_vacuum = INCREMENTAL`.
pub fn run_incremental_vacuum(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA incremental_vacuum(500);")?;
    Ok(())
}

/// Pure threshold check, kept separate from filesystem access so it's
/// unit-testable without a real multi-GB file.
pub fn exceeds_size_warning_threshold(size_bytes: u64) -> bool {
    size_bytes > SIZE_WARNING_BYTES
}

/// Step 4: checks `finance.db`'s on-disk size and emits `db_size_warning`
/// once it exceeds 2 GB.
pub fn check_db_size_warning(app_handle: &AppHandle, db_path: &Path) -> Result<()> {
    let size_bytes = std::fs::metadata(db_path)?.len();
    if exceeds_size_warning_threshold(size_bytes) {
        let _ = app_handle.emit(
            crate::ipc::events::AppEvent::DbSizeWarning.as_str(),
            serde_json::json!({
                "size_bytes": size_bytes,
                "message": "Your database has grown past 2 GB. Consider cleaning up old data.",
            }),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrity_check_passes_on_a_fresh_db() {
        let conn = crate::db::test_helpers::setup_test_db();
        assert!(run_integrity_check(&conn).unwrap());
    }

    #[test]
    fn size_warning_threshold_boundary() {
        assert!(!exceeds_size_warning_threshold(SIZE_WARNING_BYTES));
        assert!(exceeds_size_warning_threshold(SIZE_WARNING_BYTES + 1));
        assert!(!exceeds_size_warning_threshold(1024));
    }

    #[test]
    fn incremental_vacuum_does_not_error_on_empty_db() {
        let conn = crate::db::test_helpers::setup_test_db();
        run_incremental_vacuum(&conn).unwrap();
    }
}
