use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use chrono::Local;
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

/// Doc 28 §4.2 (J5 fix): crash reports are retained at most 7 days.
const CRASH_REPORT_RETENTION_DAYS: u64 = 7;
/// Domain-separation label so this key never collides with the PDF-password
/// key or the SQLCipher database key, despite all three deriving from the
/// same Keychain-stored base key (Doc 22 §6.3 pattern).
const CRASH_REPORT_KEY_LABEL: &[u8] = b"dinero-crash-report-key-v1";

fn crash_report_key() -> Option<Vec<u8>> {
    let base_key = crate::db::crypto::get_or_create_base_key().ok()?;
    let mut hasher = Sha256::new();
    hasher.update(base_key.as_bytes());
    hasher.update(CRASH_REPORT_KEY_LABEL);
    Some(hasher.finalize().to_vec())
}

/// J5 fix: encrypts a crash report at rest (AES-256-GCM, CSPRNG nonce) using
/// the same Keychain-derived key material as the rest of the app — crash
/// reports previously sat on disk as plaintext indefinitely. Falls back to
/// plaintext only if the Keychain is genuinely unreachable (e.g. mid-panic
/// during a Keychain-access-denial scenario), since a crash report is still
/// more useful in that rare case than none at all.
fn encrypt_report(plaintext: &str) -> Vec<u8> {
    if let Some(key_bytes) = crash_report_key() {
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
    tracing::warn!("Could not encrypt crash report — writing plaintext as a fallback");
    plaintext.as_bytes().to_vec()
}

/// J5 fix: decrypts a crash report written by `encrypt_report`, for the
/// diagnostic bundle to include. Returns `None` if the blob is too short to
/// contain a nonce, the key can't be derived, or decryption fails (e.g. a
/// pre-J5-fix plaintext file — callers should fall back to reading it as
/// plain text in that case).
pub fn decrypt_report(blob: &[u8]) -> Option<String> {
    const NONCE_LEN: usize = 12;
    if blob.len() <= NONCE_LEN {
        return None;
    }
    let key_bytes = crash_report_key()?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&blob[..NONCE_LEN]);
    let plaintext = cipher.decrypt(nonce, &blob[NONCE_LEN..]).ok()?;
    String::from_utf8(plaintext).ok()
}

/// Best-effort deletion of crash reports older than the retention window.
fn prune_old_reports(crash_dir: &std::path::Path) {
    let max_age = Duration::from_secs(CRASH_REPORT_RETENTION_DAYS * 24 * 60 * 60);
    let Ok(entries) = std::fs::read_dir(crash_dir) else {
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

/// Initializes the local crash reporter.
/// It intercepts Rust panics and writes an encrypted report to the local audit_log directory.
///
/// Note: this replaces any panic hook set before it (e.g. lib.rs's general
/// diagnostic hook) — it re-logs via `tracing::error!` itself so that
/// behavior isn't lost, in addition to writing the encrypted crash file.
pub fn init(app_data_dir: PathBuf) {
    let crash_dir = app_data_dir.join("audit_log").join("crash_reports");
    if let Err(e) = std::fs::create_dir_all(&crash_dir) {
        tracing::error!("Failed to create crash report directory: {}", e);
        return;
    }

    prune_old_reports(&crash_dir);

    std::panic::set_hook(Box::new(move |panic_info| {
        let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
        // J5 fix: `.enc` extension signals this file is not plain text.
        let report_path = crash_dir.join(format!("crash_{}.enc", timestamp));

        let payload = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            *s
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.as_str()
        } else {
            "Unknown panic payload"
        };

        let location = if let Some(loc) = panic_info.location() {
            format!("file '{}' at line {}", loc.file(), loc.line())
        } else {
            "unknown location".to_string()
        };

        tracing::error!("PANIC at {}: {}", location, payload);

        let report = format!(
            "Dinero App Crash Report\nTimestamp: {}\n\nPanic occurred at {}:\n{}\n",
            Local::now().to_rfc3339(),
            location,
            payload
        );
        let encrypted = encrypt_report(&report);

        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&report_path)
        {
            let _ = file.write_all(&encrypted);
        }

        tracing::error!(
            "Application panicked! Wrote encrypted crash report to {}",
            report_path.display()
        );
    }));
}
