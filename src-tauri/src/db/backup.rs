use aes_gcm::aead::{rand_core::RngCore, Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, Result};
use argon2::Argon2;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;

fn derive_key_from_password(password: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let mut key_bytes = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut key_bytes)
        .map_err(|e| anyhow!("Argon2 key derivation failed: {}", e))?;
    Ok(key_bytes)
}

/// Doc 30 TASK-API-008: `settings_export_encrypted_backup`'s core -- the
/// manual, password-protected Mac-to-Mac transfer path, distinct from
/// TASK-DB-020's automatic daily SQLCipher-native backup. Encrypts a raw
/// SQLite snapshot with a caller-supplied password via AES-256-GCM, key
/// derived through Argon2id with a fresh random salt per export -- not the
/// app's own SQLCipher base key, since this backup must be restorable on a
/// different Mac with no access to this machine's Keychain.
///
/// Output format: `<16-byte salt><12-byte nonce><ciphertext>`.
pub fn encrypt_backup(plaintext_db_bytes: &[u8], password: &str) -> Result<Vec<u8>> {
    if password.is_empty() {
        return Err(anyhow!("password must not be empty"));
    }
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);

    let key_bytes = derive_key_from_password(password, &salt)?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext_db_bytes)
        .map_err(|e| anyhow!("AES-GCM encrypt error: {:?}", e))?;

    let mut blob = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&salt);
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ciphertext);
    Ok(blob)
}

/// Doc 30 TASK-API-008: `settings_import_encrypted_backup`'s core. A wrong
/// password fails cleanly via AES-GCM's built-in authentication tag check
/// (`test_import_backup_wrong_password_fails_cleanly`) -- it can never
/// silently produce corrupt-but-accepted plaintext, unlike a non-AEAD
/// cipher would. Returns the raw decrypted SQLite bytes; safely swapping
/// them into place as the live database (integrity-checking, atomic
/// replace, leaving the original untouched on failure) is TASK-OPS-002's
/// job, not this function's -- this only reverses `encrypt_backup`.
pub fn decrypt_backup(blob: &[u8], password: &str) -> Result<Vec<u8>> {
    if blob.len() <= SALT_LEN + NONCE_LEN {
        return Err(anyhow!("backup file is too short to be valid"));
    }
    let salt = &blob[..SALT_LEN];
    let nonce_bytes = &blob[SALT_LEN..SALT_LEN + NONCE_LEN];
    let ciphertext = &blob[SALT_LEN + NONCE_LEN..];

    let key_bytes = derive_key_from_password(password, salt)?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| anyhow!("decryption failed -- wrong password or corrupt backup file"))
}

/// Doc 30 TASK-OPS-002: verifies a SQLCipher-native rolling backup
/// (`finance.db.daily.bak`/`finance.db.bak.*`, written by `VACUUM INTO` from
/// the live encrypted connection — a completely different format from
/// `encrypt_backup`/`decrypt_backup`'s AES-256-GCM password-wrapped manual
/// export above) actually opens cleanly and passes `PRAGMA integrity_check`
/// before it's ever trusted as a restore source. Opens the file directly
/// (not through the app's `deadpool_sqlite` pool, which is bound to one
/// fixed path) with the same key-derivation and SQLCipher PRAGMA sequence
/// `db::init_db` uses, so a genuinely wrong/missing key surfaces the same
/// "file is not a database" SQLCipher error `init_db` already handles,
/// rather than a confusing new failure mode.
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

/// Doc 30 TASK-OPS-002 acceptance `test_restore_failure_leaves_original_db_intact`:
/// copies `source` into place at `dest` via a temp-file-then-rename so a
/// failure partway through (disk full, permissions, process killed) never
/// leaves `dest` partially overwritten -- `rename` on the same filesystem is
/// atomic, unlike `std::fs::copy` writing directly over the live path.
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

    /// Doc 30 TASK-OPS-002 acceptance: `test_backup_verification_detects_corruption`.
    /// A file that isn't a valid SQLite/SQLCipher database at all (the
    /// simplest, most unambiguous corruption case) must fail verification,
    /// not be silently accepted as a valid restore source.
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

    /// Doc 30 TASK-OPS-002 acceptance:
    /// `test_restore_failure_leaves_original_db_intact` (the atomic-replace
    /// half). If the source doesn't exist, the destination must be
    /// completely untouched -- never partially written.
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

    #[test]
    fn test_export_backup_requires_password() {
        let result = encrypt_backup(b"fake sqlite bytes", "");
        assert!(result.is_err(), "an empty password must be rejected");
    }

    #[test]
    fn test_import_backup_wrong_password_fails_cleanly() {
        let original = b"pretend this is a real SQLite file's bytes";
        let blob = encrypt_backup(original, "correct-horse-battery-staple").unwrap();

        let result = decrypt_backup(&blob, "wrong-password");
        assert!(
            result.is_err(),
            "a wrong password must fail cleanly, not silently produce garbage plaintext"
        );
    }

    #[test]
    fn test_encrypt_decrypt_backup_roundtrip() {
        let original = b"pretend this is a real SQLite file's bytes";
        let blob = encrypt_backup(original, "correct-horse-battery-staple").unwrap();

        let decrypted = decrypt_backup(&blob, "correct-horse-battery-staple").unwrap();
        assert_eq!(decrypted, original);
    }
}
