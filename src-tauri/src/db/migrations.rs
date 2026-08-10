//! Schema migrations, with a backup taken before each run.
//!
//! A failed migration on a financial ledger is the worst case this codebase
//! plans for, so a full copy is written before any schema change is applied. If
//! the migration then fails, startup can offer to roll back to that exact
//! pre-migration state.
//!
//! Only the most recent few backups are retained; without a cap they would
//! accumulate a full database copy per release indefinitely.

use anyhow::{Context, Result};
use rusqlite::Connection;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection as SqlxConnectionTrait, SqliteConnection};
use std::path::{Path, PathBuf};
use std::str::FromStr;

// Keep only the last few pre-migration backups -- each is a full copy of the
// database, so unbounded retention would grow without limit.
const PRE_MIGRATION_BACKUP_RETENTION: usize = 3;

/// Copies the database before a migration runs.
///
/// The safety net that makes a failed migration recoverable: startup can offer a
/// rollback to exactly this pre-migration state.
pub fn create_pre_migration_backup(conn: &Connection, db_path: &Path) -> Result<PathBuf> {
    let backup_dir = db_path.parent().unwrap_or_else(|| Path::new("."));
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S%3f").to_string();
    let backup_path = backup_dir.join(format!("finance.db.bak.{}", timestamp));

    conn.execute(
        "VACUUM INTO ?1",
        [backup_path.to_string_lossy().to_string()],
    )
    .map_err(|e| anyhow::anyhow!("Failed to create pre-migration backup: {}", e))?;

    if let Err(e) = crate::db::backup::verify_backup_integrity(&backup_path) {
        let _ = std::fs::remove_file(&backup_path);
        return Err(anyhow::anyhow!(
            "Pre-migration backup was created but failed verification ({}). \
             Refusing to migrate without a restorable backup.",
            e
        ));
    }

    prune_old_pre_migration_backups(backup_dir);
    Ok(backup_path)
}

/// Deletes all but the most recent few backups.
///
/// Each is a full copy, so unbounded retention would grow by a database per
/// release.
fn prune_old_pre_migration_backups(backup_dir: &Path) {
    let entries = match std::fs::read_dir(backup_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Failed to read backup directory for pruning: {}", e);
            return;
        }
    };

    let mut backups: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("finance.db.bak."))
                .unwrap_or(false)
        })
        .collect();

    backups.sort();

    if backups.len() > PRE_MIGRATION_BACKUP_RETENTION {
        for old in &backups[..backups.len() - PRE_MIGRATION_BACKUP_RETENTION] {
            if let Err(e) = std::fs::remove_file(old) {
                tracing::warn!("Failed to prune old pre-migration backup {:?}: {}", old, e);
            }
        }
    }
}

/// Applies outstanding schema migrations.
///
/// The connection must already be keyed, since an encrypted database cannot even
/// be read to determine its current version until it is.
pub async fn run_migrations(db_path: &Path, sqlcipher_key: Option<&str>) -> Result<()> {
    let mut opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path.display()))
        .context("Invalid database path for sqlx migration connection")?
        .create_if_missing(true);

    if let Some(key) = sqlcipher_key {
        opts = opts
            .pragma("key", format!("'{}'", key))
            .pragma("cipher_page_size", "4096")
            .pragma("kdf_iter", "256000")
            .pragma("cipher_hmac_algorithm", "HMAC_SHA512");
    }

    let mut conn = SqliteConnection::connect_with(&opts)
        .await
        .context("Failed to open sqlx connection for migrations")?;

    sqlx::migrate!("./migrations")
        .run(&mut conn)
        .await
        .context("sqlx migration run failed")?;

    let _ = conn.close().await;
    Ok(())
}
