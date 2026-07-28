use chrono::Utc;
use deadpool_sqlite::Pool;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::db::processing_checkpoints::{
    claim_checkpoint_in_progress, get_checkpoint, upsert_checkpoint, ClaimOutcome,
    ProcessingCheckpointRow,
};
use crate::ingestion::gmail_client::GmailClient;
use crate::ingestion::message_processor::{MessageProcessor, ProcessResult};
use crate::ingestion::oauth::get_valid_access_token;

/// Doc 30 TASK-GMAIL-007: checkpoint every 5 processed messages.
const CHECKPOINT_INTERVAL: usize = 5;

/// Doc 19 §18 Scans group / Doc 30 TASK-API-009: `scans_cancel`. Keyed by
/// `account_id` since up to 10 accounts (Doc 03 §8.2) can have concurrent
/// scans. Checked once per checkpoint interval in `scans_historical`'s main
/// loop (the same cadence `wait_while_paused` already uses) -- didn't exist
/// at all before this task; a scan could previously only run to completion
/// or fail, never be stopped mid-flight.
fn cancelled_scans() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static CELL: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    CELL.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

fn is_scan_cancelled(account_id: &str) -> bool {
    cancelled_scans().lock().unwrap().contains(account_id)
}

fn clear_scan_cancellation(account_id: &str) {
    cancelled_scans().lock().unwrap().remove(account_id);
}

#[tauri::command]
pub async fn scans_cancel(account_id: String) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_account_id("account_id", &account_id)?;
    cancelled_scans().lock().unwrap().insert(account_id);
    Ok("cancel_requested".to_string())
}

/// Doc 19 §18 Scans group / Doc 30 TASK-API-009's `sync_get_scan_status`.
/// Named acceptance test: `test_scan_status_reflects_checkpoint_state`.
#[derive(Serialize)]
pub struct ScanStatusResponse {
    pub status: String,
    pub processed: usize,
    pub total: usize,
    pub transactions_found: usize,
    pub statements_found: usize,
    pub mandate_events_found: usize,
    pub errors: usize,
    pub pending_enrichment: usize,
}

fn checkpoint_to_status(checkpoint: Option<ProcessingCheckpointRow>) -> ScanStatusResponse {
    match checkpoint {
        None => ScanStatusResponse {
            status: "not_started".to_string(),
            processed: 0,
            total: 0,
            transactions_found: 0,
            statements_found: 0,
            mandate_events_found: 0,
            errors: 0,
            pending_enrichment: 0,
        },
        Some(cp) => {
            let state: ScanCheckpointState =
                serde_json::from_str(&cp.checkpoint_state_json).unwrap_or_default();
            ScanStatusResponse {
                status: cp.status,
                processed: state.processed_count,
                total: state.all_message_ids.len(),
                transactions_found: state.transactions_found,
                statements_found: state.statements_found,
                mandate_events_found: state.mandate_events_found,
                errors: state.errors,
                pending_enrichment: state.pending_enrichment,
            }
        }
    }
}

#[tauri::command]
pub async fn scans_status(
    account_id: String,
    pool: State<'_, Pool>,
) -> Result<ScanStatusResponse, crate::error::AppError> {
    crate::ipc::validation::validate_account_id("account_id", &account_id)?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let checkpoint = conn
        .interact(move |c| get_checkpoint(c, "historical_scan", &account_id))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    Ok(checkpoint_to_status(checkpoint))
}

/// Doc 19 §18 Scans group's `scans_resume`. `scans_historical`'s own
/// checkpoint-resume logic (a `"paused"`/`"failed"`/`"cancelled"` checkpoint
/// picks up from `state.all_message_ids`/`state.processed_count` rather than
/// re-querying Gmail) already makes re-invoking it with the same date range
/// a true resume, not a restart -- so this only needs to recover that
/// original `start_date`/`end_date` from the stored checkpoint before
/// delegating, rather than duplicating `scans_historical`'s own body.
#[tauri::command]
pub async fn scans_resume<R: tauri::Runtime>(
    app: AppHandle<R>,
    pool: State<'_, Pool>,
    account_id: String,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_account_id("account_id", &account_id)?;
    let account_id_clone = account_id.clone();
    let checkpoint = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .interact(move |c| get_checkpoint(c, "historical_scan", &account_id_clone))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let cp = checkpoint.ok_or_else(|| {
        crate::error::AppError::Validation(
            "no prior scan found for this account to resume".to_string(),
        )
    })?;
    let state: ScanCheckpointState = serde_json::from_str(&cp.checkpoint_state_json)
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    scans_historical(app, pool, account_id, state.start_date, state.end_date).await
}

fn should_checkpoint(batch_count: usize) -> bool {
    batch_count >= CHECKPOINT_INTERVAL
}

/// Doc 30 TASK-GMAIL-007 / TASK-GMAIL-002: bounded concurrent batches, max 50
/// full-message fetches in flight at once — the same figure the shared Gmail
/// quota semaphore in `gmail_client.rs` enforces globally across both the
/// live poll worker and historical scans.
const MAX_CONCURRENT_FETCHES: usize = 50;

#[derive(Clone, Serialize)]
struct ScanProgressPayload {
    account_id: String,
    processed: usize,
    total: usize,
    transactions_found: usize,
    statements_found: usize,
    mandate_events_found: usize,
    non_financial: usize,
    errors: usize,
    pending_enrichment: usize,
    error_message: Option<String>,
}

/// `pub`/pub fields for the same reason as `run_scan_batches` above --
/// `tests/historical_scan_benchmark.rs` (Doc 30 TASK-QA-002) constructs this
/// directly to drive a scan against a pre-built synthetic message-id list.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ScanCheckpointState {
    pub start_date: String,
    pub end_date: String,
    pub all_message_ids: Vec<String>,
    pub processed_count: usize,
    #[serde(default)]
    pub transactions_found: usize,
    #[serde(default)]
    pub statements_found: usize,
    #[serde(default)]
    pub mandate_events_found: usize,
    #[serde(default)]
    pub non_financial: usize,
    #[serde(default)]
    pub errors: usize,
    #[serde(default)]
    pub pending_enrichment: usize,
}

#[tauri::command]
pub async fn scans_historical<R: tauri::Runtime>(
    app: AppHandle<R>,
    pool: State<'_, Pool>,
    account_id: String,
    start_date: String,
    end_date: String,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_account_id("account_id", &account_id)?;
    crate::ipc::validation::validate_date_range(&start_date, &end_date)?;
    // Doc 22 §11.5: LOCKED blocks "all writes, including new ingestion on both
    // queues" — a historical scan is new ingestion.
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let pool = pool.inner().clone();

    let access_token = get_valid_access_token(&app, &pool, &account_id)
        .await
        .map_err(|e| crate::error::AppError::Auth(e.to_string()))?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let parsed_start =
        chrono::NaiveDate::parse_from_str(&start_date, "%Y-%m-%d").map_err(|_| {
            crate::error::AppError::Unknown("Invalid start_date format. Use YYYY-MM-DD".into())
        })?;
    let parsed_end = chrono::NaiveDate::parse_from_str(&end_date, "%Y-%m-%d").map_err(|_| {
        crate::error::AppError::Unknown("Invalid end_date format. Use YYYY-MM-DD".into())
    })?;

    if parsed_start > parsed_end {
        return Err(crate::error::AppError::Unknown(
            "start_date cannot be after end_date".into(),
        ));
    }

    // Doc 30 TASK-API-009 / Document 19: "scans_historical -- dedupe active
    // scans via SCAN_ALREADY_RUNNING." Atomically checks-and-claims the
    // in_progress slot in one statement (see `claim_checkpoint_in_progress`)
    // rather than a separate read-then-later-write -- the old version only
    // wrote `status = 'in_progress'` deep inside `run_scan`, after an
    // awaited Gmail network round-trip, leaving a wide window in which two
    // near-simultaneous calls for the same account could both pass a
    // read-only check and both start scanning.
    let existing = conn
        .interact({
            let account_id_clone = account_id.clone();
            move |c| claim_checkpoint_in_progress(c, "historical_scan", &account_id_clone)
        })
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let existing = match existing {
        ClaimOutcome::AlreadyInProgress => {
            return Err(crate::error::AppError::ScanAlreadyRunning(
                "A historical scan is already in progress for this account".into(),
            ));
        }
        ClaimOutcome::Claimed(existing) => existing,
    };

    // TASK-DESK-003: register with the global background-task indicator
    // before the scan starts running. `account_id` is a stable task id here
    // -- Doc 19 §3.6 already guarantees only one active scan per account.
    if let Some(registry) =
        app.try_state::<crate::background_tasks::indicator::BackgroundTaskRegistry>()
    {
        registry.register_or_update(
            &app,
            &account_id,
            "historical_scan",
            &format!("Historical scan: {}", account_id),
            0,
            0,
            "Starting historical scan…",
        );
    }

    tokio::spawn(async move {
        let scan_result = run_scan(
            app.clone(),
            pool.clone(),
            account_id.clone(),
            start_date,
            end_date,
            access_token,
            existing,
        )
        .await;

        // TASK-DESK-003: deregister on completion, success or failure --
        // this is the signal the frontend removes the task on, not an
        // inferred `current == total`.
        if let Some(registry) =
            app.try_state::<crate::background_tasks::indicator::BackgroundTaskRegistry>()
        {
            use crate::background_tasks::indicator::TaskStatus;
            match &scan_result {
                Ok(_) => registry.deregister(
                    &app,
                    &account_id,
                    TaskStatus::Completed,
                    "Historical scan completed",
                ),
                Err(e) => {
                    registry.deregister(&app, &account_id, TaskStatus::Failed, &e.to_string())
                }
            }
        }

        if let Err(e) = scan_result {
            tracing::error!("Historical scan failed: {}", e);

            // Mark checkpoint as failed
            if let Ok(conn) = pool.get().await {
                let _ = conn
                    .interact({
                        let acc_id = account_id.clone();
                        move |c| {
                            if let Ok(Some(mut cp)) = get_checkpoint(c, "historical_scan", &acc_id)
                            {
                                cp.status = "failed".to_string();
                                cp.updated_at = Some(chrono::Utc::now().naive_utc());
                                let _ = upsert_checkpoint(c, &cp);
                            }
                        }
                    })
                    .await;
            }

            let _ = crate::ipc::events::emit_event(
                &app,
                crate::ipc::events::AppEvent::ScanFailed,
                ScanProgressPayload {
                    account_id,
                    processed: 0,
                    total: 0,
                    transactions_found: 0,
                    statements_found: 0,
                    mandate_events_found: 0,
                    non_financial: 0,
                    errors: 1,
                    pending_enrichment: 0,
                    error_message: Some(e.to_string()),
                },
            );
        }
    });

    Ok("Scan started".to_string())
}

async fn run_scan<R: tauri::Runtime>(
    app: AppHandle<R>,
    pool: Pool,
    account_id: String,
    start_date: String,
    end_date: String,
    access_token: String,
    existing_checkpoint: Option<ProcessingCheckpointRow>,
) -> anyhow::Result<()> {
    // Doc 19 §18 Scans group: `scans_resume`/a fresh `scans_historical` call
    // for this account must not inherit a stale cancellation from a
    // previous, already-finished run.
    clear_scan_cancellation(&account_id);

    let refresher = crate::ingestion::oauth::create_token_refresher(&app, &pool, &account_id);
    let client = GmailClient::new(access_token, pool.clone(), refresher);

    let mut state = if let Some(cp) = existing_checkpoint {
        if cp.status == "paused" || cp.status == "failed" || cp.status == "cancelled" {
            serde_json::from_str::<ScanCheckpointState>(&cp.checkpoint_state_json).unwrap_or_else(
                |_| ScanCheckpointState {
                    start_date: start_date.clone(),
                    end_date: end_date.clone(),
                    ..Default::default()
                },
            )
        } else {
            ScanCheckpointState {
                start_date: start_date.clone(),
                end_date: end_date.clone(),
                ..Default::default()
            }
        }
    } else {
        ScanCheckpointState {
            start_date: start_date.clone(),
            end_date: end_date.clone(),
            ..Default::default()
        }
    };

    if state.all_message_ids.is_empty() {
        // Gmail's `before:` operator is exclusive. To make `end_date` inclusive, we add 1 day.
        let parsed_end = chrono::NaiveDate::parse_from_str(&end_date, "%Y-%m-%d")
            .unwrap_or_else(|_| chrono::Utc::now().naive_utc().date());
        let inclusive_end = parsed_end + chrono::Duration::days(1);

        let query = format!(
            "after:{} before:{}",
            start_date,
            inclusive_end.format("%Y-%m-%d")
        );
        // The search/pagination phase (`search_messages`) is one long
        // `await` from here -- for a wide date range on a large mailbox it
        // can take many sequential page fetches before returning at all,
        // and `run_scan_batches` doesn't emit its first `scan_progress`
        // until this whole phase is done and the real `total` is known.
        // Without this, the UI's counters sit frozen at "0 / 0" for however
        // long the search takes, indistinguishable from a genuine hang.
        // `on_page` fires after every page with the running count found so
        // far, so at minimum the numbers visibly move.
        let account_id_for_search_progress = account_id.clone();
        let app_for_search_progress = app.clone();
        tracing::info!("Starting search_messages with query: {}", query);
        let ids = client
            .search_messages(&query, move |found_so_far| {
                tracing::info!("search_messages found so far: {}", found_so_far);
                let _ = crate::ipc::events::emit_event(
                    &app_for_search_progress,
                    crate::ipc::events::AppEvent::ScanProgress,
                    ScanProgressPayload {
                        account_id: account_id_for_search_progress.clone(),
                        processed: 0,
                        total: found_so_far,
                        transactions_found: 0,
                        statements_found: 0,
                        mandate_events_found: 0,
                        non_financial: 0,
                        errors: 0,
                        pending_enrichment: 0,
                        error_message: None,
                    },
                );
            })
            .await?;
        tracing::info!("search_messages completed. Total messages found: {}", ids.len());
        state.all_message_ids = ids;
        state.processed_count = 0;

        let initial_cp = ProcessingCheckpointRow {
            id: Uuid::new_v4().to_string(),
            job_type: "historical_scan".to_string(),
            job_key: account_id.clone(),
            checkpoint_state_json: serde_json::to_string(&state)?,
            last_processed_token: None,
            status: "in_progress".to_string(),
            updated_at: Some(Utc::now().naive_utc()),
        };

        let conn = pool.get().await?;
        conn.interact(move |c| upsert_checkpoint(c, &initial_cp))
            .await
            .map_err(|e| anyhow::anyhow!("Interact error: {}", e))??;
    } else {
        let cp = ProcessingCheckpointRow {
            id: Uuid::new_v4().to_string(),
            job_type: "historical_scan".to_string(),
            job_key: account_id.clone(),
            checkpoint_state_json: serde_json::to_string(&state)?,
            last_processed_token: None,
            status: "in_progress".to_string(),
            updated_at: Some(Utc::now().naive_utc()),
        };

        let conn = pool.get().await?;
        conn.interact(move |c| upsert_checkpoint(c, &cp))
            .await
            .map_err(|e| anyhow::anyhow!("Interact error: {}", e))??;
    }

    let _total = state.all_message_ids.len();

    run_scan_batches(app, pool, account_id, state, client).await
}

/// `pub` (rather than private, like `run_scan`) specifically so
/// `tests/historical_scan_benchmark.rs` (Doc 30 TASK-QA-002) can drive the
/// real batch/checkpoint/concurrency loop directly against a mocked
/// `GmailClient`, without needing a full `scans_historical` OAuth+licensing
/// call chain -- the exact seam this file's own internal tests already use.
pub async fn run_scan_batches<R: tauri::Runtime>(
    app: AppHandle<R>,
    pool: Pool,
    account_id: String,
    mut state: ScanCheckpointState,
    client: GmailClient,
) -> anyhow::Result<()> {
    let total = state.all_message_ids.len();
    let to_process = state.all_message_ids.clone();
    let mut processed_count = state.processed_count;

    let client = Arc::new(client);
    let pool_arc = Arc::new(pool.clone());

    // TASK-TXN-001: resolved once per scan and threaded into every spawned
    // `process_message` call so Layer 5 (local LLM fallback) can actually
    // run during a historical scan — previously hardcoded to `None`.
    let app_dir = app.path().app_data_dir().ok();
    let llm_eligible = app
        .try_state::<crate::startup::LlmEligibility>()
        .map(|s| s.eligible)
        .unwrap_or(false);
    let layer6_tx = app
        .state::<crate::ingestion::queues::QueueHandles>()
        .layer6_tx
        .clone();

    let mut join_set: JoinSet<(String, anyhow::Result<Option<ProcessResult>>)> = JoinSet::new();

    let mut was_cancelled = false;
    let mut batch_count = 0;

    let mut batch_start_time = std::time::Instant::now();

    // Emit initial progress so the UI knows how many emails were fetched immediately
    let _ = crate::ipc::events::emit_event(
        &app,
        crate::ipc::events::AppEvent::ScanProgress,
        ScanProgressPayload {
            account_id: account_id.clone(),
            processed: processed_count,
            total,
            transactions_found: state.transactions_found,
            statements_found: state.statements_found,
            mandate_events_found: state.mandate_events_found,
            non_financial: state.non_financial,
            errors: state.errors,
            pending_enrichment: state.pending_enrichment,
            error_message: None,
        },
    );

    async fn wait_while_paused() {
        loop {
            let paused = crate::commands::debug::SCAN_QUEUE_PAUSED
                .load(std::sync::atomic::Ordering::Relaxed);
            if !paused {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }

    fn spawn_fetch(
        join_set: &mut JoinSet<(String, anyhow::Result<Option<ProcessResult>>)>,
        client: Arc<GmailClient>,
        pool_arc: Arc<Pool>,
        msg_id: String,
        app_dir: Option<std::path::PathBuf>,
        llm_eligible: bool,
        layer6_tx: tokio::sync::mpsc::Sender<crate::ingestion::queues::Layer6Job>,
    ) {
        tracing::info!("Spawning fetch for msg_id='{}'", msg_id);
        join_set.spawn(async move {
            let res = MessageProcessor::process_message(
                &pool_arc,
                &client,
                &msg_id,
                app_dir,
                llm_eligible,
                Some(layer6_tx),
            )
            .await;
            (msg_id, res)
        });
    }

    let mut ids_iter = to_process.into_iter().skip(processed_count);

    // Doc 30 TASK-GMAIL-007: keep up to MAX_CONCURRENT_FETCHES tasks in
    // flight at once. Priming here, then refilling one-for-one as each
    // completes below (rather than spawning a single task and draining it
    // to empty before spawning the next) is what actually makes the bound
    // meaningful — the previous spawn-then-immediately-drain pattern here
    // meant effective concurrency was always 1, regardless of the semaphore
    // that used to gate it.
    for _ in 0..MAX_CONCURRENT_FETCHES {
        wait_while_paused().await;
        match ids_iter.next() {
            Some(msg_id) => spawn_fetch(
                &mut join_set,
                Arc::clone(&client),
                Arc::clone(&pool_arc),
                msg_id,
                app_dir.clone(),
                llm_eligible,
                layer6_tx.clone(),
            ),
            None => break,
        }
    }

    // TASK-RT-CANCEL-RESPONSIVE: `join_set.join_next().await` alone only
    // gets a chance to notice `is_scan_cancelled` once some in-flight fetch
    // completes. If every currently in-flight task is stuck -- a Gmail
    // rate-limit backoff sleep (`execute_with_retry`'s up-to-~14s
    // exponential backoff), or a slow/unresponsive local LLM call during
    // Layer 6 classification -- a `scans_cancel` request could go
    // unnoticed for an unbounded amount of time, which is exactly the
    // "clicked Cancel and it never stops" symptom this fixes. Racing
    // `join_next()` against a 1s ticker bounds worst-case cancellation
    // latency to ~1s no matter how slow or stuck any individual in-flight
    // task is. The per-message check further down is left in place too --
    // this is additive, not a replacement.
    let mut cancel_poll = tokio::time::interval(std::time::Duration::from_secs(1));
    cancel_poll.tick().await; // first tick fires immediately; consume it up front

    loop {
        let join_res = tokio::select! {
            res = join_set.join_next() => match res {
                Some(r) => r,
                None => break,
            },
            _ = cancel_poll.tick() => {
                if is_scan_cancelled(&account_id) {
                    tracing::warn!("Scan cancelled by user for account_id='{}'", account_id);
                    clear_scan_cancellation(&account_id);
                    was_cancelled = true;
                    break;
                }
                continue;
            }
        };

        match join_res {
            Ok((msg_id, result)) => {
                tracing::info!("Finished processing msg_id='{}'", msg_id);
                match result {
                    Ok(Some(ProcessResult::TransactionAlert(extracted, boxed_obs, html))) => {
                        tracing::info!("Classified msg_id='{}' as Transaction", msg_id);
                        // Doc 15 §2 principle 7 / Doc 12 §6.2a: route to the Transaction
                        // Queue rather than processing inline — no code path may write an
                        // observation directly, only via the queue's shared worker logic.
                        state.transactions_found += 1;
                        // TASK-DB-008 fix: was "historical_scan" -- that
                        // conflated *evidence origin* (what this field
                        // means, Document 18 §4.4) with *ingestion
                        // trigger mechanism* (already tracked separately
                        // by `processing_checkpoints.job_type`). A
                        // historical scan still reads Gmail transaction
                        // alert messages, exactly like live polling.
                        let job = crate::ingestion::queues::TransactionJob {
                            obs: *boxed_obs,
                            source_pipeline: "gmail_transaction".to_string(),
                            source_record_id: msg_id.clone(),
                            connected_account_id: account_id.clone(),
                            raw_body: extracted.text_body.clone(),
                            raw_html: html,
                        };
                        let tx = app
                            .state::<crate::ingestion::queues::QueueHandles>()
                            .transaction_tx
                            .clone();
                        if tx.send(job).await.is_err() {
                            tracing::error!(
                                "Transaction Queue closed — dropping job for msg_id='{}'",
                                msg_id
                            );
                        }
                    }
                    Ok(Some(ProcessResult::StatementEmail(extracted, email_meta))) => {
                        tracing::info!("Classified msg_id='{}' as Statement", msg_id);
                        // Doc 15 §2 principle 7 / Doc 12 §7.2: email-detected statements
                        // route onto the same Statement Queue as manual uploads — no
                        // lesser-validated path for either entry point.
                        state.statements_found += 1;
                        if extracted.pdf_attachments.is_empty() {
                            tracing::warn!(
                                "StatementEmail for msg_id='{}' has has_pdf_attachment=true \
                                     but no downloadable attachment_ids — skipping parse. \
                                     skipped_parts=[{}]",
                                msg_id,
                                extracted.skipped_pdf_parts.join("; ")
                            );
                        } else {
                            for att in &extracted.pdf_attachments {
                                let filename = &att.filename;
                                // Prefer bytes Gmail already inlined in the payload
                                // (`body.data`) — no network round-trip needed, and this
                                // is exactly the case that used to be silently dropped
                                // (see `PdfAttachmentMeta::inline_bytes`'s doc comment).
                                let fetch_result: anyhow::Result<Vec<u8>> =
                                    if let Some(bytes) = &att.inline_bytes {
                                        Ok(bytes.clone())
                                    } else if let Some(att_id) = &att.attachment_id {
                                        client.fetch_attachment(&msg_id, att_id).await
                                    } else {
                                        // push_pdf_attachment never inserts an entry with
                                        // neither source, so this is unreachable in practice.
                                        continue;
                                    };
                                match fetch_result {
                                    Ok(pdf_bytes) => {
                                        // Doc 18 §4.7: the `statements` row must exist in
                                        // `queued` state before parsing begins, regardless
                                        // of entry point — same invariant as manual upload.
                                        let stmt_id = uuid::Uuid::new_v4().to_string();

                                        // TASK-GMAIL: this path previously skipped password
                                        // resolution entirely — an encrypted attachment was
                                        // enqueued with no password and died in pdfium with
                                        // PasswordError. Same choke point manual upload uses
                                        // (see `resolve_statement_password`'s doc comment).
                                        let password = match crate::statements::password::resolve_statement_password(
                                            &stmt_id,
                                            &pdf_bytes,
                                            filename,
                                            &msg_id,
                                            &pool,
                                            &app,
                                            email_meta.clone(),
                                        )
                                        .await
                                        {
                                            Ok(crate::statements::password::StatementPasswordResolution::Proceed(password)) => password,
                                            Ok(crate::statements::password::StatementPasswordResolution::PromptCreated) => {
                                                continue;
                                            }
                                            Err(e) => {
                                                tracing::error!(
                                                    "Password resolution failed for msg_id='{}' file='{}': {}",
                                                    msg_id, filename, e
                                                );
                                                continue;
                                            }
                                        };

                                        if let Ok(conn) = pool.get().await {
                                            let id = stmt_id.clone();
                                            let msg_id_for_row = msg_id.clone();
                                            let _ = conn
                                                .interact(move |c| {
                                                    crate::db::statements::insert_queued(
                                                        c,
                                                        &id,
                                                        "gmail_email",
                                                        Some(&msg_id_for_row),
                                                        None,
                                                    )
                                                })
                                                .await;
                                        }
                                        let job = crate::ingestion::queues::StatementJob {
                                            bytes: pdf_bytes,
                                            filename: filename.clone(),
                                            // Use message_id as the source_record_id /
                                            // file_hash proxy for email-sourced statements.
                                            file_hash: msg_id.clone(),
                                            stmt_id,
                                            // Doc 30 TASK-STMT-009: batch progress is a
                                            // manual-upload-batch concept only.
                                            batch_progress: None,
                                            password,
                                            origin: "email_scan".to_string(),
                                        };
                                        let st_tx = app
                                            .state::<crate::ingestion::queues::QueueHandles>()
                                            .statement_tx
                                            .clone();
                                        if st_tx.send(job).await.is_err() {
                                            tracing::error!(
                                                    "Statement Queue closed — dropping job for msg_id='{}' file='{}'",
                                                    msg_id, filename
                                                );
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "Failed to fetch PDF attachment '{}' for \
                                                 msg_id='{}': {}",
                                            filename,
                                            msg_id,
                                            e
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Ok(Some(ProcessResult::MandateEvent(
                        extracted,
                        mandate_extraction,
                        event_type,
                    ))) => {
                        tracing::info!("Classified msg_id='{}' as Mandate", msg_id);
                        // docs/superpowers/specs/2026-07-18-mandate-tracking-design.md
                        // §4.2: route to the Mandate Queue, same shared worker logic
                        // as the live-poll entry point.
                        state.mandate_events_found += 1;
                        let job = crate::ingestion::queues::MandateJob {
                            extraction: mandate_extraction,
                            event_type,
                            source_pipeline: "gmail_transaction".to_string(),
                            source_record_id: msg_id.clone(),
                            connected_account_id: account_id.clone(),
                            raw_body: extracted.text_body.clone(),
                        };
                        let mandate_tx = app
                            .state::<crate::ingestion::queues::QueueHandles>()
                            .mandate_tx
                            .clone();
                        if mandate_tx.send(job).await.is_err() {
                            tracing::error!(
                                "Mandate Queue closed — dropping job for msg_id='{}'",
                                msg_id
                            );
                        }
                    }
                    Ok(None) => {
                        tracing::info!("Classified msg_id='{}' as Non-Financial", msg_id);
                        state.non_financial += 1;
                    }
                    Ok(Some(ProcessResult::EnqueuedForEnrichment)) => {
                        tracing::info!(
                            "msg_id='{}' enqueued for background Layer 6 enrichment",
                            msg_id
                        );
                        state.pending_enrichment += 1;
                    }
                    Err(e) => {
                        state.errors += 1;
                        crate::dev_review::record(
                            "error",
                            serde_json::json!({ "id": msg_id }),
                            None,
                            None,
                            None,
                            None,
                            Some(&e.to_string()),
                            Vec::new(),
                        );
                        tracing::error!("Failed to process message {}: {}", msg_id, e);
                    }
                }
            }
            Err(e) => {
                state.errors += 1;
                crate::dev_review::record(
                    "error",
                    serde_json::json!({}),
                    None,
                    None,
                    None,
                    None,
                    Some(&format!("join_error: {e}")),
                    Vec::new(),
                );
                tracing::error!("Join error: {}", e);
            }
        }

        processed_count += 1;
        batch_count += 1;

        if should_checkpoint(batch_count) {
            state.processed_count = processed_count;

            let cp = ProcessingCheckpointRow {
                id: Uuid::new_v4().to_string(),
                job_type: "historical_scan".to_string(),
                job_key: account_id.clone(),
                checkpoint_state_json: serde_json::to_string(&state).unwrap_or_default(),
                last_processed_token: None,
                status: "in_progress".to_string(),
                updated_at: Some(Utc::now().naive_utc()),
            };

            if let Ok(conn) = pool.get().await {
                let _ = conn.interact(move |c| upsert_checkpoint(c, &cp)).await;
            }

            batch_count = 0;
            let elapsed = batch_start_time.elapsed();
            tracing::info!(
                elapsed_ms = elapsed.as_millis(),
                batch_size = CHECKPOINT_INTERVAL,
                processed = processed_count,
                total = total,
                "Historical scan batch completed"
            );
            batch_start_time = std::time::Instant::now();

            let _ = crate::ipc::events::emit_event(
                &app,
                crate::ipc::events::AppEvent::ScanProgress,
                ScanProgressPayload {
                    account_id: account_id.clone(),
                    processed: processed_count,
                    total,
                    transactions_found: state.transactions_found,
                    statements_found: state.statements_found,
                    mandate_events_found: state.mandate_events_found,
                    non_financial: state.non_financial,
                    errors: state.errors,
                    pending_enrichment: state.pending_enrichment,
                    error_message: None,
                },
            );
        }

        wait_while_paused().await;

        // Doc 19 §18 Scans group / Doc 30 TASK-API-009: `scans_cancel`.
        // Checked at the same checkpoint cadence as the pause check above
        // rather than every single message, matching how `wait_while_paused`
        // itself is only consulted here, not per-message.
        if is_scan_cancelled(&account_id) {
            clear_scan_cancellation(&account_id);
            was_cancelled = true;
            break;
        }

        if let Some(next_id) = ids_iter.next() {
            spawn_fetch(
                &mut join_set,
                Arc::clone(&client),
                Arc::clone(&pool_arc),
                next_id,
                app_dir.clone(),
                llm_eligible,
                layer6_tx.clone(),
            );
        }
    }

    // Root cause of slow cancellation: `is_scan_cancelled` breaks this loop
    // within ~1s (the `cancel_poll` race above), but up to
    // `MAX_CONCURRENT_FETCHES` (50) `process_message` tasks are still
    // running in the background at that point -- each potentially mid a
    // Gmail retry sequence that can take up to ~51s
    // (`gmail_client::execute_with_retry`'s 15s timeout x 3 attempts +
    // backoff) and each pulling connections from the same shared,
    // finite-size `pool` for its own DB work (sender reputation, audit log,
    // extraction-ladder lookups). Left running, they keep contending for
    // that pool right through the cancellation cleanup below (which also
    // needs a connection from it), so the `scan_cancelled` event the UI is
    // waiting on doesn't fire until that backlog happens to clear -- not
    // because cancellation was detected slowly, but because nothing ever
    // told those tasks to stop. `abort_all()` cuts them immediately (Tokio
    // interrupts each at its next await point), instead of leaving them to
    // run until `join_set`'s implicit drop at this function's end, which is
    // *after* cleanup and event emission.
    join_set.abort_all();

    state.processed_count = processed_count;

    // Dev-mode manual-review export (Doc: gmail_export/reviewer_app.py) --
    // flush now so a short scan's last (< FLUSH_EVERY) buffered records
    // aren't left stranded in memory. No-op in release builds.
    crate::dev_review::flush();

    if was_cancelled {
        // User requested to cancel and wipe progress so the next scan starts from scratch,
        // but keep any successfully extracted transactions/statements.
        if let Ok(conn) = pool.get().await {
            let acct_id = account_id.clone();
            let msg_ids = state.all_message_ids.clone();
            
            let _ = conn.interact(move |c| {
                if let Ok(tx) = c.transaction() {
                    // 1. Delete checkpoint so progress is completely wiped
                    let _ = tx.execute(
                        "DELETE FROM processing_checkpoints WHERE job_type = 'historical_scan' AND job_key = ?",
                        rusqlite::params![acct_id],
                    );
                    
                    // 2. Wipe unresolved items generated by THIS scan's fetched messages
                    // Chunking to stay well under SQLite's parameter limits
                    for chunk in msg_ids.chunks(900) {
                        let placeholders = vec!["?"; chunk.len()].join(", ");
                        
                        let sql_unassigned = format!(
                            "DELETE FROM unassigned_transactions WHERE observation_id IN (
                                SELECT id FROM transaction_observations WHERE source_record_id IN ({})
                            )",
                            placeholders
                        );
                        let _ = tx.execute(&sql_unassigned, rusqlite::params_from_iter(chunk.iter()));

                        let sql_obs = format!(
                            "DELETE FROM transaction_observations 
                             WHERE source_record_id IN ({}) 
                             AND canonical_transaction_id IS NULL
                             AND id NOT IN (SELECT observation_id FROM match_decisions)",
                            placeholders
                        );
                        let _ = tx.execute(&sql_obs, rusqlite::params_from_iter(chunk.iter()));

                        let sql_ignored = format!(
                            "DELETE FROM ignored_messages WHERE message_id IN ({})",
                            placeholders
                        );
                        let _ = tx.execute(&sql_ignored, rusqlite::params_from_iter(chunk.iter()));

                        let sql_unproc = format!(
                            "DELETE FROM unprocessed_statements WHERE json_extract(statement_source_json, '$.message_id') IN ({})",
                            placeholders
                        );
                        let _ = tx.execute(&sql_unproc, rusqlite::params_from_iter(chunk.iter()));
                    }
                    
                    let _ = tx.commit();
                }
            }).await;
        }
    } else {
        let final_cp = ProcessingCheckpointRow {
            id: Uuid::new_v4().to_string(),
            job_type: "historical_scan".to_string(),
            job_key: account_id.clone(),
            checkpoint_state_json: serde_json::to_string(&state).unwrap_or_default(),
            last_processed_token: None,
            status: "completed".to_string(),
            updated_at: Some(Utc::now().naive_utc()),
        };

        if let Ok(conn) = pool.get().await {
            let _ = conn
                .interact(move |c| upsert_checkpoint(c, &final_cp))
                .await;
        }
    }

    // A cancelled scan didn't actually complete -- `scan_completed` would
    // misrepresent it to the UI (progress bar shows 100%, "Sync Now" button
    // re-enables as if nothing is pending). Emits a dedicated `scan_cancelled`
    // event instead so the frontend has a definitive signal that the scan
    // has actually stopped -- previously nothing was emitted at all here,
    // leaving the UI's "Scanning..." state with no way to ever learn the
    // cancellation it requested had taken effect.
    let final_payload = ScanProgressPayload {
        account_id: account_id.clone(),
        processed: processed_count,
        total,
        transactions_found: state.transactions_found,
        statements_found: state.statements_found,
        mandate_events_found: state.mandate_events_found,
        non_financial: state.non_financial,
        errors: state.errors,
        pending_enrichment: state.pending_enrichment,
        error_message: None,
    };
    let _ = crate::ipc::events::emit_event(
        &app,
        if was_cancelled {
            crate::ipc::events::AppEvent::ScanCancelled
        } else {
            crate::ipc::events::AppEvent::ScanCompleted
        },
        final_payload,
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;
    use std::fs;
    use tauri::test::{mock_builder, mock_context};

    /// `run_scan_batches` now unconditionally sources `layer6_tx` from
    /// `QueueHandles` (Doc 2026-07-26 mail scan performance), so any test
    /// driving it through a bare `mock_builder()` app needs this managed or
    /// `app.state::<QueueHandles>()` panics with "state() called before
    /// manage()" even when no message in the test actually reaches Layer 6.
    fn test_queue_handles() -> crate::ingestion::queues::QueueHandles {
        let (transaction_tx, _) = tokio::sync::mpsc::channel(1);
        let (statement_tx, _) = tokio::sync::mpsc::channel(1);
        let (mandate_tx, _) = tokio::sync::mpsc::channel(1);
        let (layer6_tx, _) = tokio::sync::mpsc::channel(1);
        crate::ingestion::queues::QueueHandles {
            transaction_tx,
            statement_tx,
            mandate_tx,
            layer6_tx,
        }
    }

    /// Doc 30 TASK-GMAIL-007: pure, deterministic proof that the checkpoint
    /// cadence is every 5 (not 10, the value the code used to have before
    /// this fix — a wall-clock/DB-timing test can't reliably distinguish "a
    /// checkpoint fired at 5" from "only the final one fired" once fetches
    /// run concurrently, so this checks the actual threshold value directly).
    #[test]
    fn test_historical_scan_checkpoints_every_5() {
        for n in 0..CHECKPOINT_INTERVAL {
            assert!(
                !should_checkpoint(n),
                "must not checkpoint before {} processed",
                CHECKPOINT_INTERVAL
            );
        }
        assert!(should_checkpoint(CHECKPOINT_INTERVAL));
        assert!(should_checkpoint(CHECKPOINT_INTERVAL + 1));
    }

    /// Doc 30 TASK-API-009 acceptance test: `scans_status` reflects the
    /// real stored checkpoint state -- both "no checkpoint yet" and a
    /// genuine in-progress one with real progress numbers.
    #[test]
    fn test_scan_status_reflects_checkpoint_state() {
        let not_started = checkpoint_to_status(None);
        assert_eq!(not_started.status, "not_started");
        assert_eq!(not_started.processed, 0);

        let state = ScanCheckpointState {
            start_date: "2026-01-01".to_string(),
            end_date: "2026-02-01".to_string(),
            all_message_ids: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            processed_count: 2,
            transactions_found: 1,
            statements_found: 0,
            mandate_events_found: 0,
            non_financial: 1,
            errors: 0,
        pending_enrichment: 0,
        };
        let cp = ProcessingCheckpointRow {
            id: "cp_status".into(),
            job_type: "historical_scan".into(),
            job_key: "acc_1".into(),
            checkpoint_state_json: serde_json::to_string(&state).unwrap(),
            last_processed_token: None,
            status: "in_progress".into(),
            updated_at: None,
        };

        let status = checkpoint_to_status(Some(cp));
        assert_eq!(status.status, "in_progress");
        assert_eq!(status.processed, 2);
        assert_eq!(status.total, 3);
        assert_eq!(status.transactions_found, 1);
    }

    #[test]
    fn test_scan_status_reflects_pending_enrichment_count() {
        let state = ScanCheckpointState {
            start_date: "2026-01-01".to_string(),
            end_date: "2026-02-01".to_string(),
            all_message_ids: vec!["a".to_string()],
            processed_count: 1,
            pending_enrichment: 3,
            ..Default::default()
        };
        let cp = ProcessingCheckpointRow {
            id: "cp_pending".into(),
            job_type: "historical_scan".into(),
            job_key: "acc_1".into(),
            checkpoint_state_json: serde_json::to_string(&state).unwrap(),
            last_processed_token: None,
            status: "completed".into(),
            updated_at: None,
        };
        let status = checkpoint_to_status(Some(cp));
        assert_eq!(status.pending_enrichment, 3);
    }

    /// Doc 30 TASK-GMAIL-007 / Doc 19 §3.6 / TASK-API-009: only one active
    /// scan per account -- and the claim itself (not just a preceding
    /// read) is what enforces it, so a second claim while the first is
    /// still in_progress must be rejected even though both calls would see
    /// the same "not in progress" state if they read at the same time.
    #[tokio::test]
    async fn test_concurrent_scan_claim_rejected() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test_claim.db");
        let pool = init_db(db_path.clone()).await.expect("DB init failed");
        let conn = pool.get().await.unwrap();

        // First claim on a brand-new job_key: succeeds, no prior checkpoint.
        let first = conn
            .interact(|c| claim_checkpoint_in_progress(c, "historical_scan", "acc_1"))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(first, ClaimOutcome::Claimed(None)));

        // Second claim for the same job_key while still in_progress: rejected.
        let second = conn
            .interact(|c| claim_checkpoint_in_progress(c, "historical_scan", "acc_1"))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(second, ClaimOutcome::AlreadyInProgress));

        // Mark it completed, then a fresh claim succeeds again.
        conn.interact(|c| {
            upsert_checkpoint(
                c,
                &ProcessingCheckpointRow {
                    id: "cp1".into(),
                    job_type: "historical_scan".into(),
                    job_key: "acc_1".into(),
                    checkpoint_state_json: "{}".into(),
                    last_processed_token: None,
                    status: "completed".into(),
                    updated_at: None,
                },
            )
        })
        .await
        .unwrap()
        .unwrap();

        let third = conn
            .interact(|c| claim_checkpoint_in_progress(c, "historical_scan", "acc_1"))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(third, ClaimOutcome::Claimed(Some(_))));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_historical_scan_completes_and_checkpoints_final_state() {
        let app = mock_builder()
            .build(mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone();
        app.manage(test_queue_handles());

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test_scan.db");
        let pool = init_db(db_path.clone()).await.expect("DB init failed");

        let account_id = "acc_test_1".to_string();

        let mut ids = vec![];
        for i in 0..7 {
            ids.push(format!("msg_{}", i));
        }

        let state = ScanCheckpointState {
            start_date: "2023-01-01".into(),
            end_date: "2023-01-31".into(),
            all_message_ids: ids,
            processed_count: 0,
            transactions_found: 0,
            statements_found: 0,
            mandate_events_found: 0,
            non_financial: 0,
            errors: 0,
        pending_enrichment: 0,
        };

        let client = GmailClient::new("fake_token".into(), pool.clone(), None);

        // This will process all 7 messages. It should checkpoint at 5, and at 7 (completion).
        run_scan_batches(app, pool.clone(), account_id.clone(), state, client)
            .await
            .expect("run_scan_batches failed");

        let conn = pool.get().await.unwrap();
        let cp = conn
            .interact(move |c| {
                get_checkpoint(c, "historical_scan", &account_id)
                    .unwrap()
                    .unwrap()
            })
            .await
            .unwrap();

        assert_eq!(cp.status, "completed");
        let final_state: ScanCheckpointState =
            serde_json::from_str(&cp.checkpoint_state_json).unwrap();
        assert_eq!(final_state.processed_count, 7);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    /// Regression test: a cancelled scan previously emitted nothing at all
    /// once its loop broke, leaving the frontend's "Scanning..." state with
    /// no definitive signal the cancellation actually took effect. Must now
    /// emit `scan_cancelled` (not `scan_completed`, which would misrepresent
    /// a stopped-early scan as having finished) and checkpoint `"cancelled"`.
    #[tokio::test]
    async fn test_historical_scan_cancellation_emits_scan_cancelled_not_scan_completed() {
        use tauri::Listener;

        let app = mock_builder()
            .build(mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone();
        app.manage(test_queue_handles());

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test_scan_cancel.db");
        let pool = init_db(db_path.clone()).await.expect("DB init failed");

        // `scans_cancel` validates the `gmail_<uuid>` shape (unlike the
        // other tests in this module, which construct a `ScanCheckpointState`
        // directly and never pass their plain "acc_..." id through that
        // validator).
        let account_id = format!("gmail_{}", uuid::Uuid::new_v4());

        // Pre-request cancellation. `run_scan_batches` is called directly
        // here (bypassing `run_scan`'s own `clear_scan_cancellation` at scan
        // start), so this flag is already set the first time the loop's
        // per-checkpoint cancellation check runs, after the first
        // `CHECKPOINT_INTERVAL` (5) messages are processed.
        scans_cancel(account_id.clone()).await.unwrap();

        let ids: Vec<String> = (0..7).map(|i| format!("msg_{i}")).collect();
        let state = ScanCheckpointState {
            start_date: "2023-01-01".into(),
            end_date: "2023-01-31".into(),
            all_message_ids: ids,
            processed_count: 0,
            transactions_found: 0,
            statements_found: 0,
            mandate_events_found: 0,
            non_financial: 0,
            errors: 0,
        pending_enrichment: 0,
        };

        let client = GmailClient::new("fake_token".into(), pool.clone(), None);

        let captured_events = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let captured_clone = captured_events.clone();
        app.listen_any("scan_cancelled", move |_| {
            captured_clone
                .lock()
                .unwrap()
                .push("scan_cancelled".to_string());
        });
        let captured_clone2 = captured_events.clone();
        app.listen_any("scan_completed", move |_| {
            captured_clone2
                .lock()
                .unwrap()
                .push("scan_completed".to_string());
        });

        run_scan_batches(app, pool.clone(), account_id.clone(), state, client)
            .await
            .expect("run_scan_batches failed");

        let conn = pool.get().await.unwrap();
        let cp_opt = conn
            .interact(move |c| {
                get_checkpoint(c, "historical_scan", &account_id)
                    .unwrap()
            })
            .await
            .unwrap();
        
        // Assert the checkpoint was completely deleted so progress starts from 0 next time
        assert!(cp_opt.is_none(), "Checkpoint should be deleted on cancel");

        let captured = captured_events.lock().unwrap();
        assert!(
            captured.contains(&"scan_cancelled".to_string()),
            "expected scan_cancelled to fire, got {:?}",
            *captured
        );
        assert!(
            !captured.contains(&"scan_completed".to_string()),
            "scan_completed must NOT fire for a cancelled scan, got {:?}",
            *captured
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_historical_scan_resumes_from_checkpoint() {
        let app = mock_builder()
            .build(mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone();
        app.manage(test_queue_handles());

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test_scan_resume.db");
        let pool = init_db(db_path.clone()).await.expect("DB init failed");

        let account_id = "acc_test_2".to_string();

        let mut ids = vec![];
        for i in 0..12 {
            ids.push(format!("msg_{}", i));
        }

        // Start from 5 processed
        let state = ScanCheckpointState {
            start_date: "2023-01-01".into(),
            end_date: "2023-01-31".into(),
            all_message_ids: ids,
            processed_count: 5,
            transactions_found: 0,
            statements_found: 0,
            mandate_events_found: 0,
            non_financial: 0,
            errors: 0,
        pending_enrichment: 0,
        };

        let client = GmailClient::new("fake_token".into(), pool.clone(), None);

        run_scan_batches(app, pool.clone(), account_id.clone(), state, client)
            .await
            .expect("run_scan_batches failed");

        let conn = pool.get().await.unwrap();
        let cp = conn
            .interact(move |c| {
                get_checkpoint(c, "historical_scan", &account_id)
                    .unwrap()
                    .unwrap()
            })
            .await
            .unwrap();

        assert_eq!(cp.status, "completed");
        let final_state: ScanCheckpointState =
            serde_json::from_str(&cp.checkpoint_state_json).unwrap();

        // Processed count should be 12 total, meaning it resumed and finished
        assert_eq!(final_state.processed_count, 12);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_overlapping_scan_dedupes_at_observation_layer() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test_scan_dedupe.db");
        let pool = init_db(db_path.clone()).await.expect("DB init failed");

        let msg_id = "msg_dup_123".to_string();
        let fingerprint = format!("hist_scan_{}", msg_id);

        let row = crate::db::transaction_observations::TransactionObservationsRow {
            id: Uuid::new_v4().to_string(),
            canonical_transaction_id: None,
            source_pipeline: Some("gmail_transaction".to_string()),
            source_record_id: Some(msg_id.clone()),
            source_message_id: Some(msg_id.clone()),
            source_thread_id: None,
            statement_id: None,
            statement_entry_id: None,
            instrument_id: None,
            direction: None,
            amount: None,
            amount_minor: Some(100),
            currency: None,
            event_time: None,
            event_time_confidence: None,
            posting_date: None,
            merchant_raw: Some("Test Merchant".to_string()),
            merchant_normalized: None,
            reference_id: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            timezone_at_ingestion: None,
            fingerprint: Some(fingerprint.clone()),
            extraction_method: Some("historical_scan".to_string()),
            confidence_score: None,
            raw_payload_json: None,
            parser_version: None,
            emi_total_installments: None,
            emi_installment_number: None,
            emi_original_amount_minor: None,
            is_deleted: false,
            created_at: Some(Utc::now().naive_utc()),
            updated_at: Some(Utc::now().naive_utc()),
        };

        let conn = pool.get().await.unwrap();
        // Insert first time
        conn.interact({
            let row = row.clone();
            move |c| crate::db::transaction_observations::insert_observation(c, &row).unwrap()
        })
        .await
        .unwrap();

        // Insert second time should fail with unique constraint on fingerprint
        let res = conn
            .interact({
                let row2 = row.clone();
                move |c| crate::db::transaction_observations::insert_observation(c, &row2)
            })
            .await
            .unwrap();

        assert!(res.is_err());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
