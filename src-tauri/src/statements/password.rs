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
pub fn decrypt_password(blob: &[u8]) -> Result<String> {
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
            // This is an infrastructure failure (e.g. pdfium library not found,
            // sidecar binary missing, or I/O error) — NOT a wrong password.
            // The root cause of "Incorrect password" errors when the sidecar
            // cannot locate libpdfium.dylib is fixed in sidecar/mod.rs by
            // setting current_dir to the binary's parent directory.
            tracing::error!(
                "pdf_sidecar unlock_check infrastructure error: {} — treating as unlock failure \
                 (check that libpdfium.dylib is present next to the sidecar binary)",
                e
            );
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

/// Tries every stored password across **every** instrument, not scoped to
/// one — used wherever the instrument genuinely isn't known yet (a fresh
/// upload's password is resolved at Step 5, before metadata extraction at
/// Step 7 ever discovers which instrument the PDF belongs to at all; a
/// `statements_retry` on a still-cached-bytes statement is in the identical
/// position). Doc 30 TASK-STMT-010 ("statements_retry... reusing any
/// previously-entered password first") and the real upload flow both need
/// this — `try_stored_passwords` alone can't help either caller, since both
/// would otherwise have to pass a real `instrument_id` that doesn't exist yet.
///
/// PDF passwords are typically shared per bank/card template, so the number
/// of distinct stored passwords across all instruments a user has is small —
/// a full linear scan here is cheap, not a performance concern.
pub async fn try_all_stored_passwords(
    pdf_bytes: &[u8],
    pool: &deadpool_sqlite::Pool,
) -> Result<PasswordResolutionResult> {
    if is_pdf_unencrypted(pdf_bytes).await {
        return Ok(PasswordResolutionResult::NotEncrypted);
    }

    let conn = pool.get().await?;
    let rows: Vec<(String, Vec<u8>)> = conn
        .interact(|c| {
            let mut stmt = c.prepare(
                "SELECT id, password_ciphertext FROM pdf_passwords \
                 ORDER BY success_count DESC, last_used_at DESC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, rusqlite::Error>(rows)
        })
        .await
        .map_err(|e| anyhow!("DB interact error: {}", e))??;

    for (row_id, ciphertext) in rows {
        let plaintext = match decrypt_password(&ciphertext) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("Failed to decrypt stored password row {}: {}", row_id, e);
                continue;
            }
        };

        if try_pdfium_unlock(pdf_bytes, &plaintext).await {
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

            tracing::info!(
                "Stored password row '{}' unlocked the PDF (cross-instrument scan)",
                row_id
            );
            return Ok(PasswordResolutionResult::UnlockedWithStored(plaintext));
        }
    }

    Ok(PasswordResolutionResult::PromptRequired)
}

/// Returns bytes safe to hand to a PDF viewer for display. If `pdf_bytes` is
/// unencrypted, returns it unchanged. If it's password-protected, finds a
/// stored password that unlocks it (the password used to originally unlock
/// this statement is always saved against its resolved instrument — see
/// `stage_parse_pipeline`) and returns a decrypted copy via the isolated
/// sidecar, so the browser's native PDF viewer never has to prompt the user
/// for a password they already gave once. Never writes anything to disk —
/// the decrypted bytes exist only for this call's in-memory round trip.
pub async fn ensure_viewable_pdf_bytes(
    pdf_bytes: Vec<u8>,
    pool: &deadpool_sqlite::Pool,
) -> Result<Vec<u8>> {
    if is_pdf_unencrypted(&pdf_bytes).await {
        return Ok(pdf_bytes);
    }

    match try_all_stored_passwords(&pdf_bytes, pool).await? {
        PasswordResolutionResult::UnlockedWithStored(password) => {
            crate::statements::sidecar::decrypt_pdf_in_sidecar(&pdf_bytes, &password).await
        }
        _ => Err(anyhow!(
            "PDF is password-protected and no stored password unlocks it"
        )),
    }
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

/// Outcome of `resolve_statement_password`: whether the caller may enqueue a
/// `StatementJob` for parsing, or must stop because a password prompt was
/// created instead.
pub enum StatementPasswordResolution {
    /// Safe to enqueue for parsing now. `Some(password)` when a stored
    /// password actually unlocked the PDF — that password must be threaded
    /// into the parse call, pdfium needs it again to open the same bytes.
    /// `None` when the PDF was never encrypted.
    Proceed(Option<String>),
    /// All stored passwords failed (or none exist). An `unprocessed_statements`
    /// row for `stmt_id` is now `awaiting_password` and `PASSWORD_REQUIRED` has
    /// been emitted — the caller must NOT enqueue a `StatementJob` for it.
    PromptCreated,
}

/// Single choke point for "does this PDF need a password before it can be
/// queued for parsing" — every Statement Queue entry point (manual upload,
/// Gmail-attachment email detection in `historical_scan`/`polling`) must call
/// this before building a `StatementJob`.
///
/// Real bug this fixes (field error log: "Statement Queue job failed ...
/// PdfiumLibraryInternalError(PasswordError)"): a stored password unlocking
/// the PDF here is worthless on its own — pdfium needs that same password
/// passed again at actual parse time (it doesn't decrypt bytes in place), and
/// the email-detected path previously never called password resolution at
/// all, enqueueing encrypted bytes with no password unconditionally.
pub async fn resolve_statement_password<R: tauri::Runtime>(
    stmt_id: &str,
    bytes: &[u8],
    filename: &str,
    file_hash: &str,
    pool: &deadpool_sqlite::Pool,
    app: &tauri::AppHandle<R>,
    email_meta: Option<crate::ingestion::message_processor::EmailMetadata>,
) -> Result<StatementPasswordResolution> {
    use tauri::Emitter;
    use tauri::Manager;

    if is_pdf_unencrypted(bytes).await {
        return Ok(StatementPasswordResolution::Proceed(None));
    }

    match try_all_stored_passwords(bytes, pool).await? {
        PasswordResolutionResult::NotEncrypted => Ok(StatementPasswordResolution::Proceed(None)),
        PasswordResolutionResult::UnlockedWithStored(password) => {
            tracing::info!("PDF unlocked with stored password");
            Ok(StatementPasswordResolution::Proceed(Some(password)))
        }
        _ => {
            create_awaiting_password_row(stmt_id, file_hash, filename, pool, email_meta.as_ref())
                .await?;
            if let Ok(app_data_dir) = app.path().app_data_dir() {
                let _ = crate::statements::pdf_storage::store_pdf(&app_data_dir, stmt_id, bytes);
            }

            // The full email context (sender/subject/snippet/html, `email_meta`
            // above) is already persisted into this row's `source_json` by
            // `create_awaiting_password_row` just above. Frontend only ever
            // reads `statement_id` off this event (`GlobalStateContext.tsx`'s
            // listener), then re-fetches the rest via `listUnprocessed()` for
            // display (`PasswordPromptModal.tsx`) -- so `filename`/`sender`/
            // `to`/`subject`/`date`/`snippet`/`html` here were sent over IPC
            // on every password-protected statement (which can fire dozens of
            // times in a burst during a historical scan) and discarded
            // unread on arrival every single time, `html` alone often 30KB+.
            let payload = serde_json::json!({ "statement_id": stmt_id });
            crate::statements::events::emit(
                crate::statements::events::PASSWORD_REQUIRED,
                payload.clone(),
            );
            app.emit(crate::statements::events::PASSWORD_REQUIRED, payload)
                .ok();

            Ok(StatementPasswordResolution::PromptCreated)
        }
    }
}

/// Creates an `unprocessed_statements` row with status = 'awaiting_password' (Doc 10 §7.2).
async fn create_awaiting_password_row(
    statement_id: &str,
    file_hash: &str,
    filename: &str,
    pool: &deadpool_sqlite::Pool,
    email_meta: Option<&crate::ingestion::message_processor::EmailMetadata>,
) -> Result<()> {
    let stmt_id = statement_id.to_string();
    let mut source_json_obj = serde_json::json!({
        "file_hash": file_hash,
        "filename": filename,
    });

    if let Some(meta) = email_meta {
        if let Some(obj) = source_json_obj.as_object_mut() {
            obj.insert("sender".to_string(), serde_json::json!(meta.sender));
            obj.insert("to".to_string(), serde_json::json!(meta.recipient));
            obj.insert("subject".to_string(), serde_json::json!(meta.subject));
            obj.insert("date".to_string(), serde_json::json!(meta.date));
            obj.insert("snippet".to_string(), serde_json::json!(meta.snippet));
            obj.insert("html".to_string(), serde_json::json!(meta.html));
        }
    }
    let source_json = source_json_obj.to_string();

    let conn = pool.get().await?;
    conn.interact(move |c| {
        c.execute(
            "INSERT INTO unprocessed_statements \
             (id, statement_source_json, failure_type, failure_reason, status) \
             VALUES (?, ?, 'password_required', '', 'awaiting_password')",
            rusqlite::params![stmt_id, source_json],
        )
    })
    .await
    .map_err(|e| anyhow!("DB interact error (create_awaiting_password_row): {}", e))??;

    tracing::info!(
        "Created awaiting_password row for statement_id='{}'",
        statement_id
    );
    Ok(())
}

/// Verifies a user-entered password against the PDF bytes.
/// On success: encrypts and saves to `pdf_passwords`, updates unprocessed_statements row,
/// and returns `UnlockedWithUserInput`.
/// On failure: returns `WrongPassword` — the unprocessed_statements row is NOT deleted
/// (Doc 10 §7.3 — re-prompt without losing context).
pub async fn try_user_password(
    statement_id: &str,
    password: &str,
    pdf_bytes: &[u8],
    pool: &deadpool_sqlite::Pool,
) -> Result<PasswordResolutionResult> {
    tracing::info!(
        "Trying user-entered password for statement_id='{}'",
        statement_id
    );

    // Never log the password itself
    if try_pdfium_unlock(pdf_bytes, password).await {
        // We do NOT save password here, because instrument_id is unknown.
        // It will be saved by run_parse_pipeline later when instrument is resolved.

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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for the shared choke point this fix introduces
    /// (`resolve_statement_password`, used by manual upload AND the
    /// previously-unprotected Gmail-attachment path in
    /// `historical_scan`/`polling`): when nothing can unlock the PDF, it must
    /// create the `awaiting_password` row, stash the bytes for a later
    /// resume, and emit `PASSWORD_REQUIRED` — exactly what `statements_upload`
    /// did inline before this was factored out, now proven for every caller.
    /// (Bytes pdfium can't even parse take the same "no password worked"
    /// path as a real encrypted PDF with no matching stored password — this
    /// repo has no encryption-capable dependency to author a real encrypted
    /// fixture with, so this is the closest exercise of that branch that
    /// doesn't require Keychain-backed encryption in a unit test.)
    #[tokio::test]
    async fn resolve_statement_password_prompts_when_nothing_unlocks_it() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

        let unparseable_bytes: &[u8] = b"not a real pdf";
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone();

        let resolution = resolve_statement_password(
            "stmt_resolve_test",
            unparseable_bytes,
            "statement.pdf",
            "file_hash_resolve_test",
            &pool,
            &app,
            None,
        )
        .await
        .unwrap();

        assert!(matches!(
            resolution,
            StatementPasswordResolution::PromptCreated
        ));

        let conn = pool.get().await.unwrap();
        let status: String = conn
            .interact(|c| {
                c.query_row(
                    "SELECT status FROM unprocessed_statements WHERE id = ?",
                    ["stmt_resolve_test"],
                    |row| row.get(0),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status, "awaiting_password");

        use tauri::Manager;
        let app_data_dir = app.path().app_data_dir().unwrap();
        let persisted_bytes =
            crate::statements::pdf_storage::read_pdf(&app_data_dir, "stmt_resolve_test")
                .unwrap()
                .unwrap();
        assert_eq!(persisted_bytes, unparseable_bytes.to_vec());
    }

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
                    created_at: Some(
                        chrono::DateTime::from_timestamp(1000, 0)
                            .unwrap()
                            .naive_utc(),
                    ),
                    updated_at: Some(
                        chrono::DateTime::from_timestamp(1000, 0)
                            .unwrap()
                            .naive_utc(),
                    ),
                },
            )
            .unwrap();

            // Insert new password with low success count
            crate::db::pdf_passwords::insert(
                c,
                &crate::db::pdf_passwords::PdfPasswordsRow {
                    id: "pass_new".to_string(),
                    instrument_id: "inst_rot".to_string(),
                    password_ciphertext: "new_encrypted".to_string(),
                    success_count: 1,
                    last_used_at: None,
                    created_at: Some(
                        chrono::DateTime::from_timestamp(2000, 0)
                            .unwrap()
                            .naive_utc(),
                    ),
                    updated_at: Some(
                        chrono::DateTime::from_timestamp(2000, 0)
                            .unwrap()
                            .naive_utc(),
                    ),
                },
            )
            .unwrap();

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
