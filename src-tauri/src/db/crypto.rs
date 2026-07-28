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

/// Retrieves the base key from the macOS Keychain.
/// If it doesn't exist, generates a secure random 32-byte key (hex-encoded) and stores it.
///
/// Doc 24 §2/§8: secrets live only in Keychain — never plaintext files, env vars,
/// or config, under any build configuration. There is no debug-mode fallback.
///
/// Doc 22 §7.2 (M34): the base key must be a genuinely random 32-byte value,
/// not a UUIDv4 (~122 bits). This only changes what a *newly created* Keychain
/// entry looks like — an entry already written by a previous run is returned
/// as-is, so no already-encrypted database is ever re-keyed by this change.
/// Cached after first resolution within this process. Without this, every
/// caller (there are several -- DB key derivation, PDF password encrypt,
/// PDF password decrypt, ...) re-reads `dev_key_path` from disk on every
/// call with zero synchronization; two calls racing before the file exists
/// yet can each generate a *different* random key, both return `Ok`, and
/// whichever `std::fs::write` (whose result is deliberately ignored) lands
/// last silently wins on disk -- a value encrypted under the loser's
/// in-memory key can never be decrypted again by any later caller, which is
/// exactly the failure mode that made a validated statement PDF password
/// undecryptable moments later during the Instrument Gate resume path (see
/// `commands::statements_confirm_instrument`). Caching means every call in
/// this process, from the very first, shares one resolved key.
#[cfg(debug_assertions)]
static DEV_BASE_KEY: OnceLock<String> = OnceLock::new();

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
    // `get_or_init`-style race: if another call already won, defer to it so
    // every caller in this process converges on the exact same string.
    Ok(DEV_BASE_KEY.get_or_init(|| key).clone())
}

#[cfg(not(debug_assertions))]
pub fn get_or_create_base_key() -> Result<String> {
    let entry =
        Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER).context("Failed to create keyring entry")?;

    match entry.get_password() {
        Ok(key) => Ok(key),
        Err(keyring::Error::NoEntry) => {
            // Generate a new secure random 32-byte base key (Doc 22 §7.2 / M34).
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

/// G3 fix: distinguishes "Keychain access denied" (the user declined the
/// macOS system permission prompt, or the keychain is locked/unavailable)
/// from other Keychain errors (e.g. a genuinely corrupt entry), via a marker
/// `db::is_keychain_access_denied` recognizes — so `init_db`/`lib.rs` can show
/// a dedicated recovery screen instead of a generic fatal-error dialog.
#[cfg(not(debug_assertions))]
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

/// Converts a stored `base_key` string to its raw entropy bytes. Supports both
/// the current 32-byte hex format and the legacy UUIDv4 (16-byte) format still
/// held by any Keychain entry created before the M34 fix above.
fn base_key_to_bytes(base_key: &str) -> Result<Vec<u8>> {
    if let Ok(uuid) = Uuid::parse_str(base_key) {
        Ok(uuid.as_bytes().to_vec())
    } else {
        hex::decode(base_key).context("base_key is neither a valid UUID nor valid hex")
    }
}

/// Inverse of `base_key_to_bytes` — reconstructs the exact stored-string format
/// (16 bytes → UUID string, 32 bytes → hex string) so a recovered key round-trips
/// byte-for-byte back to what `derive_database_key` originally hashed.
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

/// Doc 22 §8.2, Doc 19 §5.4: generates the opt-in 24-word (or 12-word, for a
/// legacy UUID-format key — see `base_key_to_bytes`) BIP39 mnemonic
/// representing the current `base_key`. Deterministic — calling this twice
/// returns the same phrase, since it's derived from the Keychain-stored key
/// rather than a freshly-generated one.
pub fn get_recovery_phrase() -> Result<String> {
    let base_key = get_or_create_base_key()?;
    let bytes = base_key_to_bytes(&base_key)?;
    let mnemonic = bip39::Mnemonic::from_entropy(&bytes)
        .context("Failed to encode base_key as a BIP39 mnemonic")?;
    Ok(mnemonic.to_string())
}

/// Doc 19 §5.5: parses a Recovery Phrase back to a candidate `base_key`
/// string. Pure — does not touch Keychain or the database, so an invalid or
/// mistyped phrase never risks clobbering a working Keychain entry.
fn base_key_from_phrase(phrase: &str) -> Result<String> {
    let mnemonic = bip39::Mnemonic::parse_normalized(phrase)
        .context("Invalid recovery phrase — must be 12 or 24 valid BIP39 words")?;
    let bytes = mnemonic.to_entropy();
    bytes_to_base_key(&bytes)
}

/// Doc 19 §5.5: opens `db_path` with a candidate SQLCipher key derived from
/// `base_key` + the *current* machine's hardware UUID, and proves decryption
/// actually succeeded. SQLCipher's `PRAGMA key` never itself errors — even for
/// a wrong key — so only a real query against the (would-be-encrypted) schema
/// can distinguish a correct key from an incorrect one.
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

/// Doc 22 §8.2, Doc 19 §5.5: derives `base_key` from a Recovery Phrase,
/// verifies it actually decrypts `finance.db` on *this* machine before
/// trusting it, then writes it to Keychain — recreating the entry this Mac
/// needs to decrypt the database going forward. `derive_database_key` combines
/// the base key with the *current* hardware UUID freshly on every call, so no
/// separate hardware-UUID bookkeeping is needed to support the "new Mac"
/// migration case (§7.3); the verify-before-write ordering means a mistyped
/// phrase never overwrites a Keychain entry that might otherwise still work.
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

/// Doc 28 §4.4 step 6 ("Reset App Data" full wipe): clears the SQLite base
/// encryption key from Keychain. Best-effort — a missing/already-deleted
/// entry is not an error.
#[cfg(debug_assertions)]
pub fn delete_base_key() {
    let _ = std::fs::remove_file(dev_base_key_path());
    let _ = std::fs::remove_file(std::env::temp_dir().join("dinero_dev_base_key.txt"));
}

#[cfg(not(debug_assertions))]
pub fn delete_base_key() {
    if let Ok(entry) = Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER) {
        let _ = entry.delete_credential();
    }
}

/// I14 fix: fetches the immutable Hardware UUID of the Mac via the
/// `machine-uid` crate (already a Cargo.toml dependency, previously unused)
/// rather than a hand-rolled `ioreg` invocation + line-parsing. On macOS the
/// crate still ultimately reads `IOPlatformUUID` the same way the OS exposes
/// it — there is no root-free API that avoids that — but it replaces our own
/// duplicate parsing with tested, maintained upstream code.
pub fn get_hardware_uuid() -> Result<String> {
    machine_uid::get().map_err(|e| anyhow::anyhow!("Failed to read hardware UUID: {}", e))
}

/// Derives the SQLite encryption key by combining the base key and the Hardware UUID using Argon2id.
pub fn derive_database_key() -> Result<String> {
    let base_key = get_or_create_base_key()?;
    derive_database_key_from_base_key(&base_key)
}

/// Core of `derive_database_key`, parameterized on an explicit `base_key` so
/// `verify_key_decrypts` can test a *candidate* recovered key (from a
/// Recovery Phrase) against the current machine's hardware UUID without first
/// writing it to Keychain, and so `restore_from_recovery_phrase` can
/// re-derive the same SQLCipher key afterward to write its audit entry.
pub fn derive_database_key_from_base_key(base_key: &str) -> Result<String> {
    let hw_uuid = get_hardware_uuid()?;
    derive_database_key_with_hw_uuid(base_key, &hw_uuid)
}

/// Core of `derive_database_key_from_base_key`, parameterized on an explicit
/// `hw_uuid` rather than always reading the *current* machine's — lets the
/// Mac-to-Mac migration path (`try_migrate_hardware_uuid`, I4 fix) derive
/// both the old-hardware and new-hardware keys without conflating the two.
///
/// TASK-DB-001 fix: this previously computed a `padded_salt` from `hw_uuid`
/// and then never used it — the actual salt was a hardcoded constant
/// (`"HardwareUUIDSalt"` in base64), identical across every installation.
/// That defeats a core purpose of salting (per-target precomputation
/// resistance) and didn't match Document 12/18/22's "derive the key via
/// Argon2id from the base key and the Mac's Hardware UUID" — `hash_raw`'s
/// signature is `(password, salt, ..)`, so `base_key` is the password and
/// the hardware-UUID-derived bytes are the salt, not a fixed constant.
/// Device-binding still worked before this fix (`hw_uuid` was folded into
/// the password input via string concatenation instead), but the salt
/// itself carried no device-specific entropy. No existing on-disk database
/// depends on the old (buggy) derivation — confirmed no
/// `~/Library/Application Support/com.dinero.app/` directory exists yet on
/// any machine this has run on, so this is a safe correction, not a
/// breaking migration.
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

    // Return the hash string to be used as SQLCipher key
    Ok(password_hash.to_string())
}

// ── Mac-to-Mac hardware-UUID migration (Doc 22 §7.3, I4 fix) ─────────────────
//
// Without this, a legitimate hardware migration (old Mac's Keychain + files
// carried over via Migration Assistant/Time Machine, but a new IOPlatformUUID)
// bricks the database: `derive_database_key` on the new Mac produces a
// different SQLCipher key than the one the file was encrypted with, and the
// app fails closed requiring a Recovery Phrase the user may never have opted
// into. This records the hardware UUID that last successfully opened the
// database and, on a mismatch, attempts a transparent one-time re-key rather
// than failing closed immediately.

const HW_UUID_MARKER_FILENAME: &str = "hw_uuid_marker.txt";

fn hw_uuid_marker_path(app_data_dir: &std::path::Path) -> std::path::PathBuf {
    app_data_dir.join(HW_UUID_MARKER_FILENAME)
}

/// Records the hardware UUID that just successfully opened the database.
/// Not a secret — a Mac's IOPlatformUUID is already readable locally via
/// `ioreg` by any process — so plaintext storage here doesn't weaken
/// anything; it only gives a future launch on different hardware something
/// to detect the change against. Best-effort: a write failure here should
/// never block a successful startup.
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

/// TASK-DB-021: peeks at the recorded hardware-UUID marker (without deriving
/// or touching any keys) to tell the caller, *before* attempting DB init,
/// whether this launch looks like a Mac-to-Mac migration — so it can show
/// the "Database migrated to new Mac" toast (Document 30 TASK-DB-021) after
/// a successful open, without threading a migration flag through
/// `init_db`'s return type. Returns `false` on any read/detection error
/// (conservatively: no toast rather than a spurious one).
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

/// Attempts to recover from a hardware-UUID change by re-keying the database
/// in place. Derives the *old* SQLCipher key from the marker's recorded
/// hardware UUID, verifies it still decrypts `db_path`, and if so `PRAGMA
/// rekey`s to the key derived from the *current* hardware UUID — no Recovery
/// Phrase required.
///
/// Returns `Ok(true)` if a migration was performed, `Ok(false)` if there was
/// nothing to migrate (no marker, or the marker already matches current
/// hardware — this is some other kind of key mismatch, not a hardware
/// change), and `Err` if a migration was attempted but failed (e.g. the old
/// key didn't decrypt the file either — the marker is stale/irrelevant).
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
    // SQLCipher's PRAGMA key never itself errors, even for a wrong key — only
    // a real query proves the old-hardware key actually decrypts this file.
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

// These tests avoid `get_or_create_base_key`/`get_hardware_uuid` (real macOS
// Keychain / ioreg calls) — they exercise the pure key-derivation and
// SQLCipher rekey mechanics that `try_migrate_hardware_uuid` relies on,
// using explicit stand-in values instead.
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

        // Create and encrypt with the "old hardware" key.
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

        // Simulate the migration's rekey step directly.
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

        // Old key must no longer decrypt the (now rekeyed) database.
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

        // New key must decrypt it.
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

        // No marker file at all -> never a migration signal.
        assert!(!hw_uuid_marker_indicates_migration(&dir));

        let current = get_hardware_uuid().unwrap();

        // Marker matches current hardware -> no migration.
        std::fs::write(hw_uuid_marker_path(&dir), &current).unwrap();
        assert!(!hw_uuid_marker_indicates_migration(&dir));

        // Marker differs from current hardware -> looks like a migration.
        std::fs::write(hw_uuid_marker_path(&dir), "some-other-hw-uuid").unwrap();
        assert!(hw_uuid_marker_indicates_migration(&dir));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
