use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use anyhow::{Context, Result};
use aes_gcm::aead::rand_core::RngCore;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::db::crypto::derive_database_key;

const STATEMENTS_DIR: &str = "statements";

/// Derives a 32-byte AES key from the Argon2id SQLCipher database key.
/// By hashing the Argon2 output with SHA-256, we get exactly 32 bytes for AES-256-GCM.
fn derive_storage_key() -> Result<[u8; 32]> {
    let db_key = derive_database_key().context("Failed to derive database key for PDF storage")?;
    let mut hasher = Sha256::new();
    hasher.update(db_key.as_bytes());
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    Ok(key)
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
