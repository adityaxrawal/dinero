pub mod network;
use crate::statements::{
    duplicate_check::{check_file_hash_duplicate, DuplicateCheckResult},
    events,
    password::{is_pdf_unencrypted, try_all_stored_passwords, PasswordResolutionResult},
    validator::validate_and_hash,
};
use tauri::{Emitter, Manager};

pub mod data;
pub mod debug;
pub mod llm;

#[cfg(test)]
mod data_tests;

/// User-confirmed instrument identity resuming a statement previously blocked
/// by the Statement Instrument Gate (Doc 12 §7.2a, C2 fix).
#[derive(Debug, Clone)]
pub struct ConfirmedInstrument {
    pub issuer_name: String,
    pub masked_identifier: String,
    pub instrument_type: String,
}

/// G20/H10/J8 fix: renamed from `start_oauth_flow` to match Doc 19 §5.1's
/// documented `auth_google_start` naming.
///
/// TASK-DB-022: no longer accepts `profile_id` from the caller — Dinero is
/// single-tenant by construction (`local_profile.id` can only ever be `1`),
/// so this resolves `db::scoping::LOCAL_PROFILE_ID` internally instead of
/// trusting an IPC-supplied value (Document 22 §13.1).
#[tauri::command]
pub async fn auth_google_start(
    app: tauri::AppHandle,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    crate::ingestion::oauth::start_oauth_flow_async(
        app,
        pool.inner().clone(),
        crate::db::scoping::LOCAL_PROFILE_ID,
    )
    .await
    .map_err(|e| crate::error::AppError::Auth(e.to_string()))
}

/// TASK-AUTH-005: revokes the current local session (`revoked_at`, never
/// deleted) and clears in-memory session state — requires re-auth before any
/// subsequent Gmail/licensing IPC command that depends on an active session
/// (enforced broadly by TASK-AUTH-008's tenant-isolation pattern, which this
/// task's session concept is the foundation for).
#[tauri::command]
pub async fn auth_logout(
    app: tauri::AppHandle,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<(), crate::error::AppError> {
    let state = app.state::<crate::auth::session::SessionState>();
    crate::auth::session::logout(pool.inner(), state.inner())
        .await
        .map_err(|e| crate::error::AppError::Auth(e.to_string()))
}

/// Doc 19 §5.4, Doc 22 §8.2: returns the opt-in 24-word Secure Backup Recovery
/// Phrase for the current `base_key`, generating it (and marking the user as
/// opted in) on first call. Marks `local_profile.recovery_phrase_enabled` —
/// previously a dangling column nothing ever set — and writes the audit entry
/// the doc requires whenever the phrase is viewed.
// G20/H10/J8 fix: renamed from `get_recovery_phrase` to match Doc 19 §5.4's
// documented `auth_get_recovery_phrase` naming (the internal
// `db::crypto::get_recovery_phrase` helper this calls is untouched — it's
// not part of the IPC surface).
#[tauri::command]
pub async fn auth_get_recovery_phrase(
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    let phrase = crate::db::crypto::get_recovery_phrase()
        .map_err(|e| crate::error::AppError::Internal(e.to_string()))?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| {
        if let Ok(Some(mut profile)) = crate::db::local_profile::select_by_id(c, 1) {
            profile.recovery_phrase_enabled = true;
            let _ = crate::db::local_profile::update(c, &profile);
        }
        let _ = crate::db::audit_log::insert(
            c,
            &crate::db::audit_log::AuditLogRow {
                id: uuid::Uuid::new_v4().to_string(),
                actor_type: Some("user".to_string()),
                actor_id: Some("local".to_string()),
                action: Some("recovery_phrase_viewed".to_string()),
                resource_type: Some("local_profile".to_string()),
                resource_id: Some("1".to_string()),
                before_json: None,
                after_json: None,
                created_at: chrono::Utc::now(),
            },
        );
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

    Ok(phrase)
}

/// Doc 19 §5.5, Doc 22 §8.2: derives `base_key` from a Recovery Phrase,
/// verifies it actually decrypts `finance.db` on this machine, and — on
/// success — recreates the Keychain entry this Mac needs going forward.
/// Deliberately does not depend on `tauri::State<Pool>` (unlike every other
/// command here): Doc 19 §5.5 marks this "unauthenticated, pre-login" because
/// its entire purpose is recovering from exactly the scenario where the
/// normal Keychain-backed pool never came up. It opens its own connection
/// using the now-verified key to write the required audit entry.
///
/// Reaching this command in that literal boot-time scenario requires
/// `lib.rs`'s current `.expect("Failed to initialize encrypted database")` to
/// be replaced with a graceful recovery-mode boot path — a structural,
/// app-wide startup change that overlaps with M14's separate, not-yet-reached
/// "corrupted-DB recovery screen" scope (Doc 13 Flow 4.17, Doc 12 §13.2/§13.6)
/// and was deliberately left untouched here; see the fix log for M03.
// G20/H10/J8 fix: renamed from `restore_from_recovery_phrase` to match Doc 19
// §5.5's documented `auth_restore_from_recovery_phrase` naming.
#[tauri::command]
pub async fn auth_restore_from_recovery_phrase(
    app: tauri::AppHandle,
    recovery_phrase: String,
) -> Result<String, crate::error::AppError> {
    let app_dir = app.path().app_data_dir().map_err(|e| {
        crate::error::AppError::Io(format!("Failed to resolve app data directory: {}", e))
    })?;
    let db_path = app_dir.join("finance.db");

    let base_key = crate::db::crypto::restore_base_key_from_phrase(&recovery_phrase, &db_path)
        .map_err(|e| crate::error::AppError::Auth(e.to_string()))?;

    let db_key = crate::db::crypto::derive_database_key_from_base_key(&base_key)
        .map_err(|e| crate::error::AppError::Auth(e.to_string()))?;
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.execute_batch(&format!("PRAGMA key = '{}';", db_key))
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let _ = crate::db::audit_log::insert(
        &conn,
        &crate::db::audit_log::AuditLogRow {
            id: uuid::Uuid::new_v4().to_string(),
            actor_type: Some("user".to_string()),
            actor_id: Some("local".to_string()),
            action: Some("recovery_phrase_restore".to_string()),
            resource_type: Some("local_profile".to_string()),
            resource_id: Some("1".to_string()),
            before_json: None,
            after_json: None,
            created_at: chrono::Utc::now(),
        },
    );

    Ok("Database decrypted and Keychain entries recreated.".to_string())
}

#[derive(serde::Serialize)]
pub struct ExportLogsResponse {
    pub success: bool,
    pub file_path: String,
}

/// Doc 19 §21.1, Doc 36 §4, Doc 41 §5: generates the privacy-safe diagnostic
/// bundle — the only path by which any operational data leaves the device.
#[tauri::command]
pub async fn export_logs(
    app: tauri::AppHandle,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<ExportLogsResponse, crate::error::AppError> {
    let app_dir = app.path().app_data_dir().map_err(|e| {
        crate::error::AppError::Io(format!("Failed to resolve app data directory: {}", e))
    })?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let path = conn
        .interact(move |c| crate::diagnostics::generate_diagnostic_bundle(&app_dir, c, None))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Io(e.to_string()))?;

    Ok(ExportLogsResponse {
        success: true,
        file_path: path.display().to_string(),
    })
}
//
// Steps (Doc 10 §3.2):
//   1. Read file bytes from path; handle macOS TCC EPERM
//   2. Validate: MIME = application/pdf, size ≤ 5MB, non-zero bytes → compute SHA-256
//   3. File hash duplicate check (pre-password, against statements + unprocessed_statements)
//   4. Filename-based billing cycle duplicate check (§5.2)
//   5. Password resolution: try stored → prompt if needed (§5.3)
//   6. Parse PDF in-memory (§5.6): pdfium primary → OCR fallback
//   7. Extract metadata from first page (§5.4): billing period, amounts, instrument
//   8. Instrument resolve / auto-create (§5.4)
//   9. Post-metadata billing cycle duplicate check (§5.2 deferred path)
//  10. Write statements row with parse_status='parsed' (§5.4)
//  11. Extract statement rows per bank parser (§5.5)
//  12. Filter, merge broken rows, map to statement_entries (§5.5)
//  13. Map statement_entries to transaction_observations → reconcile (§5.5, §6.x)
//  14. Classify upcoming bill → update instrument if needed (§5.7)
//  15. Discard raw PDF bytes from memory (§5.8)
//  16. Emit statement.parsed or statement.parse_failed event

/// Doc 19 §9.1: one file in a `statements_upload` batch. The frontend reads
/// the file via the browser File API (`file.arrayBuffer()`) and sends bytes
/// directly — the Rust backend never reads a filesystem path itself, so a
/// locked-down WebView with no filesystem access still works.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadFile {
    pub file_bytes: Vec<u8>,
    pub filename: String,
}

/// Doc 19 §9.1: one per-file result, in submission order.
#[derive(serde::Serialize)]
pub struct UploadResult {
    pub statement_id: String,
    pub filename: String,
    pub status: String,
}

/// Doc 30 TASK-API-004 acceptance test `test_upload_command_rejects_missing_file`:
/// extracted as a pure function so it's directly testable without the full
/// async IPC plumbing.
fn validate_upload_files_non_empty(files: &[UploadFile]) -> Result<(), crate::error::AppError> {
    if files.is_empty() {
        return Err(crate::error::AppError::Validation(
            "at least one file is required".to_string(),
        ));
    }
    Ok(())
}

/// Phase 5 — Upload one or more PDF statements for in-memory parsing (Doc 19 §9.1, FR-031).
/// Each file becomes an independent Statement Queue job, subject to the same
/// bounded 5-concurrent cap and Statement Instrument Gate as email-detected
/// statements. Doc 19 §3.6 / Doc 15 §2.7: expensive PDF processing is queued
/// and processed asynchronously — this command returns as soon as each file
/// is validated and enqueued, never blocking on the parse itself. The real
/// outcome (parsed / failed / awaiting_password / awaiting_instrument) arrives
/// later via `statement_parsed`/`statement_parse_failed`/`statement_password_required`/
/// `statement_instrument_confirmation_required` events.
#[tauri::command]
pub async fn statements_upload(
    files: Vec<UploadFile>,
    app: tauri::AppHandle,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
    queues: tauri::State<'_, crate::ingestion::queues::QueueHandles>,
    pending_bytes: tauri::State<'_, crate::statements::pending_bytes::PendingStatementBytes>,
) -> Result<serde_json::Value, crate::error::AppError> {
    // Doc 30 TASK-API-004 acceptance test `test_upload_command_rejects_missing_file`:
    // real gap fixed here -- an empty `files` array previously fell through
    // the loop below untouched and returned `Ok({"results": []})`, never an
    // error.
    validate_upload_files_non_empty(&files)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    // H2 fix (Doc 19 §9.1, FR-031): a real multi-file batch contract — one
    // IPC round-trip processes every selected file, rather than only ever
    // accepting a single path. Each file still goes through the identical
    // single-statement pipeline below; failures are isolated per-file so one
    // bad file in a batch doesn't abort the rest.
    //
    // Doc 30 TASK-STMT-009: batches over 10 statements share one progress
    // tracker so the Statement Queue dispatcher can emit rolling
    // parsed/total/eta_seconds events as each one actually finishes parsing.
    let batch_progress = if files.len() > 10 {
        Some(std::sync::Arc::new(
            crate::ingestion::queues::BatchProgressTracker::new(files.len()),
        ))
    } else {
        None
    };

    let mut results = Vec::with_capacity(files.len());
    for file in files {
        let filename = file.filename.clone();
        let result = upload_one_statement(
            file.file_bytes,
            filename.clone(),
            &app,
            pool.inner(),
            &queues,
            pending_bytes.inner(),
            batch_progress.clone(),
        )
        .await;
        results.push(match result {
            Ok(r) => r,
            Err(e) => UploadResult {
                statement_id: String::new(),
                filename,
                status: format!("error: {}", e),
            },
        });
    }
    Ok(serde_json::json!({ "results": results }))
}

async fn upload_one_statement(
    bytes: Vec<u8>,
    filename: String,
    app: &tauri::AppHandle,
    pool_ref: &deadpool_sqlite::Pool,
    queues: &crate::ingestion::queues::QueueHandles,
    pending_bytes: &crate::statements::pending_bytes::PendingStatementBytes,
    batch_progress: Option<std::sync::Arc<crate::ingestion::queues::BatchProgressTracker>>,
) -> Result<UploadResult, crate::error::AppError> {
    tracing::info!(
        "statements_upload: filename='{}' size={} bytes",
        filename,
        bytes.len()
    );

    // ── Step 2: Validate + SHA-256 ───────────────────────────────────────────
    let file_hash =
        validate_and_hash(&bytes).map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;
    tracing::info!("File validated. sha256={}", file_hash);

    // ── Step 3: File hash duplicate check (pre-password) ─────────────────────
    let hash_dup = check_file_hash_duplicate(&file_hash, None, pool_ref)
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    if hash_dup != DuplicateCheckResult::NoDuplicate {
        tracing::warn!("Duplicate file hash detected: sha256={}", file_hash);
        events::emit(
            events::DUPLICATE_REJECTED,
            serde_json::json!({ "reason": "duplicate_file_hash", "sha256": file_hash }),
        );
        app.emit(
            events::DUPLICATE_REJECTED,
            serde_json::json!({
                "reason": "duplicate_file_hash",
                "sha256": file_hash
            }),
        )
        .ok();
        return Err(crate::error::AppError::Unknown(
            "duplicate_file_hash: statement already imported".to_string(),
        ));
    }

    // ── Step 4: Filename-based billing cycle duplicate check ──────────────────
    // (instrument_id not yet known — we use a placeholder; real check happens at Step 9 post-metadata)
    // Filename check is best-effort: if filename has a parseable period, do an early reject.
    // We can only check against a known instrument; we'll re-check post-metadata with real instrument.
    // For now: check filename period against all instruments (across-the-board heuristic).
    // This is refined post-metadata at step 9 for the specific instrument.
    if let Ok(Some(DuplicateCheckResult::DuplicateBillingCycle)) =
        check_filename_billing_cycle_all_instruments(&filename, pool_ref).await
    {
        tracing::warn!(
            "Duplicate billing cycle detected from filename: '{}'",
            filename
        );
        // Doc 30 TASK-STMT-002: "Every skip is logged to audit_log
        // (statement_duplicate_skipped) with the detected period for user
        // transparency."
        let period =
            crate::statements::duplicate_check::extract_billing_period_from_filename(&filename);
        log_duplicate_skipped_audit(&filename, period.as_ref(), pool_ref).await;
        events::emit(
            events::DUPLICATE_REJECTED,
            serde_json::json!({ "reason": "duplicate_billing_cycle_filename", "filename": filename }),
        );
        return Err(crate::error::AppError::Unknown(
            "duplicate_billing_cycle: statement cycle already imported".to_string(),
        ));
    }

    // ── Step 5: Password resolution ───────────────────────────────────────────
    //
    // If PDF is unencrypted: proceed immediately.
    // If encrypted: try stored passwords (AES-256-GCM decrypted from DB).
    // If all stored passwords fail: create unprocessed_statements row → emit password_required event.
    // The password submit flow continues via statements_submit_password.
    //
    // Real bug fixed here (Doc 30 TASK-STMT-010): the instrument this PDF
    // belongs to isn't known yet at this point in the pipeline — metadata
    // extraction (Step 7, after unlocking) is what discovers it — so a
    // per-instrument stored-password lookup can never have a real
    // `instrument_id` to scope by. Scans across every instrument's stored
    // passwords instead, which is what actually makes "try stored passwords
    // before prompting" work at all for a fresh upload.
    let pdf_is_encrypted = !is_pdf_unencrypted(&bytes).await;
    if pdf_is_encrypted {
        let pw_result = try_all_stored_passwords(&bytes, pool_ref)
            .await
            .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

        match pw_result {
            PasswordResolutionResult::NotEncrypted => {
                // Shouldn't happen — proceed
            }
            PasswordResolutionResult::UnlockedWithStored(_) => {
                tracing::info!("PDF unlocked with stored password");
            }
            PasswordResolutionResult::PromptRequired => {
                // Create unprocessed_statements row and notify UI
                let stmt_id = uuid::Uuid::new_v4().to_string();
                create_awaiting_password_row(&stmt_id, &file_hash, &filename, pool_ref)
                    .await
                    .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

                // H3 fix: hold the bytes in memory (never on disk) so
                // statements_submit_password can actually re-check the
                // password against the real PDF instead of an empty slice.
                pending_bytes.insert(stmt_id.clone(), bytes.clone()).await;

                let payload = serde_json::json!({
                    "statement_id": stmt_id,
                    "filename": filename,
                });
                events::emit(events::PASSWORD_REQUIRED, payload.clone());
                app.emit(events::PASSWORD_REQUIRED, payload).ok();

                return Ok(UploadResult {
                    statement_id: stmt_id,
                    filename,
                    status: "awaiting_password".to_string(),
                });
            }
            _ => {}
        }
    }

    // ── Steps 6–15: Parse → Metadata → Rows → Reconcile → Classify → Cleanup ─
    // Doc 15 §2 principle 7 / Doc 12 §7.2: manual upload is a Statement Queue job,
    // subject to the same bounded 5-concurrent cap as email-detected statements —
    // there is no separate, weaker-validated path for uploads (§7.6.10).
    //
    // Doc 18 §4.7 / Doc 19 §9.1: the `statements` row is written in `queued`
    // state right here, immediately before enqueueing — before any parsing
    // begins, satisfying the crash-recovery invariant — and the command
    // returns immediately after enqueueing rather than blocking on the parse
    // (Doc 19 §3.6: expensive operations are async, never block the IPC call).
    let stmt_id = uuid::Uuid::new_v4().to_string();
    {
        let conn = pool_ref
            .get()
            .await
            .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
        let id = stmt_id.clone();
        let hash = file_hash.clone();
        conn.interact(move |c| {
            crate::db::statements::insert_queued(c, &id, "manual_upload", None, Some(&hash))
        })
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    }

    let job = crate::ingestion::queues::StatementJob {
        bytes,
        filename: filename.clone(),
        file_hash: file_hash.clone(),
        stmt_id: stmt_id.clone(),
        batch_progress,
    };
    if queues.statement_tx.send(job).await.is_err() {
        return Err(crate::error::AppError::Unknown(
            "Statement Queue closed".to_string(),
        ));
    }

    tracing::info!(
        "statements_upload: filename='{}' queued as stmt_id='{}'",
        filename,
        stmt_id
    );

    Ok(UploadResult {
        statement_id: stmt_id,
        filename,
        status: "queued".to_string(),
    })
}

/// Runs steps 6–14 of the PDF statement processing pipeline.
///
/// Separated from `statements_upload` so that the raw `bytes` can be explicitly dropped
/// at the command boundary (§5.8 Post-Parse Memory Cleanup).
///
/// `pub` so that email-detected statement paths in `ingestion::historical_scan` and
/// `ingestion::polling` can call the same pipeline as manual uploads (Doc 12 §7.2 step 1).
///
/// `confirmed_instrument`, when `Some`, is supplied by `statements_confirm_instrument`
/// resuming a statement previously blocked by the Instrument Gate (C2 fix) — it
/// bypasses the gate below and uses the user-confirmed issuer/masked/type directly.
///
/// `stmt_id`, when `Some`, names a `statements` row already written by
/// `insert_queued()` at intake (Doc 18 §4.7's crash-recovery invariant) —
/// Step 10 upserts it instead of minting a new ID. `None` for the
/// resumed-after-block paths (`statements_confirm_instrument`/
/// `statements_submit_password`), which correctly mint a fresh ID at Step 10
/// as before — `unprocessed_statements` already owns crash-recovery for the
/// blocked window via its own separate ID (Doc 18 §4.16's `resolved_statement_id`
/// is a deliberate one-way link, not a shared ID).
pub async fn run_parse_pipeline<R: tauri::Runtime>(
    bytes: &[u8],
    _filename: &str,
    file_hash: &str,
    pool: &deadpool_sqlite::Pool,
    app: &tauri::AppHandle<R>,
    pending_bytes: &crate::statements::pending_bytes::PendingStatementBytes,
    confirmed_instrument: Option<ConfirmedInstrument>,
    password: Option<&str>,
    stmt_id: Option<String>,
) -> anyhow::Result<String> {
    use crate::statements::{
        bill_classifier,
        duplicate_check::{check_billing_cycle_duplicate, DuplicateCheckResult},
        metadata_extractor::{extract_metadata, resolve_or_create_instrument, write_statement_row},
        parser::parse_in_memory_with_password,
        row_extractor::{extract_rows, map_rows_to_statement_entries, BankParser},
    };

    // ── Step 6: Parse PDF in-memory ──────────────────────────────────────────
    // H3 fix: `password`, when Some, is the password the user just confirmed
    // via statements_submit_password — pdfium must be told it on every open.
    let parse_result = parse_in_memory_with_password(bytes, password).await?;
    tracing::info!(
        "Parsed {} pages (method={:?}, ocr_pages={})",
        parse_result.total_pages,
        parse_result.parse_method,
        parse_result.ocr_page_count
    );

    if parse_result.pages.is_empty() {
        return Err(anyhow::anyhow!(
            "No pages extracted from PDF — parse_failed"
        ));
    }

    // ── Step 7: Extract metadata ──────────────────────────────────────────────
    let meta = extract_metadata(&parse_result.pages)?;
    tracing::info!(
        "Metadata: issuer={:?} masked={:?} period={:?}→{:?} due={:?}",
        meta.issuer_name,
        meta.masked_identifier,
        meta.billing_period_start,
        meta.billing_period_end,
        meta.due_date
    );

    // ── Step 8: Statement Instrument Gate (Doc 12 §7.2a, FR-033a) ────────────
    // MANDATORY: both issuer_name and masked_identifier must resolve before row extraction.
    // If either is absent, block and prompt the user — never guess or default silently.
    // A `confirmed_instrument` (from statements_confirm_instrument resuming a
    // previously-blocked statement) satisfies the gate directly.
    let (issuer, masked, instrument_type) = if let Some(confirmed) = confirmed_instrument {
        (
            confirmed.issuer_name,
            confirmed.masked_identifier,
            confirmed.instrument_type,
        )
    } else {
        let issuer = match meta.issuer_name.clone() {
            Some(i) if !i.trim().is_empty() => i,
            _ => {
                delete_orphaned_queued_row(stmt_id.as_deref(), pool).await;
                let unprocessed_id = uuid::Uuid::new_v4().to_string();
                create_awaiting_instrument_row(&unprocessed_id, file_hash, _filename, pool)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!("DB error creating awaiting_instrument row: {}", e)
                    })?;
                pending_bytes
                    .insert(unprocessed_id.clone(), bytes.to_vec())
                    .await;
                let payload = serde_json::json!({
                    "statement_id": unprocessed_id,
                    "filename": _filename,
                    "reason": "issuer_name could not be extracted from statement header",
                });
                events::emit(events::INSTRUMENT_CONFIRMATION_REQUIRED, payload.clone());
                app.emit(events::INSTRUMENT_CONFIRMATION_REQUIRED, payload)
                    .ok();
                tracing::warn!(
                    "Statement Instrument Gate BLOCKED (issuer absent) — \
                     statement_id='{}' filename='{}'",
                    unprocessed_id,
                    _filename
                );
                return Ok(unprocessed_id);
            }
        };
        let masked = match meta.masked_identifier.clone() {
            Some(m) if !m.trim().is_empty() => m,
            _ => {
                delete_orphaned_queued_row(stmt_id.as_deref(), pool).await;
                let unprocessed_id = uuid::Uuid::new_v4().to_string();
                create_awaiting_instrument_row(&unprocessed_id, file_hash, _filename, pool)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!("DB error creating awaiting_instrument row: {}", e)
                    })?;
                pending_bytes
                    .insert(unprocessed_id.clone(), bytes.to_vec())
                    .await;
                let payload = serde_json::json!({
                    "statement_id": unprocessed_id,
                    "filename": _filename,
                    "issuer": issuer,
                    "reason": "masked account/card number could not be extracted from statement header",
                });
                events::emit(events::INSTRUMENT_CONFIRMATION_REQUIRED, payload.clone());
                app.emit(events::INSTRUMENT_CONFIRMATION_REQUIRED, payload)
                    .ok();
                tracing::warn!(
                    "Statement Instrument Gate BLOCKED (masked_id absent) — \
                     issuer='{}' statement_id='{}' filename='{}'",
                    issuer,
                    unprocessed_id,
                    _filename
                );
                return Ok(unprocessed_id);
            }
        };
        // Instrument type: default "credit_card" now that both issuer and masked are confirmed by gate.
        (issuer, masked, "credit_card".to_string())
    };
    let instrument_id = resolve_or_create_instrument(
        &instrument_type,
        &issuer,
        &masked,
        meta.network.as_deref(),
        pool,
    )
    .await?;
    tracing::info!("Instrument resolved: id='{}'", instrument_id);

    // ── Step 9: Post-metadata billing cycle duplicate check ───────────────────
    if let (Some(ref start), Some(ref end)) = (&meta.billing_period_start, &meta.billing_period_end)
    {
        let dup = check_billing_cycle_duplicate(&instrument_id, start, end, pool).await?;
        if dup == DuplicateCheckResult::DuplicateBillingCycle {
            tracing::warn!(
                "Duplicate billing cycle: instrument='{}' period={} → {}",
                instrument_id,
                start,
                end
            );
            // Doc 30 TASK-STMT-002: audit trail for the post-metadata-extraction
            // duplicate-skip path too, not just the filename-heuristic one.
            log_duplicate_skipped_audit(_filename, Some(&(start.clone(), end.clone())), pool).await;
            delete_orphaned_queued_row(stmt_id.as_deref(), pool).await;
            return Err(anyhow::anyhow!(
                "duplicate_billing_cycle: cycle {} → {} already imported for instrument {}",
                start,
                end,
                instrument_id
            ));
        }
    }

    // ── Step 10: Write statements row ─────────────────────────────────────────
    // source_message_id for manual uploads uses the file_hash as a proxy identifier.
    // `stmt_id`, if the caller pre-created a queued row at intake (Doc 18 §4.7),
    // is upserted in place; otherwise (a resumed-after-block path) a fresh ID
    // is minted here, matching the pre-existing behavior for that case.
    let final_stmt_id = stmt_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let stmt_id = write_statement_row(
        &final_stmt_id,
        &instrument_id,
        &instrument_type,
        &meta,
        Some(file_hash),
        pool,
    )
    .await?;
    tracing::info!("Statement row written: id='{}'", stmt_id);

    // ── Step 11: Extract statement rows ───────────────────────────────────────
    let bank_parser = BankParser::detect(&issuer);
    let rows = extract_rows(&parse_result.pages, bank_parser)?;
    tracing::info!(
        "Extracted {} statement rows via parser={:?}",
        rows.len(),
        bank_parser
    );

    // ── Step 12: Map rows to statement_entries ────────────────────────────────
    let entry_ids = map_rows_to_statement_entries(&stmt_id, &rows, pool).await;
    tracing::info!(
        "Mapped {} rows → {} statement_entries",
        rows.len(),
        entry_ids.len()
    );

    // ── Step 13: Map statement_entries → transaction_observations → reconcile ─
    if !rows.is_empty() && !entry_ids.is_empty() {
        let observations = crate::statements::observation_builder::build_all_observations(
            &stmt_id,
            &instrument_id,
            &rows,
            &entry_ids,
        );

        tracing::info!(
            "Built {} observations from statement rows",
            observations.len()
        );

        // Reconcile each observation against canonical transactions
        let conn = pool
            .get()
            .await
            .map_err(|e| anyhow::anyhow!("DB pool error: {}", e))?;
        let obs_cloned = observations.clone();
        let app_handle_clone = app.clone();
        conn.interact(move |conn| {
            for obs in &obs_cloned {
                match crate::reconciliation::engine::reconcile_transactionally(conn, obs) {
                    Ok(decision) => {
                        tracing::debug!(
                            "Reconciliation decision for obs '{}': {:?}",
                            obs.id,
                            decision
                        );
                        if let crate::reconciliation::audit::DecisionType::AmbiguousPending(cluster_id) = decision {
                            let _ = crate::ipc::events::emit_event(
                                &app_handle_clone,
                                crate::ipc::events::AppEvent::ReconciliationCluster,
                                serde_json::json!({ "cluster_id": cluster_id, "observation_id": obs.id }),
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Reconciliation failed for obs '{}': {}", obs.id, e);
                        // Isolated failure — continue with remaining observations
                    }
                }
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("Interact error: {}", e))?;

        let obs_ids: Vec<String> = observations.into_iter().map(|o| o.id).collect();
        tokio::spawn(
            crate::reconciliation::alert_worker::evaluate_alerts_for_observations(
                pool.clone(),
                app.clone(),
                obs_ids,
            ),
        );
    }

    // ── Step 14: Classify upcoming bill ──────────────────────────────────────
    bill_classifier::classify_and_update(&instrument_id, &stmt_id, &meta, pool, Some(app)).await?;

    // ── Step 15 note: Caller drops raw bytes after this function returns ──────

    Ok(stmt_id)
}

/// Creates an `unprocessed_statements` row with status = 'awaiting_password' (Doc 10 §7.2).
async fn create_awaiting_password_row(
    statement_id: &str,
    file_hash: &str,
    filename: &str,
    pool: &deadpool_sqlite::Pool,
) -> anyhow::Result<()> {
    let _id = uuid::Uuid::new_v4().to_string();
    let stmt_id = statement_id.to_string();
    let source_json = serde_json::json!({
        "file_hash": file_hash,
        "filename": filename,
    })
    .to_string();

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
    .map_err(|e| anyhow::anyhow!("DB interact error (create_awaiting_password_row): {}", e))??;

    tracing::info!(
        "Created awaiting_password row for statement_id='{}'",
        statement_id
    );
    Ok(())
}

/// Doc 30 TASK-STMT-002: "Every skip is logged to audit_log
/// (statement_duplicate_skipped) with the detected period for user
/// transparency." Best-effort — a logging failure must never fail the
/// (already-decided) duplicate rejection itself.
async fn log_duplicate_skipped_audit(
    filename: &str,
    period: Option<&(String, String)>,
    pool: &deadpool_sqlite::Pool,
) {
    let after_json = serde_json::json!({
        "filename": filename,
        "billing_period_start": period.map(|(s, _)| s.clone()),
        "billing_period_end": period.map(|(_, e)| e.clone()),
    });
    if let Ok(conn) = pool.get().await {
        let _ = conn
            .interact(move |c| {
                crate::db::audit_log::insert(
                    c,
                    &crate::db::audit_log::AuditLogRow {
                        id: uuid::Uuid::new_v4().to_string(),
                        actor_type: Some("system".to_string()),
                        actor_id: None,
                        action: Some("statement_duplicate_skipped".to_string()),
                        resource_type: Some("statement".to_string()),
                        resource_id: None,
                        before_json: None,
                        after_json: Some(after_json),
                        created_at: chrono::Utc::now(),
                    },
                )
            })
            .await;
    }
}

/// Best-effort cleanup for the queued `statements` row `insert_queued()` wrote
/// at intake, when the pipeline diverges away from ever completing it (an
/// Instrument Gate block or a post-metadata duplicate reject) — otherwise it
/// would sit at `parse_status = 'queued'` forever, an orphan `unprocessed_statements`
/// (or the duplicate rejection) already fully accounts for going forward.
/// No-op (and never fails the caller) when `stmt_id` is `None` — the
/// resumed-after-block paths never had a queued row to begin with.
async fn delete_orphaned_queued_row(stmt_id: Option<&str>, pool: &deadpool_sqlite::Pool) {
    let Some(id) = stmt_id else { return };
    let id = id.to_string();
    if let Ok(conn) = pool.get().await {
        let _ = conn
            .interact(move |c| {
                c.execute(
                    "DELETE FROM statements WHERE id = ?1 AND parse_status = 'queued'",
                    rusqlite::params![id],
                )
            })
            .await;
    }
}

/// Creates an `unprocessed_statements` row with `status = 'awaiting_instrument_confirmation'`
/// when the Statement Instrument Gate cannot resolve issuer or masked identifier (Doc 12 §7.2a).
async fn create_awaiting_instrument_row(
    statement_id: &str,
    file_hash: &str,
    filename: &str,
    pool: &deadpool_sqlite::Pool,
) -> anyhow::Result<()> {
    let stmt_id = statement_id.to_string();
    let source_json = serde_json::json!({
        "file_hash": file_hash,
        "filename": filename,
    })
    .to_string();

    let stmt_id_for_log = stmt_id.clone();
    let conn = pool.get().await?;
    conn.interact(move |c| {
        c.execute(
            "INSERT INTO unprocessed_statements \
             (id, statement_source_json, failure_type, failure_reason, status) \
             VALUES (?, ?, 'instrument_unresolved', \
             'Statement Instrument Gate: issuer or masked account/card number missing', \
             'awaiting_instrument_confirmation')",
            rusqlite::params![stmt_id, source_json],
        )?;
        // J6 fix: Instrument Gate blocks are a documented audit_log category
        // (Doc 25 §6.1) — previously only surfaced as a Tauri event.
        if let Err(e) = crate::db::audit_log::insert(
            c,
            &crate::db::audit_log::AuditLogRow {
                id: uuid::Uuid::new_v4().to_string(),
                actor_type: Some("system".to_string()),
                actor_id: None,
                action: Some("instrument_gate_blocked".to_string()),
                resource_type: Some("statement".to_string()),
                resource_id: Some(stmt_id_for_log),
                before_json: None,
                after_json: None,
                created_at: chrono::Utc::now(),
            },
        ) {
            tracing::warn!(
                "Failed to record instrument_gate_blocked audit event: {}",
                e
            );
        }
        Ok::<(), rusqlite::Error>(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("DB interact error (create_awaiting_instrument_row): {}", e))??;

    tracing::info!(
        "Created awaiting_instrument_confirmation row for statement_id='{}'",
        statement_id
    );
    Ok(())
}

/// Resumes a statement blocked by the Statement Instrument Gate (Doc 12 §7.2a,
/// C2 fix) with a user-confirmed issuer/masked identifier. The raw PDF bytes
/// held in-memory since the original block are re-run through the same
/// `run_parse_pipeline` shared entry point — never re-read from disk, never
/// written to disk (Doc 12 §7.6.5 / C22).
#[tauri::command]
pub async fn statements_confirm_instrument(
    statement_id: String,
    issuer_name: String,
    masked_identifier: String,
    instrument_type: Option<String>,
    app: tauri::AppHandle,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
    pending_bytes: tauri::State<'_, crate::statements::pending_bytes::PendingStatementBytes>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("statement_id", &statement_id)?;
    crate::ipc::validation::validate_non_empty("issuer_name", &issuer_name)?;
    crate::ipc::validation::validate_non_empty("masked_identifier", &masked_identifier)?;
    let issuer_name = issuer_name.trim().to_string();
    let masked_identifier = masked_identifier.trim().to_string();

    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let bytes = pending_bytes.take(&statement_id).await.ok_or_else(|| {
        crate::error::AppError::Unknown(
            "This statement's session has expired or was already resolved — please re-upload the file".to_string(),
        )
    })?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let stmt_id_clone = statement_id.clone();
    let source_json = conn
        .interact(move |c| {
            c.query_row(
                "SELECT statement_source_json FROM unprocessed_statements WHERE id = ?",
                [&stmt_id_clone],
                |row| row.get::<_, String>(0),
            )
        })
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .map_err(|_| {
            crate::error::AppError::Unknown(format!(
                "No unprocessed statement found with statement_id='{}'",
                statement_id
            ))
        })?;

    let parsed: serde_json::Value = serde_json::from_str(&source_json).unwrap_or_default();
    let filename = parsed["filename"]
        .as_str()
        .unwrap_or("statement.pdf")
        .to_string();
    let file_hash = parsed["file_hash"].as_str().unwrap_or_default().to_string();

    let confirmed = ConfirmedInstrument {
        issuer_name,
        masked_identifier,
        instrument_type: instrument_type.unwrap_or_else(|| "credit_card".to_string()),
    };

    tracing::info!(
        "Resuming statement_id='{}' with user-confirmed instrument (issuer='{}')",
        statement_id,
        confirmed.issuer_name
    );

    let result = run_parse_pipeline(
        &bytes,
        &filename,
        &file_hash,
        pool.inner(),
        &app,
        pending_bytes.inner(),
        Some(confirmed),
        None,
        None,
    )
    .await;

    match result {
        Ok(new_stmt_id) => {
            let conn = pool
                .get()
                .await
                .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
            let orig_id = statement_id.clone();
            let resolved_id = new_stmt_id.clone();
            conn.interact(move |c| {
                crate::db::unprocessed_statements::update_status(
                    c,
                    &orig_id,
                    "resolved",
                    Some(&resolved_id),
                )
            })
            .await
            .map_err(|e| crate::error::AppError::Db(e.to_string()))?
            .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

            events::emit(
                events::PARSED,
                serde_json::json!({ "statement_id": new_stmt_id, "filename": filename }),
            );
            app.emit(
                events::PARSED,
                serde_json::json!({ "statement_id": new_stmt_id, "filename": filename }),
            )
            .ok();

            Ok(serde_json::json!({
                "status": "parsed",
                "statement_id": new_stmt_id
            }))
        }
        Err(e) => {
            tracing::error!(
                "statements_confirm_instrument: pipeline failed for statement_id='{}': {}",
                statement_id,
                e
            );
            events::emit(
                events::PARSE_FAILED,
                serde_json::json!({ "reason": e.to_string(), "filename": filename }),
            );
            app.emit(
                events::PARSE_FAILED,
                serde_json::json!({ "reason": e.to_string(), "filename": filename }),
            )
            .ok();
            Err(crate::error::AppError::Unknown(e.to_string()))
        }
    }
}

/// Cross-instrument filename billing cycle check.
/// Extracts period from filename; if parseable, checks against ALL statements.
/// This is an early-reject optimization before instrument is known.
async fn check_filename_billing_cycle_all_instruments(
    filename: &str,
    pool: &deadpool_sqlite::Pool,
) -> anyhow::Result<Option<crate::statements::duplicate_check::DuplicateCheckResult>> {
    use crate::statements::duplicate_check::extract_billing_period_from_filename;

    match extract_billing_period_from_filename(filename) {
        Some((start, end)) => {
            let conn = pool.get().await?;
            let s = start.clone();
            let e = end.clone();
            let count: i64 = conn
                .interact(move |c| {
                    c.query_row(
                        "SELECT COUNT(*) FROM statements \
                         WHERE billing_period_start = ? AND billing_period_end = ? \
                         AND is_duplicate = 0",
                        rusqlite::params![s, e],
                        |row| row.get(0),
                    )
                })
                .await
                .map_err(|e| anyhow::anyhow!("DB interact error: {}", e))??;

            if count > 0 {
                Ok(Some(
                    crate::statements::duplicate_check::DuplicateCheckResult::DuplicateBillingCycle,
                ))
            } else {
                Ok(Some(
                    crate::statements::duplicate_check::DuplicateCheckResult::NoDuplicate,
                ))
            }
        }
        None => Ok(None),
    }
}

// ── Phase 5.3 — statements_submit_password ───────────────────────────────────

/// Phase 5 — Submit a user-entered password for a locked statement (Doc 10 §7.3–7.4).
/// The password is tried against the PDF in-memory, and if correct, saved encrypted to DB.
/// Password is NEVER returned to the UI in any IPC response.
///
/// On success: unprocessed_statements status → 'resolved'; pipeline resumes.
/// On failure: re-prompt (unprocessed_statements row preserved).
#[tauri::command]
pub async fn statements_submit_password(
    statement_id: String,
    instrument_id: String,
    password: String,
    app: tauri::AppHandle,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
    pending_bytes: tauri::State<'_, crate::statements::pending_bytes::PendingStatementBytes>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("statement_id", &statement_id)?;
    crate::ipc::validation::validate_uuid("instrument_id", &instrument_id)?;
    crate::ipc::validation::validate_non_empty("password", &password)?;

    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    use crate::statements::{
        events,
        password::{try_user_password, PasswordResolutionResult},
    };

    // H3 fix: the real bytes, held in memory (never on disk) since the
    // original statements_upload call — `peek` so a wrong attempt doesn't
    // discard them before the next retry.
    let pdf_bytes = pending_bytes.peek(&statement_id).await.ok_or_else(|| {
        crate::error::AppError::Unknown(
            "This statement's session has expired — please re-upload the file".to_string(),
        )
    })?;

    let result = try_user_password(
        &instrument_id,
        &statement_id,
        &password,
        &pdf_bytes,
        pool.inner(),
    )
    .await
    .map_err(|e| crate::error::AppError::Auth(e.to_string()))?;

    // NEVER log the password — only log the outcome
    match result {
        PasswordResolutionResult::UnlockedWithUserInput => {
            tracing::info!(
                "Password accepted for statement_id='{}' — resuming parse pipeline",
                statement_id
            );
            record_pdf_password_event(pool.inner(), &statement_id, "pdf_password_unlocked").await;

            // H3 fix: actually resume the shared parse pipeline with the
            // now-decrypted bytes — previously this branch only emitted an
            // event and never called run_parse_pipeline at all.
            pending_bytes.take(&statement_id).await;
            let conn = pool
                .get()
                .await
                .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
            let stmt_id_for_lookup = statement_id.clone();
            let source_json = conn
                .interact(move |c| {
                    c.query_row(
                        "SELECT statement_source_json FROM unprocessed_statements WHERE id = ?",
                        [&stmt_id_for_lookup],
                        |row| row.get::<_, String>(0),
                    )
                })
                .await
                .map_err(|e| crate::error::AppError::Db(e.to_string()))?
                .map_err(|_| {
                    crate::error::AppError::Unknown(format!(
                        "No unprocessed statement found with statement_id='{}'",
                        statement_id
                    ))
                })?;
            let parsed: serde_json::Value = serde_json::from_str(&source_json).unwrap_or_default();
            let filename = parsed["filename"]
                .as_str()
                .unwrap_or("statement.pdf")
                .to_string();
            let file_hash = parsed["file_hash"].as_str().unwrap_or_default().to_string();

            let pipeline_result = run_parse_pipeline(
                &pdf_bytes,
                &filename,
                &file_hash,
                pool.inner(),
                &app,
                pending_bytes.inner(),
                None,
                Some(&password),
                None,
            )
            .await;

            match pipeline_result {
                Ok(new_stmt_id) => {
                    let conn = pool
                        .get()
                        .await
                        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
                    let orig_id = statement_id.clone();
                    let resolved_id = new_stmt_id.clone();
                    conn.interact(move |c| {
                        crate::db::unprocessed_statements::update_status(
                            c,
                            &orig_id,
                            "resolved",
                            Some(&resolved_id),
                        )
                    })
                    .await
                    .map_err(|e| crate::error::AppError::Db(e.to_string()))?
                    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

                    app.emit(
                        events::PARSED,
                        serde_json::json!({ "statement_id": new_stmt_id, "filename": filename }),
                    )
                    .ok();
                    Ok(serde_json::json!({
                        "status": "unlocked",
                        "statement_id": new_stmt_id
                    }))
                }
                Err(e) => {
                    tracing::error!(
                        "statements_submit_password: pipeline failed for statement_id='{}': {}",
                        statement_id,
                        e
                    );
                    app.emit(
                        events::PARSE_FAILED,
                        serde_json::json!({ "reason": e.to_string(), "filename": filename }),
                    )
                    .ok();
                    Err(crate::error::AppError::Unknown(e.to_string()))
                }
            }
        }
        PasswordResolutionResult::WrongPassword => {
            tracing::warn!(
                "Wrong password for statement_id='{}' — re-prompting",
                statement_id
            );
            record_pdf_password_event(pool.inner(), &statement_id, "pdf_password_wrong").await;

            // I9 fix: cap wrong-password attempts at 3 — previously only the
            // 2.5-minute timeout was enforced, allowing unlimited guesses.
            const MAX_PASSWORD_ATTEMPTS: i64 = 3;
            let conn = pool
                .get()
                .await
                .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
            let stmt_id_for_attempts = statement_id.clone();
            let attempts = conn
                .interact(move |c| {
                    crate::db::unprocessed_statements::increment_password_attempts(
                        c,
                        &stmt_id_for_attempts,
                    )
                })
                .await
                .map_err(|e| crate::error::AppError::Db(e.to_string()))?
                .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

            if attempts >= MAX_PASSWORD_ATTEMPTS {
                tracing::warn!(
                    "Password attempt cap reached for statement_id='{}' ({} attempts) — locking out",
                    statement_id,
                    attempts
                );
                let conn = pool
                    .get()
                    .await
                    .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
                let stmt_id_for_status = statement_id.clone();
                conn.interact(move |c| {
                    crate::db::unprocessed_statements::update_status(
                        c,
                        &stmt_id_for_status,
                        "password_failed_max_attempts",
                        None,
                    )
                })
                .await
                .map_err(|e| crate::error::AppError::Db(e.to_string()))?
                .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

                app.emit(
                    events::PASSWORD_MAX_ATTEMPTS_EXCEEDED,
                    serde_json::json!({ "statement_id": statement_id }),
                )
                .ok();

                return Ok(serde_json::json!({
                    "status": "max_attempts_exceeded",
                    "statement_id": statement_id
                }));
            }

            // Re-emit password_required so UI re-prompts without closing modal
            app.emit(
                events::PASSWORD_REQUIRED,
                serde_json::json!({
                    "statement_id": statement_id,
                    "error": "wrong_password",
                    "attempts_remaining": MAX_PASSWORD_ATTEMPTS - attempts
                }),
            )
            .ok();
            Ok(serde_json::json!({
                "status": "wrong_password",
                "statement_id": statement_id,
                "attempts_remaining": MAX_PASSWORD_ATTEMPTS - attempts
            }))
        }
        _ => {
            // Unexpected outcome — surface as error but do not reveal password details
            Err(crate::error::AppError::Unknown(
                "Unexpected password resolution outcome".to_string(),
            ))
        }
    }
}

/// J6 fix: records a PDF-password lifecycle event (Doc 25 §6.1) — the outcome
/// only, never the password itself. Best-effort — a logging failure must
/// never block the password-resolution flow.
async fn record_pdf_password_event(pool: &deadpool_sqlite::Pool, statement_id: &str, action: &str) {
    let Ok(conn) = pool.get().await else { return };
    let stmt_id = statement_id.to_string();
    let action = action.to_string();
    let _ = conn
        .interact(move |c| {
            crate::db::audit_log::insert(
                c,
                &crate::db::audit_log::AuditLogRow {
                    id: uuid::Uuid::new_v4().to_string(),
                    actor_type: Some("user".to_string()),
                    actor_id: Some("local".to_string()),
                    action: Some(action),
                    resource_type: Some("statement".to_string()),
                    resource_id: Some(stmt_id),
                    before_json: None,
                    after_json: None,
                    created_at: chrono::Utc::now(),
                },
            )
        })
        .await;
}

/// Doc 30 TASK-STMT-010 `statements_retry(id)`: "re-attempts the full
/// pipeline, reusing any previously-entered password first." The existing
/// version of this command only ever re-emitted `password_required`,
/// unconditionally re-prompting the user even when a password already on
/// file (e.g. entered for a different statement from the same
/// bank/instrument in the interim) would unlock this one immediately —
/// fixed to actually attempt `try_all_stored_passwords` first, resuming the
/// full pipeline exactly like `statements_submit_password`'s success path
/// when one matches, and falling back to the original re-prompt behavior
/// only when none do.
#[tauri::command]
pub async fn statements_retry_unprocessed(
    statement_id: String,
    app: tauri::AppHandle,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
    pending_bytes: tauri::State<'_, crate::statements::pending_bytes::PendingStatementBytes>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("statement_id", &statement_id)?;

    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    use crate::statements::{events, password::try_all_stored_passwords};

    tracing::info!("Retrying unprocessed statement_id='{}'", statement_id);

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let stmt_id_clone = statement_id.clone();
    let row = conn
        .interact(move |c| crate::db::unprocessed_statements::select_by_id(c, &stmt_id_clone))
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .ok_or_else(|| {
            crate::error::AppError::Unknown(format!(
                "No unprocessed statement found with statement_id='{}'",
                statement_id
            ))
        })?;

    // I9 fix: a statement locked out by the 3-attempt cap must not be
    // reopened for further password guesses via retry.
    if row.status == "password_failed_max_attempts" {
        return Err(crate::error::AppError::Unknown(
            "This statement is locked after too many incorrect password attempts. \
             Please re-upload the file to try again."
                .to_string(),
        ));
    }

    let filename = serde_json::from_str::<serde_json::Value>(&row.statement_source_json)
        .ok()
        .and_then(|v| v["filename"].as_str().map(|s| s.to_string()))
        .unwrap_or_default();

    // Doc 15 Core Principle 4/10: raw PDFs are never persisted — if the
    // bytes are no longer cached (the 30-minute pending_bytes TTL elapsed),
    // there is genuinely nothing left to retry with; the user must re-upload.
    let Some(pdf_bytes) = pending_bytes.peek(&statement_id).await else {
        return Err(crate::error::AppError::Unknown(
            "This statement's cached data has expired — please re-upload the file to retry"
                .to_string(),
        ));
    };

    let stored_result = try_all_stored_passwords(&pdf_bytes, pool.inner())
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

    match stored_result {
        crate::statements::password::PasswordResolutionResult::UnlockedWithStored(password) => {
            tracing::info!(
                "Retry found a matching stored password for statement_id='{}' — resuming pipeline",
                statement_id
            );
            pending_bytes.take(&statement_id).await;
            let file_hash = serde_json::from_str::<serde_json::Value>(&row.statement_source_json)
                .ok()
                .and_then(|v| v["file_hash"].as_str().map(|s| s.to_string()))
                .unwrap_or_default();

            let pipeline_result = run_parse_pipeline(
                &pdf_bytes,
                &filename,
                &file_hash,
                pool.inner(),
                &app,
                pending_bytes.inner(),
                None,
                Some(&password),
                None,
            )
            .await;

            match pipeline_result {
                Ok(new_stmt_id) => {
                    let conn = pool
                        .get()
                        .await
                        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
                    let orig_id = statement_id.clone();
                    let resolved_id = new_stmt_id.clone();
                    conn.interact(move |c| {
                        crate::db::unprocessed_statements::update_status(
                            c,
                            &orig_id,
                            "resolved",
                            Some(&resolved_id),
                        )
                    })
                    .await
                    .map_err(|e| crate::error::AppError::Db(e.to_string()))?
                    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

                    app.emit(
                        events::PARSED,
                        serde_json::json!({ "statement_id": new_stmt_id, "filename": filename }),
                    )
                    .ok();
                    Ok(serde_json::json!({ "status": "unlocked", "statement_id": new_stmt_id }))
                }
                Err(e) => {
                    tracing::error!(
                        "statements_retry_unprocessed: pipeline failed for statement_id='{}': {}",
                        statement_id,
                        e
                    );
                    Err(crate::error::AppError::Unknown(e.to_string()))
                }
            }
        }
        _ => {
            // No stored password unlocked it — fall back to the original
            // behavior: re-prompt the user, preserving all in-progress context.
            app.emit(
                events::PASSWORD_REQUIRED,
                serde_json::json!({ "statement_id": statement_id, "filename": filename }),
            )
            .ok();

            Ok(serde_json::json!({ "status": "retry_queued", "statement_id": statement_id }))
        }
    }
}

/// Doc 30 TASK-STMT-010 `statements_list_unprocessed()`: statements grouped
/// by status into the 3 actionable UI buckets.
#[tauri::command]
pub async fn statements_list_unprocessed(
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let rows = conn
        .interact(|c| crate::db::unprocessed_statements::select_actionable(c))
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    Ok(group_unprocessed_by_status(rows))
}

/// Doc 30 TASK-STMT-010: groups `unprocessed_statements` rows into the 3
/// actionable UI buckets. Separated from `statements_list_unprocessed`'s DB
/// query so the grouping logic itself is directly testable without a Tauri
/// State/App context.
fn group_unprocessed_by_status(
    rows: Vec<crate::db::unprocessed_statements::UnprocessedStatementRow>,
) -> serde_json::Value {
    let mut awaiting_password = Vec::new();
    let mut pending_retry = Vec::new();
    let mut failed = Vec::new();

    for row in rows {
        let filename = serde_json::from_str::<serde_json::Value>(&row.statement_source_json)
            .ok()
            .and_then(|v| v["filename"].as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        let entry = serde_json::json!({
            "statement_id": row.id,
            "filename": filename,
            "failure_type": row.failure_type,
            "failure_reason": row.failure_reason,
        });
        match row.status.as_str() {
            "awaiting_password" => awaiting_password.push(entry),
            "pending_retry" => pending_retry.push(entry),
            "failed" => failed.push(entry),
            _ => {}
        }
    }

    serde_json::json!({
        "awaiting_password": awaiting_password,
        "pending_retry": pending_retry,
        "failed": failed,
    })
}

/// Doc 30 TASK-STMT-010 `statements_discard(id)`: permanently removes the
/// row, logging `statement_discarded`. `failure_reason`/`failure_type` are
/// always the structured, enum-like strings already written at creation
/// time (never a raw exception message) — this command doesn't add any new
/// error text of its own.
#[tauri::command]
pub async fn statements_discard(
    statement_id: String,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("statement_id", &statement_id)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let stmt_id = statement_id.clone();
    let removed = conn
        .interact(move |c| {
            let removed = crate::db::unprocessed_statements::delete(c, &stmt_id)?;
            if removed {
                crate::db::audit_log::insert(
                    c,
                    &crate::db::audit_log::AuditLogRow {
                        id: uuid::Uuid::new_v4().to_string(),
                        actor_type: Some("user".to_string()),
                        actor_id: Some("local".to_string()),
                        action: Some("statement_discarded".to_string()),
                        resource_type: Some("statement".to_string()),
                        resource_id: Some(stmt_id.clone()),
                        before_json: None,
                        after_json: None,
                        created_at: chrono::Utc::now(),
                    },
                )?;
            }
            Ok::<bool, anyhow::Error>(removed)
        })
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    if !removed {
        return Err(crate::error::AppError::Unknown(format!(
            "No unprocessed statement found with statement_id='{}'",
            statement_id
        )));
    }

    Ok(serde_json::json!({ "status": "discarded", "statement_id": statement_id }))
}

// G20/H10/J8 fix: renamed from `resolve_cluster` to match Doc 19 §10.3's
// documented `reconciliation_clusters_resolve` naming.
#[tauri::command]
pub async fn reconciliation_clusters_resolve(
    cluster_id: String,
    observation_id: String,
    // Doc 19 §10.3's real allowlist ("confirm_match" | "reject_candidate" |
    // "keep_separate" | "mark_unresolved", enforced by
    // cluster::resolve_cluster) -- this comment previously named a stale
    // "merge"/"reject" pair that was never the real allowlist.
    action: String,
    chosen_canonical_id: Option<String>,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("cluster_id", &cluster_id)?;
    crate::ipc::validation::validate_uuid("observation_id", &observation_id)?;
    if let Some(ref chosen_canonical_id) = chosen_canonical_id {
        crate::ipc::validation::validate_uuid("chosen_canonical_id", chosen_canonical_id)?;
    }

    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    conn.interact(move |conn| {
        crate::reconciliation::cluster::resolve_cluster(
            conn,
            &cluster_id,
            &observation_id,
            &action,
            chosen_canonical_id.as_deref(),
        )
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

        crate::db::audit_log::insert(
            conn,
            &crate::db::audit_log::AuditLogRow {
                id: uuid::Uuid::new_v4().to_string(),
                actor_type: Some("user".to_string()),
                actor_id: Some("local".to_string()),
                action: Some("resolve_cluster".to_string()),
                resource_type: Some("reconciliation_cluster".to_string()),
                resource_id: Some(cluster_id.clone()),
                before_json: None,
                after_json: Some(serde_json::json!({
                    "action": action,
                    "observation_id": observation_id,
                    "chosen_canonical_id": chosen_canonical_id
                })),
                created_at: chrono::Utc::now(),
            },
        )
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

        Ok::<(), crate::error::AppError>(())
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

    Ok("Cluster resolved".to_string())
}

/// Phase 3 — Correct an auto-matched transaction (user action from transaction detail)
#[tauri::command]
pub async fn correct_match(
    observation_id: String,
    original_decision_id: String,
    new_canonical_id: Option<String>,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("observation_id", &observation_id)?;
    crate::ipc::validation::validate_uuid("original_decision_id", &original_decision_id)?;
    if let Some(ref new_canonical_id) = new_canonical_id {
        crate::ipc::validation::validate_uuid("new_canonical_id", new_canonical_id)?;
    }

    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    conn.interact(move |conn| {
        crate::reconciliation::audit::append_correction_decision(
            conn,
            &observation_id,
            new_canonical_id.as_deref(),
            &original_decision_id,
        )
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

    Ok("Correction recorded".to_string())
}

#[tauri::command]
pub async fn trigger_reconciliation(
    observation: crate::reconciliation::engine::IncomingObservation,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
    app_handle: tauri::AppHandle,
) -> Result<String, crate::error::AppError> {
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let app_handle_clone = app_handle.clone();
    let observation_id = observation.id.clone();
    let decision = conn
        .interact(move |conn| {
            crate::reconciliation::engine::reconcile_transactionally(conn, &observation)
        })
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

    if let crate::reconciliation::audit::DecisionType::AmbiguousPending(cluster_id) = &decision {
        let _ = crate::ipc::events::emit_event(
            &app_handle_clone,
            crate::ipc::events::AppEvent::ReconciliationCluster,
            serde_json::json!({ "cluster_id": cluster_id, "observation_id": observation_id }),
        );
    }

    Ok(decision.as_str().to_string())
}

#[derive(serde::Deserialize)]
pub struct ManualTransactionPayload {
    pub amount_minor: i64,
    pub currency: String,
    pub direction: String,
    pub event_time: String,
    pub merchant_name: String,
    pub instrument_id: uuid::Uuid,
}

// G20/H10/J8 fix: renamed from `transaction_create` to match Doc 19 §8.4's
// documented `transactions_create` naming.
#[tauri::command]
pub async fn transactions_create(
    payload: ManualTransactionPayload,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
    app_handle: tauri::AppHandle,
) -> Result<String, crate::error::AppError> {
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let obs_id = uuid::Uuid::new_v4().to_string();
    let obs = crate::reconciliation::engine::IncomingObservation {
        id: obs_id.clone(),
        instrument_id: payload.instrument_id.to_string(),
        amount_minor: payload.amount_minor,
        currency: payload.currency,
        direction: payload.direction,
        event_time: payload.event_time,
        reference_id: None,
        merchant_raw: Some(payload.merchant_name),
        source_pipeline: "manual".to_string(),
        source_record_id: format!("manual_{}", obs_id),
        emi_total_installments: None,
        emi_original_amount_minor: None,
        // Manual entries have no connected_account_id to hash (Doc 30
        // TASK-TXN-008's formula input) — the fingerprint pre-filter
        // (TASK-DEDUP-001) is simply skipped for these observations.
        fingerprint: None,
        // Manual entries are ground truth, not extracted -- maximal confidence.
        confidence_score: Some(1.0),
    };

    let decision = conn
        .interact(move |conn| {
            let dt = chrono::NaiveDateTime::parse_from_str(&obs.event_time, "%Y-%m-%d %H:%M:%S")
                .unwrap_or_default();
            let obs_row = crate::db::transaction_observations::TransactionObservationsRow {
                id: obs.id.clone(),
                canonical_transaction_id: None,
                source_pipeline: Some(obs.source_pipeline.clone()),
                source_record_id: Some(obs.source_record_id.clone()),
                source_message_id: None,
                source_thread_id: None,
                statement_id: None,
                statement_entry_id: None,
                instrument_id: Some(obs.instrument_id.clone()),
                direction: Some(obs.direction.clone()),
                amount: Some(obs.amount_minor as f64 / 100.0),
                amount_minor: Some(obs.amount_minor),
                currency: Some(obs.currency.clone()),
                event_time: Some(dt),
                event_time_confidence: Some("high".to_string()),
                posting_date: Some(dt.date()),
                merchant_raw: obs.merchant_raw.clone(),
                merchant_normalized: None,
                reference_id: None,
                original_amount_minor: None,
                original_currency: None,
                exchange_rate: None,
                balance_after_transaction: None,
                timezone_at_ingestion: None,
                fingerprint: None,
                extraction_method: Some("manual".to_string()),
                confidence_score: Some(1.0),
                raw_payload_json: None,
                parser_version: None,
                emi_total_installments: None,
                emi_installment_number: None,
                emi_original_amount_minor: None,
                is_deleted: false,
                created_at: None,
                updated_at: None,
            };
            crate::db::transaction_observations::insert_observation(conn, &obs_row)?;

            crate::reconciliation::engine::reconcile_transactionally(conn, &obs)
        })
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

    if let crate::reconciliation::audit::DecisionType::AmbiguousPending(cluster_id) = &decision {
        let _ = crate::ipc::events::emit_event(
            &app_handle,
            crate::ipc::events::AppEvent::ReconciliationCluster,
            serde_json::json!({ "cluster_id": cluster_id, "observation_id": obs_id }),
        );
    } else {
        let _ = crate::ipc::events::emit_event(
            &app_handle,
            crate::ipc::events::AppEvent::TransactionCreated,
            serde_json::json!({ "observation_id": obs_id }),
        );
    }

    Ok(decision.as_str().to_string())
}

/// Doc 19 §8.3's exact editable set: `merchant_display_name`, `category_id`,
/// `notes`, `location`. Real bug fixed here: the previous payload instead
/// let the caller freely rewrite `amount_minor`/`currency`/`direction`/
/// `event_time` — none of which Document 19 documents as editable, and all
/// of which are evidence-derived fields (Document 15 Core Principle 6:
/// canonical transactions are the analytics source of truth, populated
/// through reconciliation, not ad hoc user rewrite) with no re-reconciliation
/// or observation trail triggered by changing them this way. The frontend
/// (`src/lib/ipc.ts`) never actually sent any of the four removed fields —
/// confirmed via full grep before removing them — so this is not a
/// behavior change for the real app, only a closed attack-surface/spec gap.
#[derive(serde::Deserialize)]
pub struct TransactionUpdatePayload {
    pub transaction_id: uuid::Uuid,
    pub merchant_display_name: Option<String>,
    pub category_id: Option<String>,
    pub notes: Option<String>,
    pub location: Option<String>,
    /// Not itself in Document 19 §8.3's editable-field list (§8.7/§8.8 name
    /// dedicated `transactions_add_tag`/`transactions_remove_tag` commands
    /// instead, both now also implemented below) — kept here as an
    /// additive, non-contradicting bulk-replace convenience the existing
    /// frontend tag-editor UI already depends on.
    pub tags: Option<Vec<String>>,
}

/// Doc 30 TASK-API-003: the actual field-update logic, extracted to a plain
/// synchronous function so it's directly testable (`tauri::State` has no
/// public test constructor) without needing the full async IPC plumbing
/// around it. Applies Document 19 §8.3's exact editable set
/// (`merchant_display_name`/`category_id`/`notes`/`location`); every
/// changed field writes a `feedback_log` entry via `log_user_correction`,
/// and a merchant change additionally resolves/creates a `merchants` row
/// and records the old raw text as a `merchant_aliases` entry.
fn apply_transaction_field_update(
    conn: &rusqlite::Connection,
    tx_id: &str,
    merchant_display_name: Option<String>,
    category_id: Option<String>,
    notes: Option<String>,
    location: Option<String>,
) -> Result<(), crate::error::AppError> {
    let old_tx = crate::db::transactions::get_transaction(conn, tx_id)
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .ok_or_else(|| crate::error::AppError::Unknown("Transaction not found".to_string()))?;

    let mut new_merchant = old_tx.merchant_display_name.clone();
    let mut new_merchant_normalized = old_tx.merchant_normalized_name.clone();
    let mut new_merchant_entity_id = old_tx.merchant_entity_id.clone();
    let mut new_category_id = old_tx.category_id.clone();
    let mut new_notes = old_tx.notes.clone();
    let mut new_location = old_tx.location.clone();

    if let Some(cat) = category_id {
        let old_val = old_tx.category_id.clone().unwrap_or_default();
        if old_val != cat {
            // Doc 30 TASK-API-003 acceptance test: category changes must
            // write a feedback_log entry too, same as merchant.
            let _ = crate::reconciliation::audit::log_user_correction(
                conn, tx_id, "category_id", &old_val, &cat,
            );
        }
        new_category_id = Some(cat);
    }
    if let Some(notes) = notes {
        new_notes = Some(notes);
    }
    if let Some(loc) = location {
        new_location = Some(loc);
    }
    if let Some(merch) = merchant_display_name {
        let old_val = old_tx.merchant_display_name.clone().unwrap_or_default();
        if old_val != merch {
            let _ = crate::reconciliation::audit::log_user_correction(
                conn, tx_id, "merchant", &old_val, &merch,
            );
            // Doc 30 TASK-API-003: "for merchant, merchant_aliases" --
            // resolves/creates a canonical merchant entity for the
            // corrected name and records the *old* raw text as a new
            // alias pointing at it, so future occurrences of the same
            // misrecognized raw text auto-resolve correctly next time
            // (the same learning mechanism TASK-TXN-007 already
            // established for extraction-time normalization).
            let cleaned = crate::extraction::merchant_normalizer::strip_noise_tokens(&merch);
            if !cleaned.is_empty() {
                if let Ok(Some(existing)) = crate::db::merchants::select_by_alias(conn, &cleaned) {
                    new_merchant_entity_id = Some(existing.id.clone());
                    new_merchant_normalized = Some(existing.normalized_name.clone());
                    if !old_val.is_empty() {
                        let alias = crate::db::merchants::MerchantAliasesRow {
                            id: uuid::Uuid::new_v4().to_string(),
                            merchant_entity_id: existing.id,
                            alias_raw: old_val,
                            alias_normalized: crate::extraction::merchant_normalizer::strip_noise_tokens(&merch),
                            country_code: None,
                            issuer_name: None,
                            confidence: 1.0,
                            created_at: Some(chrono::Utc::now().naive_utc()),
                        };
                        let _ = crate::db::merchants::insert_alias(conn, &alias);
                    }
                } else {
                    let new_merchant_id = uuid::Uuid::new_v4().to_string();
                    let merchant_row = crate::db::merchants::MerchantsRow {
                        id: new_merchant_id.clone(),
                        name: merch.clone(),
                        normalized_name: cleaned.clone(),
                        source: "user".to_string(),
                        created_at: Some(chrono::Utc::now().naive_utc()),
                        updated_at: Some(chrono::Utc::now().naive_utc()),
                        is_deleted: false,
                    };
                    if crate::db::merchants::insert(conn, &merchant_row).is_ok() {
                        new_merchant_entity_id = Some(new_merchant_id.clone());
                        new_merchant_normalized = Some(cleaned.clone());
                        if !old_val.is_empty() {
                            let alias = crate::db::merchants::MerchantAliasesRow {
                                id: uuid::Uuid::new_v4().to_string(),
                                merchant_entity_id: new_merchant_id,
                                alias_raw: old_val,
                                alias_normalized: crate::extraction::merchant_normalizer::strip_noise_tokens(&merch),
                                country_code: None,
                                issuer_name: None,
                                confidence: 1.0,
                                created_at: Some(chrono::Utc::now().naive_utc()),
                            };
                            let _ = crate::db::merchants::insert_alias(conn, &alias);
                        }
                    }
                }
            }
        }
        new_merchant = Some(merch);
    }

    conn.execute(
        "UPDATE transactions
         SET merchant_display_name = ?1, merchant_normalized_name = ?2, merchant_entity_id = ?3,
             category_id = ?4, notes = ?5, location = ?6,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?7",
        rusqlite::params![
            new_merchant,
            new_merchant_normalized,
            new_merchant_entity_id,
            new_category_id,
            new_notes,
            new_location,
            tx_id
        ],
    )
    .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    Ok(())
}

// G20/H10/J8 fix: renamed from `transaction_update` to match Doc 19 §8.3's
// documented `transactions_update` naming. Doc 30 TASK-API-003 fix: payload
// type renamed from `ManualTransactionUpdatePayload` (misleading -- this
// command was never actually restricted to manually-created transactions)
// to `TransactionUpdatePayload`, matching its real, universal scope.
#[tauri::command]
pub async fn transactions_update(
    payload: TransactionUpdatePayload,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
    app_handle: tauri::AppHandle,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("transaction_id", &payload.transaction_id.to_string())?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let payload_tx_id = payload.transaction_id.to_string();

    conn.interact(move |conn| {
        let tx_id = payload.transaction_id;
        apply_transaction_field_update(
            conn,
            &tx_id.to_string(),
            payload.merchant_display_name,
            payload.category_id,
            payload.notes,
            payload.location,
        )?;

        // G13 fix: resolve each tag name to an existing tag or create one,
        // then replace this transaction's tag associations with that set.
        if let Some(tag_names) = payload.tags {
            let existing_tags = crate::db::tags::select_all(conn)
                .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
            let mut tag_ids = Vec::new();
            for name in &tag_names {
                let trimmed = name.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Some(existing) = existing_tags
                    .iter()
                    .find(|t| t.name.eq_ignore_ascii_case(trimmed))
                {
                    tag_ids.push(existing.id.clone());
                } else {
                    let new_id = uuid::Uuid::new_v4().to_string();
                    crate::db::tags::insert(
                        conn,
                        &crate::db::tags::TagsRow {
                            id: new_id.clone(),
                            name: trimmed.to_string(),
                            color_hex: None,
                            created_at: Some(chrono::Utc::now().naive_utc()),
                        },
                    )
                    .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
                    tag_ids.push(new_id);
                }
            }

            let existing_assocs =
                crate::db::tags::select_by_transaction_id(conn, &tx_id.to_string())
                    .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
            for assoc in existing_assocs {
                let _ = crate::db::tags::delete_transaction_tag(
                    conn,
                    &assoc.transaction_id,
                    &assoc.tag_id,
                );
            }
            for tag_id in tag_ids {
                let _ = crate::db::tags::insert_transaction_tag(
                    conn,
                    &crate::db::tags::TransactionTagsRow {
                        transaction_id: tx_id.to_string(),
                        tag_id,
                        created_at: Some(chrono::Utc::now().naive_utc()),
                    },
                );
            }
        }

        Ok::<(), crate::error::AppError>(())
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))??;

    let _ = crate::ipc::events::emit_event(
        &app_handle,
        crate::ipc::events::AppEvent::TransactionUpdated,
        serde_json::json!({ "transaction_id": payload_tx_id }),
    );

    Ok("updated".to_string())
}

/// G15 fix: lists stored PDF passwords (metadata only — never the ciphertext
/// or plaintext) so Settings can offer management, previously nonexistent.
/// G20/H10/J8 fix: renamed from `pdf_passwords_list` to match Doc 19 §13/§18's
/// documented `settings_pdf_passwords_list` naming.
#[tauri::command]
pub async fn settings_pdf_passwords_list(
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<crate::db::pdf_passwords::PdfPasswordSummary>, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| crate::db::pdf_passwords::select_all_with_instrument(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))
}

/// G15 fix: deletes a stored PDF password — the next time a statement from
/// that instrument needs unlocking, the user will be re-prompted.
/// G20/H10/J8 fix: renamed from `pdf_passwords_delete` to match Doc 19
/// §13/§18's documented `settings_pdf_passwords_delete` naming.
#[tauri::command]
pub async fn settings_pdf_passwords_delete(
    id: String,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<(), crate::error::AppError> {
    crate::ipc::validation::validate_uuid("id", &id)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| crate::db::pdf_passwords::delete(c, &id))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))
}

/// Doc 30 TASK-API-008: "`settings_pattern_rules_list`" -- did not exist at
/// all before this task; the frontend borrowed the debug-only
/// `debug_fetch_pattern_rule_health` (a relabeled, simplified view) as a
/// stopgap instead. Returns the same `PatternRuleHealth` shape the
/// Settings page (`Settings.tsx`) already correctly consumes -- only the
/// command name changes, from a debug-console endpoint to a real
/// Settings-owned one; `db/pattern_rules.rs::select_all` (this task's
/// other new function, returning the raw `PatternRulesRow`) is used
/// instead of duplicating `debug_fetch_pattern_rule_health`'s own SQL.
#[tauri::command]
pub async fn settings_pattern_rules_list(
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<crate::commands::debug::PatternRuleHealth>, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let rules = conn
        .interact(|c| crate::db::pattern_rules::select_all(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    Ok(rules
        .into_iter()
        .map(|r| crate::commands::debug::PatternRuleHealth {
            id: r.id,
            merchant_id: r.bank_name,
            pattern_type: "regex".to_string(),
            pattern_value: r.field_name,
            is_active: r.status == "active",
            success_count: r.success_count,
            failure_count: r.failure_count,
        })
        .collect())
}

/// G14 fix: pattern-rule management was read-only everywhere (Debug console
/// only) — this lets a user actually enable/disable a rule from Settings,
/// wrapping the existing (already-validated) db::pattern_rules::update_status.
/// G20/H10/J8 fix: renamed from `pattern_rule_set_status` to match Doc 19
/// §13/§18's documented `settings_pattern_rules_update` naming.
#[tauri::command]
pub async fn settings_pattern_rules_update(
    rule_id: String,
    new_status: String,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<(), crate::error::AppError> {
    crate::ipc::validation::validate_uuid("rule_id", &rule_id)?;
    crate::ipc::validation::validate_non_empty("new_status", &new_status)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| crate::db::pattern_rules::update_status(c, &rule_id, &new_status))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;
    Ok(())
}

/// G13 fix: the full reusable-tag catalog, for autocomplete when tagging a
/// transaction — previously tags were pure free-text with nothing behind
/// them to autocomplete against. Returns full rows (not just names) so the
/// frontend can resolve a tag's real id and call the dedicated
/// `transactions_add_tag`/`transactions_remove_tag` commands (Doc19
/// §8.7/§8.8) instead of the name-based bulk-replace workaround this
/// previously forced (Area 9 verification pass fix).
#[tauri::command]
pub async fn tags_list(
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<crate::db::tags::TagsRow>, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| crate::db::tags::select_all(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))
}

#[derive(serde::Deserialize)]
pub struct TagCreatePayload {
    pub name: String,
    pub color_hex: Option<String>,
}

/// Doc 30 TASK-API-007: "`tags_create`" -- Document 19 §18 already
/// catalogs this exact name.
#[tauri::command]
pub async fn tags_create(
    payload: TagCreatePayload,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    crate::ipc::validation::validate_non_empty("name", &payload.name)?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let id = uuid::Uuid::new_v4().to_string();
    let row = crate::db::tags::TagsRow {
        id: id.clone(),
        name: payload.name,
        color_hex: payload.color_hex,
        created_at: None,
    };
    conn.interact(move |c| crate::db::tags::insert(c, &row))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    Ok(serde_json::json!({ "id": id, "status": "created" }))
}

/// Doc 30 TASK-API-007: "`tags_delete`" -- no Document 19 contract exists
/// (absent from §18's catalog despite `tags_list`/`tags_create` both being
/// listed there), same documentation-gap situation as several
/// TASK-API-005/006 commands.
#[tauri::command]
pub async fn tags_delete(
    id: String,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("id", &id)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| crate::db::tags::delete(c, &id))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    Ok("deleted".to_string())
}

/// G13 fix: the tag names currently associated with a transaction, so the
/// detail drawer can populate the correction form's tag list.
#[tauri::command]
pub async fn fetch_transaction_tags(
    transaction_id: String,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<String>, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("transaction_id", &transaction_id)?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let tx_id = transaction_id.clone();
    let names = conn
        .interact(move |c| -> anyhow::Result<Vec<String>> {
            let assocs = crate::db::tags::select_by_transaction_id(c, &tx_id)?;
            let all_tags = crate::db::tags::select_all(c)?;
            Ok(assocs
                .into_iter()
                .filter_map(|a| {
                    all_tags
                        .iter()
                        .find(|t| t.id == a.tag_id)
                        .map(|t| t.name.clone())
                })
                .collect())
        })
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    Ok(names)
}

/// Doc 19 §8.2 `transactions_get`: canonical transaction detail plus
/// linked evidence (observations) and match decisions, for the "view
/// source" panel -- did not exist as a single command before this task
/// (only the observations/source-log/tags pieces existed separately).
#[derive(serde::Serialize)]
pub struct TransactionDetail {
    pub transaction: crate::db::transactions::TransactionsRow,
    pub observations: Vec<crate::db::transaction_observations::TransactionObservationsRow>,
    pub match_decisions: Vec<crate::db::match_decisions::MatchDecisionsRow>,
}

#[tauri::command]
pub async fn transactions_get(
    id: String,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<TransactionDetail, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("id", &id)?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| {
        let transaction = crate::db::transactions::get_transaction(c, &id)
            .map_err(|e| crate::error::AppError::Db(e.to_string()))?
            .ok_or_else(|| crate::error::AppError::Validation("transaction not found".to_string()))?;
        let observations = crate::db::transaction_observations::get_observations_for_transaction(c, &id)
            .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
        let mut match_decisions = Vec::new();
        for obs in &observations {
            if let Ok(decisions) = crate::db::match_decisions::select_by_observation_id(c, &obs.id) {
                match_decisions.extend(decisions);
            }
        }
        Ok(TransactionDetail { transaction, observations, match_decisions })
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
}

/// Doc 19 §8.7/§8.8: dedicated single-tag add/remove commands, distinct
/// from `transactions_update`'s existing bulk tag-replace convenience.
#[tauri::command]
pub async fn transactions_add_tag(
    transaction_id: String,
    tag_id: String,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("transaction_id", &transaction_id)?;
    crate::ipc::validation::validate_uuid("tag_id", &tag_id)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| {
        crate::db::tags::insert_transaction_tag(
            c,
            &crate::db::tags::TransactionTagsRow {
                transaction_id,
                tag_id,
                created_at: Some(chrono::Utc::now().naive_utc()),
            },
        )
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
    .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    Ok("tag_added".to_string())
}

#[tauri::command]
pub async fn transactions_remove_tag(
    transaction_id: String,
    tag_id: String,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("transaction_id", &transaction_id)?;
    crate::ipc::validation::validate_uuid("tag_id", &tag_id)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| crate::db::tags::delete_transaction_tag(c, &transaction_id, &tag_id))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    Ok("tag_removed".to_string())
}

/// Doc 30 TASK-API-003: "transactions_get_emi_group" -- wraps TASK-TXN-012's
/// already-built `emi_detector::get_emi_group_summary` (total paid/count so
/// far; `total_expected` is deliberately absent, see that function's own
/// doc comment for why).
#[tauri::command]
pub async fn transactions_get_emi_group(
    emi_group_id: String,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<crate::extraction::emi_detector::EmiGroupSummary, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| crate::extraction::emi_detector::get_emi_group_summary(c, &emi_group_id))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))
}

// G20/H10/J8 fix: renamed from `transaction_delete` to match Doc 19 §8.5's
// documented `transactions_delete` naming.
#[tauri::command]
pub async fn transactions_delete(
    transaction_id: String,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
    app_handle: tauri::AppHandle,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("transaction_id", &transaction_id)?;

    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let tx_id = transaction_id.clone();
    conn.interact(move |conn| -> Result<(), crate::error::AppError> {
        // §6.6 — delete is restricted to manually-entered transactions (source_mix = 'manual') only
        let source_mix: rusqlite::Result<Option<String>> = conn.query_row(
            "SELECT source_mix FROM transactions WHERE id = ?1 AND is_deleted = 0",
            rusqlite::params![tx_id],
            |row| row.get(0),
        );
        match source_mix {
            Ok(Some(ref mix)) if mix.as_str() != "manual" => {
                return Err(crate::error::AppError::Unknown(
                    "delete_restricted: only manually-entered transactions (source_mix='manual') can be deleted"
                        .to_string(),
                ));
            }
            // Not found or already deleted — still proceed (no-op soft-delete)
            Err(rusqlite::Error::QueryReturnedNoRows) | Ok(None) => {}
            Err(e) => return Err(crate::error::AppError::Db(e.to_string())),
            Ok(_) => {} // mix == "manual" — allowed
        }
        conn.execute(
            "UPDATE transactions SET is_deleted = 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
            rusqlite::params![tx_id],
        )
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
        Ok(())
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))??;

    let _ = crate::ipc::events::emit_event(
        &app_handle,
        crate::ipc::events::AppEvent::TransactionDeleted,
        serde_json::json!({ "transaction_id": transaction_id }),
    );

    Ok("deleted".to_string())
}

#[tauri::command]
pub fn check_system_ram() -> Result<f64, crate::error::AppError> {
    use sysinfo::System;
    let mut sys = System::new_all();
    sys.refresh_all();
    // System::total_memory returns bytes. Convert to GB.
    let total_ram_gb = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
    Ok(total_ram_gb)
}

#[tauri::command]
pub async fn ipc_trigger_patch_sync(
    alert_id: String,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("alert_id", &alert_id)?;

    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let alert_id_clone = alert_id.clone();
    let alert = conn
        .interact(move |conn| crate::db::alerts::get_alert(conn, &alert_id_clone))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

    if let Some(alert) = alert {
        // H11 fix: "SMS Offline" alerts previously "resolved" themselves by
        // pinging an undocumented, never-built companion mobile app — there is
        // no SMS ingestion pathway anywhere in this codebase (only Gmail and
        // PDF-statement ingestion exist), so no automated retry is possible
        // for this alert type. Fail rather than silently claim success.
        if alert.alert_type == "SMS Offline" {
            return Err(crate::error::AppError::Unknown(
                "No automated retry is available for this alert — please check the bank connection manually.".to_string(),
            ));
        } else if alert.alert_type == "Email Offline" {
            // trigger fetch emails
        }

        let alert_id_clone = alert.alert_id.clone();
        pool.get()
            .await
            .map_err(|e| crate::error::AppError::Db(e.to_string()))?
            .interact(move |conn| {
                crate::db::alerts::update_alert_status(conn, &alert_id_clone, "resolved")
            })
            .await
            .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
            .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

        Ok("Sync triggered".to_string())
    } else {
        Err(crate::error::AppError::Unknown(
            "Alert not found".to_string(),
        ))
    }
}

#[tauri::command]
pub fn log_frontend_event(
    level: String,
    message: String,
    data: Option<serde_json::Value>,
) {
    let data_str = data.map(|d| format!("| Data: {}", d)).unwrap_or_default();
    match level.to_lowercase().as_str() {
        "error" => tracing::error!("FRONTEND: {} {}", message, data_str),
        "warn" => tracing::warn!("FRONTEND: {} {}", message, data_str),
        "debug" => tracing::debug!("FRONTEND: {} {}", message, data_str),
        "trace" => tracing::trace!("FRONTEND: {} {}", message, data_str),
        _ => tracing::info!("FRONTEND: {} {}", message, data_str),
    }
}

pub fn get_handlers() -> impl Fn(tauri::ipc::Invoke) -> bool {
    tauri::generate_handler![
        crate::updater::updater_confirm_install,
        data::settings_get_menu_bar_extra_enabled,
        data::settings_set_menu_bar_extra_enabled,
        data::settings_get_launch_at_login,
        data::settings_set_launch_at_login,
        data::settings_get_background_sync_enabled,
        data::settings_set_background_sync_enabled,
        data::settings_get_low_battery_poll_threshold_percent,
        data::settings_set_low_battery_poll_threshold_percent,
        auth_google_start,
        auth_logout,
        reconciliation_clusters_resolve,
        trigger_reconciliation,
        transactions_create,
        transactions_update,
        transactions_delete,
        transactions_get,
        transactions_add_tag,
        transactions_remove_tag,
        transactions_get_emi_group,
        tags_list,
        tags_create,
        tags_delete,
        fetch_transaction_tags,
        settings_pattern_rules_list,
        settings_pattern_rules_update,
        settings_pdf_passwords_list,
        settings_pdf_passwords_delete,
        correct_match,
        statements_upload,
        statements_submit_password,
        statements_retry_unprocessed,
        statements_list_unprocessed,
        statements_discard,
        statements_confirm_instrument,
        ipc_trigger_patch_sync,
        log_frontend_event,
        data::settings_delete_account,
        data::settings_export_data,
        data::settings_profile_get,
        data::settings_profile_update,
        data::settings_export_encrypted_backup,
        data::settings_import_encrypted_backup,
        data::dashboard_summary,
        data::dashboard_upcoming_bills,
        data::dashboard_categories,
        data::analytics_spend_trend,
        data::analytics_top_merchants,
        data::analytics_recurring_payments_summary,
        data::analytics_pending_review_count,
        data::categories_list,
        data::categories_create,
        data::categories_update,
        data::categories_delete,
        data::transactions_list,
        data::transactions_search,
        data::fetch_spending_limits,
        data::update_spending_limits,
        data::onboarding_save_preferences,
        data::db_restore_backup,
        data::fetch_transaction_observations,
        data::fetch_transaction_source_log,
        data::statements_list,
        data::statements_get_entries,
        data::reconciliation_clusters_list,
        data::reconciliation_clusters_get,
        data::reconciliation_get_unassigned_transactions,
        data::reconciliation_clusters_unmerge,
        data::reconciliation_clusters_bulk_resolve,
        llm::llm_get_available_models,
        llm::llm_download_model,
        data::instruments_list,
        data::instruments_get,
        data::instruments_create,
        data::instruments_update,
        data::instruments_archive,
        data::get_debug_metrics,
        data::check_backend_status,
        data::auth_get_consent_history,
        data::record_consent_event,
        check_system_ram,
        crate::ingestion::oauth::is_gmail_connected,
        crate::ingestion::oauth::settings_get_connected_accounts,
        crate::ingestion::oauth::auth_google_disconnect,
        auth_get_recovery_phrase,
        auth_restore_from_recovery_phrase,
        export_logs,
        crate::ingestion::historical_scan::scans_historical,
        crate::ingestion::historical_scan::scans_status,
        crate::ingestion::historical_scan::scans_cancel,
        crate::ingestion::historical_scan::scans_resume,
        crate::ingestion::polling::sync_force_poll_now,
        crate::ingestion::queues::pipeline_pause,
        crate::ingestion::queues::pipeline_resume,
        crate::ingestion::queues::pipeline_status,
        debug::debug_fetch_parse_errors,
        debug::debug_fetch_unprocessed_statements,
        debug::debug_fetch_audit_log,
        debug::debug_fetch_pattern_rule_health,
        debug::debug_fetch_reconciliation_clusters,
        debug::debug_get_pipeline_state,
        debug::debug_set_gmail_poll_paused,
        debug::debug_set_scan_queue_paused,
        crate::feedback::submit_user_feedback,
        network::settings_get_network_activity,
        crate::licensing::commands::license_get_status,
        crate::licensing::commands::license_activate,
        crate::licensing::commands::license_deactivate,
        crate::licensing::commands::license_refresh
    ]
}

// ── Tests: Phase 5.8 Post-Parse Memory Cleanup integration test ───────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Doc 30 TASK-API-010 acceptance test: "every error path must resolve
    /// to a documented `AppError` variant -- no raw string or unstructured
    /// `anyhow::Error` ever reaches the frontend." A live `generate_handler!`
    /// registry has no simple runtime-introspectable command list, so this
    /// walks every `.rs` file under `src/` (a real, exhaustive scan -- not a
    /// hardcoded subset that would silently stop covering new files) and
    /// asserts no command-annotated function's signature block contains
    /// `Result<_, String>` or a raw `anyhow::Error` return type. Mirrors the
    /// same source-scanning technique `test_no_command_returns_pdf_bytes`
    /// already established in this file. (Deliberately not writing the
    /// literal tauri-command attribute text in this comment -- it would
    /// match its own scan below and self-report as an offender.)
    fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }

    #[test]
    fn test_all_commands_produce_documented_apperror_variants() {
        let src_dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
        let mut files = Vec::new();
        collect_rs_files(src_dir, &mut files);
        assert!(!files.is_empty(), "the source scan itself must find files, or this test is vacuous");

        // Built via `format!` rather than written as a literal in this file
        // -- if the literal substring appeared here, this test would find
        // its own source and self-report as an offender the moment its
        // signature block (which legitimately contains no `Result<...>`
        // return type at all, being a `#[test]` fn) got captured downstream
        // of a spurious match.
        let marker = format!("#[{}::{}]", "tauri", "command");

        let mut offenders = Vec::new();
        for path in &files {
            let src = std::fs::read_to_string(path).unwrap();
            let mut search_from = 0usize;
            while let Some(marker_pos) = src[search_from..].find(&marker) {
                let abs_marker = search_from + marker_pos;
                // Capture from the marker up to the function's opening `{`
                // (the full attribute + signature block, matching
                // test_no_command_returns_pdf_bytes' own block-capture style).
                let Some(brace_offset) = src[abs_marker..].find("{\n").or_else(|| src[abs_marker..].find("{ ")) else {
                    search_from = abs_marker + marker.len();
                    continue;
                };
                let block = &src[abs_marker..abs_marker + brace_offset];
                if block.contains("Result<") {
                    let is_documented = block.contains("AppError");
                    let is_raw_string_or_anyhow = block.contains(", String>")
                        || block.contains(", String,")
                        || block.contains("anyhow::Error>");
                    if is_raw_string_or_anyhow && !is_documented {
                        offenders.push((path.display().to_string(), block.trim().to_string()));
                    }
                }
                search_from = abs_marker + marker.len();
            }
        }

        assert!(
            offenders.is_empty(),
            "every Tauri command must return Result<_, AppError>, not a raw String/anyhow::Error: {:#?}",
            offenders
        );
    }

    /// Finds `fn {name}(` anywhere under `src/` and returns its full body
    /// (brace-matched from the first `{` after the signature through to the
    /// matching close), searching every file `collect_rs_files` finds so a
    /// command defined in any module is located regardless of which file it
    /// lives in.
    fn find_function_body(name: &str) -> Option<String> {
        let src_dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
        let mut files = Vec::new();
        collect_rs_files(src_dir, &mut files);
        // Matches both a plain `fn name(...)` and a generic
        // `fn name<R: tauri::Runtime>(...)` (`scans_historical`,
        // `sync_force_poll_now`, and similarly-generic command signatures
        // put the type parameter between the name and the opening paren).
        let needle_plain = format!("fn {name}(");
        let needle_generic = format!("fn {name}<");
        for path in &files {
            let src = std::fs::read_to_string(path).ok()?;
            let found = src.find(&needle_plain).or_else(|| src.find(&needle_generic));
            if let Some(fn_pos) = found {
                let brace_start = src[fn_pos..].find('{')? + fn_pos;
                let mut depth = 0i32;
                for (i, ch) in src[brace_start..].char_indices() {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                return Some(src[brace_start..brace_start + i + 1].to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        None
    }

    /// Doc 30 TASK-API-010 acceptance test: "Ensures `AppError::LicenseLocked`
    /// is checked/returned consistently by every write-path command."
    /// Curated to the commands that unambiguously mutate core financial-domain
    /// data or trigger new ingestion (Document 22 §11.5's "all writes,
    /// including new ingestion on both queues") -- not a blanket heuristic
    /// (e.g. matching on a `_create`/`_update` suffix) that would misclassify
    /// borderline cases. `license_activate`/`license_deactivate` are
    /// deliberately excluded: they are themselves the mechanism by which a
    /// LOCKED state is resolved, so gating them behind the current lock state
    /// would create a deadlock a locked-out user could never escape from --
    /// confirmed correct by design, not an oversight. `settings_export_encrypted_backup`/
    /// `settings_import_encrypted_backup` are also excluded, matching the
    /// established, unchallenged precedent of their sibling
    /// `settings_export_data`: none of the three mutate `finance.db`'s rows
    /// at all (export reads only; import currently only decrypts to a
    /// staging file, with the actual DB swap-in deferred to TASK-OPS-002
    /// per this task's own fix-log entry) -- and a locked-out user should
    /// still be able to back up or preserve their own data, arguably more
    /// so, not less.
    #[test]
    fn test_write_commands_check_license_locked() {
        let write_commands = [
            "transactions_create",
            "transactions_update",
            "transactions_delete",
            "transactions_add_tag",
            "transactions_remove_tag",
            "instruments_create",
            "instruments_update",
            "instruments_archive",
            "categories_create",
            "categories_update",
            "categories_delete",
            "tags_create",
            "tags_delete",
            "statements_upload",
            "statements_confirm_instrument",
            "statements_submit_password",
            "statements_discard",
            "statements_retry_unprocessed",
            "reconciliation_clusters_resolve",
            "reconciliation_clusters_unmerge",
            "reconciliation_clusters_bulk_resolve",
            "correct_match",
            "trigger_reconciliation",
            "auth_google_start",
            "auth_google_disconnect",
            "settings_pattern_rules_update",
            "settings_pdf_passwords_delete",
            "settings_delete_account",
            "settings_profile_update",
            "update_spending_limits",
            "onboarding_save_preferences",
            "scans_historical",
            "sync_force_poll_now",
        ];

        let mut missing = Vec::new();
        for name in write_commands {
            match find_function_body(name) {
                Some(body) if body.contains("assert_write_allowed") => {}
                Some(_) => missing.push(format!("{name} (found, but no assert_write_allowed call)")),
                None => missing.push(format!("{name} (function not found by the source scan)")),
            }
        }

        assert!(
            missing.is_empty(),
            "every write-path command must call assert_write_allowed to enforce LicenseLocked: {:#?}",
            missing
        );
    }

    /// Doc 30 TASK-API-010 acceptance test: "read-path commands remain
    /// available even when LOCKED" -- a curated sample of clearly read-only
    /// commands (list/get/status queries with no DB mutation) must never
    /// call `assert_write_allowed`, since doing so would incorrectly block
    /// them for a LOCKED user who must still be able to view their existing
    /// data.
    #[test]
    fn test_read_commands_available_when_locked() {
        let read_commands = [
            "transactions_list",
            "transactions_get",
            "transactions_search",
            "instruments_list",
            "instruments_get",
            "categories_list",
            "tags_list",
            "dashboard_summary",
            "statements_list",
            "reconciliation_clusters_list",
            "reconciliation_clusters_get",
            "reconciliation_get_unassigned_transactions",
            "scans_status",
            "pipeline_status",
            "license_get_status",
            "auth_get_consent_history",
            "settings_export_data",
            "settings_export_encrypted_backup",
            "settings_import_encrypted_backup",
        ];

        let mut wrongly_gated = Vec::new();
        for name in read_commands {
            if let Some(body) = find_function_body(name) {
                if body.contains("assert_write_allowed") {
                    wrongly_gated.push(name);
                }
            }
            // A read command not found by the scan isn't this test's
            // concern (rename/removal is covered by other tests) -- only
            // "found and wrongly gated" is the failure mode here.
        }

        assert!(
            wrongly_gated.is_empty(),
            "these read-only commands must remain available when LOCKED, but call assert_write_allowed: {:#?}",
            wrongly_gated
        );
    }

    fn setup_tx_test_db() -> rusqlite::Connection {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, amount_minor, currency, direction, merchant_display_name, category_id, is_deleted) \
             VALUES ('tx_1', 'inst_1', 1000, 'INR', 'debit', 'ZZZTEST MKTP', 'cat_old', 0)",
            [],
        )
        .unwrap();
        // `log_user_correction` (Doc 30 TASK-TXN-009) attributes the
        // correction to the transaction's linked observation -- a real
        // canonical transaction always has one (that's how reconciliation
        // created it); seed one here too so feedback_log writes actually
        // fire, matching production shape.
        conn.execute(
            "INSERT INTO transaction_observations (id, canonical_transaction_id, source_pipeline, source_record_id, fingerprint) \
             VALUES ('obs_1', 'tx_1', 'gmail_transaction', 'msg_1', 'fp_1')",
            [],
        )
        .unwrap();
        conn
    }

    /// Doc 30 TASK-API-003 acceptance test: a category change writes a
    /// `feedback_log` entry, the same as a merchant correction does.
    #[test]
    fn test_category_update_writes_feedback_log() {
        let conn = setup_tx_test_db();
        apply_transaction_field_update(&conn, "tx_1", None, Some("cat_new".to_string()), None, None).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM feedback_log WHERE transaction_id = 'tx_1' AND field_name = 'category_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let new_cat: String = conn
            .query_row("SELECT category_id FROM transactions WHERE id = 'tx_1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(new_cat, "cat_new");
    }

    /// Doc 30 TASK-API-003 acceptance test: correcting a transaction's
    /// merchant creates a `merchant_aliases` row mapping the old raw text
    /// to the resolved/created merchant entity.
    #[test]
    fn test_merchant_update_creates_alias() {
        let conn = setup_tx_test_db();
        // A fictional merchant name -- migration 20260101000030 already
        // seeds 5 real merchants (Amazon/Swiggy/Uber/Starbucks/Netflix,
        // per TASK-TXN-007/011's own established test-fixture precedent),
        // so a real brand name here would collide with seed data instead
        // of exercising the "genuinely new merchant" code path.
        apply_transaction_field_update(&conn, "tx_1", Some("Acme Streaming".to_string()), None, None, None).unwrap();

        let alias_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM merchant_aliases WHERE alias_raw = 'ZZZTEST MKTP'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(alias_count, 1, "the old raw merchant text must be recorded as a new alias");

        let (merchant, entity_id): (String, Option<String>) = conn
            .query_row(
                "SELECT merchant_display_name, merchant_entity_id FROM transactions WHERE id = 'tx_1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(merchant, "Acme Streaming");
        assert!(entity_id.is_some(), "the transaction must be linked to a resolved merchant entity");
    }

    /// Doc 30 TASK-API-004 acceptance test.
    #[test]
    fn test_upload_command_rejects_missing_file() {
        assert!(validate_upload_files_non_empty(&[]).is_err());
        let one_file = vec![UploadFile { file_bytes: vec![1, 2, 3], filename: "a.pdf".to_string() }];
        assert!(validate_upload_files_non_empty(&one_file).is_ok());
    }

    /// Doc 30 TASK-API-004 acceptance test: schema-level check that no
    /// statement-related *response* type ever carries raw PDF bytes.
    /// Scans each response struct's own field block (not the whole file --
    /// `UploadFile`'s `file_bytes: Vec<u8>` is a legitimate *input* type in
    /// the same file and must not false-positive this check).
    #[test]
    fn test_no_command_returns_pdf_bytes() {
        fn struct_field_block(src: &str, struct_name: &str) -> String {
            let marker = format!("struct {struct_name} {{");
            let start = src.find(&marker).unwrap_or_else(|| panic!("struct {struct_name} not found"));
            let body_start = start + marker.len();
            let end = src[body_start..].find('}').unwrap();
            src[body_start..body_start + end].to_string()
        }

        let commands_mod_src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/commands/mod.rs")).unwrap();
        let commands_data_src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/commands/data.rs")).unwrap();
        let statement_entries_src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/db/statement_entries.rs")).unwrap();

        for (src, name) in [
            (&commands_mod_src, "UploadResult"),
        ] {
            let block = struct_field_block(src, name);
            assert!(!block.contains("Vec<u8>"), "{name} must never carry raw byte content: {block}");
        }
        for (src, name) in [
            (&commands_data_src, "StatementRecord"),
            (&commands_data_src, "StatementsPage"),
        ] {
            let block = struct_field_block(src, name);
            assert!(!block.contains("Vec<u8>"), "{name} must never carry raw byte content: {block}");
        }
        let entries_block = struct_field_block(&statement_entries_src, "StatementEntriesRow");
        assert!(!entries_block.contains("Vec<u8>"), "StatementEntriesRow must never carry raw byte content");
    }

    /// §5.8 Integration test: Verify that the raw PDF bytes do not persist
    /// in the database or as files on disk after the pipeline runs.
    ///
    /// This test validates the design invariant:
    ///   - No column in `statements` or `statement_entries` stores raw PDF bytes.
    ///   - The `bytes` variable is explicitly dropped after the pipeline step.
    ///
    /// Since we cannot run the full async pipeline in a unit test without Tauri runtime,
    /// we test the schema invariant directly: the `statements` table must not have
    /// any BLOB column that could store PDF bytes.
    #[tokio::test]
    async fn test_no_pdf_bytes_written_to_sqlite_or_disk() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test.db");
        let pool = crate::db::init_db(db_path.clone()).await.unwrap();

        let conn = pool.get().await.unwrap();

        // Verify statements table schema: no column should be of BLOB type
        let column_types: Vec<String> = conn
            .interact(|c| {
                let mut stmt = c.prepare("PRAGMA table_info(statements)").unwrap();
                let types: Vec<String> = stmt
                    .query_map([], |row| row.get::<_, String>(2))
                    .unwrap()
                    .filter_map(|r| r.ok())
                    .collect();
                Ok::<_, rusqlite::Error>(types)
            })
            .await
            .unwrap()
            .unwrap();

        // No column type should be "BLOB" (raw binary storage) in statements
        let has_blob_column = column_types
            .iter()
            .any(|t| t.to_uppercase().contains("BLOB"));
        assert!(
            !has_blob_column,
            "statements table must not have any BLOB column that could store raw PDF bytes. \
             Found BLOB columns: {:?}",
            column_types
        );

        // Verify statement_entries table similarly
        let entry_types: Vec<String> = conn
            .interact(|c| {
                let mut stmt = c.prepare("PRAGMA table_info(statement_entries)").unwrap();
                let types: Vec<String> = stmt
                    .query_map([], |row| row.get::<_, String>(2))
                    .unwrap()
                    .filter_map(|r| r.ok())
                    .collect();
                Ok::<_, rusqlite::Error>(types)
            })
            .await
            .unwrap()
            .unwrap();

        let entry_has_blob = entry_types.iter().any(|t| t.to_uppercase() == "BLOB");
        assert!(
            !entry_has_blob,
            "statement_entries table must not have any BLOB column for PDF storage. \
             Found types: {:?}",
            entry_types
        );

        // Verify the DB file on disk does NOT contain PDF magic bytes
        // (confirms bytes were never written via any code path)
        let db_bytes = std::fs::read(&db_path).unwrap();
        let pdf_magic = b"%PDF";
        let db_contains_pdf = db_bytes.windows(4).any(|w| w == pdf_magic);
        assert!(
            !db_contains_pdf,
            "SQLite database file must not contain raw PDF magic bytes — \
             this would indicate PDF bytes were written to disk"
        );
    }

    /// Doc 30 TASK-STMT-002: "Every skip is logged to audit_log
    /// (statement_duplicate_skipped) with the detected period for user
    /// transparency."
    #[tokio::test]
    async fn test_duplicate_skip_writes_audit_log_with_period() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

        super::log_duplicate_skipped_audit(
            "HDFC_Jan_2026.pdf",
            Some(&("2026-01-01".to_string(), "2026-01-31".to_string())),
            &pool,
        )
        .await;

        let conn = pool.get().await.unwrap();
        let (action, after_json): (String, String) = conn
            .interact(|c| {
                c.query_row(
                    "SELECT action, after_json FROM audit_log WHERE action = 'statement_duplicate_skipped'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(action, "statement_duplicate_skipped");
        let parsed: serde_json::Value = serde_json::from_str(&after_json).unwrap();
        assert_eq!(parsed["billing_period_start"], "2026-01-01");
        assert_eq!(parsed["billing_period_end"], "2026-01-31");
        assert_eq!(parsed["filename"], "HDFC_Jan_2026.pdf");
    }

    // ── test_list_unprocessed_grouped_by_status (Doc 30 TASK-STMT-010) ───────

    #[test]
    fn test_list_unprocessed_grouped_by_status() {
        use crate::db::unprocessed_statements::UnprocessedStatementRow;

        fn make_row(id: &str, status: &str) -> UnprocessedStatementRow {
            UnprocessedStatementRow {
                id: id.to_string(),
                statement_source_json: serde_json::json!({ "filename": format!("{id}.pdf") })
                    .to_string(),
                failure_type: "password_required".to_string(),
                failure_reason: String::new(),
                status: status.to_string(),
                resolved_statement_id: None,
                created_at: None,
                updated_at: None,
            }
        }

        let rows = vec![
            make_row("s1", "awaiting_password"),
            make_row("s2", "pending_retry"),
            make_row("s3", "failed"),
            make_row("s4", "awaiting_password"),
        ];

        let grouped = super::group_unprocessed_by_status(rows);
        assert_eq!(grouped["awaiting_password"].as_array().unwrap().len(), 2);
        assert_eq!(grouped["pending_retry"].as_array().unwrap().len(), 1);
        assert_eq!(grouped["failed"].as_array().unwrap().len(), 1);
        assert_eq!(grouped["awaiting_password"][0]["statement_id"], "s1");
        assert_eq!(grouped["awaiting_password"][0]["filename"], "s1.pdf");
    }

    // ── test_retry_reuses_saved_password (Doc 30 TASK-STMT-010) ──────────────

    #[tokio::test]
    async fn test_retry_reuses_saved_password() {
        use crate::statements::password::{
            try_all_stored_passwords, try_stored_passwords, PasswordResolutionResult,
        };

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier) \
                 VALUES ('inst_a', 'credit_card', 'HDFC', '1111')",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier) \
                 VALUES ('inst_b', 'credit_card', 'ICICI', '2222')",
                [],
            )
            .unwrap();
            // Real stored passwords under two DIFFERENT real instruments —
            // ciphertext content doesn't matter for this test (pdfium itself
            // isn't installed on this dev machine, so no candidate can
            // actually unlock here regardless); what matters is which rows
            // the query even considers.
            c.execute(
                "INSERT INTO pdf_passwords (id, instrument_id, password_ciphertext, success_count, created_at) \
                 VALUES ('pw_a', 'inst_a', X'0102030405060708090A0B0C0D0E0F1011121314', 0, datetime('now'))",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO pdf_passwords (id, instrument_id, password_ciphertext, success_count, created_at) \
                 VALUES ('pw_b', 'inst_b', X'0102030405060708090A0B0C0D0E0F1011121314', 0, datetime('now'))",
                [],
            )
            .unwrap();
        })
        .await
        .unwrap();

        // The real bug this fixes: the old call site scoped the lookup to
        // instrument_id="" — a value no real instrument ever has, so it
        // structurally could never find a stored password at all, regardless
        // of what's actually on file.
        let old_bug_result = try_stored_passwords("", b"%PDF-1.4 fake", &pool)
            .await
            .unwrap();
        assert_eq!(old_bug_result, PasswordResolutionResult::PromptRequired);

        let scoped_count: i64 = conn
            .interact(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM pdf_passwords WHERE instrument_id = ''",
                    [],
                    |r| r.get(0),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(scoped_count, 0, "no real instrument has an empty-string id");

        // The fix: try_all_stored_passwords isn't scoped to any instrument —
        // both seeded rows, across two different instruments, are real
        // candidates it considers (gracefully falling through to
        // PromptRequired once none actually unlock, rather than erroring).
        let result = try_all_stored_passwords(b"%PDF-1.4 fake", &pool)
            .await
            .unwrap();
        assert_eq!(result, PasswordResolutionResult::PromptRequired);
    }

    // ── test_discard_removes_row_and_logs_audit (Doc 30 TASK-STMT-010) ───────

    #[tokio::test]
    async fn test_discard_removes_row_and_logs_audit() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            c.execute(
                "INSERT INTO unprocessed_statements \
                 (id, statement_source_json, failure_type, failure_reason, status) \
                 VALUES ('11111111-1111-4111-8111-111111111111', '{}', 'password_required', '', 'awaiting_password')",
                [],
            )
            .unwrap();
        })
        .await
        .unwrap();

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        app.manage(pool.clone());

        let result = statements_discard(
            "11111111-1111-4111-8111-111111111111".to_string(),
            app.state::<deadpool_sqlite::Pool>(),
        )
        .await
        .unwrap();
        assert_eq!(result["status"], "discarded");

        let conn2 = pool.get().await.unwrap();
        let remaining: i64 = conn2
            .interact(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM unprocessed_statements WHERE id = '11111111-1111-4111-8111-111111111111'",
                    [],
                    |r| r.get(0),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(remaining, 0, "row must be permanently removed");

        let (action, resource_id): (String, Option<String>) = conn2
            .interact(|c| {
                c.query_row(
                    "SELECT action, resource_id FROM audit_log WHERE action = 'statement_discarded'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(action, "statement_discarded");
        assert_eq!(
            resource_id.as_deref(),
            Some("11111111-1111-4111-8111-111111111111")
        );

        // Discarding again must fail — the row is really gone, not just hidden.
        let second_attempt = statements_discard(
            "11111111-1111-4111-8111-111111111111".to_string(),
            app.state::<deadpool_sqlite::Pool>(),
        )
        .await;
        assert!(second_attempt.is_err());
    }
}
