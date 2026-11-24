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

/// Whether any embedded migration has not yet been applied successfully.
///
/// A database without the sqlx bookkeeping table (a fresh one) has everything
/// pending.
pub fn has_pending_migrations(conn: &Connection) -> Result<bool> {
    let table_exists: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations')",
        [],
        |r| r.get(0),
    )?;
    if !table_exists {
        return Ok(true);
    }
    let applied: std::collections::HashSet<i64> = conn
        .prepare("SELECT version FROM _sqlx_migrations WHERE success = 1")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(sqlx::migrate!("./migrations")
        .iter()
        .any(|m| !applied.contains(&m.version)))
}

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
///
/// Refuses to run if the database records a migration this build does not
/// know: an older app on a newer schema would otherwise see "nothing pending"
/// and run against tables it does not understand.
///
/// Foreign keys are OFF on this connection (audit C-5): sqlx runs each
/// migration in a transaction, where `PRAGMA foreign_keys` cannot be changed,
/// so a table rebuild (create-copy-drop-rename) would otherwise fire
/// `ON DELETE CASCADE` on the children of the dropped parent. This is SQLite's
/// documented rebuild procedure; `foreign_key_check` afterwards fails the
/// migration if it left new violations. No existing migration relies on a
/// cascade firing.
pub async fn run_migrations(db_path: &Path, sqlcipher_key: Option<&str>) -> Result<()> {
    run_migrations_with(db_path, sqlcipher_key, sqlx::migrate!("./migrations")).await
}

async fn run_migrations_with(
    db_path: &Path,
    sqlcipher_key: Option<&str>,
    migrator: sqlx::migrate::Migrator,
) -> Result<()> {
    let mut opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path.display()))
        .context("Invalid database path for sqlx migration connection")?
        .create_if_missing(true)
        .foreign_keys(false);

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

    reject_unknown_applied_versions(&migrator, &mut conn).await?;

    let violations_before = foreign_key_violations(&mut conn).await?;
    migrator
        .run(&mut conn)
        .await
        .context("sqlx migration run failed")?;
    let violations_after = foreign_key_violations(&mut conn).await?;
    if violations_after > violations_before {
        anyhow::bail!(
            "migration left {} new foreign-key violation(s)",
            violations_after - violations_before
        );
    }
    if violations_after > 0 {
        tracing::warn!("{} foreign-key violation(s) predate this migration run", violations_after);
    }

    let _ = conn.close().await;
    Ok(())
}

async fn foreign_key_violations(conn: &mut SqliteConnection) -> Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
        .fetch_one(&mut *conn)
        .await
        .context("foreign_key_check failed")
}

const NEWER_SCHEMA_MESSAGE: &str =
    "This database was created by a newer version of Dinero; update the app to open it.";

/// Errors if `_sqlx_migrations` holds a version absent from this build.
async fn reject_unknown_applied_versions(
    migrator: &sqlx::migrate::Migrator,
    conn: &mut SqliteConnection,
) -> Result<()> {
    let table_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations')",
    )
    .fetch_one(&mut *conn)
    .await
    .context("Failed to inspect migration history")?;
    if !table_exists {
        return Ok(());
    }

    let applied: Vec<i64> = sqlx::query_scalar("SELECT version FROM _sqlx_migrations")
        .fetch_all(&mut *conn)
        .await
        .context("Failed to read applied migration versions")?;
    let unknown: Vec<i64> = applied
        .into_iter()
        .filter(|v| !migrator.iter().any(|m| m.version == *v))
        .collect();
    if !unknown.is_empty() {
        anyhow::bail!(
            "{} (unknown migration versions: {:?})",
            NEWER_SCHEMA_MESSAGE,
            unknown
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Audit C-5: a table-rebuild migration must not cascade-delete the
    /// children of the parent it drops.
    #[tokio::test]
    async fn table_rebuild_migration_keeps_child_rows() {
        let dir = std::env::temp_dir().join(format!("dinero_mig_{}", uuid::Uuid::new_v4()));
        let migrations = dir.join("migrations");
        std::fs::create_dir_all(&migrations).unwrap();
        std::fs::write(
            migrations.join("1_create.sql"),
            "CREATE TABLE parent (id TEXT PRIMARY KEY);
             CREATE TABLE child (id TEXT PRIMARY KEY,
                                 parent_id TEXT REFERENCES parent(id) ON DELETE CASCADE);
             INSERT INTO parent VALUES ('p');
             INSERT INTO child VALUES ('c', 'p');",
        )
        .unwrap();
        std::fs::write(
            migrations.join("2_rebuild.sql"),
            "CREATE TABLE parent_new (id TEXT PRIMARY KEY, note TEXT);
             INSERT INTO parent_new (id) SELECT id FROM parent;
             DROP TABLE parent;
             ALTER TABLE parent_new RENAME TO parent;",
        )
        .unwrap();
        let path = dir.join("t.db");
        let migrator = sqlx::migrate::Migrator::new(migrations.as_path()).await.unwrap();
        run_migrations_with(&path, None, migrator).await.unwrap();

        let children: i64 = Connection::open(&path)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM child", [], |r| r.get(0))
            .unwrap();
        assert_eq!(children, 1, "dropping the old parent must not cascade");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn refuses_database_with_migration_from_newer_build() {
        let path = crate::db::test_helpers::fresh_temp_db_path();
        run_migrations(&path, None).await.unwrap();

        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
             VALUES (99999999999999, 'from the future', 1, x'00', 0)",
            [],
        )
        .unwrap();
        drop(conn);

        let err = run_migrations(&path, None).await.unwrap_err();
        assert!(
            err.to_string().contains("newer version of Dinero"),
            "unexpected error: {err:#}"
        );
    }
}
