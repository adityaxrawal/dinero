//! Database bring-up: opens the encrypted SQLite pool and runs migrations.
//!
//! The store is SQLCipher-encrypted, so opening it is a multi-step operation
//! rather than a file handle: derive the key, key each pooled connection, apply
//! the cipher and journal pragmas, then migrate.
//!
//! `DbInitError` distinguishes the failure modes precisely because startup
//! offers a different recovery path for each -- a key mismatch, a denied
//! keychain, a failed migration and a corrupted file all need different advice,
//! and collapsing them into one error would leave the user stuck.
//!
//! Every pooled connection is keyed in a `post_create` hook. A connection that
//! skipped that step would fail on first use, so the hook rather than a
//! one-time setup is what makes pooling safe here.

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
pub mod dismissed_warnings;
pub mod feedback_log;
#[cfg(test)]
mod feedback_log_tests;
pub mod field_rules;
#[cfg(test)]
mod field_rules_tests;
pub mod ignored_messages;
pub mod instrument_rules;
pub mod instruments;
pub mod layer6_jobs;
pub mod local_profile;
pub mod maintenance;
pub mod match_decisions;
#[cfg(test)]
mod match_decisions_tests;
pub mod merchant_cleanup;
#[cfg(test)]
mod merchant_cleanup_tests;
pub mod merchants;
pub mod migrations;
pub mod pending_ingest_jobs;
pub mod processed_messages;
pub mod active_warnings;
pub mod network_activity_log;
#[cfg(test)]
mod network_activity_log_tests;
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
pub mod sender_bank_overrides;
pub mod sender_reputation;
#[cfg(test)]
mod sender_reputation_tests;
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

#[derive(Debug, Error)]
pub enum DbInitError {
    #[error(
        "The local database cannot be opened — the encryption key no longer matches. \
        This usually means the Keychain entry was cleared. \
        Use your Recovery Phrase to restore access, or reset the app data."
    )]
    KeyMismatch,

    #[error(
        "Dinero could not access the macOS Keychain, which is required to encrypt your \
        financial data. This can happen if Keychain access was denied, or the Keychain is locked."
    )]
    KeychainAccessDenied,

    #[error("Database migration failed: {source}")]
    MigrationFailed {
        source: anyhow::Error,
        backup_path: PathBuf,
    },

    #[error("Database integrity check failed: {details}")]
    IntegrityCheckFailed { details: String },

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

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

fn is_key_mismatch(msg: &str) -> bool {
    msg.contains("not a database") || msg.contains("file is not a database")
}

fn is_keychain_access_denied(msg: &str) -> bool {
    msg.contains("KEYCHAIN_ACCESS_DENIED")
}

/// Connections in the pool (audit D-1). Unset, deadpool used CPUs × 4 (32 on
/// an 8-core Mac): each new connection pays SQLCipher's key derivation, and
/// more connections only means more writers queueing on SQLite's one write
/// lock. Eight covers the ingest workers plus UI reads.
pub const POOL_MAX_SIZE: usize = 8;

/// How long a caller waits for a free pooled connection before getting a pool
/// error instead of hanging (owner decision, audit N-D3).
pub const POOL_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Runs a write that owns its transaction, retrying it when SQLite reports
/// the database busy (audit D-1). Retried only when the failure left no
/// transaction open on the connection, so nothing half-done is re-run.
pub fn retry_on_busy<T>(
    conn: &rusqlite::Connection,
    mut write: impl FnMut() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    const BACKOFF_MS: [u64; 3] = [50, 200, 800];
    let mut attempt = 0;
    loop {
        match write() {
            Err(e) if attempt < BACKOFF_MS.len() && is_busy(&e) && conn.is_autocommit() => {
                std::thread::sleep(std::time::Duration::from_millis(BACKOFF_MS[attempt]));
                attempt += 1;
            }
            other => return other,
        }
    }
}

/// Whether an error is SQLite reporting the database busy or locked.
pub fn is_busy(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<rusqlite::Error>(),
            Some(rusqlite::Error::SqliteFailure(e, _))
                if matches!(e.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
        )
    })
}

pub async fn init_db(db_path: PathBuf) -> Result<Pool, DbInitError> {
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
    let db_key_for_migration = db_key.clone();

    let mut cfg = Config::new(&db_path);
    let mut pool_cfg = deadpool_sqlite::PoolConfig::new(POOL_MAX_SIZE);
    pool_cfg.timeouts.wait = Some(POOL_WAIT_TIMEOUT);
    cfg.pool = Some(pool_cfg);
    let pool = cfg
        .builder(Runtime::Tokio1)
        // The builder's error type has no values (it cannot fail), so the
        // match is empty; mapping it produced unreachable-code warnings.
        .unwrap_or_else(|never| match never {})
        .post_create(deadpool_sqlite::Hook::async_fn(move |conn, _metrics| {
            let key = db_key.clone();
            Box::pin(async move {
                conn.interact(move |c| {
                    // Must be the first statement on the connection: SQLCipher
                    // cannot read even the header until it is keyed.
                    c.execute_batch(&format!("PRAGMA key = '{}';", key))?;

                    // Cipher settings must match those the file was created
                    // with, or the database reads as corrupt. WAL improves
                    // concurrency for the ingestion writers; incremental
                    // auto-vacuum keeps reclamation off the startup path.
                    c.execute_batch(
                        "
                        PRAGMA cipher_page_size = 4096;
                        PRAGMA kdf_iter = 256000;
                        PRAGMA cipher_hmac_algorithm = HMAC_SHA512;
                        PRAGMA journal_mode = WAL;
                        PRAGMA synchronous = NORMAL;
                        PRAGMA foreign_keys = ON;
                        PRAGMA busy_timeout = 5000;
                        ",
                    )?;
                    let auto_vacuum_mode: i64 =
                        c.query_row("PRAGMA auto_vacuum", [], |r| r.get(0))?;
                    // auto_vacuum can only be changed by a full VACUUM, so a
                    // database created before this setting is rewritten once.
                    // Set only when needed: the PRAGMA takes the write lock even
                    // when the mode is already set, so running it on every new
                    // connection failed whenever a long commit held the lock
                    // (audit N-D3).
                    if auto_vacuum_mode != 2 {
                        c.execute_batch("PRAGMA auto_vacuum = INCREMENTAL; VACUUM;")?;
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
    let integrity_or_backup: Result<Option<PathBuf>, String> = conn
        .interact(move |c| {
            let count: i64 = c.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))?;
            info!("Database initialized successfully. Tables count: {}", count);

            let integrity: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
            if integrity != "ok" {
                warn!("Database integrity check failed: {}", integrity);
                return Ok::<Result<Option<PathBuf>, String>, anyhow::Error>(Err(integrity));
            }
            info!("Database integrity check passed.");

            // A full VACUUM INTO copy is only worth taking when a migration
            // will actually run (review L3); it used to happen on every launch.
            if !migrations::has_pending_migrations(c)? {
                return Ok(Ok(None));
            }
            let backup_path = migrations::create_pre_migration_backup(c, &db_path_for_backup)
                .context("Pre-migration backup failed — aborting before migration")?;

            Ok(Ok(Some(backup_path)))
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

    if let Some(backup_path) = backup_path {
        if let Err(source) = migrations::run_migrations(&db_path, Some(&db_key_for_migration)).await {
            return Err(DbInitError::MigrationFailed {
                source,
                backup_path,
            });
        }
    }

    let db_path_for_merge = db_path.clone();
    conn.interact(move |c| -> anyhow::Result<()> {
        // The profile row is required: without it the trial check finds no
        // profile and every write is refused, so a failure here must stop
        // startup with an error instead of leaving the app silently read-only.
        c.execute(
            "INSERT OR IGNORE INTO local_profile (
                id, primary_email, display_name, timezone, spending_limit_monthly,
                limit_thresholds, recovery_phrase_enabled
             ) VALUES (1, NULL, 'Default User', 'Asia/Kolkata', 30000.0, '[80,90,100]', 0)",
            [],
        )
        .map_err(|e| anyhow::anyhow!("Could not create the default profile: {}", e))?;

        // Crash recovery: a scan left in_progress by a crash would otherwise
        // hold its claim. Not fatal, but it must be visible.
        if let Err(e) = c.execute(
            "UPDATE processing_checkpoints SET status = 'failed' WHERE status = 'in_progress'",
            [],
        ) {
            tracing::error!("Could not reset interrupted scans at startup: {}", e);
        }

        if let Err(e) = instruments::cleanup_corrupted_vpa_instruments(c) {
            tracing::warn!("Startup instrument cleanup failed: {}", e);
        }

        // One-time merge of instruments split across issuer spellings (audit
        // A-3). It rewrites ledger references, so it runs only behind a
        // verified backup; once the keys are canonical it is a no-op.
        let merge = || -> anyhow::Result<()> {
            if instruments::needs_key_canonicalization(c)? {
                let backup = migrations::create_pre_migration_backup(c, &db_path_for_merge)
                    .context("backup before the instrument merge failed, merge skipped")?;
                let merged = instruments::canonicalize_instrument_keys(c)?;
                tracing::info!(
                    "Instrument keys canonicalised; {} duplicate(s) merged (backup {})",
                    merged,
                    backup.display()
                );
            }
            Ok(())
        };
        if let Err(e) = merge() {
            tracing::error!("Instrument key canonicalisation failed: {:#}", e);
        }
        Ok(())
    })
    .await
    .map_err(|e| {
        DbInitError::Other(anyhow::anyhow!(
            "Interact error during post-migration seeding: {}",
            e
        ))
    })?
    .map_err(DbInitError::Other)?;

    crypto::record_last_known_hw_uuid(&app_data_dir);
    Ok(pool)
}

#[cfg(test)]
mod migration_rollback_tests {
    use super::restore_backup_file;

    /// Startup must fail loudly, not continue read-only, when the default
    /// profile cannot be created.
    #[tokio::test]
    async fn startup_fails_if_the_default_profile_cannot_be_created() {
        let dir = std::env::temp_dir().join(format!("dinero_profile_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = super::init_db(dir.join("finance.db")).await.unwrap();
        pool.get()
            .await
            .unwrap()
            .interact(|c| {
                c.execute_batch(
                    "DELETE FROM local_profile;
                     CREATE TRIGGER no_profile BEFORE INSERT ON local_profile
                     BEGIN SELECT RAISE(ABORT, 'simulated failure'); END;",
                )
            })
            .await
            .unwrap()
            .unwrap();
        pool.close();
        assert!(super::init_db(dir.join("finance.db")).await.is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Review L3: with no pending migration, startup takes no full backup.
    #[tokio::test]
    async fn no_pre_migration_backup_when_nothing_is_pending() {
        let dir = std::env::temp_dir().join(format!("dinero_l3_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let count_baks = || {
            std::fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("finance.db.bak."))
                .count()
        };
        let pool = super::init_db(dir.join("finance.db")).await.unwrap();
        let after_first = count_baks();
        pool.close();
        let pool = super::init_db(dir.join("finance.db")).await.unwrap();
        assert_eq!(count_baks(), after_first, "second start must not copy the database");
        let conn = pool.get().await.unwrap();
        let pending = conn
            .interact(|c| super::migrations::has_pending_migrations(c))
            .await
            .unwrap()
            .unwrap();
        assert!(!pending);
        std::fs::remove_dir_all(&dir).unwrap();
    }

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

#[cfg(test)]
mod busy_retry_tests {
    use super::*;

    fn busy() -> anyhow::Error {
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY), None).into()
    }

    /// Audit D-1: a busy database is retried; other errors are not; and a
    /// failure that left a transaction open is never re-run.
    #[test]
    fn busy_writes_are_retried_only_when_nothing_is_left_open() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let mut calls = 0;
        let result = retry_on_busy(&conn, || {
            calls += 1;
            if calls < 3 { Err(busy()) } else { Ok(calls) }
        });
        assert_eq!(result.unwrap(), 3);

        let mut calls = 0;
        let _ = retry_on_busy(&conn, || -> anyhow::Result<()> {
            calls += 1;
            Err(anyhow::anyhow!("not busy"))
        });
        assert_eq!(calls, 1);

        conn.execute_batch("BEGIN").unwrap();
        let mut calls = 0;
        let _ = retry_on_busy(&conn, || -> anyhow::Result<()> {
            calls += 1;
            Err(busy())
        });
        assert_eq!(calls, 1, "an open transaction is not retried");
    }

    #[tokio::test]
    async fn pool_is_bounded() {
        let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = init_db(dir.join("t.db")).await.unwrap();
        assert_eq!(pool.status().max_size, POOL_MAX_SIZE);
        // Audit N-D3 (owner decision): waiting for a connection times out.
        assert_eq!(pool.timeouts().wait, Some(POOL_WAIT_TIMEOUT));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Audit N-D3: opening a new pooled connection no longer needs the write
    /// lock, so it succeeds while another connection holds it (a long
    /// statement commit), and a new database still gets incremental
    /// auto-vacuum.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn new_connection_opens_while_the_write_lock_is_held() {
        let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = init_db(dir.join("t.db")).await.unwrap();
        let holder = pool.get().await.unwrap();
        holder.interact(|c| c.execute_batch("BEGIN IMMEDIATE")).await.unwrap().unwrap();

        let started = std::time::Instant::now();
        let fresh = pool.get().await.expect("a new connection must open under a held write lock");
        assert!(started.elapsed() < std::time::Duration::from_secs(4), "it must not wait for the lock");
        let mode: i64 = fresh
            .interact(|c| c.query_row("PRAGMA auto_vacuum", [], |r| r.get(0)))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(mode, 2, "incremental auto-vacuum");

        holder.interact(|c| c.execute_batch("COMMIT")).await.unwrap().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
