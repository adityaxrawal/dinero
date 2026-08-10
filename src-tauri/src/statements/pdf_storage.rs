//! Short-lived storage for statement PDFs.
//!
//! Retained only long enough to support retry and review, then purged by the
//! cleanup sweep on the daily loop. Raw statements are the most sensitive
//! artefacts the app touches, so retention is deliberately brief rather than
//! indefinite.
use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::db::crypto::derive_database_key;

const STATEMENTS_DIR: &str = "statements";

static STORAGE_KEY: OnceLock<[u8; 32]> = OnceLock::new();

/// Derives the key used to encrypt stored statement PDFs.
///
/// Statements are the most sensitive documents this app holds, so they are
/// encrypted at rest rather than left as plain files on disk.
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
    Ok(*STORAGE_KEY.get_or_init(|| key))
}

/// Path of a stored statement, keyed by its id.
fn statement_path(app_data_dir: &Path, statement_id: &str) -> PathBuf {
    app_data_dir
        .join(STATEMENTS_DIR)
        .join(format!("{}.pdf.enc", statement_id))
}

/// Stores a statement PDF, encrypted.
pub fn store_pdf(app_data_dir: &Path, statement_id: &str, bytes: &[u8]) -> Result<()> {
    let statements_dir = app_data_dir.join(STATEMENTS_DIR);
    if !statements_dir.exists() {
        std::fs::create_dir_all(&statements_dir)
            .context("Failed to create statements directory")?;
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

/// Reads and decrypts a stored statement, if it still exists.
pub fn read_pdf(app_data_dir: &Path, statement_id: &str) -> Result<Option<Vec<u8>>> {
    let path = statement_path(app_data_dir, statement_id);
    if !path.exists() {
        return Ok(None);
    }

    let data = std::fs::read(&path).context("Failed to read encrypted PDF from disk")?;
    if data.len() < 12 {
        return Err(anyhow::anyhow!(
            "Encrypted PDF file is too short to contain a nonce"
        ));
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

/// Deletes a stored statement.
pub fn delete_pdf(app_data_dir: &Path, statement_id: &str) -> Result<()> {
    let path = statement_path(app_data_dir, statement_id);
    if path.exists() {
        std::fs::remove_file(&path).context("Failed to delete encrypted PDF")?;
    }
    Ok(())
}

/// Deletes statements past their retention window.
///
/// Runs on every launch. Retention is deliberately brief -- these are raw
/// financial documents, kept only long enough to support retry and review.
pub async fn cleanup_expired_pdfs(app_data_dir: &Path, pool: &deadpool_sqlite::Pool) -> Result<()> {
    let conn = pool
        .get()
        .await
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;

    let expired_ids: Vec<String> = conn
        .interact(|c| {
            let mut stmt = c.prepare(
                "SELECT id FROM unprocessed_statements WHERE pdf_retained_until < datetime('now')",
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
            tracing::warn!(
                "Failed to delete expired PDF for statement_id='{}': {}",
                id,
                e
            );
        }

        let id_clone = id.clone();
        let _ = conn
            .interact(move |c| {
                c.execute(
                    "UPDATE unprocessed_statements SET pdf_retained_until = NULL WHERE id = ?",
                    [&id_clone],
                )
            })
            .await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_key_is_stable_and_pdfs_round_trip() {
        let first = derive_storage_key().unwrap();
        let second = derive_storage_key().unwrap();
        assert_eq!(first, second, "storage key must be stable across calls");

        let dir = std::env::temp_dir().join(format!("dinero_pdf_storage_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let payload = b"%PDF-1.4 statement bytes";
        store_pdf(&dir, "stmt_round_trip", payload).unwrap();

        let on_disk = std::fs::read(dir.join("statements/stmt_round_trip.pdf.enc")).unwrap();
        assert!(
            on_disk.windows(payload.len()).all(|w| w != payload),
            "the statement PDF must not be readable on disk"
        );

        let recovered = read_pdf(&dir, "stmt_round_trip").unwrap().unwrap();
        assert_eq!(recovered, payload);

        delete_pdf(&dir, "stmt_round_trip").unwrap();
        assert!(read_pdf(&dir, "stmt_round_trip").unwrap().is_none());
        delete_pdf(&dir, "stmt_round_trip").unwrap();

        let _ = std::fs::remove_dir_all(&dir);
    }
}
