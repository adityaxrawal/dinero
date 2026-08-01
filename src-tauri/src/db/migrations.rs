use anyhow::{Context, Result};
use rusqlite::Connection;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection as SqlxConnectionTrait, SqliteConnection};
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Pre-migration backups retained before older ones are pruned (Doc 18 §12.1).
const PRE_MIGRATION_BACKUP_RETENTION: usize = 3;

/// Creates `finance.db.bak.[timestamp]` before migrations run (Doc 18 §12.1),
/// then prunes to the last `PRE_MIGRATION_BACKUP_RETENTION` backups. Uses
/// `VACUUM INTO` from the live connection rather than a raw file copy — the
/// database runs in WAL mode, so copying just the main `.db` file could miss
/// data still sitting in the `-wal` sidecar; `VACUUM INTO` always produces a
/// complete, consistent snapshot of the current committed state.
///
/// Returns the backup's path so a failed migration can offer a rollback to it
/// (C18 fix — see `db::restore_backup_file`).
pub fn create_pre_migration_backup(conn: &Connection, db_path: &Path) -> Result<PathBuf> {
    let backup_dir = db_path.parent().unwrap_or_else(|| Path::new("."));
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S%3f").to_string();
    let backup_path = backup_dir.join(format!("finance.db.bak.{}", timestamp));

    conn.execute(
        "VACUUM INTO ?1",
        [backup_path.to_string_lossy().to_string()],
    )
    .map_err(|e| anyhow::anyhow!("Failed to create pre-migration backup: {}", e))?;

    // audit_08 #10: `VACUUM INTO` from an encrypted connection does produce an
    // encrypted, complete file — but nothing checked it, and this is the copy
    // a failed migration rolls back to. `verify_backup_integrity` re-opens it
    // with the derived key and runs `integrity_check`, so an unopenable,
    // unencrypted or truncated backup is caught now rather than at the moment
    // it is needed. Its doc comment already named `finance.db.bak.*` as its
    // subject; it was simply never wired into this path.
    //
    // Verify before pruning: a bad new backup must not push a good older one
    // out of the retention window.
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

/// Deletes all but the most recent `PRE_MIGRATION_BACKUP_RETENTION` pre-migration
/// backups. Pruning failures are logged, not propagated — a stale extra backup
/// file is not worth aborting startup over.
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

    // Timestamps are formatted %Y%m%d%H%M%S%3f, so lexicographic sort is
    // chronological — oldest first.
    backups.sort();

    if backups.len() > PRE_MIGRATION_BACKUP_RETENTION {
        for old in &backups[..backups.len() - PRE_MIGRATION_BACKUP_RETENTION] {
            if let Err(e) = std::fs::remove_file(old) {
                tracing::warn!("Failed to prune old pre-migration backup {:?}: {}", old, e);
            }
        }
    }
}

/// TASK-DB-002: runs all pending migrations via `sqlx::migrate!`, embedded
/// at compile time from `migrations/` (path is resolved relative to
/// `CARGO_MANIFEST_DIR` — i.e. `src-tauri/migrations/` — by the macro
/// itself, regardless of which source file invokes it).
///
/// sqlx and the app's normal rusqlite/`deadpool_sqlite` connection stack are
/// entirely separate — this opens its own dedicated `sqlx::SqliteConnection`
/// to the same file, setting the identical `key` and cipher pragmas
/// `db::init_db` uses when `sqlcipher_key` is `Some` (production always
/// passes a key; test call sites across the codebase pass `None` to get a
/// plain, unencrypted schema-only database — matching the semantics the
/// old `Connection::open_in_memory()` test fixtures had, and avoiding
/// needing to thread a key through every pooled test helper afterward).
///
/// This works — sqlx can open an SQLCipher-encrypted file at all — only
/// because `sqlx-sqlite` and `rusqlite` both resolve to the exact same
/// `libsqlite3-sys` version; Cargo's feature unification means enabling
/// `bundled-sqlcipher` via rusqlite's `Cargo.toml` entry compiles that
/// *shared* `libsqlite3-sys` artifact with SQLCipher support, which sqlx's
/// connection then also links against. This was verified empirically (a
/// throwaway feasibility test, not kept in the tree) before committing to
/// this rewrite — sqlx has no official SQLCipher feature flag of its own.
///
/// `sqlx::migrate!` tracks applied migrations in its own `_sqlx_migrations`
/// table, which Document 18 §13.1/§13.2 (schema version gate) already
/// assumes exists — that section was written assuming sqlx, independent of
/// this task, and is further confirmation this is the intended engine.
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
