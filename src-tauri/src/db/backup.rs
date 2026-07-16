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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(result.is_err(), "a wrong password must fail cleanly, not silently produce garbage plaintext");
    }

    #[test]
    fn test_encrypt_decrypt_backup_roundtrip() {
        let original = b"pretend this is a real SQLite file's bytes";
        let blob = encrypt_backup(original, "correct-horse-battery-staple").unwrap();

        let decrypted = decrypt_backup(&blob, "correct-horse-battery-staple").unwrap();
        assert_eq!(decrypted, original);
    }
}
