//! Periodic database maintenance: integrity, vacuuming and size warnings.
//!
//! Run on the daily background loop. The integrity check is the important one:
//! silent corruption of a financial ledger is exactly the failure that must not
//! go unnoticed, so a failure is escalated to the incident monitor rather than
//! merely logged. Incremental vacuum reclaims space in bounded steps, avoiding
//! the long full-database lock a plain VACUUM would take.
use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

pub const SIZE_WARNING_BYTES: u64 = 2 * 1024 * 1024 * 1024;

static LAST_INTEGRITY_OK: AtomicBool = AtomicBool::new(true);

/// The most recent recorded integrity result.
pub fn last_known_integrity_ok() -> bool {
    LAST_INTEGRITY_OK.load(Ordering::Relaxed)
}

/// Runs SQLite's integrity check over the database.
pub fn run_integrity_check(conn: &Connection) -> Result<bool> {
    let result: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    Ok(result.eq_ignore_ascii_case("ok"))
}

/// Runs the integrity check and escalates a failure.
///
/// A failure is reported rather than merely logged, because silent corruption of
/// a financial ledger is precisely the fault that must not pass unnoticed.
pub fn check_integrity_and_report(conn: &Connection, app_handle: &AppHandle) -> Result<bool> {
    let ok = run_integrity_check(conn)?;
    LAST_INTEGRITY_OK.store(ok, Ordering::Relaxed);
    if !ok {
        let _ = app_handle.emit(
            crate::ipc::events::AppEvent::DbCorrupted.as_str(),
            serde_json::json!({}),
        );
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

/// Reclaims free pages in bounded steps.
///
/// Incremental rather than a full VACUUM, which would lock the whole database
/// for the duration and rewrite the entire file.
pub fn run_incremental_vacuum(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA incremental_vacuum(500);")?;
    Ok(())
}

/// Whether the database has grown past the point worth warning about.
pub fn exceeds_size_warning_threshold(size_bytes: u64) -> bool {
    size_bytes > SIZE_WARNING_BYTES
}

/// Warns the user if the database has grown unusually large.
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
