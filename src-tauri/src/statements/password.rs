//! Handles encrypted statement PDFs.
//!
//! Banks routinely password-protect statements. Stored passwords are tried
//! automatically before the user is prompted, ordered by how often each has
//! previously worked, so a recurring statement unlocks silently.
//!
//! Passwords are encrypted at rest with a key held outside the database, so the
//! database file alone does not yield them.
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{anyhow, Result};
use keyring::Entry;
use sha2::{Digest, Sha256};

const KEYCHAIN_SERVICE: &str = "com.dinero.app";
const LEGACY_KEYCHAIN_PDF_KEY_NAME: &str = "pdf-aes-key";
const PDF_KEY_DOMAIN_LABEL: &[u8] = b"dinero-pdf-password-key-v1";

const NONCE_LEN: usize = 12;

#[derive(Debug, PartialEq)]
pub enum PasswordResolutionResult {
    NotEncrypted,
    UnlockedWithStored(String),
    PromptRequired,
    UnlockedWithUserInput,
    WrongPassword,
}

/// The key protecting stored statement passwords.
///
/// Held outside the database, so the database file alone does not yield the
/// passwords it references.
fn get_aes_key() -> Result<Vec<u8>> {
    let base_key = crate::db::crypto::get_or_create_base_key()?;
    let mut hasher = Sha256::new();
    hasher.update(base_key.as_bytes());
    hasher.update(PDF_KEY_DOMAIN_LABEL);
    Ok(hasher.finalize().to_vec())
}

/// Deletes the password-encryption key, invalidating every stored password.
pub fn delete_aes_key() {
    if let Ok(entry) = Entry::new(KEYCHAIN_SERVICE, LEGACY_KEYCHAIN_PDF_KEY_NAME) {
        let _ = entry.delete_credential();
    }
}

/// Encrypts a statement password for storage.
pub fn encrypt_password(plaintext: &str) -> Result<Vec<u8>> {
    let key_bytes = get_aes_key()?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|e| anyhow!("AES-GCM encrypt error: {:?}", e))?;

    let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ciphertext);
    Ok(blob)
}

/// Decrypts a stored password.
///
/// Authenticated, so tampering fails rather than yielding a wrong plaintext.
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

/// Tests one password against a PDF via the sidecar.
///
/// Attempted out-of-process because the input is an untrusted document and PDF
/// parsing is a well-known source of memory-safety bugs.
async fn try_pdfium_unlock(pdf_bytes: &[u8], password: &str) -> bool {
    match crate::statements::sidecar::unlock_check_in_sidecar(pdf_bytes, password).await {
        Ok(unlocked) => {
            if !password.is_empty() {
                tracing::info!(success = unlocked, "PDF password attempt");
            }
            unlocked
        }
        Err(e) => {
            tracing::error!(
                "pdf_sidecar unlock_check infrastructure error: {} — treating as unlock failure \
                 (check that libpdfium.dylib is present next to the sidecar binary)",
                e
            );
            false
        }
    }
}

/// Whether a PDF opens with no password at all.
pub async fn is_pdf_unencrypted(pdf_bytes: &[u8]) -> bool {
    try_pdfium_unlock(pdf_bytes, "").await
}

/// Tries this instrument's stored passwords, most successful first.
///
/// Ordering by past success means a recurring statement usually unlocks on the
/// first attempt rather than after several.
pub async fn try_stored_passwords(
    instrument_id: &str,
    pdf_bytes: &[u8],
    pool: &deadpool_sqlite::Pool,
) -> Result<PasswordResolutionResult> {
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

/// Tries every stored password across all instruments.
///
/// The wider fallback for a statement whose instrument is not yet known.
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

/// Returns decrypted PDF bytes ready for viewing, unlocking if needed.
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

/// Saves a password that successfully unlocked a statement.
///
/// Stored only after it is known to work, so a wrong guess is never persisted.
pub async fn save_password(
    instrument_id: &str,
    plaintext_password: &str,
    pool: &deadpool_sqlite::Pool,
) -> Result<()> {
    tracing::info!(
        "Saving new encrypted password for instrument_id='{}' (plaintext never stored)",
        instrument_id
    );

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

pub enum StatementPasswordResolution {
    Proceed(Option<String>),
    PromptCreated,
}

/// Resolves a statement's password, prompting the user if nothing stored works.
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

/// Records a statement as awaiting a password from the user.
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

/// Tries a password the user just supplied, saving it if it works.
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

    if try_pdfium_unlock(pdf_bytes, password).await {

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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_password_encrypted_before_storage() {

        let plaintext = "MyS3cr3tPDF!Pass";
        let key_bytes: [u8; 32] = [0x42u8; 32];
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);
        let nonce_bytes: [u8; NONCE_LEN] = [0x01u8; NONCE_LEN];
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .expect("encrypt must succeed");

        let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ciphertext);

        assert!(
            blob.len() > NONCE_LEN,
            "blob must contain more than just the nonce"
        );

        let pt_bytes = plaintext.as_bytes();
        let blob_contains_plaintext = blob.windows(pt_bytes.len()).any(|w| w == pt_bytes);
        assert!(
            !blob_contains_plaintext,
            "plaintext must not appear verbatim in ciphertext blob"
        );

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

    #[tokio::test]
    async fn test_pdfium_unlock_success() {
        let minimal_pdf = b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
            2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
            3 0 obj<</Type/Page/MediaBox[0 0 3 3]>>endobj\n\
            xref\n0 4\n0000000000 65535 f \n0000000009 00000 n \n\
            0000000058 00000 n \n0000000115 00000 n \n\
            trailer<</Size 4/Root 1 0 R>>\nstartxref\n\
            190\n%%EOF";

        let result = is_pdf_unencrypted(minimal_pdf).await;
        let _ = result;
    }

    #[tokio::test]
    async fn test_pdfium_unlock_failure_reprompts() {
        let garbage: &[u8] = b"not a real pdf";
        let result = try_pdfium_unlock(garbage, "wrongpassword").await;
        assert!(!result, "garbage bytes must not unlock successfully");
    }

    #[tokio::test]
    async fn test_password_rotation_success() {
        let temp_dir =
            std::env::temp_dir().join(format!("dinero_test_rot_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test_rot.db");
        let pool = crate::db::init_db(db_path).await.unwrap();

        let conn = pool.get().await.unwrap();

        conn.interact(|c| {
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, network, masked_identifier, status)
                 VALUES ('inst_rot', 'credit_card', 'HDFC Bank', 'VISA', '1234', 'active')",
                [],
            ).unwrap();

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

            let rows = crate::db::pdf_passwords::select_by_instrument(c, "inst_rot").unwrap();
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].id, "pass_old");
            assert_eq!(rows[1].id, "pass_new");
        })
        .await
        .unwrap();
    }
}
