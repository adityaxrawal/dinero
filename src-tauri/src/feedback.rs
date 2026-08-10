//! User-submitted feedback, optionally bundled with diagnostic logs.
//!
//! Submission is explicit and user-initiated -- nothing is sent in the
//! background. Attaching logs is a separate opt-in, since logs are far more
//! revealing than the feedback text itself.

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key};
use chrono::Local;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Duration;
use tauri::State;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

const FEEDBACK_RETENTION_DAYS: u64 = 7;
const FEEDBACK_KEY_LABEL: &[u8] = b"dinero-feedback-note-key-v1";

#[derive(Clone)]
pub struct FeedbackManager {
    app_data_dir: PathBuf,
}

/// Key protecting stored feedback notes.
fn feedback_key() -> Option<Vec<u8>> {
    let base_key = crate::db::crypto::get_or_create_base_key().ok()?;
    let mut hasher = Sha256::new();
    hasher.update(base_key.as_bytes());
    hasher.update(FEEDBACK_KEY_LABEL);
    Some(hasher.finalize().to_vec())
}

/// Encrypts a feedback note for storage.
///
/// Encrypted at rest because a user describing a problem routinely quotes the
/// financial data involved.
fn encrypt_note(plaintext: &str) -> Vec<u8> {
    if let Some(key_bytes) = feedback_key() {
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        if let Ok(ciphertext) = cipher.encrypt(&nonce, plaintext.as_bytes()) {
            let mut blob = Vec::with_capacity(12 + ciphertext.len());
            blob.extend_from_slice(&nonce);
            blob.extend_from_slice(&ciphertext);
            return blob;
        }
    }
    tracing::warn!("Could not encrypt feedback note — writing plaintext as a fallback");
    plaintext.as_bytes().to_vec()
}

/// Deletes feedback notes past the retention window.
fn prune_old_notes(feedback_dir: &std::path::Path) {
    let max_age = Duration::from_secs(FEEDBACK_RETENTION_DAYS * 24 * 60 * 60);
    let Ok(entries) = std::fs::read_dir(feedback_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let age = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|modified| modified.elapsed().ok());
        if age.map(|a| a > max_age).unwrap_or(false) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

impl FeedbackManager {
    /// Creates a manager rooted at the app data directory.
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self { app_data_dir }
    }

    /// Stores a feedback note, encrypted.
    pub async fn submit_feedback_note(&self, text: String) -> Result<String, String> {
        let feedback_dir = self.app_data_dir.join("audit_log").join("feedback");
        if let Err(e) = tokio::fs::create_dir_all(&feedback_dir).await {
            tracing::error!("Failed to create feedback directory: {}", e);
            return Err("Failed to create feedback directory".into());
        }

        prune_old_notes(&feedback_dir);

        let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
        let report_path = feedback_dir.join(format!("feedback_{}.enc", timestamp));

        let report = format!(
            "Dinero User Feedback\nTimestamp: {}\nInclude Logs: false\n\nFeedback:\n{}\n",
            Local::now().to_rfc3339(),
            text
        );
        let encrypted = encrypt_note(&report);

        match OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&report_path)
            .await
        {
            Ok(mut file) => {
                if let Err(e) = file.write_all(&encrypted).await {
                    tracing::error!("Failed to write feedback: {}", e);
                    return Err("Failed to write feedback".into());
                }
                tracing::info!(
                    "User feedback written (encrypted) to {}",
                    report_path.display()
                );
                Ok(report_path.display().to_string())
            }
            Err(e) => {
                tracing::error!("Failed to open feedback file: {}", e);
                Err("Failed to open feedback file".into())
            }
        }
    }

    /// The app data directory this manager writes into.
    pub fn app_data_dir(&self) -> &std::path::Path {
        &self.app_data_dir
    }
}

#[tauri::command]
/// Submits feedback, optionally attaching diagnostic logs.
///
/// Log attachment is a separate opt-in, since logs reveal far more than the
/// feedback text itself.
pub async fn submit_user_feedback(
    text: String,
    include_logs: bool,
    manager: State<'_, FeedbackManager>,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    if include_logs {
        let app_dir = manager.app_data_dir().to_path_buf();
        let conn = pool
            .get()
            .await
            .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
        conn.interact(move |c| {
            crate::diagnostics::generate_diagnostic_bundle(&app_dir, c, Some(&text))
        })
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Io(e.to_string()))
        .map(|p| p.display().to_string())
    } else {
        manager
            .submit_feedback_note(text)
            .await
            .map_err(crate::error::AppError::Io)
    }
}
