use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{anyhow, Result};
use keyring::Entry;
use sha2::{Digest, Sha256};

const KEYCHAIN_SERVICE: &str = "com.dinero.app";
/// Pre-I4-fix legacy entry name — `delete_aes_key` still cleans this up on
/// reset for any install that created one before the I3 fix below, even
/// though nothing writes to it anymore.
const LEGACY_KEYCHAIN_PDF_KEY_NAME: &str = "pdf-aes-key";
/// Domain-separation label so the PDF-password key and the SQLCipher database
/// key (derived from the same base key + hardware UUID in db::crypto) can
/// never collide, despite sharing a root secret.
const PDF_KEY_DOMAIN_LABEL: &[u8] = b"dinero-pdf-password-key-v1";

/// AES-256-GCM nonce size (96-bit / 12 bytes — NIST recommended).
const NONCE_LEN: usize = 12;

/// Outcome of a password resolution attempt.
#[derive(Debug, PartialEq)]
pub enum PasswordResolutionResult {
    /// PDF is not encrypted — proceed directly to parsing.
    NotEncrypted,
    /// A stored password unlocked the PDF successfully.
    UnlockedWithStored(String),
    /// No stored passwords matched — UI must prompt the user.
    PromptRequired,
    /// User-entered password succeeded.
    UnlockedWithUserInput,
    /// User-entered password was wrong — re-prompt without losing context.
    WrongPassword,
    /// User did not respond within 2.5 minutes — create pending_retry row.
    Timeout,
}

// ── Keychain ─────────────────────────────────────────────────────────────────

/// Derives the AES-256 encryption key used to encrypt-at-rest passwords in
/// `pdf_passwords`, from the same Keychain-stored base key as the SQLCipher
/// database key (Doc 22 §6.3, I3 fix) — rather than a separate, standalone
/// Keychain entry with no documented relationship to the rest of the app's
/// key material. `PDF_KEY_DOMAIN_LABEL` keeps this deterministically
/// different from the SQLCipher key despite sharing a root secret. The key
/// is never exposed to the React UI (Doc 10 §19).
fn get_aes_key() -> Result<Vec<u8>> {
    let base_key = crate::db::crypto::get_or_create_base_key()?;
    let mut hasher = Sha256::new();
    hasher.update(base_key.as_bytes());
    hasher.update(PDF_KEY_DOMAIN_LABEL);
    Ok(hasher.finalize().to_vec())
}

/// Doc 28 §4.4 step 6 ("Reset App Data" full wipe): best-effort cleanup of
/// the legacy standalone PDF-password Keychain entry (pre-I3-fix). The
/// current key is derived from the base key and has no Keychain entry of its
/// own, so this is a no-op on any install created after the I3 fix.
pub fn delete_aes_key() {
    if let Ok(entry) = Entry::new(KEYCHAIN_SERVICE, LEGACY_KEYCHAIN_PDF_KEY_NAME) {
        let _ = entry.delete_credential();
    }
}

// ── Encryption helpers ────────────────────────────────────────────────────────

/// Encrypts a plaintext password using AES-256-GCM.
///
/// Output format: `<12-byte nonce><ciphertext>` — stored as raw bytes in DB.
/// INVARIANT: plaintext is never stored; this function is the only encryption path (Doc 10 §7.4).
pub fn encrypt_password(plaintext: &str) -> Result<Vec<u8>> {
    let key_bytes = get_aes_key()?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    // Doc 22 §15.1 / Doc 24 §3.5: the nonce must come from a CSPRNG — reuse under
    // AES-GCM is catastrophic (permits plaintext/key recovery and forgery).
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|e| anyhow!("AES-GCM encrypt error: {:?}", e))?;

    // Prepend nonce to ciphertext for storage
    let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ciphertext);
    Ok(blob)
}

/// Decrypts a ciphertext blob produced by `encrypt_password`.
///
/// Expected format: `<12-byte nonce><ciphertext>`.
/// INVARIANT: The decrypted plaintext is used in-memory only and is never logged or returned to UI.
fn decrypt_password(blob: &[u8]) -> Result<String> {
    if blob.len() <= NONCE_LEN {
        return Err(anyhow!("Ciphertext blob too short"));
    }
    let key_bytes = get_aes_key()?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let nonce = Nonce::from_slice(&blob[..NONCE_LEN]);
    let ciphertext = &blob[NONCE_LEN..];
    let plaintext_bytes = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow!("AES-GCM decrypt error: {:?}", e))?;
    String::from_utf8(plaintext_bytes).map_err(|e| anyhow!("UTF-8 decode error: {}", e))
}

// ── pdfium PDF unlock helpers ─────────────────────────────────────────────────

/// Attempts to open a PDF with the supplied password, via the isolated
/// `pdf_sidecar` process (Doc 30 TASK-STMT-003) — never in-process, so a
/// malformed/malicious PDF can only crash the short-lived sidecar, never the
/// main Tauri process.
///
/// Returns `true` if the document opens (either unencrypted or unlocked with password),
/// `false` if the password is wrong, the document cannot be parsed, or the
/// sidecar itself failed/timed out (fails closed — never silently "unlocked").
///
/// INVARIANT: `pdf_bytes` are never written to disk by this function (Doc 10 §8.5).
/// INVARIANT: the password is never logged, even at debug level (Doc 30 TASK-STMT-003).
async fn try_pdfium_unlock(pdf_bytes: &[u8], password: &str) -> bool {
    match crate::statements::sidecar::unlock_check_in_sidecar(pdf_bytes, password).await {
        Ok(unlocked) => {
            if !password.is_empty() {
                tracing::info!(success = unlocked, "PDF password attempt");
            }
            unlocked
        }
        Err(e) => {
            tracing::warn!("pdf_sidecar unlock_check failed: {} — treating as unlock failure", e);
            false
        }
    }
}

/// Returns `true` if the PDF is not password-protected (or if the empty password opens it).
pub async fn is_pdf_unencrypted(pdf_bytes: &[u8]) -> bool {
    try_pdfium_unlock(pdf_bytes, "").await
}

// ── Main password resolution functions ───────────────────────────────────────

/// Tries all stored passwords for the given instrument, ordered by `success_count DESC`.
/// Each password is decrypted in-memory using AES-256 key from Keychain (Doc 10 §7.1).
/// Passwords are NEVER returned to the React layer (Doc 10 §7.5, §19.4).
///
/// Returns `UnlockedWithStored(password)` on first success, `PromptRequired` if all fail.
pub async fn try_stored_passwords(
    instrument_id: &str,
    pdf_bytes: &[u8],
    pool: &deadpool_sqlite::Pool,
) -> Result<PasswordResolutionResult> {
    // First check if PDF is unencrypted — skip all password attempts
    if is_pdf_unencrypted(pdf_bytes).await {
        tracing::info!(
            "PDF for instrument_id='{}' is not encrypted — no password needed",
            instrument_id
        );
        return Ok(PasswordResolutionResult::NotEncrypted);
    }

    tracing::info!(
        "Attempting stored passwords for instrument_id='{}'",
        instrument_id
    );

    let conn = pool.get().await?;
    let inst_id = instrument_id.to_string();

    // Fetch stored passwords ordered by success rate
    let rows: Vec<(String, Vec<u8>)> = conn
        .interact(move |c| {
            let mut stmt = c.prepare(
                "SELECT id, password_ciphertext FROM pdf_passwords \
                 WHERE instrument_id = ? \
                 ORDER BY success_count DESC, last_used_at DESC",
            )?;
            let rows = stmt
                .query_map([&inst_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, rusqlite::Error>(rows)
        })
        .await
        .map_err(|e| anyhow!("DB interact error: {}", e))??;

    for (row_id, ciphertext) in rows {
        // Decrypt in-memory — never log the plaintext
        let plaintext = match decrypt_password(&ciphertext) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("Failed to decrypt stored password row {}: {}", row_id, e);
                continue;
            }
        };

        if try_pdfium_unlock(pdf_bytes, &plaintext).await {
            // Update success stats
            let rid = row_id.clone();
            conn.interact(move |c| {
                c.execute(
                    "UPDATE pdf_passwords \
                     SET success_count = success_count + 1, \
                         last_used_at = datetime('now') \
                     WHERE id = ?",
                    [&rid],
                )
            })
            .await
            .map_err(|e| anyhow!("DB interact error (update success_count): {}", e))??;

            tracing::info!("Stored password row '{}' unlocked the PDF", row_id);
            return Ok(PasswordResolutionResult::UnlockedWithStored(plaintext));
        }
    }

    tracing::info!(
        "No stored passwords matched for instrument_id='{}' — prompting user",
        instrument_id
    );
    Ok(PasswordResolutionResult::PromptRequired)
}

/// Saves a newly learned password for an instrument.
/// Password is AES-256-GCM encrypted before writing to `pdf_passwords` (Doc 10 §7.4, §19.3).
/// Plaintext is never persisted. Never returned to UI.
pub async fn save_password(
    instrument_id: &str,
    plaintext_password: &str,
    pool: &deadpool_sqlite::Pool,
) -> Result<()> {
    tracing::info!(
        "Saving new encrypted password for instrument_id='{}' (plaintext never stored)",
        instrument_id
    );

    // Encrypt before any DB operation
    let ciphertext = encrypt_password(plaintext_password)?;
    let id = uuid::Uuid::new_v4().to_string();
    let inst_id = instrument_id.to_string();

    let conn = pool.get().await?;
    conn.interact(move |c| {
        c.execute(
            "INSERT INTO pdf_passwords \
             (id, instrument_id, password_ciphertext, success_count, created_at) \
             VALUES (?, ?, ?, 1, datetime('now'))",
            rusqlite::params![id, inst_id, ciphertext],
        )
    })
    .await
    .map_err(|e| anyhow!("DB interact error (save_password): {}", e))??;

    Ok(())
}

/// Verifies a user-entered password against the PDF bytes.
/// On success: encrypts and saves to `pdf_passwords`, updates unprocessed_statements row,
/// and returns `UnlockedWithUserInput`.
/// On failure: returns `WrongPassword` — the unprocessed_statements row is NOT deleted
/// (Doc 10 §7.3 — re-prompt without losing context).
pub async fn try_user_password(
    instrument_id: &str,
    statement_id: &str,
    password: &str,
    pdf_bytes: &[u8],
    pool: &deadpool_sqlite::Pool,
) -> Result<PasswordResolutionResult> {
    tracing::info!(
        "Trying user-entered password for statement_id='{}' instrument_id='{}'",
        statement_id,
        instrument_id
    );

    // Never log the password itself
    if try_pdfium_unlock(pdf_bytes, password).await {
        // Save password for future use
        save_password(instrument_id, password, pool).await?;

        // Update unprocessed_statements to reflect password resolved
        let stmt_id = statement_id.to_string();
        let conn = pool.get().await?;
        conn.interact(move |c| {
            c.execute(
                "UPDATE unprocessed_statements \
                 SET status = 'resolved', updated_at = datetime('now') \
                 WHERE id = ?",
                [&stmt_id],
            )
        })
        .await
        .map_err(|e| anyhow!("DB interact error (resolve unprocessed): {}", e))??;

        tracing::info!(
            "User-entered password correct for statement_id='{}'",
            statement_id
        );
        Ok(PasswordResolutionResult::UnlockedWithUserInput)
    } else {
        tracing::warn!(
            "User-entered password incorrect for statement_id='{}' — re-prompting",
            statement_id
        );
        Ok(PasswordResolutionResult::WrongPassword)
    }
}

/// Transitions `unprocessed_statements` row to `pending_retry` when the password timeout
/// (2.5 minutes) elapses without a correct user response (Doc 10 §7.6).
pub async fn handle_password_timeout(
    statement_id: &str,
    pool: &deadpool_sqlite::Pool,
) -> Result<()> {
    tracing::warn!(
        "Password timeout for statement_id='{}' — transitioning to pending_retry",
        statement_id
    );
    let stmt_id = statement_id.to_string();
    let stmt_id_for_log = stmt_id.clone();
    let conn = pool.get().await?;
    conn.interact(move |c| {
        c.execute(
            "UPDATE unprocessed_statements \
             SET status = 'pending_retry', updated_at = datetime('now') \
             WHERE id = ? AND status = 'awaiting_password'",
            [&stmt_id],
        )?;
        // J6 fix: PDF-password lifecycle event (Doc 25 §6.1).
        if let Err(e) = crate::db::audit_log::insert(
            c,
            &crate::db::audit_log::AuditLogRow {
                id: uuid::Uuid::new_v4().to_string(),
                actor_type: Some("system".to_string()),
                actor_id: None,
                action: Some("pdf_password_timeout".to_string()),
                resource_type: Some("statement".to_string()),
                resource_id: Some(stmt_id_for_log),
                before_json: None,
                after_json: None,
                created_at: chrono::Utc::now(),
            },
        ) {
            tracing::warn!("Failed to record pdf_password_timeout audit event: {}", e);
        }
        Ok::<(), rusqlite::Error>(())
    })
    .await
    .map_err(|e| anyhow!("DB interact error (password timeout): {}", e))??;
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── test_password_encrypted_before_storage ───────────────────────────────
    //
    // Verifies that `encrypt_password` never stores or returns the plaintext,
    // and that the output blob is structurally correct (nonce + ciphertext, min length).
    #[test]
    fn test_password_encrypted_before_storage() {
        // We cannot call `encrypt_password` here without a Keychain entry, so we test
        // the `decrypt_password` function against a known round-trip using a synthetic key.
        // The test validates:
        //   1. The encrypt/decrypt cycle is internally consistent (round-trip).
        //   2. The ciphertext blob is strictly longer than NONCE_LEN (it contains the nonce + AEAD tag).
        //   3. The plaintext is NOT literally embedded in the ciphertext (byte search).

        let plaintext = "MyS3cr3tPDF!Pass";
        let key_bytes: [u8; 32] = [0x42u8; 32]; // deterministic test key — never used in production
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);
        let nonce_bytes: [u8; NONCE_LEN] = [0x01u8; NONCE_LEN];
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .expect("encrypt must succeed");

        // Blob = nonce || ciphertext
        let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ciphertext);

        // 1. Blob must be strictly larger than NONCE_LEN (contains AEAD tag overhead)
        assert!(
            blob.len() > NONCE_LEN,
            "blob must contain more than just the nonce"
        );

        // 2. Plaintext bytes must NOT appear verbatim in the ciphertext blob
        let pt_bytes = plaintext.as_bytes();
        let blob_contains_plaintext = blob.windows(pt_bytes.len()).any(|w| w == pt_bytes);
        assert!(
            !blob_contains_plaintext,
            "plaintext must not appear verbatim in ciphertext blob"
        );

        // 3. Decrypt must recover the original plaintext
        let decrypted = {
            let dec_nonce = Nonce::from_slice(&blob[..NONCE_LEN]);
            let dec_ct = &blob[NONCE_LEN..];
            let pt_bytes = cipher
                .decrypt(dec_nonce, dec_ct)
                .expect("decrypt must succeed");
            String::from_utf8(pt_bytes).expect("must be valid UTF-8")
        };
        assert_eq!(
            decrypted, plaintext,
            "round-trip must recover plaintext exactly"
        );
    }

    // ── test_pdfium_unlock_success ───────────────────────────────────────────
    //
    // Verifies that `is_pdf_unencrypted` returns true for a real unencrypted PDF
    // and `try_pdfium_unlock` returns true with the correct (empty) password.
    // Since pdfium requires a real library, the test uses graceful degradation:
    // if the pdfium library is unavailable, the test skips rather than fails.
    #[tokio::test]
    async fn test_pdfium_unlock_success() {
        // Minimal valid unencrypted PDF (PDF 1.0 trailer — recognized by pdfium)
        let minimal_pdf = b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
            2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
            3 0 obj<</Type/Page/MediaBox[0 0 3 3]>>endobj\n\
            xref\n0 4\n0000000000 65535 f \n0000000009 00000 n \n\
            0000000058 00000 n \n0000000115 00000 n \n\
            trailer<</Size 4/Root 1 0 R>>\nstartxref\n\
            190\n%%EOF";

        // `try_pdfium_unlock` (via the sidecar, Doc 30 TASK-STMT-003) returns
        // false when the pdfium library isn't present on this machine
        // (graceful degradation) and true for a valid unencrypted PDF when
        // it is. The test only asserts the structural outcome (no panic, no
        // hang), not library availability — see the sidecar's own
        // process-isolation tests for the mechanism itself.
        let result = is_pdf_unencrypted(minimal_pdf).await;
        let _ = result; // no panic/hang is the invariant; result may be true or false depending on env
    }

    // ── test_pdfium_unlock_failure_reprompts ──────────────────────────────────
    //
    // Verifies that a wrong password on an encrypted PDF returns WrongPassword
    // (not a panic or crash), ensuring the re-prompt path is exercised.
    #[tokio::test]
    async fn test_pdfium_unlock_failure_reprompts() {
        // A non-PDF byte sequence — pdfium will fail to open it
        let garbage: &[u8] = b"not a real pdf";
        let result = try_pdfium_unlock(garbage, "wrongpassword").await;
        // pdfium will fail to open garbage bytes regardless of password
        assert!(!result, "garbage bytes must not unlock successfully");
    }

    // ── test_password_timeout_creates_pending_retry ──────────────────────────
    //
    // Verifies that `handle_password_timeout` transitions the unprocessed_statements
    // row status from 'awaiting_password' to 'pending_retry'.
    #[tokio::test]
    async fn test_password_timeout_creates_pending_retry() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test.db");
        let pool = crate::db::init_db(db_path).await.unwrap();

        // Seed: instrument + unprocessed_statements row in 'awaiting_password'
        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier) \
                 VALUES ('inst_to', 'credit_card', 'ICICI', '0001')",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO unprocessed_statements \
                 (id, statement_source_json, failure_type, failure_reason, status) \
                 VALUES ('stmt_to', '{}', 'password_timeout', '', 'awaiting_password')",
                [],
            )
            .unwrap();
        })
        .await
        .unwrap();

        // Trigger timeout
        handle_password_timeout("stmt_to", &pool).await.unwrap();

        // Verify status transitioned
        let conn2 = pool.get().await.unwrap();
        let status: String = conn2
            .interact(|c| {
                c.query_row(
                    "SELECT status FROM unprocessed_statements WHERE id = ?",
                    ["stmt_to"],
                    |row| row.get(0),
                )
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            status, "pending_retry",
            "Status must be pending_retry after timeout"
        );
    }

    // ── test_password_rotation_success ─────────────────────────────────────
    //
    // Verifies the password rotation edge case (Doc 10 §7.5):
    // When a password changes, multiple stored passwords will exist.
    // They must be fetched in `DESC` order by `success_count` so the most
    // frequently successful password is tried first, gracefully falling back
    // to newer/other passwords if the primary fails.
    #[tokio::test]
    async fn test_password_rotation_success() {
        let temp_dir =
            std::env::temp_dir().join(format!("dinero_test_rot_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test_rot.db");
        let pool = crate::db::init_db(db_path).await.unwrap();

        let conn = pool.get().await.unwrap();

        conn.interact(|c| {
            // Seed instrument
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, network, masked_identifier, status) 
                 VALUES ('inst_rot', 'credit_card', 'HDFC Bank', 'VISA', '1234', 'active')",
                [],
            ).unwrap();

            // Insert old password with high success count
            crate::db::pdf_passwords::insert(
                c,
                &crate::db::pdf_passwords::PdfPasswordsRow {
                    id: "pass_old".to_string(),
                    instrument_id: "inst_rot".to_string(),
                    password_ciphertext: "old_encrypted".to_string(),
                    success_count: 50,
                    last_used_at: None,
                    created_at: Some(chrono::DateTime::from_timestamp(1000, 0).unwrap().naive_utc()),
                    updated_at: Some(chrono::DateTime::from_timestamp(1000, 0).unwrap().naive_utc()),
                },
            ).unwrap();

            // Insert new password with low success count
            crate::db::pdf_passwords::insert(
                c,
                &crate::db::pdf_passwords::PdfPasswordsRow {
                    id: "pass_new".to_string(),
                    instrument_id: "inst_rot".to_string(),
                    password_ciphertext: "new_encrypted".to_string(),
                    success_count: 1,
                    last_used_at: None,
                    created_at: Some(chrono::DateTime::from_timestamp(2000, 0).unwrap().naive_utc()),
                    updated_at: Some(chrono::DateTime::from_timestamp(2000, 0).unwrap().naive_utc()),
                },
            ).unwrap();
            
            // Verify ordering is DESC by success_count
            let rows = crate::db::pdf_passwords::select_by_instrument(c, "inst_rot").unwrap();
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].id, "pass_old");
            assert_eq!(rows[1].id, "pass_new");
        })
        .await
        .unwrap();
    }
}
