pub mod alerts;
pub mod audit_log;
#[cfg(test)]
mod audit_log_tests;
pub mod backup;
pub mod batch_writer;
pub mod categories;
#[cfg(test)]
mod categories_tests;
pub mod connected_accounts;
mod connection_tests;
pub mod crypto;
pub mod feedback_log;
#[cfg(test)]
mod feedback_log_tests;
pub mod ignored_messages;
pub mod instruments;
pub mod local_profile;
pub mod maintenance;
pub mod match_decisions;
#[cfg(test)]
mod match_decisions_tests;
pub mod merchants;
pub mod migrations;
pub mod network_activity_log;
#[cfg(test)]
mod network_activity_log_tests;
pub mod pattern_rules;
#[cfg(test)]
mod pattern_rules_tests;
pub mod pdf_passwords;
#[cfg(test)]
mod pdf_passwords_tests;
pub mod processing_checkpoints;
#[cfg(test)]
mod processing_checkpoints_tests;
pub mod reconciliation_cluster_members;
pub mod reconciliation_clusters;
pub mod recurring_payments;
#[cfg(test)]
mod recurring_payments_tests;
pub mod retention;
pub mod scan_failed_messages;
pub mod scoping;
pub mod sender_reputation;
#[cfg(test)]
mod sender_reputation_tests;
pub mod sessions;
#[cfg(test)]
mod sessions_tests;
pub mod statement_drafts;
pub mod statement_entries;
pub mod statements;
pub mod tags;
#[cfg(test)]
mod tags_tests;
#[cfg(test)]
pub mod test_helpers;
mod tests;
pub mod transaction_observations;
pub mod transactions;
pub mod unassigned_transactions;
#[cfg(test)]
mod unassigned_transactions_tests;
pub mod unprocessed_statements;
#[cfg(test)]
mod unprocessed_statements_tests;
pub mod unresolved_mandate_cancellations;

use anyhow::{Context, Result};
use deadpool_sqlite::{Config, Pool, Runtime};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{error, info, warn};

/// Distinct error type for `init_db` so callers can pattern-match on
/// `KeyMismatch` without inspecting raw error strings.
#[derive(Debug, Error)]
pub enum DbInitError {
    /// The on-disk database file cannot be decrypted with the current key.
    /// This happens when the macOS Keychain entry is deleted/rotated while a
    /// previously-encrypted `finance.db` still exists on disk.  The caller should
    /// surface a user-facing dialog and exit cleanly rather than panic.
    #[error(
        "The local database cannot be opened — the encryption key no longer matches. \
        This usually means the Keychain entry was cleared. \
        Use your Recovery Phrase to restore access, or reset the app data."
    )]
    KeyMismatch,

    /// G3 fix: the user denied the macOS Keychain access prompt (or the
    /// Keychain is locked/unavailable) — distinct from `KeyMismatch` (a wrong
    /// key) or a generic fatal error. The caller should show a dedicated
    /// explanation of why Keychain access is required and how to grant it,
    /// rather than a generic "could not start" dialog.
    #[error(
        "Dinero could not access the macOS Keychain, which is required to encrypt your \
        financial data. This can happen if Keychain access was denied, or the Keychain is locked."
    )]
    KeychainAccessDenied,

    /// A schema migration failed after a pre-migration backup was already
    /// taken (Doc 18 §12.1, C18 fix). The caller can offer the user a
    /// one-click rollback to `backup_path` via `restore_backup_file`
    /// rather than just exiting.
    #[error("Database migration failed: {source}")]
    MigrationFailed {
        source: anyhow::Error,
        backup_path: PathBuf,
    },

    /// `PRAGMA integrity_check` reported corruption (Doc 22 §7.5/§19.5, I6
    /// fix). Previously this only logged a warning and continued — silently
    /// operating on a corrupt database. Fails closed instead: the caller can
    /// offer to restore the daily backup if one exists.
    #[error("Database integrity check failed: {details}")]
    IntegrityCheckFailed { details: String },

    /// Any other initialisation failure.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Replaces the live (post-failed-migration) database file with a
/// pre-migration backup taken by `create_pre_migration_backup` (C18 fix).
/// Removes any WAL/SHM sidecar files left over from the failed attempt so the
/// restored file is opened fresh on the next `init_db` call.
pub fn restore_backup_file(db_path: &Path, backup_path: &Path) -> Result<()> {
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{}", db_path.display(), suffix));
        let _ = std::fs::remove_file(sidecar);
    }
    std::fs::copy(backup_path, db_path)
        .with_context(|| format!("Failed to restore backup from {}", backup_path.display()))?;
    info!(
        "Restored pre-migration backup {} → {}",
        backup_path.display(),
        db_path.display()
    );
    Ok(())
}

/// Returns `true` if the error message indicates a SQLCipher key mismatch
/// (i.e. the file exists but cannot be decrypted with the supplied key).
fn is_key_mismatch(msg: &str) -> bool {
    msg.contains("not a database") || msg.contains("file is not a database")
}

/// G3 fix: matches the marker `crypto::classify_keychain_error` embeds when
/// the underlying error is `keyring::Error::NoStorageAccess` (access denied
/// or the Keychain is locked), rather than some other Keychain failure.
fn is_keychain_access_denied(msg: &str) -> bool {
    msg.contains("KEYCHAIN_ACCESS_DENIED")
}

/// Initializes the database with SQLCipher encryption, runs core migrations,
/// and returns a connection pool.
///
/// Returns `DbInitError::KeyMismatch` (rather than panicking) when the
/// existing on-disk `finance.db` cannot be decrypted with the current key.
pub async fn init_db(db_path: PathBuf) -> Result<Pool, DbInitError> {
    // 1. Get or derive the SQLite encryption key
    let db_key = crypto::derive_database_key().map_err(|e| {
        let msg = e.to_string();
        if is_keychain_access_denied(&msg) {
            error!(
                "Keychain access denied while deriving database key: {}",
                msg
            );
            DbInitError::KeychainAccessDenied
        } else {
            DbInitError::Other(e.context("Failed to derive database encryption key"))
        }
    })?;
    // TASK-DB-002: cloned before `db_key` is moved into the post_create hook
    // below — sqlx's migration connection (a separate connection stack from
    // rusqlite/deadpool_sqlite) needs its own copy of the same key.
    let db_key_for_migration = db_key.clone();

    // 2. Configure the deadpool_sqlite pool
    let cfg = Config::new(&db_path);
    let pool = cfg
        .builder(Runtime::Tokio1)
        .map_err(|e| DbInitError::Other(anyhow::anyhow!(e)))?
        .post_create(deadpool_sqlite::Hook::async_fn(move |conn, _metrics| {
            let key = db_key.clone();
            Box::pin(async move {
                conn.interact(move |c| {
                    // Set encryption key first
                    c.execute_batch(&format!("PRAGMA key = '{}';", key))?;

                    // Set SQLCipher and performance PRAGMAs. TASK-DB-001:
                    // added `auto_vacuum = INCREMENTAL` — Document 30's spec
                    // requires it, and it must be set from the very first
                    // connection: DB-019's daily `PRAGMA incremental_vacuum`
                    // background task is a no-op unless auto_vacuum mode was
                    // INCREMENTAL before the schema was created (changing it
                    // later requires a full VACUUM to take effect).
                    c.execute_batch(
                        "
                        PRAGMA cipher_page_size = 4096;
                        PRAGMA kdf_iter = 256000;
                        PRAGMA cipher_hmac_algorithm = HMAC_SHA512;
                        PRAGMA journal_mode = WAL;
                        PRAGMA synchronous = NORMAL;
                        PRAGMA foreign_keys = ON;
                        PRAGMA auto_vacuum = INCREMENTAL;
                        PRAGMA busy_timeout = 5000;
                        ",
                    )?;
                    // SQLCipher's own header/salt write on `PRAGMA key` means
                    // the file is no longer "virgin" by the time auto_vacuum
                    // is set above, so the mode change silently doesn't take
                    // effect without an explicit VACUUM. Only run that VACUUM
                    // once — on every *later* pooled connection (this hook
                    // fires once per physical connection, e.g. as the
                    // Transaction Queue's 2-8 workers spin more up) the mode
                    // will already read back as INCREMENTAL (2) and the
                    // check below skips it; without this guard, every new
                    // connection to an already-large financial database
                    // would trigger a full VACUUM.
                    let auto_vacuum_mode: i64 =
                        c.query_row("PRAGMA auto_vacuum", [], |r| r.get(0))?;
                    if auto_vacuum_mode != 2 {
                        c.execute_batch("VACUUM;")?;
                    }
                    Ok::<(), rusqlite::Error>(())
                })
                .await
                .map_err(|e| deadpool_sqlite::HookError::Message(e.to_string().into()))?
                .map_err(|e| deadpool_sqlite::HookError::Message(e.to_string().into()))?;
                Ok(())
            })
        }))
        .build()
        .map_err(|e| DbInitError::Other(anyhow::anyhow!(e)))?;

    // 3. Verify encryption is working and run integrity check.
    //    `pool.get()` triggers the `post_create` hook which executes
    //    `PRAGMA key`.  If the existing file was encrypted with a *different*
    //    key the hook surfaces "file is not a database" — we catch that here.
    //    Before giving up, try the Mac-to-Mac hardware-UUID migration path
    //    (I4 fix): if a hardware-UUID marker from a previous successful open
    //    exists and differs from this machine's, this looks like a legitimate
    //    hardware migration rather than a cleared Keychain — re-key the
    //    database transparently and retry, rather than failing closed.
    let app_data_dir = db_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let conn = match pool.get().await {
        Ok(conn) => conn,
        Err(e) => {
            let msg = e.to_string();
            if !is_key_mismatch(&msg) {
                return Err(DbInitError::Other(anyhow::anyhow!(
                    "Failed to acquire DB connection: {}",
                    msg
                )));
            }

            match crypto::try_migrate_hardware_uuid(&db_path, &app_data_dir) {
                Ok(true) => {
                    info!("Hardware UUID migration succeeded — retrying database connection.");
                    pool.get().await.map_err(|retry_err| {
                        DbInitError::Other(anyhow::anyhow!(
                            "Database still could not be opened after hardware-UUID migration: {}",
                            retry_err
                        ))
                    })?
                }
                Ok(false) => {
                    error!(
                        "DB key mismatch: existing finance.db cannot be decrypted with the current Keychain \
                        key, and no hardware-UUID migration marker applies. The Keychain entry was likely \
                        cleared. Error: {}",
                        msg
                    );
                    return Err(DbInitError::KeyMismatch);
                }
                Err(migrate_err) => {
                    error!(
                        "DB key mismatch and hardware-UUID migration attempt failed: {}. Original error: {}",
                        migrate_err, msg
                    );
                    return Err(DbInitError::KeyMismatch);
                }
            }
        }
    };

    let db_path_for_backup = db_path.clone();
    // Step A: integrity check + pre-migration backup, still via the pooled
    // rusqlite connection (sync). Returns `Err(details)` for an integrity
    // failure, or `Ok(backup_path)` once the pre-migration backup exists.
    let integrity_or_backup: Result<PathBuf, String> = conn
        .interact(move |c| {
            // Test query to ensure decryption succeeds
            let count: i64 = c.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))?;
            info!("Database initialized successfully. Tables count: {}", count);

            // Run integrity check. I6 fix: fail closed on corruption rather than
            // just logging a warning and continuing to operate on a corrupt db.
            let integrity: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
            if integrity != "ok" {
                warn!("Database integrity check failed: {}", integrity);
                return Ok::<Result<PathBuf, String>, anyhow::Error>(Err(integrity));
            }
            info!("Database integrity check passed.");

            // Doc 18 §12.1: back up before every migration run so a failed
            // migration can be recovered from rather than corrupting the user's
            // only copy of their financial data. Fail closed — if the backup
            // itself can't be created, do not proceed with the migration.
            let backup_path = migrations::create_pre_migration_backup(c, &db_path_for_backup)
                .context("Pre-migration backup failed — aborting before migration")?;

            Ok(Ok(backup_path))
        })
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if is_key_mismatch(&msg) {
                error!("DB key mismatch detected during init interact: {}", msg);
                DbInitError::KeyMismatch
            } else {
                DbInitError::Other(anyhow::anyhow!("Interact error: {}", msg))
            }
        })?
        .map_err(|e| {
            DbInitError::Other(e.context("Failed during database initialization phase"))
        })?;

    let backup_path = match integrity_or_backup {
        Err(details) => return Err(DbInitError::IntegrityCheckFailed { details }),
        Ok(path) => path,
    };

    // Step B (TASK-DB-002): run migrations via sqlx, on its own dedicated
    // connection — a failure here does NOT abort via `?`; the backup above
    // already succeeded, so the caller can offer a one-click rollback to it
    // (C18 fix) instead of just exiting.
    if let Err(source) = migrations::run_migrations(&db_path, Some(&db_key_for_migration)).await {
        return Err(DbInitError::MigrationFailed {
            source,
            backup_path,
        });
    }

    // Step C: post-migration seeding + stuck-checkpoint reset, back on the
    // pooled rusqlite connection.
    conn.interact(move |c| {
        // Ensure default local_profile exists (ID=1)
        let _ = c.execute(
            // TASK-DB-003: timezone/limit_thresholds defaults corrected to
            // Document 30's spec ('Asia/Kolkata', [80,90,100]) — were 'UTC'
            // and '[]', contradicting Document 18 §5.3's IST-normalization
            // requirement ("All month-boundary calculations rely on IST
            // normalization to correctly align with Indian banking cycles")
            // and the 80/90/100% threshold default Document 18 §4.1 states.
            "INSERT OR IGNORE INTO local_profile (
                id, primary_email, display_name, timezone, spending_limit_monthly,
                limit_thresholds, recovery_phrase_enabled
             ) VALUES (1, NULL, 'Default User', 'Asia/Kolkata', 30000.0, '[80,90,100]', 0)",
            [],
        );

        // Reset any stuck in-progress checkpoints from a previous app run
        let _ = c.execute(
            "UPDATE processing_checkpoints SET status = 'failed' WHERE status = 'in_progress'",
            [],
        );

        // Run automated cleanup for any legacy corrupted VPA instruments
        let _ = instruments::cleanup_corrupted_vpa_instruments(c);
    })
    .await
    .map_err(|e| {
        DbInitError::Other(anyhow::anyhow!(
            "Interact error during post-migration seeding: {}",
            e
        ))
    })?;

    // I4 fix: refresh the hardware-UUID marker on every successful open (not
    // just after a migration) so it always reflects the machine that last
    // opened the db, ready for the next migration.
    crypto::record_last_known_hw_uuid(&app_data_dir);
    Ok(pool)
}

#[cfg(test)]
mod migration_rollback_tests {
    use super::restore_backup_file;

    #[test]
    fn restore_copies_backup_over_live_db_and_clears_sidecars() {
        let dir =
            std::env::temp_dir().join(format!("dinero_rollback_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("data.db");
        let backup_path = dir.join("finance.db.bak.20260101000000000");
        let wal_path = dir.join("data.db-wal");
        let shm_path = dir.join("data.db-shm");

        std::fs::write(&db_path, b"post-migration-failed-state").unwrap();
        std::fs::write(&backup_path, b"pre-migration-good-state").unwrap();
        std::fs::write(&wal_path, b"stale-wal").unwrap();
        std::fs::write(&shm_path, b"stale-shm").unwrap();

        restore_backup_file(&db_path, &backup_path).unwrap();

        assert_eq!(
            std::fs::read(&db_path).unwrap(),
            b"pre-migration-good-state"
        );
        assert!(!wal_path.exists());
        assert!(!shm_path.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_errors_when_backup_missing() {
        let dir =
            std::env::temp_dir().join(format!("dinero_rollback_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("data.db");
        let missing_backup = dir.join("finance.db.bak.does-not-exist");

        assert!(restore_backup_file(&db_path, &missing_backup).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
