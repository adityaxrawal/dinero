//! Every Tauri command the frontend can invoke.
//!
//! This is the IPC surface. `get_handlers` registers the complete set at
//! startup, so a command absent from that list is unreachable regardless of
//! whether it exists here.
//!
//! Commands follow a consistent shape: validate arguments, check the licence
//! gate where the operation writes, do the work, and return either a typed
//! response or an `AppError` whose variant name becomes the code the frontend
//! branches on.
pub mod network;
use crate::statements::{
    duplicate_check::{check_file_hash_duplicate, DuplicateCheckResult},
    events,
    validator::validate_and_hash,
};
use tauri::{Emitter, Manager};

pub mod data;
pub mod debug;
pub mod llm;
pub mod merchant_cleanup;
pub mod release_readiness;

#[cfg(test)]
mod data_tests;

#[derive(Debug, Clone)]
pub struct ConfirmedInstrument {
    pub issuer_name: String,
    pub masked_identifier: String,
    pub instrument_type: String,
}

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

#[tauri::command]
pub fn log_renderer_error(message: String, stack: Option<String>, source: String) {
    tracing::error!(
        "RENDERER ERROR [{}]: {}{}",
        source,
        message,
        stack.map(|s| format!("\nStack: {}", s)).unwrap_or_default(),
    );
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadFile {
    pub file_bytes: Vec<u8>,
    pub filename: String,
}

#[derive(serde::Serialize)]
pub struct UploadResult {
    pub statement_id: String,
    pub filename: String,
    pub status: String,
}

fn validate_upload_files_non_empty(files: &[UploadFile]) -> Result<(), crate::error::AppError> {
    if files.is_empty() {
        return Err(crate::error::AppError::Validation(
            "at least one file is required".to_string(),
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn statements_upload(
    files: Vec<UploadFile>,
    app: tauri::AppHandle,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
    queues: tauri::State<'_, crate::ingestion::queues::QueueHandles>,
) -> Result<serde_json::Value, crate::error::AppError> {
    validate_upload_files_non_empty(&files)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    if let Ok(app_data_dir) = app.path().app_data_dir() {
        if let Err(e) =
            crate::statements::pdf_storage::cleanup_expired_pdfs(&app_data_dir, pool.inner()).await
        {
            tracing::warn!("Lazy cleanup of expired PDFs failed: {}", e);
        }
    }

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
            batch_progress.clone(),
        )
        .await;
        // The tracker is seeded with the file count, but a duplicate, a
        // password-protected file or a failed upload never becomes a queued job
        // and so never records a completion of its own. Counting it here is what
        // lets the batch reach its total instead of stalling a few short of it
        // for the rest of the session.
        let reaches_the_queue = matches!(&result, Ok(r) if r.status != "awaiting_password");
        results.push(match result {
            Ok(r) => r,
            Err(e) => UploadResult {
                statement_id: String::new(),
                filename,
                status: format!("error: {}", e),
            },
        });
        if !reaches_the_queue {
            if let Some(tracker) = &batch_progress {
                let (parsed, total, eta_seconds) = tracker.record_skipped();
                crate::ingestion::queues::emit_batch_progress(&app, parsed, total, eta_seconds);
            }
        }
    }
    Ok(serde_json::json!({ "results": results }))
}

async fn upload_one_statement(
    bytes: Vec<u8>,
    filename: String,
    app: &tauri::AppHandle,
    pool_ref: &deadpool_sqlite::Pool,
    queues: &crate::ingestion::queues::QueueHandles,
    batch_progress: Option<std::sync::Arc<crate::ingestion::queues::BatchProgressTracker>>,
) -> Result<UploadResult, crate::error::AppError> {
    tracing::info!(
        "statements_upload: filename='{}' size={} bytes",
        filename,
        bytes.len()
    );

    let file_hash =
        validate_and_hash(&bytes).map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;
    tracing::info!("File validated. sha256={}", file_hash);

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

    if let Ok(Some(DuplicateCheckResult::DuplicateBillingCycle)) =
        check_filename_billing_cycle_all_instruments(&filename, pool_ref).await
    {
        tracing::warn!(
            "Duplicate billing cycle detected from filename: '{}'",
            filename
        );
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

    let stmt_id = uuid::Uuid::new_v4().to_string();
    let resolved_password = match crate::statements::password::resolve_statement_password(
        &stmt_id, &bytes, &filename, &file_hash, pool_ref, app, None,
    )
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
    {
        crate::statements::password::StatementPasswordResolution::Proceed(password) => password,
        crate::statements::password::StatementPasswordResolution::PromptCreated => {
            return Ok(UploadResult {
                statement_id: stmt_id,
                filename,
                status: "awaiting_password".to_string(),
            });
        }
    };

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

    {
        let app_data_dir = app.path().app_data_dir().map_err(|e| {
            crate::error::AppError::Io(format!("Failed to resolve app data directory: {}", e))
        })?;
        crate::statements::pdf_storage::store_pdf(&app_data_dir, &stmt_id, &bytes)
            .map_err(|e| crate::error::AppError::Io(e.to_string()))?;
    }
    drop(bytes);

    let job = crate::ingestion::queues::StatementJob {
        filename: filename.clone(),
        file_hash: file_hash.clone(),
        stmt_id: stmt_id.clone(),
        batch_progress,
        password: resolved_password,
        origin: "manual_upload".to_string(),
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

async fn llm_rows_for_unparsed_pages<R: tauri::Runtime>(
    pages: &[crate::statements::parser::ParsedPage],
    parser: crate::statements::row_extractor::BankParser,
    issuer: &str,
    pool: &deadpool_sqlite::Pool,
    app: &tauri::AppHandle<R>,
    start_index: usize,
) -> Vec<crate::statements::row_extractor::StatementRow> {
    let eligible = app
        .try_state::<crate::startup::LlmEligibility>()
        .map(|e| e.eligible)
        .unwrap_or(false);
    if !eligible {
        return Vec::new();
    }
    let Ok(app_dir) = app.path().app_data_dir() else {
        return Vec::new();
    };

    let stored = match pool.get().await {
        Ok(conn) => conn
            .interact(|c| crate::db::local_profile::get_llm_model(c))
            .await
            .ok()
            .and_then(|r| r.ok())
            .flatten(),
        Err(_) => None,
    };
    let downloaded: Vec<String> = crate::llm_manager::get_available_models()
        .into_iter()
        .filter(|m| crate::llm_manager::get_model_path(&app_dir, &m.id).is_some())
        .map(|m| m.id)
        .collect();
    let Some(model_id) = crate::llm_manager::resolve_active_model(&downloaded, stored.as_deref())
    else {
        return Vec::new();
    };

    crate::statements::row_llm::extract_unparsed_pages(
        pages,
        parser,
        issuer,
        &app_dir,
        &model_id,
        start_index,
    )
    .await
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub enum PipelineOutcome {
    Staged(String),
    BlockedAwaitingInstrument(String),
}

#[allow(clippy::too_many_arguments)]
pub async fn stage_parse_pipeline<R: tauri::Runtime>(
    bytes: &[u8],
    _filename: &str,
    file_hash: &str,
    pool: &deadpool_sqlite::Pool,
    app: &tauri::AppHandle<R>,
    confirmed_instrument: Option<ConfirmedInstrument>,
    password: Option<&str>,
    origin: &str,
    stmt_id: Option<String>,
) -> anyhow::Result<PipelineOutcome> {
    use crate::statements::{
        duplicate_check::{check_billing_cycle_duplicate, DuplicateCheckResult},
        metadata_extractor::{extract_metadata, resolve_or_create_instrument},
        parser::parse_in_memory_with_password,
        row_extractor::{extract_rows, BankParser},
    };

    let draft_id = stmt_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if let Ok(app_data_dir) = app.path().app_data_dir() {
        let _ = crate::statements::pdf_storage::store_pdf(&app_data_dir, &draft_id, bytes);
    }

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
    emit_processing_progress(app, stmt_id.as_deref(), "pending", "parsing", 10);

    let meta = extract_metadata(&parse_result.pages)?;
    tracing::info!(
        "Metadata: issuer={:?} masked={:?} period={:?}→{:?} due={:?}",
        meta.issuer_name,
        meta.masked_identifier,
        meta.billing_period_start,
        meta.billing_period_end,
        meta.due_date
    );
    emit_processing_progress(app, stmt_id.as_deref(), "pending", "metadata", 30);

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
                create_awaiting_instrument_row(
                    &unprocessed_id,
                    file_hash,
                    _filename,
                    password,
                    pool,
                )
                .await
                .map_err(|e| anyhow::anyhow!("DB error creating awaiting_instrument row: {}", e))?;
                if let Ok(app_data_dir) = app.path().app_data_dir() {
                    let _ = crate::statements::pdf_storage::store_pdf(
                        &app_data_dir,
                        &unprocessed_id,
                        bytes,
                    );
                }
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
                return Ok(PipelineOutcome::BlockedAwaitingInstrument(unprocessed_id));
            }
        };
        let masked = match meta.masked_identifier.clone() {
            Some(m) if !m.trim().is_empty() => m,
            _ => {
                delete_orphaned_queued_row(stmt_id.as_deref(), pool).await;
                let unprocessed_id = uuid::Uuid::new_v4().to_string();
                create_awaiting_instrument_row(
                    &unprocessed_id,
                    file_hash,
                    _filename,
                    password,
                    pool,
                )
                .await
                .map_err(|e| anyhow::anyhow!("DB error creating awaiting_instrument row: {}", e))?;
                if let Ok(app_data_dir) = app.path().app_data_dir() {
                    let _ = crate::statements::pdf_storage::store_pdf(
                        &app_data_dir,
                        &unprocessed_id,
                        bytes,
                    );
                }
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
                return Ok(PipelineOutcome::BlockedAwaitingInstrument(unprocessed_id));
            }
        };
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

    if let Some(pwd) = password {
        crate::statements::password::save_password(&instrument_id, pwd, pool)
            .await
            .ok();
    }

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

    emit_processing_progress(
        app,
        stmt_id.as_deref(),
        &instrument_id,
        "duplicate_check",
        50,
    );

    let bank_parser = BankParser::detect(&issuer, &instrument_type);
    let mut rows = extract_rows(&parse_result.pages, bank_parser)?;
    tracing::info!(
        "Extracted {} statement rows via parser={:?}",
        rows.len(),
        bank_parser
    );

    crate::statements::learned_rows::apply_learned_rules_to_rows(pool, &issuer, &mut rows).await;

    let llm_rows = llm_rows_for_unparsed_pages(
        &parse_result.pages,
        bank_parser,
        &issuer,
        pool,
        app,
        rows.len(),
    )
    .await;
    if !llm_rows.is_empty() {
        tracing::info!("LLM assist recovered {} additional rows", llm_rows.len());
        rows.extend(llm_rows);
    }
    emit_processing_progress(
        app,
        stmt_id.as_deref(),
        &instrument_id,
        "extracting_rows",
        65,
    );

    delete_orphaned_queued_row(Some(&draft_id), pool).await;

    let rows_json = serde_json::to_string(&rows)
        .map_err(|e| anyhow::anyhow!("Failed to serialize statement rows: {}", e))?;
    let draft = crate::db::statement_drafts::StatementDraftRow {
        id: draft_id.clone(),
        origin: origin.to_string(),
        file_hash: file_hash.to_string(),
        instrument_id: Some(instrument_id.clone()),
        issuer_name: Some(issuer.clone()),
        masked_identifier: Some(masked.clone()),
        instrument_type: Some(instrument_type.clone()),
        billing_period_start: meta.billing_period_start.clone(),
        billing_period_end: meta.billing_period_end.clone(),
        due_date: meta.due_date.clone(),
        statement_date: meta.statement_date.clone(),
        current_balance: meta.current_balance,
        minimum_due: meta.minimum_due,
        rows_json,
        status: "pending_review".to_string(),
        created_at: None,
        updated_at: None,
    };
    let conn = pool
        .get()
        .await
        .map_err(|e| anyhow::anyhow!("DB pool error: {}", e))?;
    conn.interact(move |c| crate::db::statement_drafts::insert(c, &draft))
        .await
        .map_err(|e| anyhow::anyhow!("Interact error: {}", e))?
        .map_err(|e| anyhow::anyhow!("Failed to write statement draft: {}", e))?;

    tracing::info!(
        "Statement draft staged: id='{}' origin='{}'",
        draft_id,
        origin
    );
    emit_processing_progress(app, Some(&draft_id), &instrument_id, "staged", 100);

    let staged_payload = serde_json::json!({ "draft_id": draft_id, "origin": origin });
    events::emit(events::STAGED, staged_payload.clone());
    app.emit(events::STAGED, staged_payload).ok();

    Ok(PipelineOutcome::Staged(draft_id))
}

fn emit_processing_progress<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    draft_id: Option<&str>,
    instrument_id: &str,
    stage: &str,
    percent: u8,
) {
    let payload = serde_json::json!({
        "draft_id": draft_id,
        "instrument_id": instrument_id,
        "stage": stage,
        "percent": percent,
    });
    events::emit(events::PROCESSING_PROGRESS, payload.clone());
    let _ = app.emit(events::PROCESSING_PROGRESS, payload);
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct DraftMetadataUpdate {
    pub issuer_name: String,
    pub masked_identifier: String,
    pub instrument_type: String,
    pub billing_period_start: Option<String>,
    pub billing_period_end: Option<String>,
    pub due_date: Option<String>,
    pub statement_date: Option<String>,
    pub current_balance: Option<i64>,
    pub minimum_due: Option<i64>,
}

pub async fn commit_staged_draft<R: tauri::Runtime>(
    draft_id: &str,
    edited_metadata: DraftMetadataUpdate,
    edited_rows: Vec<crate::statements::row_extractor::StatementRow>,
    pool: &deadpool_sqlite::Pool,
    app: &tauri::AppHandle<R>,
) -> anyhow::Result<String> {
    use crate::db::transaction_observations::{
        insert_observation_idempotent, TransactionObservationsRow,
    };
    use crate::statements::{
        bill_classifier,
        metadata_extractor::{
            resolve_or_create_instrument, write_statement_row, StatementMetadata,
        },
        observation_builder::build_all_observations,
        row_extractor::map_rows_to_statement_entries,
    };

    let conn = pool
        .get()
        .await
        .map_err(|e| anyhow::anyhow!("DB pool error: {}", e))?;
    let draft_id_owned = draft_id.to_string();
    let draft = conn
        .interact(move |c| crate::db::statement_drafts::select_by_id(c, &draft_id_owned))
        .await
        .map_err(|e| anyhow::anyhow!("Interact error: {}", e))?
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?
        .ok_or_else(|| anyhow::anyhow!("draft not found: {}", draft_id))?;

    if draft.status != "pending_review" {
        return Err(anyhow::anyhow!(
            "draft '{}' is not pending_review (status='{}') — already committed or discarded",
            draft_id,
            draft.status
        ));
    }

    let instrument_id = resolve_or_create_instrument(
        &edited_metadata.instrument_type,
        &edited_metadata.issuer_name,
        &edited_metadata.masked_identifier,
        None,
        pool,
    )
    .await?;

    let meta = StatementMetadata {
        billing_period_start: edited_metadata.billing_period_start,
        billing_period_end: edited_metadata.billing_period_end,
        due_date: edited_metadata.due_date,
        minimum_due: edited_metadata.minimum_due,
        current_balance: edited_metadata.current_balance,
        issuer_name: Some(edited_metadata.issuer_name.clone()),
        masked_identifier: Some(edited_metadata.masked_identifier.clone()),
        network: None,
        rewards_summary_json: None,
        statement_date: edited_metadata.statement_date,
    };

    let fresh_stmt_id = uuid::Uuid::new_v4().to_string();
    let stmt_id = write_statement_row(
        &fresh_stmt_id,
        &instrument_id,
        &edited_metadata.instrument_type,
        &meta,
        Some(&draft.file_hash),
        pool,
    )
    .await?;

    let entry_ids = map_rows_to_statement_entries(&stmt_id, &edited_rows, pool).await;

    if !edited_rows.is_empty() && !entry_ids.is_empty() {
        let observations =
            build_all_observations(&stmt_id, &instrument_id, &edited_rows, &entry_ids);
        let conn = pool
            .get()
            .await
            .map_err(|e| anyhow::anyhow!("DB pool error: {}", e))?;
        let obs_cloned = observations.clone();
        let stmt_id_for_obs = stmt_id.clone();
        conn.interact(move |c| {
            for obs in &obs_cloned {
                let fmt = "%Y-%m-%d %H:%M:%S";
                let event_time = chrono::NaiveDateTime::parse_from_str(&obs.event_time, fmt)
                    .or_else(|_| {
                        chrono::NaiveDateTime::parse_from_str(
                            &format!("{} 00:00:00", obs.event_time),
                            fmt,
                        )
                    })
                    .ok();
                let row = TransactionObservationsRow {
                    id: obs.id.clone(),
                    canonical_transaction_id: None,
                    source_pipeline: Some(obs.source_pipeline.clone()),
                    source_record_id: Some(obs.source_record_id.clone()),
                    source_message_id: None,
                    source_thread_id: None,
                    statement_id: Some(stmt_id_for_obs.clone()),
                    statement_entry_id: Some(obs.source_record_id.clone()),
                    instrument_id: Some(obs.instrument_id.clone()),
                    direction: Some(obs.direction.clone()),
                    amount: None,
                    amount_minor: Some(obs.amount_minor),
                    currency: Some(obs.currency.clone()),
                    event_time,
                    event_time_confidence: obs.event_time_confidence.clone(),
                    posting_date: None,
                    merchant_raw: obs.merchant_raw.clone(),
                    merchant_normalized: None,
                    reference_id: obs.reference_id.clone(),
                    original_amount_minor: None,
                    original_currency: None,
                    exchange_rate: None,
                    balance_after_transaction: None,
                    timezone_at_ingestion: None,
                    fingerprint: obs.fingerprint.clone(),
                    extraction_method: Some("statement_row_parser".to_string()),
                    confidence_score: obs.confidence_score,
                    raw_payload_json: None,
                    parser_version: None,
                    emi_total_installments: obs.emi_total_installments,
                    emi_installment_number: None,
                    emi_original_amount_minor: obs.emi_original_amount_minor,
                    channel: None,
                    is_deleted: false,
                    created_at: None,
                    updated_at: None,
                };
                if let Err(e) = insert_observation_idempotent(c, &row) {
                    tracing::warn!("Failed to insert observation row for '{}': {}", obs.id, e);
                    continue;
                }
                if let Err(e) = crate::reconciliation::engine::reconcile_transactionally(c, obs) {
                    tracing::warn!("Reconciliation failed for obs '{}': {}", obs.id, e);
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

    bill_classifier::classify_and_update(&instrument_id, &stmt_id, &meta, pool, Some(app)).await?;

    let draft_id_for_retention = draft_id.to_string();
    let stmt_id_for_retention = stmt_id.clone();
    let conn = pool
        .get()
        .await
        .map_err(|e| anyhow::anyhow!("DB pool error: {}", e))?;
    conn.interact(move |c| {
        c.execute(
            "INSERT OR IGNORE INTO unprocessed_statements \
             (id, statement_source_json, failure_type, failure_reason, status) \
             VALUES (?1, '{}', 'staged_review', '', 'pending_review')",
            rusqlite::params![draft_id_for_retention],
        )?;
        crate::db::unprocessed_statements::update_status(
            c,
            &draft_id_for_retention,
            "resolved",
            Some(&stmt_id_for_retention),
        )
    })
    .await
    .map_err(|e| anyhow::anyhow!("Interact error: {}", e))?
    .map_err(|e| anyhow::anyhow!("DB error linking PDF retention: {}", e))?;

    let draft_id_owned = draft_id.to_string();
    conn.interact(move |c| {
        crate::db::statement_drafts::update_status(c, &draft_id_owned, "committed")
    })
    .await
    .map_err(|e| anyhow::anyhow!("Interact error: {}", e))?
    .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;

    let rows_extracted = entry_ids.len() as i64;
    let issuer_name_for_event = instrument_id.clone();
    let issuer_name = conn
        .interact(move |c| crate::db::instruments::get_instrument(c, &issuer_name_for_event))
        .await
        .ok()
        .and_then(|r| r.ok())
        .flatten()
        .map(|i| i.issuer_name);
    let parsed_payload = serde_json::json!({
        "statement_id": stmt_id,
        "instrument_id": instrument_id,
        "issuer_name": issuer_name,
        "rows_extracted": rows_extracted,
    });
    events::emit(events::PARSED, parsed_payload.clone());
    app.emit(events::PARSED, parsed_payload).ok();

    Ok(stmt_id)
}

pub async fn discard_staged_draft(
    draft_id: &str,
    app_data_dir: &std::path::Path,
    pool: &deadpool_sqlite::Pool,
) -> anyhow::Result<()> {
    let _ = crate::statements::pdf_storage::delete_pdf(app_data_dir, draft_id);
    let conn = pool
        .get()
        .await
        .map_err(|e| anyhow::anyhow!("DB pool error: {}", e))?;
    let id = draft_id.to_string();
    conn.interact(move |c| crate::db::statement_drafts::delete(c, &id))
        .await
        .map_err(|e| anyhow::anyhow!("Interact error: {}", e))?
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;
    Ok(())
}

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

async fn create_awaiting_instrument_row(
    statement_id: &str,
    file_hash: &str,
    filename: &str,
    password: Option<&str>,
    pool: &deadpool_sqlite::Pool,
) -> anyhow::Result<()> {
    let stmt_id = statement_id.to_string();
    let mut json_obj = serde_json::json!({
        "file_hash": file_hash,
        "filename": filename,
    });

    if let Some(pwd) = password {
        let blob = crate::statements::password::encrypt_password(pwd)?;
        use base64::Engine;
        let base64_blob = base64::engine::general_purpose::STANDARD.encode(&blob);
        json_obj
            .as_object_mut()
            .unwrap()
            .insert("password_blob".to_string(), serde_json::json!(base64_blob));
    }

    let source_json = json_obj.to_string();

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

#[tauri::command]
pub async fn statements_confirm_instrument(
    statement_id: String,
    issuer_name: String,
    masked_identifier: String,
    instrument_type: Option<String>,
    app: tauri::AppHandle,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("statement_id", &statement_id)?;
    crate::ipc::validation::validate_non_empty("issuer_name", &issuer_name)?;
    crate::ipc::validation::validate_non_empty("masked_identifier", &masked_identifier)?;
    let issuer_name = issuer_name.trim().to_string();
    let masked_identifier = masked_identifier.trim().to_string();

    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let app_data_dir = app.path().app_data_dir().map_err(|_| {
        crate::error::AppError::Unknown("Failed to determine app data directory".to_string())
    })?;
    let bytes = crate::statements::pdf_storage::read_pdf(&app_data_dir, &statement_id)
        .map_err(|_| {
            crate::error::AppError::Unknown(
                "This statement's PDF file could not be read".to_string(),
            )
        })?
        .ok_or_else(|| {
            crate::error::AppError::Unknown(
                "This statement's PDF file could not be found — please re-upload the file"
                    .to_string(),
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

    let mut decrypted_password = None;
    if let Some(b64) = parsed["password_blob"].as_str() {
        use base64::Engine;
        match base64::engine::general_purpose::STANDARD.decode(b64) {
            Ok(blob) => match crate::statements::password::decrypt_password(&blob) {
                Ok(pwd) => decrypted_password = Some(pwd),
                Err(e) => tracing::error!(
                    "statements_confirm_instrument: failed to decrypt persisted password_blob \
                     for statement_id='{}' — resuming without a password: {}",
                    statement_id,
                    e
                ),
            },
            Err(e) => tracing::error!(
                "statements_confirm_instrument: password_blob for statement_id='{}' is not \
                 valid base64 — resuming without a password: {}",
                statement_id,
                e
            ),
        }
    }

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

    let result = stage_parse_pipeline(
        &bytes,
        &filename,
        &file_hash,
        pool.inner(),
        &app,
        Some(confirmed),
        decrypted_password.as_deref(),
        "password_unlock",
        Some(statement_id.clone()),
    )
    .await;

    match result {
        Ok(PipelineOutcome::Staged(draft_id)) => Ok(serde_json::json!({
            "status": "staged",
            "draft_id": draft_id
        })),
        Ok(PipelineOutcome::BlockedAwaitingInstrument(unprocessed_id)) => Ok(serde_json::json!({
            "status": "awaiting_instrument_confirmation",
            "statement_id": unprocessed_id
        })),
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

#[tauri::command]
pub async fn statements_submit_password(
    statement_id: String,
    password: String,
    app: tauri::AppHandle,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("statement_id", &statement_id)?;
    crate::ipc::validation::validate_non_empty("password", &password)?;

    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    use crate::statements::{
        events,
        password::{try_user_password, PasswordResolutionResult},
    };

    let app_data_dir = app.path().app_data_dir().map_err(|_| {
        crate::error::AppError::Unknown("Failed to determine app data directory".to_string())
    })?;
    let pdf_bytes = crate::statements::pdf_storage::read_pdf(&app_data_dir, &statement_id)
        .map_err(|_| {
            crate::error::AppError::Unknown(
                "This statement's PDF file could not be read".to_string(),
            )
        })?
        .ok_or_else(|| {
            crate::error::AppError::Unknown(
                "This statement's PDF file could not be found — please re-upload the file"
                    .to_string(),
            )
        })?;

    let result = try_user_password(&statement_id, &password, &pdf_bytes, pool.inner())
        .await
        .map_err(|e| crate::error::AppError::Auth(e.to_string()))?;

    match result {
        PasswordResolutionResult::UnlockedWithUserInput => {
            tracing::info!(
                "Password accepted for statement_id='{}' — resuming parse pipeline",
                statement_id
            );
            record_pdf_password_event(pool.inner(), &statement_id, "pdf_password_unlocked").await;

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

            let pipeline_result = stage_parse_pipeline(
                &pdf_bytes,
                &filename,
                &file_hash,
                pool.inner(),
                &app,
                None,
                Some(&password),
                "password_unlock",
                Some(statement_id.clone()),
            )
            .await;

            match pipeline_result {
                Ok(PipelineOutcome::Staged(draft_id)) => Ok(serde_json::json!({
                    "status": "unlocked",
                    "draft_id": draft_id
                })),
                Ok(PipelineOutcome::BlockedAwaitingInstrument(unprocessed_id)) => {
                    let conn = pool
                        .get()
                        .await
                        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
                    let orig_id = statement_id.clone();
                    conn.interact(move |c| {
                        let _ = crate::db::unprocessed_statements::delete(c, &orig_id);
                    })
                    .await
                    .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

                    Ok(serde_json::json!({
                        "status": "awaiting_instrument_confirmation",
                        "statement_id": unprocessed_id
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

            let conn = pool
                .get()
                .await
                .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
            let stmt_id_for_attempts = statement_id.clone();
            let _attempts = conn
                .interact(move |c| {
                    crate::db::unprocessed_statements::increment_password_attempts(
                        c,
                        &stmt_id_for_attempts,
                    )
                })
                .await
                .map_err(|e| crate::error::AppError::Db(e.to_string()))?
                .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

            app.emit(
                events::PASSWORD_REQUIRED,
                serde_json::json!({
                    "statement_id": statement_id,
                    "error": "wrong_password"
                }),
            )
            .ok();
            Ok(serde_json::json!({
                "status": "wrong_password",
                "statement_id": statement_id
            }))
        }
        _ => Err(crate::error::AppError::Unknown(
            "Unexpected password resolution outcome".to_string(),
        )),
    }
}

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

#[tauri::command]
pub async fn statements_retry_unprocessed(
    statement_id: String,
    app: tauri::AppHandle,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("statement_id", &statement_id)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let outcome = retry_one_unprocessed(&statement_id, &app, pool.inner()).await?;

    if let RetryOutcome::StillLocked { ref filename } = outcome {
        app.emit(
            crate::statements::events::PASSWORD_REQUIRED,
            serde_json::json!({ "statement_id": statement_id, "filename": filename }),
        )
        .ok();
    }

    Ok(outcome.into_response(&statement_id))
}

enum RetryOutcome {
    Unlocked { draft_id: String },
    AwaitingInstrument { statement_id: String },
    StillLocked { filename: String },
    BytesExpired { filename: String },
}

impl RetryOutcome {
    fn into_response(self, statement_id: &str) -> serde_json::Value {
        match self {
            Self::Unlocked { draft_id } => {
                serde_json::json!({ "status": "unlocked", "draft_id": draft_id })
            }
            Self::AwaitingInstrument { statement_id } => serde_json::json!({
                "status": "awaiting_instrument_confirmation",
                "statement_id": statement_id
            }),
            Self::StillLocked { .. } => {
                serde_json::json!({ "status": "retry_queued", "statement_id": statement_id })
            }
            Self::BytesExpired { filename } => serde_json::json!({
                "status": "bytes_expired",
                "statement_id": statement_id,
                "filename": filename,
                "message": "This statement's PDF file could not be found. \
                            Please re-upload the file to continue."
            }),
        }
    }
}

async fn retry_one_unprocessed(
    statement_id: &str,
    app: &tauri::AppHandle,
    pool: &deadpool_sqlite::Pool,
) -> Result<RetryOutcome, crate::error::AppError> {
    use crate::statements::password::try_all_stored_passwords;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let stmt_id_clone = statement_id.to_string();
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

    let source = serde_json::from_str::<serde_json::Value>(&row.statement_source_json).ok();
    let field = |key: &str| {
        source
            .as_ref()
            .and_then(|v| v[key].as_str().map(|s| s.to_string()))
            .unwrap_or_default()
    };
    let filename = field("filename");

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::error::AppError::Io(e.to_string()))?;
    let pdf_bytes = match crate::statements::pdf_storage::read_pdf(&app_data_dir, statement_id) {
        Ok(Some(bytes)) => bytes,
        Ok(None) | Err(_) => {
            tracing::warn!(
                "retry_one_unprocessed: bytes not found on disk for statement_id='{}' \
                 (file deleted or missing) — returning bytes_expired",
                statement_id
            );
            return Ok(RetryOutcome::BytesExpired { filename });
        }
    };

    let stored_result = try_all_stored_passwords(&pdf_bytes, pool)
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

    let crate::statements::password::PasswordResolutionResult::UnlockedWithStored(password) =
        stored_result
    else {
        return Ok(RetryOutcome::StillLocked { filename });
    };

    tracing::info!(
        "Retry found a matching stored password for statement_id='{}' — resuming pipeline",
        statement_id
    );

    let pipeline_result = stage_parse_pipeline(
        &pdf_bytes,
        &filename,
        &field("file_hash"),
        pool,
        app,
        None,
        Some(&password),
        "password_unlock",
        Some(statement_id.to_string()),
    )
    .await;

    match pipeline_result {
        Ok(PipelineOutcome::Staged(draft_id)) => Ok(RetryOutcome::Unlocked { draft_id }),
        Ok(PipelineOutcome::BlockedAwaitingInstrument(unprocessed_id)) => {
            let conn = pool
                .get()
                .await
                .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
            let orig_id = statement_id.to_string();
            conn.interact(move |c| {
                let _ = crate::db::unprocessed_statements::delete(c, &orig_id);
            })
            .await
            .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

            Ok(RetryOutcome::AwaitingInstrument {
                statement_id: unprocessed_id,
            })
        }
        Err(e) => {
            tracing::error!(
                "retry_one_unprocessed: pipeline failed for statement_id='{}': {}",
                statement_id,
                e
            );
            Err(crate::error::AppError::Unknown(e.to_string()))
        }
    }
}

#[tauri::command]
pub async fn statements_reparse_all(
    app: tauri::AppHandle,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    static REPARSE_RUNNING: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    if REPARSE_RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err(crate::error::AppError::Validation(
            "A re-parse is already running.".to_string(),
        ));
    }
    struct RunningGuard;
    impl Drop for RunningGuard {
        fn drop(&mut self) {
            REPARSE_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let _guard = RunningGuard;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let rows = conn
        .interact(|c| crate::db::unprocessed_statements::select_actionable(c))
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let total = rows.len();
    let (mut parsed, mut still_locked, mut expired, mut failed) = (0usize, 0usize, 0usize, 0usize);

    for (index, row) in rows.iter().enumerate() {
        let _ = crate::ipc::events::emit_event(
            &app,
            crate::ipc::events::AppEvent::StatementReparseProgress,
            serde_json::json!({
                "processed": index,
                "total": total,
                "current": row.id,
                "done": false,
            }),
        );

        match retry_one_unprocessed(&row.id, &app, pool.inner()).await {
            Ok(RetryOutcome::Unlocked { .. }) | Ok(RetryOutcome::AwaitingInstrument { .. }) => {
                parsed += 1
            }
            Ok(RetryOutcome::StillLocked { .. }) => still_locked += 1,
            Ok(RetryOutcome::BytesExpired { .. }) => expired += 1,
            Err(e) => {
                tracing::warn!(
                    "statements_reparse_all: statement_id='{}' failed: {}",
                    row.id,
                    e
                );
                failed += 1;
            }
        }
    }

    let summary = serde_json::json!({
        "processed": total,
        "total": total,
        "parsed": parsed,
        "still_locked": still_locked,
        "bytes_expired": expired,
        "failed": failed,
        "done": true,
    });
    let _ = crate::ipc::events::emit_event(
        &app,
        crate::ipc::events::AppEvent::StatementReparseProgress,
        summary.clone(),
    );

    tracing::info!(
        "statements_reparse_all: {} queued, {} parsed, {} still locked, {} expired, {} failed",
        total,
        parsed,
        still_locked,
        expired,
        failed
    );
    Ok(summary)
}

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
    let draft_rows = conn
        .interact(|c| crate::db::statement_drafts::select_pending_review(c))
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let mut grouped = group_unprocessed_by_status(rows);
    grouped["awaiting_review"] = group_drafts_for_review(draft_rows);
    Ok(grouped)
}

fn group_drafts_for_review(
    rows: Vec<crate::db::statement_drafts::StatementDraftRow>,
) -> serde_json::Value {
    let entries: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|row| {
            serde_json::json!({
                "draft_id": row.id,
                "issuer_name": row.issuer_name,
                "masked_identifier": row.masked_identifier,
                "origin": row.origin,
                "created_at": row.created_at.map(|dt| dt.to_string()),
            })
        })
        .collect();
    serde_json::Value::Array(entries)
}

fn group_unprocessed_by_status(
    rows: Vec<crate::db::unprocessed_statements::UnprocessedStatementRow>,
) -> serde_json::Value {
    let mut awaiting_password = Vec::new();
    let mut pending_retry = Vec::new();
    let mut failed = Vec::new();

    for row in rows {
        let source_json =
            serde_json::from_str::<serde_json::Value>(&row.statement_source_json).ok();
        let filename = source_json
            .as_ref()
            .and_then(|v| v["filename"].as_str().map(|s| s.to_string()))
            .unwrap_or_default();

        let sender = source_json
            .as_ref()
            .and_then(|v| v["sender"].as_str().map(|s| s.to_string()));
        let to = source_json
            .as_ref()
            .and_then(|v| v["to"].as_str().map(|s| s.to_string()));
        let subject = source_json
            .as_ref()
            .and_then(|v| v["subject"].as_str().map(|s| s.to_string()));
        let date = source_json
            .as_ref()
            .and_then(|v| v["date"].as_str().map(|s| s.to_string()));
        let snippet = source_json
            .as_ref()
            .and_then(|v| v["snippet"].as_str().map(|s| s.to_string()));
        let html = source_json
            .as_ref()
            .and_then(|v| v["html"].as_str().map(|s| s.to_string()));

        let display_name = crate::statements::display_name::derive_display_name(
            &crate::statements::display_name::StatementNameSource {
                filename: &filename,
                sender: sender.as_deref(),
                subject: subject.as_deref(),
                snippet: snippet.as_deref(),
                date: date.as_deref(),
            },
        );

        let entry = serde_json::json!({
            "statement_id": row.id,
            "filename": filename,
            "display_name": display_name,
            "failure_type": row.failure_type,
            "failure_reason": row.failure_reason,
            "sender": sender,
            "to": to,
            "subject": subject,
            "date": date,
            "snippet": snippet,
            "html": html,
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

#[tauri::command]
pub async fn statements_commit_draft(
    draft_id: String,
    edited_metadata: DraftMetadataUpdate,
    edited_rows: Vec<crate::statements::row_extractor::StatementRow>,
    app: tauri::AppHandle,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("draft_id", &draft_id)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let stmt_id = commit_staged_draft(&draft_id, edited_metadata, edited_rows, pool.inner(), &app)
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

    Ok(serde_json::json!({ "status": "committed", "statement_id": stmt_id }))
}

#[tauri::command]
pub async fn statements_discard_draft(
    draft_id: String,
    app: tauri::AppHandle,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("draft_id", &draft_id)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let app_data_dir = app.path().app_data_dir().map_err(|_| {
        crate::error::AppError::Unknown("Failed to determine app data directory".to_string())
    })?;
    discard_staged_draft(&draft_id, &app_data_dir, pool.inner())
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

    Ok(serde_json::json!({ "status": "discarded" }))
}

#[tauri::command]
pub async fn statements_get_draft_pdf(
    draft_id: String,
    app: tauri::AppHandle,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("draft_id", &draft_id)?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let id = draft_id.clone();
    let exists = conn
        .interact(move |c| crate::db::statement_drafts::select_by_id(c, &id))
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .is_some();
    if !exists {
        return Err(crate::error::AppError::Unknown(
            "Draft not found".to_string(),
        ));
    }

    let app_data_dir = app.path().app_data_dir().map_err(|_| {
        crate::error::AppError::Unknown("Failed to determine app data directory".to_string())
    })?;
    let bytes = crate::statements::pdf_storage::read_pdf(&app_data_dir, &draft_id)
        .map_err(|_| {
            crate::error::AppError::Unknown(
                "This statement's PDF file could not be read".to_string(),
            )
        })?
        .ok_or_else(|| {
            crate::error::AppError::Unknown(
                "This statement's PDF file could not be found".to_string(),
            )
        })?;

    let viewable = crate::statements::password::ensure_viewable_pdf_bytes(bytes, pool.inner())
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

    use base64::Engine;
    Ok(base64::engine::general_purpose::STANDARD.encode(&viewable))
}

#[tauri::command]
pub async fn statements_get_draft(
    draft_id: String,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("draft_id", &draft_id)?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let id = draft_id.clone();
    let draft = conn
        .interact(move |c| crate::db::statement_drafts::select_by_id(c, &id))
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .ok_or_else(|| crate::error::AppError::Unknown("Draft not found".to_string()))?;

    let rows: Vec<crate::statements::row_extractor::StatementRow> =
        serde_json::from_str(&draft.rows_json).unwrap_or_default();

    Ok(serde_json::json!({
        "id": draft.id,
        "origin": draft.origin,
        "issuer_name": draft.issuer_name,
        "masked_identifier": draft.masked_identifier,
        "instrument_type": draft.instrument_type,
        "billing_period_start": draft.billing_period_start,
        "billing_period_end": draft.billing_period_end,
        "due_date": draft.due_date,
        "statement_date": draft.statement_date,
        "current_balance": draft.current_balance,
        "minimum_due": draft.minimum_due,
        "rows": rows,
        "status": draft.status,
    }))
}

#[tauri::command]
pub async fn reconciliation_clusters_resolve(
    cluster_id: String,
    observation_id: String,
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
    pub reference_id: Option<String>,
}

#[tauri::command]
pub async fn transactions_create(
    payload: ManualTransactionPayload,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
    app_handle: tauri::AppHandle,
) -> Result<String, crate::error::AppError> {
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    create_manual_transaction(payload, pool.inner(), &app_handle).await
}

pub(crate) async fn create_manual_transaction<R: tauri::Runtime>(
    payload: ManualTransactionPayload,
    pool: &deadpool_sqlite::Pool,
    app_handle: &tauri::AppHandle<R>,
) -> Result<String, crate::error::AppError> {
    crate::licensing::gate::assert_write_allowed(pool).await?;

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
        reference_id: payload.reference_id.clone(),
        merchant_raw: Some(payload.merchant_name),
        source_pipeline: "manual".to_string(),
        source_record_id: format!("manual_{}", obs_id),
        emi_total_installments: None,
        emi_original_amount_minor: None,
        fingerprint: None,
        confidence_score: Some(1.0),
        event_time_confidence: None,
        channel: None,
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
                reference_id: obs.reference_id.clone(),
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
                channel: None,
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
            app_handle,
            crate::ipc::events::AppEvent::ReconciliationCluster,
            serde_json::json!({ "cluster_id": cluster_id, "observation_id": obs_id }),
        );
    } else {
        let _ = crate::ipc::events::emit_event(
            app_handle,
            crate::ipc::events::AppEvent::TransactionCreated,
            serde_json::json!({ "observation_id": obs_id }),
        );
    }

    Ok(decision.as_str().to_string())
}

#[derive(serde::Deserialize)]
pub struct TransactionUpdatePayload {
    pub transaction_id: uuid::Uuid,
    pub merchant_display_name: Option<String>,
    pub category_id: Option<String>,
    pub notes: Option<String>,
    pub location: Option<String>,
    pub tags: Option<Vec<String>>,
    pub amount_minor: Option<i64>,
    pub direction: Option<String>,
    pub event_time: Option<String>,
    pub instrument_id: Option<String>,
}

const CORRECTABLE_FIELDS: &[(&str, &str)] = &[
    ("merchant_display_name", "merchant"),
    ("amount_minor", "amount"),
    ("direction", "direction"),
    ("best_event_time", "event_time"),
];

#[allow(clippy::too_many_arguments)]
fn apply_transaction_field_update(
    conn: &rusqlite::Connection,
    tx_id: &str,
    merchant_display_name: Option<String>,
    category_id: Option<String>,
    notes: Option<String>,
    location: Option<String>,
    amount_minor: Option<i64>,
    direction: Option<String>,
    event_time: Option<String>,
    instrument_id: Option<String>,
) -> Result<Vec<crate::reconciliation::audit::CorrectionContext>, crate::error::AppError> {
    let old_tx = crate::db::transactions::get_transaction(conn, tx_id)
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .ok_or_else(|| crate::error::AppError::Unknown("Transaction not found".to_string()))?;

    let mut new_merchant = old_tx.merchant_display_name.clone();
    let mut new_merchant_normalized = old_tx.merchant_normalized_name.clone();
    let mut new_merchant_entity_id = old_tx.merchant_entity_id.clone();
    let mut new_category_id = old_tx.category_id.clone();
    let mut new_notes = old_tx.notes.clone();
    let mut new_location = old_tx.location.clone();
    let mut new_amount_minor = old_tx.amount_minor;
    let mut new_direction = old_tx.direction.clone();
    let mut new_event_time = old_tx.best_event_time;
    let mut new_instrument_id = old_tx.instrument_id.clone();

    if let Some(amt_minor) = amount_minor {
        new_amount_minor = Some(amt_minor);
    }
    if let Some(dir) = direction {
        new_direction = Some(dir);
    }
    if let Some(ev_time) = event_time {
        let parsed = chrono::NaiveDateTime::parse_from_str(&ev_time, "%Y-%m-%d %H:%M:%S")
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(&ev_time, "%Y-%m-%dT%H:%M:%S"))
            .ok()
            .or_else(|| {
                chrono::NaiveDate::parse_from_str(&ev_time, "%Y-%m-%d")
                    .ok()
                    .and_then(|d| d.and_hms_opt(0, 0, 0))
            });
        if let Some(dt) = parsed {
            new_event_time = Some(dt);
        }
    }
    if let Some(inst_id) = instrument_id {
        new_instrument_id = Some(inst_id);
    }

    if let Some(cat) = category_id {
        let old_val = old_tx.category_id.clone().unwrap_or_default();
        if old_val != cat {
            let _ = crate::reconciliation::audit::log_user_correction(
                conn,
                tx_id,
                "category_id",
                &old_val,
                &cat,
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
                            alias_normalized:
                                crate::extraction::merchant_normalizer::strip_noise_tokens(&merch),
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
                                alias_normalized:
                                    crate::extraction::merchant_normalizer::strip_noise_tokens(
                                        &merch,
                                    ),
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
             amount_minor = ?7, direction = ?8, best_event_time = ?9, instrument_id = ?10,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?11",
        rusqlite::params![
            new_merchant,
            new_merchant_normalized,
            new_merchant_entity_id,
            new_category_id,
            new_notes,
            new_location,
            new_amount_minor,
            new_direction,
            new_event_time,
            new_instrument_id,
            tx_id
        ],
    )
    .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let changes: Vec<(&str, Option<String>, String)> = CORRECTABLE_FIELDS
        .iter()
        .filter_map(|(column, field)| {
            let (before, after) = match *column {
                "merchant_display_name" => {
                    (old_tx.merchant_display_name.clone(), new_merchant.clone())
                }
                "amount_minor" => (
                    old_tx.amount_minor.map(|v| v.to_string()),
                    new_amount_minor.map(|v| v.to_string()),
                ),
                "direction" => (old_tx.direction.clone(), new_direction.clone()),
                "best_event_time" => (
                    old_tx.best_event_time.map(|v| v.to_string()),
                    new_event_time.map(|v| v.to_string()),
                ),
                _ => (None, None),
            };
            let after = after?;
            if before.as_deref() == Some(after.as_str()) {
                return None;
            }
            Some((*field, before, after))
        })
        .collect();

    let mut contexts = Vec::new();
    for (field, before, after) in changes {
        match crate::reconciliation::audit::log_user_correction(
            conn,
            tx_id,
            field,
            before.as_deref().unwrap_or(""),
            &after,
        ) {
            Ok(Some(ctx)) => contexts.push(ctx),
            Ok(None) => {}
            Err(e) => tracing::warn!("failed to log correction for {field}: {e}"),
        }
    }

    Ok(contexts)
}

#[tauri::command]
pub async fn transactions_update(
    payload: TransactionUpdatePayload,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
    learning: tauri::State<'_, crate::learning::LearningHandle>,
    app_handle: tauri::AppHandle,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("transaction_id", &payload.transaction_id.to_string())?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let payload_tx_id = payload.transaction_id.to_string();

    let contexts = conn
        .interact(move |conn| {
            let tx_id = payload.transaction_id;
            let contexts = apply_transaction_field_update(
                conn,
                &tx_id.to_string(),
                payload.merchant_display_name,
                payload.category_id,
                payload.notes,
                payload.location,
                payload.amount_minor,
                payload.direction,
                payload.event_time,
                payload.instrument_id,
            )?;

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

            Ok::<_, crate::error::AppError>(contexts)
        })
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))??;

    let _ = crate::ipc::events::emit_event(
        &app_handle,
        crate::ipc::events::AppEvent::TransactionUpdated,
        serde_json::json!({ "transaction_id": payload_tx_id }),
    );

    let app_dir = app_handle.path().app_data_dir().ok();
    for ctx in contexts {
        crate::learning::enqueue(
            &learning,
            crate::learning::FeedbackJob {
                feedback_log_id: ctx.feedback_log_id,
                bank_name: ctx.bank_name,
                field_name: ctx.field_name,
                source_type: ctx.source_type,
                source_text: ctx.source_text.unwrap_or_default(),
                old_value: ctx.old_value,
                new_value: ctx.new_value,
                observation_id: ctx.observation_id,
                learned_from: "user_edit".to_string(),
                app_dir: app_dir.clone(),
            },
        )
        .await;
    }

    Ok("updated".to_string())
}

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

fn apply_wrong_bank_report(
    conn: &rusqlite::Connection,
    transaction_id: &str,
    domain: &str,
    bank_name: &str,
) -> Result<(), crate::error::AppError> {
    let domain = domain.trim();
    let bank_name = bank_name.trim();
    if domain.is_empty() || bank_name.is_empty() {
        return Err(crate::error::AppError::Validation(
            "a wrong-bank report needs both a sender domain and a bank name".to_string(),
        ));
    }

    let observation_id: Option<String> = conn
        .query_row(
            "SELECT id FROM transaction_observations
             WHERE canonical_transaction_id = ?1 LIMIT 1",
            rusqlite::params![transaction_id],
            |r| r.get(0),
        )
        .ok();

    let _ = crate::db::feedback_log::record_manual_correction(
        conn,
        transaction_id,
        observation_id.as_deref(),
        "sender_bank",
        None,
        bank_name,
    );

    let feedback_id: Option<String> = conn
        .query_row(
            "SELECT id FROM feedback_log
             WHERE transaction_id = ?1 AND field_name = 'sender_bank'
             ORDER BY created_at DESC LIMIT 1",
            rusqlite::params![transaction_id],
            |r| r.get(0),
        )
        .ok();

    crate::db::sender_bank_overrides::upsert(conn, domain, bank_name, None, feedback_id.as_deref())
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub async fn feedback_report_wrong_bank(
    transaction_id: String,
    domain: String,
    bank_name: String,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<(), crate::error::AppError> {
    crate::ipc::validation::validate_uuid("transaction_id", &transaction_id)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| apply_wrong_bank_report(c, &transaction_id, &domain, &bank_name))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
}

#[tauri::command]
pub async fn settings_known_bank_names() -> Result<Vec<String>, crate::error::AppError> {
    Ok(crate::ingestion::verified_senders::SenderValidator::new().all_display_names())
}

#[tauri::command]
pub async fn settings_learned_rules_list(
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<crate::db::field_rules::FieldRuleVariant>, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| crate::db::field_rules::select_all(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))
}

#[tauri::command]
pub async fn settings_learned_rules_revert(
    rule_id: String,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<(), crate::error::AppError> {
    crate::ipc::validation::validate_uuid("rule_id", &rule_id)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| crate::db::field_rules::revert(c, &rule_id, "reverted from Settings"))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))
}

#[tauri::command]
pub async fn settings_sender_overrides_list(
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<crate::db::sender_bank_overrides::SenderBankOverride>, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| crate::db::sender_bank_overrides::select_all(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))
}

#[tauri::command]
pub async fn settings_sender_overrides_revert(
    id: String,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<(), crate::error::AppError> {
    crate::ipc::validation::validate_uuid("id", &id)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| crate::db::sender_bank_overrides::deactivate(c, &id))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))
}

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
            .ok_or_else(|| {
                crate::error::AppError::Validation("transaction not found".to_string())
            })?;
        let observations =
            crate::db::transaction_observations::get_observations_for_transaction(c, &id)
                .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
        let mut match_decisions = Vec::new();
        for obs in &observations {
            if let Ok(decisions) = crate::db::match_decisions::select_by_observation_id(c, &obs.id)
            {
                match_decisions.extend(decisions);
            }
        }
        Ok(TransactionDetail {
            transaction,
            observations,
            match_decisions,
        })
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
}

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
            Err(rusqlite::Error::QueryReturnedNoRows) | Ok(None) => {}
            Err(e) => return Err(crate::error::AppError::Db(e.to_string())),
            Ok(_) => {}
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
        if alert.alert_type == "SMS Offline" {
            return Err(crate::error::AppError::Unknown(
                "No automated retry is available for this alert — please check the bank connection manually.".to_string(),
            ));
        } else if alert.alert_type == "Email Offline" {
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
    target: Option<String>,
    level: String,
    message: String,
    data: Option<String>,
) {
    let t = target.as_deref().unwrap_or("frontend");
    let data_str = data.map(|d| format!(" | data: {}", d)).unwrap_or_default();
    match t {
        "api_calls" => match level.to_lowercase().as_str() {
            "error" => tracing::error!(target: "api_calls", "{}{}", message, data_str),
            "warn" => tracing::warn!(target: "api_calls", "{}{}", message, data_str),
            "debug" => tracing::debug!(target: "api_calls", "{}{}", message, data_str),
            "trace" => tracing::trace!(target: "api_calls", "{}{}", message, data_str),
            _ => tracing::info!(target: "api_calls", "{}{}", message, data_str),
        },
        "network" => match level.to_lowercase().as_str() {
            "error" => tracing::error!(target: "network", "{}{}", message, data_str),
            "warn" => tracing::warn!(target: "network", "{}{}", message, data_str),
            "debug" => tracing::debug!(target: "network", "{}{}", message, data_str),
            "trace" => tracing::trace!(target: "network", "{}{}", message, data_str),
            _ => tracing::info!(target: "network", "{}{}", message, data_str),
        },
        _ => match level.to_lowercase().as_str() {
            "error" => tracing::error!(target: "frontend", "{}{}", message, data_str),
            "warn" => tracing::warn!(target: "frontend", "{}{}", message, data_str),
            "debug" => tracing::debug!(target: "frontend", "{}{}", message, data_str),
            "trace" => tracing::trace!(target: "frontend", "{}{}", message, data_str),
            _ => tracing::info!(target: "frontend", "{}{}", message, data_str),
        },
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
        feedback_report_wrong_bank,
        settings_known_bank_names,
        settings_learned_rules_list,
        settings_learned_rules_revert,
        settings_sender_overrides_list,
        settings_sender_overrides_revert,
        settings_pdf_passwords_list,
        settings_pdf_passwords_delete,
        correct_match,
        statements_upload,
        statements_submit_password,
        statements_retry_unprocessed,
        statements_reparse_all,
        statements_list_unprocessed,
        statements_discard,
        statements_commit_draft,
        statements_discard_draft,
        statements_get_draft_pdf,
        statements_get_draft,
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
        data::statements_get_pdf,
        data::statements_delete_pdf,
        data::reconciliation_clusters_list,
        data::reconciliation_clusters_get,
        data::reconciliation_get_unassigned_transactions,
        data::reconciliation_dismiss_unassigned_transaction,
        data::reconciliation_resolve_unassigned_transaction_manually,
        data::reconciliation_clusters_unmerge,
        data::reconciliation_clusters_bulk_resolve,
        llm::llm_get_available_models,
        llm::llm_download_model,
        llm::llm_delete_model,
        llm::llm_cancel_download,
        llm::llm_get_downloaded_models,
        llm::llm_get_active_model,
        llm::llm_set_active_model,
        llm::llm_get_hardware_info,
        llm::llm_set_parallel_slots,
        merchant_cleanup::merchant_cleanup_preview,
        merchant_cleanup::merchant_cleanup_start,
        merchant_cleanup::merchant_cleanup_cancel,
        merchant_cleanup::merchant_cleanup_revert,
        merchant_cleanup::merchant_cleanup_runs,
        merchant_cleanup::merchant_cleanup_revert_correction,
        data::instruments_list,
        data::instruments_get,
        data::instruments_create,
        data::instruments_update,
        data::instruments_archive,
        data::get_debug_metrics,
        data::check_backend_status,
        crate::health::get_health_report,
        data::auth_get_consent_history,
        data::record_consent_event,
        check_system_ram,
        crate::ingestion::oauth::is_gmail_connected,
        crate::ingestion::oauth::settings_get_connected_accounts,
        crate::ingestion::oauth::auth_google_disconnect,
        auth_get_recovery_phrase,
        auth_restore_from_recovery_phrase,
        export_logs,
        log_renderer_error,
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
        debug::debug_fetch_reconciliation_clusters,
        debug::debug_get_pipeline_state,
        debug::debug_set_gmail_poll_paused,
        debug::debug_set_scan_queue_paused,
        debug::debug_audit_scan_coverage,
        release_readiness::release_readiness_capture_snapshot,
        release_readiness::release_readiness_list_snapshots,
        crate::feedback::submit_user_feedback,
        network::settings_get_network_activity,
        crate::licensing::commands::license_get_status,
        crate::licensing::commands::license_activate,
        crate::licensing::commands::license_deactivate,
        crate::licensing::commands::billing_start_checkout,
        crate::ipc::system_warnings::get_active_system_warnings,
        crate::ipc::system_warnings::settings_dismiss_system_warning,
        crate::background_tasks::indicator::get_active_background_tasks,
        crate::licensing::commands::license_refresh
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
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
        assert!(
            !files.is_empty(),
            "the source scan itself must find files, or this test is vacuous"
        );

        let marker = format!("#[{}::{}]", "tauri", "command");

        let mut offenders = Vec::new();
        for path in &files {
            let src = std::fs::read_to_string(path).unwrap();
            let mut search_from = 0usize;
            while let Some(marker_pos) = src[search_from..].find(&marker) {
                let abs_marker = search_from + marker_pos;
                let Some(brace_offset) = src[abs_marker..]
                    .find("{\n")
                    .or_else(|| src[abs_marker..].find("{ "))
                else {
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

    fn find_function_body(name: &str) -> Option<String> {
        let src_dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
        let mut files = Vec::new();
        collect_rs_files(src_dir, &mut files);
        let needle_plain = format!("fn {name}(");
        let needle_generic = format!("fn {name}<");
        for path in &files {
            let src = std::fs::read_to_string(path).ok()?;
            let found = src
                .find(&needle_plain)
                .or_else(|| src.find(&needle_generic));
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
            "reconciliation_dismiss_unassigned_transaction",
            "reconciliation_resolve_unassigned_transaction_manually",
            "correct_match",
            "trigger_reconciliation",
            "auth_google_start",
            "auth_google_disconnect",
            "settings_pdf_passwords_delete",
            "settings_learned_rules_revert",
            "feedback_report_wrong_bank",
            "settings_sender_overrides_revert",
            "settings_delete_account",
            "settings_profile_update",
            "update_spending_limits",
            "onboarding_save_preferences",
            "scans_historical",
            "sync_force_poll_now",
        ];

        const GATE_ENFORCING_DELEGATES: [&str; 1] = ["perform_account_deletion"];

        let delegates_to_gated_helper = |body: &str| {
            GATE_ENFORCING_DELEGATES.iter().any(|delegate| {
                body.contains(delegate)
                    && find_function_body(delegate)
                        .is_some_and(|d| d.contains("assert_write_allowed"))
            })
        };

        let mut missing = Vec::new();
        for name in write_commands {
            match find_function_body(name) {
                Some(body)
                    if body.contains("assert_write_allowed")
                        || delegates_to_gated_helper(&body) => {}
                Some(_) => {
                    missing.push(format!("{name} (found, but no assert_write_allowed call)"))
                }
                None => missing.push(format!("{name} (function not found by the source scan)")),
            }
        }

        assert!(
            missing.is_empty(),
            "every write-path command must call assert_write_allowed to enforce LicenseLocked: {:#?}",
            missing
        );
    }

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
            "statements_get_pdf",
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
        conn.execute(
            "INSERT INTO transaction_observations (id, canonical_transaction_id, source_pipeline, source_record_id, fingerprint) \
             VALUES ('obs_1', 'tx_1', 'gmail_transaction', 'msg_1', 'fp_1')",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_category_update_writes_feedback_log() {
        let conn = setup_tx_test_db();
        apply_transaction_field_update(
            &conn,
            "tx_1",
            None,
            Some("cat_new".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM feedback_log WHERE transaction_id = 'tx_1' AND field_name = 'category_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let new_cat: String = conn
            .query_row(
                "SELECT category_id FROM transactions WHERE id = 'tx_1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(new_cat, "cat_new");
    }

    #[test]
    fn test_merchant_update_creates_alias() {
        let conn = setup_tx_test_db();
        apply_transaction_field_update(
            &conn,
            "tx_1",
            Some("Acme Streaming".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let alias_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM merchant_aliases WHERE alias_raw = 'ZZZTEST MKTP'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            alias_count, 1,
            "the old raw merchant text must be recorded as a new alias"
        );

        let (merchant, entity_id): (String, Option<String>) = conn
            .query_row(
                "SELECT merchant_display_name, merchant_entity_id FROM transactions WHERE id = 'tx_1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(merchant, "Acme Streaming");
        assert!(
            entity_id.is_some(),
            "the transaction must be linked to a resolved merchant entity"
        );
    }

    #[test]
    fn test_unchanged_value_produces_no_feedback() {
        let conn = setup_tx_test_db();
        let contexts = apply_transaction_field_update(
            &conn,
            "tx_1",
            Some("ZZZTEST MKTP".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(
            contexts.is_empty(),
            "a no-op edit must not enqueue learning work"
        );

        let logged: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM feedback_log WHERE transaction_id = 'tx_1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(logged, 0);
    }

    #[test]
    fn test_amount_and_date_corrections_are_captured() {
        let conn = setup_tx_test_db();
        let contexts = apply_transaction_field_update(
            &conn,
            "tx_1",
            None,
            None,
            None,
            None,
            Some(99900),
            Some("credit".to_string()),
            Some("2026-07-14".to_string()),
            None,
        )
        .unwrap();

        let fields: std::collections::HashSet<&str> =
            contexts.iter().map(|c| c.field_name.as_str()).collect();
        assert!(fields.contains("amount"), "got {fields:?}");
        assert!(fields.contains("direction"), "got {fields:?}");
        assert!(fields.contains("event_time"), "got {fields:?}");
    }

    #[test]
    fn test_category_is_logged_but_never_queued_for_learning() {
        let conn = setup_tx_test_db();
        let contexts = apply_transaction_field_update(
            &conn,
            "tx_1",
            None,
            Some("cat_new".to_string()),
            Some("a note".to_string()),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        assert!(
            contexts
                .iter()
                .all(|c| c.field_name != "category_id" && c.field_name != "notes"),
            "classification fields must not reach the rule synthesizer"
        );
        let logged: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM feedback_log
                 WHERE transaction_id = 'tx_1' AND field_name = 'category_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(logged, 1, "but the audit trail must still record it");
    }

    #[test]
    fn test_wrong_bank_report_writes_override_and_feedback() {
        let conn = setup_tx_test_db();
        apply_wrong_bank_report(&conn, "tx_1", "alerts.example.net", "Kotak Bank").unwrap();

        let overrides = crate::db::sender_bank_overrides::select_active(&conn).unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].domain, "alerts.example.net");
        assert_eq!(overrides[0].bank_name, "Kotak Bank");

        let logged: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM feedback_log
                 WHERE transaction_id = 'tx_1' AND field_name = 'sender_bank'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(logged, 1);

        let instrument: Option<String> = conn
            .query_row(
                "SELECT instrument_id FROM transactions WHERE id = 'tx_1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            instrument.as_deref(),
            Some("inst_1"),
            "a wrong-bank report must not reassign the instrument"
        );
    }

    #[test]
    fn test_wrong_bank_report_rejects_an_empty_domain() {
        let conn = setup_tx_test_db();
        assert!(apply_wrong_bank_report(&conn, "tx_1", "   ", "Kotak Bank").is_err());
    }

    #[test]
    fn test_wrong_bank_report_is_idempotent_per_domain() {
        let conn = setup_tx_test_db();
        apply_wrong_bank_report(&conn, "tx_1", "alerts.example.net", "Kotak Bank").unwrap();
        apply_wrong_bank_report(&conn, "tx_1", "alerts.example.net", "Axis Bank").unwrap();

        let overrides = crate::db::sender_bank_overrides::select_all(&conn).unwrap();
        assert_eq!(overrides.len(), 1, "one domain, one override");
        assert_eq!(overrides[0].bank_name, "Axis Bank", "the newer report wins");
    }

    #[test]
    fn test_upload_command_rejects_missing_file() {
        assert!(validate_upload_files_non_empty(&[]).is_err());
        let one_file = vec![UploadFile {
            file_bytes: vec![1, 2, 3],
            filename: "a.pdf".to_string(),
        }];
        assert!(validate_upload_files_non_empty(&one_file).is_ok());
    }

    #[test]
    fn test_no_command_returns_pdf_bytes() {
        fn struct_field_block(src: &str, struct_name: &str) -> String {
            let marker = format!("struct {struct_name} {{");
            let start = src
                .find(&marker)
                .unwrap_or_else(|| panic!("struct {struct_name} not found"));
            let body_start = start + marker.len();
            let end = src[body_start..].find('}').unwrap();
            src[body_start..body_start + end].to_string()
        }

        let commands_mod_src =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/commands/mod.rs"))
                .unwrap();
        let commands_data_src =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/commands/data.rs"))
                .unwrap();
        let statement_entries_src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/db/statement_entries.rs"
        ))
        .unwrap();

        {
            let (src, name) = (&commands_mod_src, "UploadResult");
            let block = struct_field_block(src, name);
            assert!(
                !block.contains("Vec<u8>"),
                "{name} must never carry raw byte content: {block}"
            );
        }
        for (src, name) in [
            (&commands_data_src, "StatementRecord"),
            (&commands_data_src, "StatementsPage"),
        ] {
            let block = struct_field_block(src, name);
            assert!(
                !block.contains("Vec<u8>"),
                "{name} must never carry raw byte content: {block}"
            );
        }
        let entries_block = struct_field_block(&statement_entries_src, "StatementEntriesRow");
        assert!(
            !entries_block.contains("Vec<u8>"),
            "StatementEntriesRow must never carry raw byte content"
        );
    }

    #[tokio::test]
    async fn test_no_pdf_bytes_written_to_sqlite_or_disk() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test.db");
        let pool = crate::db::init_db(db_path.clone()).await.unwrap();

        let conn = pool.get().await.unwrap();

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

        let has_blob_column = column_types
            .iter()
            .any(|t| t.to_uppercase().contains("BLOB"));
        assert!(
            !has_blob_column,
            "statements table must not have any BLOB column that could store raw PDF bytes. \
             Found BLOB columns: {:?}",
            column_types
        );

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

        let db_bytes = std::fs::read(&db_path).unwrap();
        let pdf_magic = b"%PDF";
        let db_contains_pdf = db_bytes.windows(4).any(|w| w == pdf_magic);
        assert!(
            !db_contains_pdf,
            "SQLite database file must not contain raw PDF magic bytes — \
             this would indicate PDF bytes were written to disk"
        );
    }

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

        let result = try_all_stored_passwords(b"%PDF-1.4 fake", &pool)
            .await
            .unwrap();
        assert_eq!(result, PasswordResolutionResult::PromptRequired);
    }

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

        let second_attempt = statements_discard(
            "11111111-1111-4111-8111-111111111111".to_string(),
            app.state::<deadpool_sqlite::Pool>(),
        )
        .await;
        assert!(second_attempt.is_err());
    }

    async fn seed_test_draft(pool: &deadpool_sqlite::Pool, id: &str, inst_id: &str, masked: &str) {
        let conn = pool.get().await.unwrap();
        let id = id.to_string();
        let inst_id_owned = inst_id.to_string();
        let masked_owned = masked.to_string();
        conn.interact(move |c| {
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier) VALUES (?1, 'credit_card', 'HDFC', ?2)",
                rusqlite::params![inst_id_owned, masked_owned],
            )
            .unwrap();
            let row = crate::db::statement_drafts::StatementDraftRow {
                id: id.clone(),
                origin: "manual_upload".to_string(),
                file_hash: format!("hash_{}", id),
                instrument_id: Some(inst_id_owned.clone()),
                issuer_name: Some("HDFC".to_string()),
                masked_identifier: Some(masked_owned.clone()),
                instrument_type: Some("credit_card".to_string()),
                billing_period_start: Some("2026-06-01".to_string()),
                billing_period_end: Some("2026-06-30".to_string()),
                due_date: Some("2026-07-10".to_string()),
                statement_date: None,
                current_balance: Some(100_000),
                minimum_due: Some(5_000),
                rows_json: serde_json::to_string(&vec![crate::statements::row_extractor::StatementRow {
                    transaction_date: "2026-06-05".to_string(),
                    merchant_raw: "ORIGINAL MERCHANT".to_string(),
                    amount_minor: 10000,
                    currency: "INR".to_string(),
                    direction: "debit".to_string(),
                    reference_id: None,
                    row_index: 0,
                    llm_extracted: false,
                }])
                .unwrap(),
                status: "pending_review".to_string(),
                created_at: None,
                updated_at: None,
            };
            crate::db::statement_drafts::insert(c, &row).unwrap();
        })
        .await
        .unwrap();
    }

    fn edited_metadata_fixture() -> DraftMetadataUpdate {
        DraftMetadataUpdate {
            issuer_name: "HDFC".to_string(),
            masked_identifier: "3333".to_string(),
            instrument_type: "credit_card".to_string(),
            billing_period_start: Some("2026-06-01".to_string()),
            billing_period_end: Some("2026-06-30".to_string()),
            due_date: Some("2026-07-20".to_string()),
            statement_date: Some("2026-06-30".to_string()),
            current_balance: Some(100_000),
            minimum_due: Some(5_000),
        }
    }

    fn edited_rows_fixture() -> Vec<crate::statements::row_extractor::StatementRow> {
        vec![crate::statements::row_extractor::StatementRow {
            transaction_date: "2026-06-05".to_string(),
            merchant_raw: "CORRECTED MERCHANT".to_string(),
            amount_minor: 10000,
            currency: "INR".to_string(),
            direction: "debit".to_string(),
            reference_id: None,
            row_index: 0,
            llm_extracted: false,
        }]
    }

    #[tokio::test]
    async fn test_commit_staged_draft_persists_edited_values_not_original() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();

        seed_test_draft(&pool, "draft_commit", "inst_commit", "3333").await;

        let stmt_id = commit_staged_draft(
            "draft_commit",
            edited_metadata_fixture(),
            edited_rows_fixture(),
            &pool,
            app.handle(),
        )
        .await
        .unwrap();

        let conn2 = pool.get().await.unwrap();
        let (due_date, statement_date): (Option<String>, Option<String>) = {
            let sid = stmt_id.clone();
            conn2
                .interact(move |c| {
                    c.query_row(
                        "SELECT due_date, statement_date FROM statements WHERE id = ?",
                        [&sid],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                })
                .await
                .unwrap()
                .unwrap()
        };
        assert_eq!(due_date.as_deref(), Some("2026-07-20"));
        assert_eq!(statement_date.as_deref(), Some("2026-06-30"));

        let merchant: String = conn2
            .interact(move |c| {
                c.query_row(
                    "SELECT merchant_raw FROM statement_entries WHERE statement_id = ?",
                    [&stmt_id],
                    |r| r.get(0),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(merchant, "CORRECTED MERCHANT");

        let draft_status: String = conn2
            .interact(|c| {
                c.query_row(
                    "SELECT status FROM statement_drafts WHERE id = 'draft_commit'",
                    [],
                    |r| r.get(0),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(draft_status, "committed");
    }

    #[tokio::test]
    async fn test_commit_staged_draft_twice_errors_second_time() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();

        seed_test_draft(&pool, "draft_commit2", "inst_commit2", "4444").await;

        let _ = commit_staged_draft(
            "draft_commit2",
            edited_metadata_fixture(),
            edited_rows_fixture(),
            &pool,
            app.handle(),
        )
        .await
        .unwrap();

        let second = commit_staged_draft(
            "draft_commit2",
            edited_metadata_fixture(),
            edited_rows_fixture(),
            &pool,
            app.handle(),
        )
        .await;
        assert!(second.is_err());
    }

    #[tokio::test]
    async fn test_discard_staged_draft_removes_row_and_pdf() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let app_data_dir = temp_dir.join("app_data");
        std::fs::create_dir_all(&app_data_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

        seed_test_draft(&pool, "draft_discard", "inst_discard", "5555").await;
        crate::statements::pdf_storage::store_pdf(&app_data_dir, "draft_discard", b"%PDF-fake")
            .unwrap();

        discard_staged_draft("draft_discard", &app_data_dir, &pool)
            .await
            .unwrap();

        let conn = pool.get().await.unwrap();
        let gone = conn
            .interact(|c| crate::db::statement_drafts::select_by_id(c, "draft_discard"))
            .await
            .unwrap()
            .unwrap();
        assert!(gone.is_none());
        assert!(
            crate::statements::pdf_storage::read_pdf(&app_data_dir, "draft_discard")
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_list_unprocessed_includes_awaiting_review_drafts() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

        seed_test_draft(&pool, "draft_lu", "inst_lu", "6666").await;

        let conn = pool.get().await.unwrap();
        let rows = conn
            .interact(|c| crate::db::statement_drafts::select_pending_review(c))
            .await
            .unwrap()
            .unwrap();
        let grouped = group_drafts_for_review(rows);
        assert_eq!(grouped.as_array().unwrap().len(), 1);
        assert_eq!(grouped[0]["issuer_name"], "HDFC");
        assert_eq!(grouped[0]["masked_identifier"], "6666");
    }

    #[tokio::test]
    async fn test_manual_transaction_persists_reference_id() {
        use tauri::test::{mock_builder, mock_context};

        let app_handle = mock_builder()
            .build(mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone();

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

        let instrument_id = uuid::Uuid::new_v4();
        let conn = pool.get().await.unwrap();
        conn.interact(move |c| {
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier) \
                 VALUES (?1, 'credit_card', 'HDFC', '4021')",
                rusqlite::params![instrument_id.to_string()],
            )
        })
        .await
        .unwrap()
        .unwrap();

        let payload = ManualTransactionPayload {
            amount_minor: 10000,
            currency: "INR".to_string(),
            direction: "debit".to_string(),
            event_time: "2026-06-10 12:00:00".to_string(),
            merchant_name: "Amazon".to_string(),
            instrument_id,
            reference_id: Some("REF999".to_string()),
        };

        create_manual_transaction(payload, &pool, &app_handle)
            .await
            .unwrap();

        let conn = pool.get().await.unwrap();
        let stored: Option<String> = conn
            .interact(|c| {
                c.query_row(
                    "SELECT reference_id FROM transaction_observations WHERE merchant_raw = 'Amazon'",
                    [],
                    |r| r.get(0),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored, Some("REF999".to_string()));
    }
}
