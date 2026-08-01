use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use anyhow::{Context, Result};
use aes_gcm::aead::rand_core::RngCore;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::db::crypto::derive_database_key;

const STATEMENTS_DIR: &str = "statements";

/// Process-lifetime cache for `derive_storage_key`.
///
/// The key is deterministic — `derive_database_key` combines the
/// Keychain-held base key (itself cached) with the machine's hardware UUID,
/// neither of which changes while the process is running — so caching cannot
/// return a different key than a fresh derivation would.
///
/// Worth caching because the derivation runs Argon2id, which is deliberately
/// expensive (~tens of ms). Since audit_04 #1 the Statement Queue does a
/// `store_pdf` at intake plus a `read_pdf` in the worker for *every*
/// statement, so a 64-file batch would otherwise pay ~128 Argon2 derivations
/// for a value that never varies.
static STORAGE_KEY: OnceLock<[u8; 32]> = OnceLock::new();

/// Derives a 32-byte AES key from the Argon2id SQLCipher database key.
/// By hashing the Argon2 output with SHA-256, we get exactly 32 bytes for AES-256-GCM.
fn derive_storage_key() -> Result<[u8; 32]> {
    if let Some(key) = STORAGE_KEY.get() {
        return Ok(*key);
    }
    let db_key = derive_database_key().context("Failed to derive database key for PDF storage")?;
    let mut hasher = Sha256::new();
    hasher.update(db_key.as_bytes());
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    // A concurrent caller may have won the race; defer to whichever landed
    // first so every caller in this process uses the identical key.
    Ok(*STORAGE_KEY.get_or_init(|| key))
}

fn statement_path(app_data_dir: &Path, statement_id: &str) -> PathBuf {
    app_data_dir.join(STATEMENTS_DIR).join(format!("{}.pdf.enc", statement_id))
}

pub fn store_pdf(app_data_dir: &Path, statement_id: &str, bytes: &[u8]) -> Result<()> {
    let statements_dir = app_data_dir.join(STATEMENTS_DIR);
    if !statements_dir.exists() {
        std::fs::create_dir_all(&statements_dir).context("Failed to create statements directory")?;
    }

    let key = derive_storage_key()?;
    let cipher = Aes256Gcm::new(&key.into());

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, bytes)
        .map_err(|e| anyhow::anyhow!("Encryption failed: {:?}", e))?;

    let mut out = Vec::with_capacity(12 + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);

    let path = statement_path(app_data_dir, statement_id);
    std::fs::write(&path, out).context("Failed to write encrypted PDF to disk")?;
    
    Ok(())
}

pub fn read_pdf(app_data_dir: &Path, statement_id: &str) -> Result<Option<Vec<u8>>> {
    let path = statement_path(app_data_dir, statement_id);
    if !path.exists() {
        return Ok(None);
    }

    let data = std::fs::read(&path).context("Failed to read encrypted PDF from disk")?;
    if data.len() < 12 {
        return Err(anyhow::anyhow!("Encrypted PDF file is too short to contain a nonce"));
    }

    let key = derive_storage_key()?;
    let cipher = Aes256Gcm::new(&key.into());

    let nonce = Nonce::from_slice(&data[..12]);
    let ciphertext = &data[12..];

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("Decryption failed: {:?}", e))?;

    Ok(Some(plaintext))
}

pub fn delete_pdf(app_data_dir: &Path, statement_id: &str) -> Result<()> {
    let path = statement_path(app_data_dir, statement_id);
    if path.exists() {
        std::fs::remove_file(&path).context("Failed to delete encrypted PDF")?;
    }
    Ok(())
}

pub async fn cleanup_expired_pdfs(
    app_data_dir: &Path,
    pool: &deadpool_sqlite::Pool,
) -> Result<()> {
    let conn = pool.get().await.map_err(|e| anyhow::anyhow!("DB error: {}", e))?;
    
    let expired_ids: Vec<String> = conn.interact(|c| {
        let mut stmt = c.prepare(
            "SELECT id FROM unprocessed_statements WHERE pdf_retained_until < datetime('now')"
        )?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut ids = Vec::new();
        for id in rows {
            ids.push(id?);
        }
        Ok::<_, rusqlite::Error>(ids)
    })
    .await
    .map_err(|e| anyhow::anyhow!("DB interact error: {}", e))?
    .map_err(|e| anyhow::anyhow!("SQL error: {}", e))?;

    for id in &expired_ids {
        if let Err(e) = delete_pdf(app_data_dir, id) {
            tracing::warn!("Failed to delete expired PDF for statement_id='{}': {}", id, e);
        }
        
        // Also remove the retained_until flag so we don't keep trying to delete it
        let id_clone = id.clone();
        let _ = conn.interact(move |c| {
            c.execute(
                "UPDATE unprocessed_statements SET pdf_retained_until = NULL WHERE id = ?",
                [&id_clone],
            )
        }).await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// audit_04 #1 introduced `STORAGE_KEY`, a process-lifetime cache in front
    /// of the Argon2id derivation, because the Statement Queue now does a
    /// `store_pdf` at intake and a `read_pdf` in the worker for every
    /// statement. A cache that returned a different key than the derivation it
    /// replaces would make every previously-stored PDF undecryptable, so pin
    /// both halves: the key is stable across calls, and a payload encrypted
    /// under it still round-trips.
    #[test]
    fn storage_key_is_stable_and_pdfs_round_trip() {
        let first = derive_storage_key().unwrap();
        let second = derive_storage_key().unwrap();
        assert_eq!(first, second, "storage key must be stable across calls");

        let dir = std::env::temp_dir().join(format!("dinero_pdf_storage_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let payload = b"%PDF-1.4 statement bytes";
        store_pdf(&dir, "stmt_round_trip", payload).unwrap();

        // The stored file must not contain the plaintext -- these are bank
        // statements, and the whole reason this path isn't a plain temp file.
        let on_disk = std::fs::read(dir.join("statements/stmt_round_trip.pdf.enc")).unwrap();
        assert!(
            on_disk.windows(payload.len()).all(|w| w != payload),
            "the statement PDF must not be readable on disk"
        );

        let recovered = read_pdf(&dir, "stmt_round_trip").unwrap().unwrap();
        assert_eq!(recovered, payload);

        delete_pdf(&dir, "stmt_round_trip").unwrap();
        assert!(read_pdf(&dir, "stmt_round_trip").unwrap().is_none());
        // Deleting an already-deleted PDF is not an error -- the Statement
        // Queue worker deletes unconditionally after parsing.
        delete_pdf(&dir, "stmt_round_trip").unwrap();

        let _ = std::fs::remove_dir_all(&dir);
    }
}
