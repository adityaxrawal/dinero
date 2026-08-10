//! Key management for the encrypted database.
//!
//! Two keys exist and the distinction between them is the heart of this module.
//! A *base key* is generated once and stored in the OS keychain; it is the
//! user's actual secret and is what a recovery phrase reconstructs. The
//! *database key* is derived from the base key using the machine's hardware UUID
//! as an Argon2 salt, and is what SQLCipher is actually given.
//!
//! Deriving with a hardware-bound salt means a database file copied to another
//! machine cannot be opened there even with the same base key -- the salt
//! differs, so the derived key differs. That is also why a hardware change is
//! detected and reported at startup rather than surfacing as inexplicable
//! corruption: the marker file records the UUID last seen, so a mismatch is
//! recognisable as a migration rather than a broken database.

use aes_gcm::aead::{rand_core::RngCore, OsRng};
use anyhow::{Context, Result};
use argon2::{
    password_hash::{PasswordHasher, SaltString},
    Argon2,
};
use keyring::Entry;
use std::sync::OnceLock;
use uuid::Uuid;

const KEYCHAIN_SERVICE: &str = "com.dinero.app";
const KEYCHAIN_USER: &str = "dinero-base-key";

#[cfg(debug_assertions)]
static DEV_BASE_KEY: OnceLock<String> = OnceLock::new();

/// Where the development base key is kept.
///
/// Development builds deliberately avoid the keychain: it prompts on every
/// rebuild and each unsigned binary is treated as a different application. Falls
/// back to a temp path when HOME is unset, as it is under some test runners.
#[cfg(debug_assertions)]
fn dev_base_key_path() -> std::path::PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        let dir = std::path::PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("com.dinero.app");
        let _ = std::fs::create_dir_all(&dir);
        dir.join("dinero_dev_base_key.txt")
    } else {
        std::env::temp_dir().join("dinero_dev_base_key.txt")
    }
}

/// Development base key, read from disk or generated on first use.
///
/// Checks the persistent location first, then migrates a key from the older temp
/// location if one is found, so an existing dev database stays openable across
/// the change. Cached in a OnceLock to avoid re-reading per connection.
///
/// This path never touches the keychain and is compiled out of release builds.
#[cfg(debug_assertions)]
pub fn get_or_create_base_key() -> Result<String> {
    if let Some(key) = DEV_BASE_KEY.get() {
        return Ok(key.clone());
    }
    let persistent_path = dev_base_key_path();
    let temp_path = std::env::temp_dir().join("dinero_dev_base_key.txt");

    let key = if let Ok(key) = std::fs::read_to_string(&persistent_path) {
        key
    } else if let Ok(key) = std::fs::read_to_string(&temp_path) {
        let _ = std::fs::write(&persistent_path, &key);
        key
    } else {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        let new_key = hex::encode(bytes);
        let _ = std::fs::write(&persistent_path, &new_key);
        let _ = std::fs::write(&temp_path, &new_key);
        new_key
    };
    Ok(DEV_BASE_KEY.get_or_init(|| key).clone())
}

/// Release base key, held in the OS keychain.
///
/// Generated from 32 bytes of OS entropy on first launch. A missing entry is the
/// expected first-run case and is created; every other error is classified, since
/// a denied keychain needs different handling from a genuinely absent key.
#[cfg(not(debug_assertions))]
pub fn get_or_create_base_key() -> Result<String> {
    let entry =
        Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER).context("Failed to create keyring entry")?;

    match entry.get_password() {
        Ok(key) => Ok(key),
        Err(keyring::Error::NoEntry) => {
            let mut bytes = [0u8; 32];
            OsRng.fill_bytes(&mut bytes);
            let new_key = hex::encode(bytes);
            entry
                .set_password(&new_key)
                .map_err(|e| classify_keychain_error(e, "save new base key to"))?;
            Ok(new_key)
        }
        Err(e) => Err(classify_keychain_error(e, "read base key from")),
    }
}

#[cfg(not(debug_assertions))]
/// Distinguishes a denied keychain from other keychain failures.
///
/// The marker prefix is matched upstream to raise the permission overlay: access
/// denial is recoverable by the user, whereas other failures are not.
fn classify_keychain_error(e: keyring::Error, action: &str) -> anyhow::Error {
    match e {
        keyring::Error::NoStorageAccess(inner) => {
            anyhow::anyhow!(
                "KEYCHAIN_ACCESS_DENIED: failed to {} Keychain: {}",
                action,
                inner
            )
        }
        other => anyhow::anyhow!("Keychain error: {}", other),
    }
}

/// Decodes a base key to raw bytes, accepting both stored formats.
///
/// Older installs stored a UUID, newer ones hex. Both are accepted so a recovery
/// phrase can be produced regardless of when the key was created.
fn base_key_to_bytes(base_key: &str) -> Result<Vec<u8>> {
    if let Ok(uuid) = Uuid::parse_str(base_key) {
        Ok(uuid.as_bytes().to_vec())
    } else {
        hex::decode(base_key).context("base_key is neither a valid UUID nor valid hex")
    }
}

/// Encodes recovered bytes back into a base key, choosing format by length.
///
/// 16 bytes means the legacy UUID form, 32 the current hex form. Any other length
/// indicates a corrupted phrase and is rejected rather than guessed at.
fn bytes_to_base_key(bytes: &[u8]) -> Result<String> {
    match bytes.len() {
        16 => Ok(Uuid::from_slice(bytes)?.to_string()),
        32 => Ok(hex::encode(bytes)),
        n => Err(anyhow::anyhow!(
            "Unexpected recovered key length: {} bytes",
            n
        )),
    }
}

/// Renders the base key as a BIP39 mnemonic.
///
/// A word list is used because this is the one secret a user must transcribe by
/// hand; BIP39 words are unambiguous when written down and carry a checksum, so a
/// mistyped phrase is rejected rather than silently deriving a wrong key.
pub fn get_recovery_phrase() -> Result<String> {
    let base_key = get_or_create_base_key()?;
    let bytes = base_key_to_bytes(&base_key)?;
    let mnemonic = bip39::Mnemonic::from_entropy(&bytes)
        .context("Failed to encode base_key as a BIP39 mnemonic")?;
    Ok(mnemonic.to_string())
}

/// Reconstructs the base key from a mnemonic.
///
/// Parsing is normalised, so casing and spacing differences in a hand-typed
/// phrase do not cause a spurious failure.
fn base_key_from_phrase(phrase: &str) -> Result<String> {
    let mnemonic = bip39::Mnemonic::parse_normalized(phrase)
        .context("Invalid recovery phrase — must be 12 or 24 valid BIP39 words")?;
    let bytes = mnemonic.to_entropy();
    bytes_to_base_key(&bytes)
}

/// Confirms a candidate key actually opens the database before it is stored.
///
/// Without this, a valid-looking but wrong phrase would overwrite the keychain
/// entry and leave the real database permanently unopenable. Reading from
/// sqlite_master is the cheapest operation that genuinely requires decryption --
/// opening the file alone succeeds even with the wrong key.
fn verify_key_decrypts(db_path: &std::path::Path, base_key: &str) -> Result<()> {
    let db_key = derive_database_key_from_base_key(base_key)?;
    let conn = rusqlite::Connection::open(db_path).context("Failed to open database file")?;
    conn.execute_batch(&format!("PRAGMA key = '{}';", db_key))?;
    conn.execute_batch(
        "PRAGMA cipher_page_size = 4096;
         PRAGMA kdf_iter = 256000;
         PRAGMA cipher_hmac_algorithm = HMAC_SHA512;",
    )?;
    let _: i64 = conn
        .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
        .context("Recovery phrase did not decrypt the database — it is likely incorrect")?;
    Ok(())
}

/// Restores the base key from a recovery phrase and writes it to the keychain.
///
/// Verification precedes the write, so a failed recovery leaves the existing
/// entry untouched.
pub fn restore_base_key_from_phrase(phrase: &str, db_path: &std::path::Path) -> Result<String> {
    let base_key = base_key_from_phrase(phrase)?;
    verify_key_decrypts(db_path, &base_key)?;

    let entry =
        Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER).context("Failed to create keyring entry")?;
    entry
        .set_password(&base_key)
        .context("Failed to write recovered base key to Keychain")?;
    Ok(base_key)
}

/// Removes the development key from both its current and legacy locations.
#[cfg(debug_assertions)]
pub fn delete_base_key() {
    let _ = std::fs::remove_file(dev_base_key_path());
    let _ = std::fs::remove_file(std::env::temp_dir().join("dinero_dev_base_key.txt"));
}

/// Removes the keychain entry.
///
/// Errors are ignored: this runs during data deletion, where an already-absent
/// entry is the desired end state anyway.
#[cfg(not(debug_assertions))]
pub fn delete_base_key() {
    if let Ok(entry) = Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER) {
        let _ = entry.delete_credential();
    }
}

/// Reads the machine's hardware UUID, used as the key-derivation salt.
pub fn get_hardware_uuid() -> Result<String> {
    machine_uid::get().map_err(|e| anyhow::anyhow!("Failed to read hardware UUID: {}", e))
}

/// Derives the SQLCipher key for this machine.
pub fn derive_database_key() -> Result<String> {
    let base_key = get_or_create_base_key()?;
    derive_database_key_from_base_key(&base_key)
}

/// Derives the database key from a supplied base key.
///
/// Separate from the above so recovery can verify a candidate key without first
/// writing it to the keychain.
pub fn derive_database_key_from_base_key(base_key: &str) -> Result<String> {
    let hw_uuid = get_hardware_uuid()?;
    derive_database_key_with_hw_uuid(base_key, &hw_uuid)
}

/// Derive the SQLCipher key from the base key, salted with the hardware UUID.
///
/// Argon2 is used rather than a plain hash because the base key is the only
/// secret protecting the database at rest, and a memory-hard derivation makes
/// offline brute force expensive.
///
/// The salt is padded or truncated to a fixed 16 bytes, since hardware UUID
/// length is not guaranteed across platforms and the salt size must be stable --
/// changing it would make every existing database underivable.
fn derive_database_key_with_hw_uuid(base_key: &str, hw_uuid: &str) -> Result<String> {
    let salt_bytes = hw_uuid.as_bytes();
    let mut padded_salt = [0u8; 16];
    let len = salt_bytes.len().min(16);
    padded_salt[..len].copy_from_slice(&salt_bytes[..len]);

    let salt = SaltString::encode_b64(&padded_salt)
        .map_err(|e| anyhow::anyhow!("Salt encoding error: {}", e))?;

    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(base_key.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("Argon2 hashing failed: {}", e))?;

    Ok(password_hash.to_string())
}

// Records the hardware UUID last seen. A change means the database has moved to
// a different machine, which startup reports as a migration rather than letting
// it surface as an unexplained key failure.
const HW_UUID_MARKER_FILENAME: &str = "hw_uuid_marker.txt";

/// Path of the file recording the hardware UUID last seen.
fn hw_uuid_marker_path(app_data_dir: &std::path::Path) -> std::path::PathBuf {
    app_data_dir.join(HW_UUID_MARKER_FILENAME)
}

/// Records the current hardware UUID after a successful open.
///
/// Written only on success, so the marker always reflects a UUID the database
/// was genuinely opened with.
pub fn record_last_known_hw_uuid(app_data_dir: &std::path::Path) {
    match get_hardware_uuid() {
        Ok(hw_uuid) => {
            if let Err(e) = std::fs::write(hw_uuid_marker_path(app_data_dir), &hw_uuid) {
                tracing::warn!("Failed to write hardware UUID marker: {}", e);
            }
        }
        Err(e) => tracing::warn!("Failed to read current hardware UUID for marker: {}", e),
    }
}

/// Whether the machine appears to have changed since the last successful open.
///
/// A missing marker is not a migration -- that is simply a first run, or an
/// install predating the marker.
pub fn hw_uuid_marker_indicates_migration(app_data_dir: &std::path::Path) -> bool {
    let Ok(old_hw_uuid) = std::fs::read_to_string(hw_uuid_marker_path(app_data_dir)) else {
        return false;
    };
    let old_hw_uuid = old_hw_uuid.trim();
    if old_hw_uuid.is_empty() {
        return false;
    }
    match get_hardware_uuid() {
        Ok(current) => old_hw_uuid != current,
        Err(_) => false,
    }
}

/// Re-keys the database after the machine's hardware UUID changed.
///
/// Because the UUID is the derivation salt, moving to new hardware changes the
/// derived key and the file stops opening. This re-derives with the previous UUID
/// and re-keys to the new one, which is what allows a restored or migrated Mac to
/// recover without the recovery phrase.
pub fn try_migrate_hardware_uuid(
    db_path: &std::path::Path,
    app_data_dir: &std::path::Path,
) -> Result<bool> {
    let marker_path = hw_uuid_marker_path(app_data_dir);
    let Ok(old_hw_uuid) = std::fs::read_to_string(&marker_path) else {
        return Ok(false);
    };
    let old_hw_uuid = old_hw_uuid.trim().to_string();
    let current_hw_uuid = get_hardware_uuid()?;
    if old_hw_uuid.is_empty() || old_hw_uuid == current_hw_uuid {
        return Ok(false);
    }

    let base_key = get_or_create_base_key()?;
    let old_key = derive_database_key_with_hw_uuid(&base_key, &old_hw_uuid)?;
    let new_key = derive_database_key_with_hw_uuid(&base_key, &current_hw_uuid)?;

    let conn = rusqlite::Connection::open(db_path).context("Failed to open database file")?;
    conn.execute_batch(&format!("PRAGMA key = '{}';", old_key))?;
    conn.execute_batch(
        "PRAGMA cipher_page_size = 4096;
         PRAGMA kdf_iter = 256000;
         PRAGMA cipher_hmac_algorithm = HMAC_SHA512;",
    )?;
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    })
    .context("Old hardware-UUID key did not decrypt the database — not a hardware migration")?;

    conn.execute_batch(&format!("PRAGMA rekey = '{}';", new_key))
        .context("Failed to rekey database to the current hardware UUID")?;
    drop(conn);

    if let Err(e) = std::fs::write(&marker_path, &current_hw_uuid) {
        tracing::warn!(
            "Rekeyed successfully but failed to update hardware UUID marker: {}",
            e
        );
    }

    tracing::info!("Hardware UUID migration: re-keyed database to the current machine's hardware");
    Ok(true)
}

#[cfg(test)]
mod hardware_migration_tests {
    use super::*;

    #[test]
    fn derive_database_key_is_deterministic_per_hw_uuid() {
        let key_a = derive_database_key_with_hw_uuid("base", "hw-old").unwrap();
        let key_b = derive_database_key_with_hw_uuid("base", "hw-old").unwrap();
        assert_eq!(key_a, key_b);
    }

    #[test]
    fn derive_database_key_differs_across_hw_uuid() {
        let old_key = derive_database_key_with_hw_uuid("base", "hw-old").unwrap();
        let new_key = derive_database_key_with_hw_uuid("base", "hw-new").unwrap();
        assert_ne!(old_key, new_key);
    }

    #[test]
    fn rekey_round_trip_old_key_stops_working_new_key_opens() {
        let dir =
            std::env::temp_dir().join(format!("dinero_hw_migration_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("data.db");

        let old_key = derive_database_key_with_hw_uuid("base", "hw-old").unwrap();
        let new_key = derive_database_key_with_hw_uuid("base", "hw-new").unwrap();

        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(&format!("PRAGMA key = '{}';", old_key))
                .unwrap();
            conn.execute_batch(
                "PRAGMA cipher_page_size = 4096; PRAGMA kdf_iter = 256000; \
                 PRAGMA cipher_hmac_algorithm = HMAC_SHA512;",
            )
            .unwrap();
            conn.execute("CREATE TABLE t (id INTEGER PRIMARY KEY)", [])
                .unwrap();
        }

        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(&format!("PRAGMA key = '{}';", old_key))
                .unwrap();
            conn.execute_batch(
                "PRAGMA cipher_page_size = 4096; PRAGMA kdf_iter = 256000; \
                 PRAGMA cipher_hmac_algorithm = HMAC_SHA512;",
            )
            .unwrap();
            conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap();
            conn.execute_batch(&format!("PRAGMA rekey = '{}';", new_key))
                .unwrap();
        }

        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(&format!("PRAGMA key = '{}';", old_key))
                .unwrap();
            let result = conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
                r.get::<_, i64>(0)
            });
            assert!(
                result.is_err(),
                "old key should no longer decrypt the rekeyed database"
            );
        }

        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(&format!("PRAGMA key = '{}';", new_key))
                .unwrap();
            let count: i64 = conn
                .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 1);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hw_uuid_marker_migration_detection() {
        let dir =
            std::env::temp_dir().join(format!("dinero_hw_marker_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        assert!(!hw_uuid_marker_indicates_migration(&dir));

        let current = get_hardware_uuid().unwrap();

        std::fs::write(hw_uuid_marker_path(&dir), &current).unwrap();
        assert!(!hw_uuid_marker_indicates_migration(&dir));

        std::fs::write(hw_uuid_marker_path(&dir), "some-other-hw-uuid").unwrap();
        assert!(hw_uuid_marker_indicates_migration(&dir));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
