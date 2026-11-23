//! Encrypted database backups and their verification.
//!
//! Daily and pre-migration backups are keyed with the device key, like the live
//! database. Portable backups are the same SQLCipher database re-keyed to a
//! password the user chooses, so they can be restored on another machine. `atomic_replace` is
//! how a restore lands: writing in place would leave an unusable half-restored
//! database if the process died partway, so the new file is put in position
//! atomically or not at all.
use anyhow::{anyhow, Result};
use rusqlite::Connection;

/// Minimum length of a portable-backup password.
pub const MIN_PORTABLE_PASSWORD_LEN: usize = 8;

/// Tables a file must contain to be accepted as a Dinero backup.
const REQUIRED_TABLES: [&str; 3] = ["transactions", "instruments", "statements"];

fn validate_portable_password(password: &str) -> Result<()> {
    if password.chars().count() < MIN_PORTABLE_PASSWORD_LEN {
        return Err(anyhow!(
            "backup password must be at least {MIN_PORTABLE_PASSWORD_LEN} characters"
        ));
    }
    Ok(())
}

/// Copies the open database `conn` into a new SQLCipher file at `dest` keyed
/// with `key`, carrying the schema version across.
///
/// `sqlcipher_export` re-encrypts page by page, so no plaintext copy of the
/// database is ever written to disk. The copy is built beside `dest` and renamed
/// into place, so a failed export never leaves a half-written file at `dest`.
fn export_rekeyed(conn: &Connection, dest: &std::path::Path, key: &str) -> Result<()> {
    let parent = dest
        .parent()
        .ok_or_else(|| anyhow!("destination path has no parent directory"))?;
    let partial = parent.join(format!(".{}.partial", uuid::Uuid::new_v4()));
    let partial_str = partial.to_string_lossy().to_string();
    let result = (|| -> Result<()> {
        conn.execute("ATTACH DATABASE ?1 AS rekeyed KEY ?2", [&partial_str, key])?;
        let exported = (|| -> Result<()> {
            conn.query_row("SELECT sqlcipher_export('rekeyed')", [], |_| Ok(()))?;
            let version: i64 = conn.query_row("PRAGMA main.user_version", [], |r| r.get(0))?;
            conn.execute_batch(&format!("PRAGMA rekeyed.user_version = {version};"))?;
            Ok(())
        })();
        conn.execute_batch("DETACH DATABASE rekeyed;")?;
        exported?;
        std::fs::rename(&partial, dest)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result
}

/// Writes a portable backup: the live database re-keyed to a user password.
///
/// The device database key is bound to this machine's hardware UUID, so a raw
/// copy is unreadable anywhere else. Re-keying to a password the user chooses
/// makes the file restorable on any Mac.
pub fn export_portable_backup(
    conn: &Connection,
    dest: &std::path::Path,
    password: &str,
) -> Result<()> {
    validate_portable_password(password)?;
    export_rekeyed(conn, dest, password)
}

/// Opens a portable backup with its password, verifies it, and writes a copy
/// re-keyed to `device_key` into `staging_dir`. Returns the staged file's path.
///
/// A wrong password, a corrupt file, or a file that is not a Dinero database
/// all fail here, before the live database is touched.
pub fn stage_portable_restore(
    src: &std::path::Path,
    password: &str,
    staging_dir: &std::path::Path,
    device_key: &str,
) -> Result<std::path::PathBuf> {
    if !src.exists() {
        return Err(anyhow!("backup file does not exist: {}", src.display()));
    }
    let conn = Connection::open(src).map_err(|e| anyhow!("failed to open backup file: {e}"))?;
    conn.pragma_update(None, "key", password)?;
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(|_| anyhow!("could not open backup -- wrong password or not a Dinero backup"))?;
    if integrity != "ok" {
        return Err(anyhow!("backup failed integrity check: {integrity}"));
    }
    for table in REQUIRED_TABLES {
        let found: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |r| r.get(0),
        )?;
        if found == 0 {
            return Err(anyhow!(
                "file is not a Dinero backup (missing table {table})"
            ));
        }
    }
    let staged = staging_dir.join(format!("finance.db.restore.{}", uuid::Uuid::new_v4()));
    export_rekeyed(&conn, &staged, device_key)?;
    Ok(staged)
}

/// Confirms a backup file is a readable, structurally sound database.
///
/// An unverified backup is worse than none: it invites false confidence, and the
/// failure would only be discovered when it is needed.
pub fn verify_backup_integrity(backup_path: &std::path::Path) -> Result<()> {
    if !backup_path.exists() {
        return Err(anyhow!(
            "backup file does not exist: {}",
            backup_path.display()
        ));
    }
    let key = crate::db::crypto::derive_database_key()
        .map_err(|e| anyhow!("failed to derive database key for backup verification: {e}"))?;
    let conn = rusqlite::Connection::open(backup_path)
        .map_err(|e| anyhow!("failed to open backup file: {e}"))?;
    conn.execute_batch(&format!("PRAGMA key = '{}';", key))
        .map_err(|e| anyhow!("failed to unlock backup with the derived key: {e}"))?;
    conn.execute_batch(
        "PRAGMA cipher_page_size = 4096; PRAGMA kdf_iter = 256000; PRAGMA cipher_hmac_algorithm = HMAC_SHA512;",
    )
    .map_err(|e| anyhow!("failed to set SQLCipher parameters on backup: {e}"))?;

    let result: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(|e| anyhow!("integrity_check query failed (backup is likely corrupt or not a valid database): {e}"))?;
    if result != "ok" {
        return Err(anyhow!("backup failed integrity_check: {result}"));
    }
    Ok(())
}

/// Puts a restored file into place atomically.
///
/// A direct overwrite that is interrupted leaves a truncated database where a
/// working one used to be. Renaming into position means the file is either the
/// old one or the new one, never a partial mixture.
pub fn atomic_replace(source: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    let dest_parent = dest
        .parent()
        .ok_or_else(|| anyhow!("destination path has no parent directory"))?;
    let temp_path = dest_parent.join(format!(".{}.restoring.tmp", uuid::Uuid::new_v4()));
    std::fs::copy(source, &temp_path).map_err(|e| anyhow!("failed to stage restore copy: {e}"))?;
    std::fs::rename(&temp_path, dest).map_err(|e| {
        let _ = std::fs::remove_file(&temp_path);
        anyhow!("failed to atomically replace {}: {e}", dest.display())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_verification_detects_corruption() {
        let dir =
            std::env::temp_dir().join(format!("dinero_backup_verify_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let corrupt_path = dir.join("corrupt.bak");
        std::fs::write(&corrupt_path, b"this is not a sqlite database at all").unwrap();

        let result = verify_backup_integrity(&corrupt_path);
        assert!(
            result.is_err(),
            "a non-database file must fail backup verification"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_backup_verification_fails_on_missing_file() {
        let missing =
            std::env::temp_dir().join(format!("dinero_missing_{}.bak", uuid::Uuid::new_v4()));
        assert!(verify_backup_integrity(&missing).is_err());
    }

    #[test]
    fn test_atomic_replace_leaves_dest_untouched_on_missing_source() {
        let dir =
            std::env::temp_dir().join(format!("dinero_atomic_replace_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("finance.db");
        std::fs::write(&dest, b"original content").unwrap();
        let missing_source = dir.join("does_not_exist.bak");

        let result = atomic_replace(&missing_source, &dest);
        assert!(result.is_err());
        assert_eq!(
            std::fs::read(&dest).unwrap(),
            b"original content",
            "a failed restore must leave the original destination file completely untouched"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_atomic_replace_succeeds_with_valid_source() {
        let dir =
            std::env::temp_dir().join(format!("dinero_atomic_replace_ok_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("backup.bak");
        let dest = dir.join("finance.db");
        std::fs::write(&source, b"restored content").unwrap();
        std::fs::write(&dest, b"stale content").unwrap();

        atomic_replace(&source, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"restored content");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
