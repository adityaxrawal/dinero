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

#[cfg(test)]
pub(crate) fn set_last_integrity_ok_for_test(ok: bool) {
    LAST_INTEGRITY_OK.store(ok, Ordering::Relaxed);
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

/// Daily backup file names: the newest keeps the name the corruption-recovery
/// path restores from, older generations get a numeric suffix.
pub const DAILY_BACKUP_NAME: &str = "finance.db.daily.bak";
/// How many daily backups are kept (newest plus six older).
pub const BACKUP_GENERATIONS: usize = 7;
const MAINTENANCE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60 * 60 * 24);
const MAINTENANCE_MARKER: &str = "last_maintenance_at";

fn generation_path(backup_dir: &Path, n: usize) -> std::path::PathBuf {
    if n == 0 {
        backup_dir.join(DAILY_BACKUP_NAME)
    } else {
        backup_dir.join(format!("{DAILY_BACKUP_NAME}.{n}"))
    }
}

/// Promotes a freshly written backup only once it has been verified.
///
/// `verify` runs on the temp file first; on failure the temp file is deleted
/// and every existing generation is left untouched, so a corrupt snapshot can
/// never replace a good one. On success the generations shift down by one (the
/// oldest is dropped) and the temp file becomes the newest.
pub fn promote_verified_backup(
    backup_dir: &Path,
    tmp: &Path,
    verify: impl FnOnce(&Path) -> Result<()>,
) -> Result<std::path::PathBuf> {
    if let Err(e) = verify(tmp) {
        let _ = std::fs::remove_file(tmp);
        return Err(e);
    }
    let _ = std::fs::remove_file(generation_path(backup_dir, BACKUP_GENERATIONS - 1));
    for n in (0..BACKUP_GENERATIONS - 1).rev() {
        let from = generation_path(backup_dir, n);
        if from.exists() {
            std::fs::rename(&from, generation_path(backup_dir, n + 1))?;
        }
    }
    let newest = generation_path(backup_dir, 0);
    std::fs::rename(tmp, &newest)?;
    Ok(newest)
}

/// How long to wait before the next daily maintenance run.
///
/// Read from a marker file rather than counting uptime, so the run happens on
/// the next launch once 24h have passed, even if the app never stays open that
/// long. A missing or unreadable marker means "due now".
pub fn time_until_maintenance(app_dir: &Path, now: chrono::DateTime<Utc>) -> std::time::Duration {
    let last = std::fs::read_to_string(app_dir.join(MAINTENANCE_MARKER))
        .ok()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s.trim()).ok())
        .map(|d| d.with_timezone(&Utc));
    match last {
        Some(last) if last <= now => (now - last)
            .to_std()
            .map(|elapsed| MAINTENANCE_INTERVAL.saturating_sub(elapsed))
            .unwrap_or_default(),
        // A marker in the future means the clock moved back: run now.
        _ => std::time::Duration::ZERO,
    }
}

/// Records that daily maintenance just ran.
pub fn record_maintenance_run(app_dir: &Path, now: chrono::DateTime<Utc>) -> std::io::Result<()> {
    std::fs::write(app_dir.join(MAINTENANCE_MARKER), now.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("dinero_maint_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn failed_verification_keeps_existing_backup_and_drops_tmp() {
        let dir = scratch_dir();
        std::fs::write(dir.join(DAILY_BACKUP_NAME), "good").unwrap();
        let tmp = dir.join("tmp");
        std::fs::write(&tmp, "corrupt").unwrap();
        let res = promote_verified_backup(&dir, &tmp, |_| Err(anyhow::anyhow!("bad")));
        assert!(res.is_err());
        assert!(!tmp.exists());
        assert_eq!(std::fs::read_to_string(dir.join(DAILY_BACKUP_NAME)).unwrap(), "good");
        assert!(!dir.join(format!("{DAILY_BACKUP_NAME}.1")).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn verified_backups_rotate_and_keep_seven_generations() {
        let dir = scratch_dir();
        for i in 0..10 {
            let tmp = dir.join("tmp");
            std::fs::write(&tmp, i.to_string()).unwrap();
            promote_verified_backup(&dir, &tmp, |_| Ok(())).unwrap();
        }
        assert_eq!(std::fs::read_to_string(dir.join(DAILY_BACKUP_NAME)).unwrap(), "9");
        assert_eq!(std::fs::read_to_string(dir.join(format!("{DAILY_BACKUP_NAME}.6"))).unwrap(), "3");
        assert!(!dir.join(format!("{DAILY_BACKUP_NAME}.7")).exists());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), BACKUP_GENERATIONS);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn maintenance_is_due_on_first_launch_and_after_24h() {
        let dir = scratch_dir();
        let now = Utc::now();
        assert_eq!(time_until_maintenance(&dir, now), std::time::Duration::ZERO);

        record_maintenance_run(&dir, now - chrono::Duration::hours(2)).unwrap();
        let wait = time_until_maintenance(&dir, now);
        assert_eq!(wait.as_secs(), 22 * 3600);

        record_maintenance_run(&dir, now - chrono::Duration::hours(30)).unwrap();
        assert_eq!(time_until_maintenance(&dir, now), std::time::Duration::ZERO);

        record_maintenance_run(&dir, now + chrono::Duration::hours(5)).unwrap();
        assert_eq!(time_until_maintenance(&dir, now), std::time::Duration::ZERO);
        std::fs::remove_dir_all(&dir).unwrap();
    }

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
