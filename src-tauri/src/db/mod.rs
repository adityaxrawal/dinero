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

    let cfg = Config::new(&db_path);
    let pool = cfg
        .builder(Runtime::Tokio1)
        .map_err(|e| DbInitError::Other(anyhow::anyhow!(e)))?
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
                        PRAGMA auto_vacuum = INCREMENTAL;
                        PRAGMA busy_timeout = 5000;
                        ",
                    )?;
                    let auto_vacuum_mode: i64 =
                        c.query_row("PRAGMA auto_vacuum", [], |r| r.get(0))?;
                    // auto_vacuum can only be changed by a full VACUUM, so a
                    // database created before this setting is rewritten once.
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
    let integrity_or_backup: Result<PathBuf, String> = conn
        .interact(move |c| {
            let count: i64 = c.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))?;
            info!("Database initialized successfully. Tables count: {}", count);

            let integrity: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
            if integrity != "ok" {
                warn!("Database integrity check failed: {}", integrity);
                return Ok::<Result<PathBuf, String>, anyhow::Error>(Err(integrity));
            }
            info!("Database integrity check passed.");

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

    if let Err(source) = migrations::run_migrations(&db_path, Some(&db_key_for_migration)).await {
        return Err(DbInitError::MigrationFailed {
            source,
            backup_path,
        });
    }

    conn.interact(move |c| {
        let _ = c.execute(
            "INSERT OR IGNORE INTO local_profile (
                id, primary_email, display_name, timezone, spending_limit_monthly,
                limit_thresholds, recovery_phrase_enabled
             ) VALUES (1, NULL, 'Default User', 'Asia/Kolkata', 30000.0, '[80,90,100]', 0)",
            [],
        );

        let _ = c.execute(
            "UPDATE processing_checkpoints SET status = 'failed' WHERE status = 'in_progress'",
            [],
        );

        let _ = instruments::cleanup_corrupted_vpa_instruments(c);
    })
    .await
    .map_err(|e| {
        DbInitError::Other(anyhow::anyhow!(
            "Interact error during post-migration seeding: {}",
            e
        ))
    })?;

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
