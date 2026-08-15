//! Backfills months or years of mail on demand.
//!
//! The long-running counterpart to incremental polling, used at onboarding and
//! when a user widens their date range. Because a scan can run for hours it is
//! checkpointed continuously, cancellable, and resumable from where it stopped.
//!
//! The coverage audit exists to answer a question the progress counter cannot:
//! whether a date range was genuinely scanned end to end, or whether a gap was
//! left behind by an interruption.
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

const CHECKPOINT_INTERVAL: usize = 5;

/// Set of accounts whose scans have been asked to stop.
///
/// In-memory: a cancellation applies to the running scan, and a restart begins
/// afresh from its checkpoint rather than inheriting a stale cancellation.
fn cancelled_scans() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static CELL: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    CELL.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// Whether this account's scan has been cancelled.
fn is_scan_cancelled(account_id: &str) -> bool {
    cancelled_scans().lock().unwrap().contains(account_id)
}

/// Clears a cancellation once the scan has actually stopped.
fn clear_scan_cancellation(account_id: &str) {
    cancelled_scans().lock().unwrap().remove(account_id);
}

/// Blocks while the scan is paused, reporting whether it was cancelled meanwhile.
async fn wait_while_paused(account_id: &str) -> bool {
    loop {
        let paused =
            crate::commands::debug::SCAN_QUEUE_PAUSED.load(std::sync::atomic::Ordering::Relaxed);
        if !paused {
            return false;
        }
        if is_scan_cancelled(account_id) {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

#[tauri::command]
/// Requests cancellation of a running scan.
///
/// Cooperative: the flag is set here and the scan loop stops at its next
/// checkpoint, so it halts at a consistent point rather than mid-write.
pub async fn scans_cancel(account_id: String) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_account_id("account_id", &account_id)?;
    cancelled_scans().lock().unwrap().insert(account_id);
    Ok("cancel_requested".to_string())
}

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

/// Projects a checkpoint into the status the frontend displays.
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
/// Reports the current scan status for an account.
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

#[tauri::command]
/// Resumes a scan from its last checkpoint.
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

/// Whether enough messages have been processed to warrant a checkpoint.
///
/// Checkpointing every message would dominate the scan's cost; too rarely and an
/// interruption discards more work than necessary.
fn should_checkpoint(batch_count: usize) -> bool {
    batch_count >= CHECKPOINT_INTERVAL
}

const MAX_CONCURRENT_FETCHES: usize = 25;

const MAX_SENDER_CLAUSE_CHARS: usize = 1500;

/// Builds Gmail queries scoped to known financial sender domains.
///
/// Querying by sender is far cheaper than fetching a date range and filtering
/// locally, since it moves the selection to Gmail's index.
fn build_sender_scoped_queries(date_range: &str, domains: &[String]) -> Vec<String> {
    if domains.is_empty() {
        return vec![date_range.to_string()];
    }

    let mut queries = Vec::new();
    let mut chunk: Vec<&str> = Vec::new();
    let mut chunk_len = 0usize;

    for domain in domains {
        let cost = domain.len() + 9;
        if !chunk.is_empty() && chunk_len + cost > MAX_SENDER_CLAUSE_CHARS {
            queries.push(format_sender_query(date_range, &chunk));
            chunk.clear();
            chunk_len = 0;
        }
        chunk.push(domain);
        chunk_len += cost;
    }
    if !chunk.is_empty() {
        queries.push(format_sender_query(date_range, &chunk));
    }
    queries.push(build_rescue_subject_query(date_range));
    queries
}

/// Formats one sender-scoped Gmail query.
fn format_sender_query(date_range: &str, domains: &[&str]) -> String {
    let clause = domains
        .iter()
        .map(|d| format!("from:{d}"))
        .collect::<Vec<_>>()
        .join(" OR ");
    format!("({clause}) {date_range}")
}

/// Builds a subject-based query to catch mail from unknown senders.
///
/// The rescue pass: a bank whose domain is not yet known would be missed entirely
/// by sender-scoped queries alone.
fn build_rescue_subject_query(date_range: &str) -> String {
    let clause = crate::ingestion::content_classifier::RESCUE_SUBJECT_TERMS
        .iter()
        .map(|t| format!("subject:({t})"))
        .collect::<Vec<_>>()
        .join(" OR ");
    format!("({clause}) {date_range}")
}

#[derive(Debug, Serialize)]
pub struct ScanCoverageAudit {
    pub unfiltered_total: usize,
    pub filtered_total: usize,
    pub excluded_total: usize,
    pub excluded_checked: usize,
    pub missed_total: usize,
    pub missed_samples: Vec<String>,
}

/// Audits whether a date range was genuinely scanned end to end.
///
/// Answers what the progress counter cannot -- whether an interruption left a gap
/// in coverage that would otherwise show up only as permanently missing
/// transactions.
pub async fn audit_scan_coverage(
    pool: &Pool,
    client: &GmailClient,
    start_date: &str,
    end_date: &str,
) -> anyhow::Result<ScanCoverageAudit> {
    let parsed_end = chrono::NaiveDate::parse_from_str(end_date, "%Y-%m-%d")
        .unwrap_or_else(|_| chrono::Utc::now().naive_utc().date());
    let inclusive_end = parsed_end + chrono::Duration::days(1);
    let date_range = format!(
        "after:{} before:{}",
        start_date,
        inclusive_end.format("%Y-%m-%d")
    );

    let mut sender_domains =
        crate::ingestion::message_processor::get_sender_validator().registry_domains();
    if let Ok(conn) = pool.get().await {
        if let Ok(Ok(rows)) = conn
            .interact(|c| crate::db::sender_reputation::select_approved_domains(c))
            .await
        {
            sender_domains.extend(rows.into_iter().map(|r| r.domain.to_lowercase()));
        }
    }
    sender_domains.sort();
    sender_domains.dedup();

    let filtered: std::collections::HashSet<String> = {
        let mut acc = std::collections::HashSet::new();
        for q in build_sender_scoped_queries(&date_range, &sender_domains) {
            acc.extend(client.search_messages(&q, |_| {}).await?);
        }
        acc
    };

    let unfiltered = client.search_messages(&date_range, |_| {}).await?;
    let unfiltered_total = unfiltered.len();

    let excluded: Vec<String> = unfiltered
        .into_iter()
        .filter(|id| !filtered.contains(id))
        .collect();
    let excluded_total = excluded.len();

    let mut excluded_checked = 0usize;
    let mut missed_total = 0usize;
    let mut missed_samples = Vec::new();

    for msg_id in &excluded {
        let Ok(msg) = client
            .fetch_message(
                msg_id,
                crate::ingestion::gmail_client::FetchFormat::Metadata,
            )
            .await
        else {
            continue;
        };
        excluded_checked += 1;

        let domain =
            crate::ingestion::message_processor::MessageProcessor::extract_sender_domain(&msg);
        let (approved, overrides) = match (&domain, pool.get().await) {
            (Some(_), Ok(conn)) => conn
                .interact(move |c| {
                    (
                        crate::db::sender_reputation::select_approved_domains(c)
                            .unwrap_or_default(),
                        crate::db::sender_bank_overrides::select_active(c).unwrap_or_default(),
                    )
                })
                .await
                .unwrap_or((Vec::new(), Vec::new())),
            _ => (Vec::new(), Vec::new()),
        };

        let verdict = crate::ingestion::message_processor::MessageProcessor::evaluate_metadata_gate(
            &msg, &approved, &overrides,
        );
        if matches!(
            verdict,
            crate::ingestion::verified_senders::SenderVerificationResult::VerifiedTransactionCandidate(_)
                | crate::ingestion::verified_senders::SenderVerificationResult::VerifiedStatementCandidate(_)
        ) {
            missed_total += 1;
            if missed_samples.len() < 50 {
                missed_samples.push(format!(
                    "{} | {} | {}",
                    msg_id,
                    domain.unwrap_or_else(|| "?".into()),
                    crate::ingestion::message_processor::MessageProcessor::header_value(
                        &msg, "subject"
                    )
                ));
            }
        }
    }

    Ok(ScanCoverageAudit {
        unfiltered_total,
        filtered_total: filtered.len(),
        excluded_total,
        excluded_checked,
        missed_total,
        missed_samples,
    })
}

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
/// Entry point that starts a historical scan for an account.
pub async fn scans_historical<R: tauri::Runtime>(
    app: AppHandle<R>,
    pool: State<'_, Pool>,
    account_id: String,
    start_date: String,
    end_date: String,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_account_id("account_id", &account_id)?;
    crate::ipc::validation::validate_date_range(&start_date, &end_date)?;
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

/// Runs the scan, checkpointing as it progresses.
async fn run_scan<R: tauri::Runtime>(
    app: AppHandle<R>,
    pool: Pool,
    account_id: String,
    start_date: String,
    end_date: String,
    access_token: String,
    existing_checkpoint: Option<ProcessingCheckpointRow>,
) -> anyhow::Result<()> {
    clear_scan_cancellation(&account_id);

    let refresher = crate::ingestion::oauth::create_token_refresher(&app, &pool, &account_id);
    let client = GmailClient::new(access_token, pool.clone(), refresher);

    let mut state = if let Some(cp) = existing_checkpoint {
        // Restore state from the checkpoint for paused, failed, cancelled, or
        // in-progress runs. The "in_progress" case covers a crash: the app never
        // updated the status to "paused" or "completed" before it exited, so the
        // row is still "in_progress" at the next launch. Treating it like any other
        // resumable status lets a post-crash restart continue from where it stopped
        // rather than re-scanning the entire date range from scratch.
        if cp.status == "paused"
            || cp.status == "failed"
            || cp.status == "cancelled"
            || cp.status == "in_progress"
        {

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
        let parsed_end = chrono::NaiveDate::parse_from_str(&end_date, "%Y-%m-%d")
            .unwrap_or_else(|_| chrono::Utc::now().naive_utc().date());
        let inclusive_end = parsed_end + chrono::Duration::days(1);

        let date_range = format!(
            "after:{} before:{}",
            start_date,
            inclusive_end.format("%Y-%m-%d")
        );

        let mut sender_domains =
            crate::ingestion::message_processor::get_sender_validator().registry_domains();
        if let Ok(conn) = pool.get().await {
            if let Ok(Ok(rows)) = conn
                .interact(|c| crate::db::sender_reputation::select_approved_domains(c))
                .await
            {
                sender_domains.extend(rows.into_iter().map(|r| r.domain.to_lowercase()));
            }
        }
        sender_domains.sort();
        sender_domains.dedup();

        let queries = build_sender_scoped_queries(&date_range, &sender_domains);
        tracing::info!(
            domains = sender_domains.len(),
            queries = queries.len(),
            "Scoping scan to known sender domains"
        );
        let mut ids: Vec<String> = Vec::new();
        let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for query in &queries {
            let account_id_for_search_progress = account_id.clone();
            let app_for_search_progress = app.clone();
            let carried = ids.len();
            tracing::info!("Starting search_messages with query: {}", query);
            let chunk = client
                .search_messages(query, move |found_so_far| {
                    let running = carried + found_so_far;
                    tracing::info!("search_messages found so far: {}", running);
                    let _ = crate::ipc::events::emit_event(
                        &app_for_search_progress,
                        crate::ipc::events::AppEvent::ScanProgress,
                        ScanProgressPayload {
                            account_id: account_id_for_search_progress.clone(),
                            processed: 0,
                            total: running,
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
            for id in chunk {
                if seen_ids.insert(id.clone()) {
                    ids.push(id);
                }
            }
        }
        tracing::info!(
            "search_messages completed. Total messages found: {}",
            ids.len()
        );
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

/// Processes the scan in batches, honouring pause and cancellation.
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

    let scan_batcher: crate::ingestion::message_processor::ScanBatcherHandle = Arc::new(
        tokio::sync::Mutex::new(crate::ingestion::scan_db_batcher::ScanDbBatcher::new()),
    );

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

    #[allow(clippy::too_many_arguments)]
    /// Spawns the concurrent message-fetch task.
    ///
    /// Fetching runs ahead of processing so network latency and extraction overlap
    /// rather than alternating.
    fn spawn_fetch(
        join_set: &mut JoinSet<(String, anyhow::Result<Option<ProcessResult>>)>,
        client: Arc<GmailClient>,
        pool_arc: Arc<Pool>,
        msg_id: String,
        app_dir: Option<std::path::PathBuf>,
        llm_eligible: bool,
        layer6_tx: tokio::sync::mpsc::Sender<crate::ingestion::queues::Layer6Job>,
        scan_batcher: crate::ingestion::message_processor::ScanBatcherHandle,
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
                Some(&scan_batcher),
            )
            .await;
            (msg_id, res)
        });
    }

    let mut ids_iter = to_process.into_iter().skip(processed_count);

    for _ in 0..MAX_CONCURRENT_FETCHES {
        if wait_while_paused(&account_id).await {
            break;
        }
        match ids_iter.next() {
            Some(msg_id) => spawn_fetch(
                &mut join_set,
                Arc::clone(&client),
                Arc::clone(&pool_arc),
                msg_id,
                app_dir.clone(),
                llm_eligible,
                layer6_tx.clone(),
                Arc::clone(&scan_batcher),
            ),
            None => break,
        }
    }

    let mut cancel_poll = tokio::time::interval(std::time::Duration::from_secs(1));
    cancel_poll.tick().await;

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
                    Ok(Some(ProcessResult::TransactionAlert(extracted, boxed_obs, email_meta))) => {
                        tracing::info!("Classified msg_id='{}' as Transaction", msg_id);
                        state.transactions_found += 1;
                        let job = crate::ingestion::queues::TransactionJob {
                            obs: *boxed_obs,
                            source_pipeline: "gmail_transaction".to_string(),
                            source_record_id: msg_id.clone(),
                            connected_account_id: account_id.clone(),
                            raw_body: extracted.text_body.clone(),
                            email_meta: Some(email_meta),
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
                            let client = Arc::clone(&client);
                            let pool = pool.clone();
                            let app = app.clone();
                            let msg_id = msg_id.clone();
                            let email_meta = email_meta.clone();
                            let attachments = extracted.pdf_attachments.clone();
                            tokio::spawn(async move {
                                for att in &attachments {
                                    let filename = &att.filename;
                                    let fetch_result: anyhow::Result<Vec<u8>> =
                                        if let Some(bytes) = &att.inline_bytes {
                                            Ok(bytes.clone())
                                        } else if let Some(att_id) = &att.attachment_id {
                                            client.fetch_attachment(&msg_id, att_id).await
                                        } else {
                                            continue;
                                        };
                                    match fetch_result {
                                        Ok(pdf_bytes) => {
                                            let file_hash = match crate::statements::duplicate_check::hash_email_attachment_if_new(
                                                &pdf_bytes, filename, &msg_id, &pool,
                                            )
                                            .await
                                            {
                                                Some(h) => h,
                                                None => continue,
                                            };

                                            let stmt_id = uuid::Uuid::new_v4().to_string();

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
                                            if let Ok(dir) = app.path().app_data_dir() {
                                                if let Err(e) =
                                                    crate::statements::pdf_storage::store_pdf(
                                                        &dir, &stmt_id, &pdf_bytes,
                                                    )
                                                {
                                                    tracing::warn!(
                                                        "Failed to stage statement PDF for stmt_id='{}': {} — skipping",
                                                        stmt_id, e
                                                    );
                                                    continue;
                                                }
                                            } else {
                                                tracing::warn!(
                                                    "Could not resolve app data dir to stage statement PDF for stmt_id='{}' — skipping",
                                                    stmt_id
                                                );
                                                continue;
                                            }
                                            drop(pdf_bytes);

                                            let job = crate::ingestion::queues::StatementJob {
                                                filename: filename.clone(),
                                                file_hash,
                                                stmt_id,
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
                            });
                        }
                    }
                    Ok(Some(ProcessResult::MandateEvent(
                        extracted,
                        mandate_extraction,
                        event_type,
                    ))) => {
                        tracing::info!("Classified msg_id='{}' as Mandate", msg_id);
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
                        tracing::error!("Failed to process message {}: {}", msg_id, e);

                        let failed_row = crate::db::scan_failed_messages::ScanFailedMessageRow {
                            id: Uuid::new_v4().to_string(),
                            account_id: account_id.clone(),
                            msg_id: msg_id.clone(),
                            error: e.to_string(),
                            failed_at: None,
                        };
                        if let Ok(conn) = pool.get().await {
                            let _ = conn
                                .interact(move |c| {
                                    crate::db::scan_failed_messages::insert(c, &failed_row)
                                })
                                .await;
                        }
                    }
                }
            }
            Err(e) => {
                state.errors += 1;
                tracing::error!("Join error: {}", e);
            }
        }

        processed_count += 1;
        batch_count += 1;

        if should_checkpoint(batch_count) {
            state.processed_count = processed_count;

            if let Err(e) = scan_batcher.lock().await.flush(&pool).await {
                tracing::warn!("scan_batcher flush failed (best-effort): {}", e);
            }

            let key = account_id.clone();
            let (p, t, s, m, n, e, pe) = (
                processed_count,
                state.transactions_found,
                state.statements_found,
                state.mandate_events_found,
                state.non_financial,
                state.errors,
                state.pending_enrichment,
            );
            if let Ok(conn) = pool.get().await {
                let _ = conn
                    .interact(move |c| {
                        crate::db::processing_checkpoints::patch_scan_progress(
                            c, &key, p, t, s, m, n, e, pe,
                        )
                    })
                    .await;
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

        wait_while_paused(&account_id).await;

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
                Arc::clone(&scan_batcher),
            );
        }
    }

    if let Err(e) = scan_batcher.lock().await.flush(&pool).await {
        tracing::warn!("scan_batcher final flush failed (best-effort): {}", e);
    }

    join_set.abort_all();

    state.processed_count = processed_count;

    if was_cancelled {
        if let Ok(conn) = pool.get().await {
            let acct_id = account_id.clone();
            let msg_ids = state.all_message_ids.clone();

            let _ = conn.interact(move |c| {
                if let Ok(tx) = c.transaction() {
                    let _ = tx.execute(
                        "DELETE FROM processing_checkpoints WHERE job_type = 'historical_scan' AND job_key = ?",
                        rusqlite::params![acct_id],
                    );

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

    #[cfg(debug_assertions)]
    {
        if !was_cancelled {
            if let Err(e) = export_unassigned_transactions_for_dev(&app, &pool).await {
                tracing::warn!("Failed to export unassigned transactions for dev: {}", e);
            }
        }
    }

    Ok(())
}

#[cfg(debug_assertions)]
async fn export_unassigned_transactions_for_dev<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &deadpool_sqlite::Pool,
) -> anyhow::Result<()> {
    let unassigned = pool
        .get()
        .await?
        .interact(|conn| crate::db::unassigned_transactions::select_open_with_context(conn))
        .await
        .map_err(|e| anyhow::anyhow!("Interact error: {}", e))??;

    tracing::info!("DEV ONLY: export_unassigned_transactions_for_dev called, found {} unassigned transactions", unassigned.len());

    if unassigned.is_empty() {
        tracing::info!("DEV ONLY: No unassigned transactions to export");
        return Ok(());
    }

    match app.path().app_data_dir() {
        Ok(app_dir) => {
            let file_path = app_dir.join("logs").join("unassigned_transactions_dump.json");
            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            
            let json_data = serde_json::to_string_pretty(&unassigned)?;
            std::fs::write(&file_path, json_data)?;
            
            tracing::info!(
                "DEV ONLY: Exported {} unassigned transactions to {:?}",
                unassigned.len(),
                file_path
            );
        }
        Err(e) => {
            tracing::error!("DEV ONLY: Failed to get app_data_dir: {}", e);
        }
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;
    use std::fs;
    use tauri::test::{mock_builder, mock_context};

    #[tokio::test]
    async fn paused_scan_still_observes_cancellation() {
        use std::sync::atomic::Ordering;
        let account_id = "acct_paused_cancel";
        let paused = &crate::commands::debug::SCAN_QUEUE_PAUSED;

        paused.store(false, Ordering::Relaxed);
        assert!(!wait_while_paused(account_id).await);

        paused.store(true, Ordering::Relaxed);
        cancelled_scans()
            .lock()
            .unwrap()
            .insert(account_id.to_string());
        let cancelled = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            wait_while_paused(account_id),
        )
        .await
        .expect("a cancelled scan must not block on the pause flag");
        assert!(cancelled);

        assert!(is_scan_cancelled(account_id));

        clear_scan_cancellation(account_id);
        paused.store(false, Ordering::Relaxed);
    }

    #[test]
    fn sender_scoped_queries_cover_every_domain_within_the_size_budget() {
        let date_range = "after:2026-05-28 before:2026-07-29";

        assert_eq!(
            build_sender_scoped_queries(date_range, &[]),
            vec![date_range.to_string()]
        );

        let domains: Vec<String> = (0..204)
            .map(|i| format!("bank{i:03}.example.com"))
            .collect();
        let queries = build_sender_scoped_queries(date_range, &domains);

        assert!(queries.len() > 1, "204 domains must split into chunks");
        for q in &queries {
            assert!(
                q.len() <= MAX_SENDER_CLAUSE_CHARS + date_range.len() + 16,
                "chunk overshot the URL budget: {} chars",
                q.len()
            );
            assert!(q.ends_with(date_range), "every chunk keeps the date range");
        }
        for d in &domains {
            assert!(
                queries.iter().any(|q| q.contains(&format!("from:{d}"))),
                "domain {d} was dropped from the query set"
            );
        }

        assert!(
            queries.iter().any(|q| q.contains("subject:(debited)")),
            "sender-scoped queries must still include the Gate 1 subject rescue"
        );
    }

    #[test]
    fn subject_terms_cover_classifier_phrases() {
        use crate::ingestion::content_classifier::{ContentClassifier, RESCUE_SUBJECT_TERMS};

        // The invariant that stops a transaction being lost outright: anything
        // classify() would accept must be retrievable from an unknown sender.
        let unreachable = ContentClassifier::phrases_unreachable_by(RESCUE_SUBJECT_TERMS);
        assert!(
            unreachable.is_empty(),
            "classifier accepts phrases the rescue query can never fetch: {unreachable:?}"
        );

        let q = build_rescue_subject_query("after:2026-01-01 before:2026-07-29");
        for term in RESCUE_SUBJECT_TERMS {
            assert!(
                q.contains(&format!("subject:({term})")),
                "rescue query is missing {term:?}"
            );
        }
        assert!(
            q.len() <= MAX_SENDER_CLAUSE_CHARS,
            "rescue query overshot the URL budget: {} chars",
            q.len()
        );
    }

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

    #[tokio::test]
    async fn test_concurrent_scan_claim_rejected() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test_claim.db");
        let pool = init_db(db_path.clone()).await.expect("DB init failed");
        let conn = pool.get().await.unwrap();

        let first = conn
            .interact(|c| claim_checkpoint_in_progress(c, "historical_scan", "acc_1"))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(first, ClaimOutcome::Claimed(None)));

        let second = conn
            .interact(|c| claim_checkpoint_in_progress(c, "historical_scan", "acc_1"))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(second, ClaimOutcome::AlreadyInProgress));

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

    #[test]
    fn patching_scan_progress_updates_counters_without_touching_the_id_list() {
        let conn = crate::db::test_helpers::setup_test_db();

        let ids: Vec<String> = (0..500).map(|i| format!("msg_{i}")).collect();
        let state = ScanCheckpointState {
            start_date: "2023-01-01".into(),
            end_date: "2023-01-31".into(),
            all_message_ids: ids.clone(),
            processed_count: 0,
            ..Default::default()
        };
        crate::db::processing_checkpoints::upsert_checkpoint(
            &conn,
            &ProcessingCheckpointRow {
                id: "cp_1".into(),
                job_type: "historical_scan".into(),
                job_key: "acct_1".into(),
                checkpoint_state_json: serde_json::to_string(&state).unwrap(),
                last_processed_token: None,
                status: "in_progress".into(),
                updated_at: None,
            },
        )
        .unwrap();

        crate::db::processing_checkpoints::patch_scan_progress(
            &conn, "acct_1", 42, 7, 3, 2, 11, 1, 5,
        )
        .unwrap();

        let cp =
            crate::db::processing_checkpoints::get_checkpoint(&conn, "historical_scan", "acct_1")
                .unwrap()
                .unwrap();
        let patched: ScanCheckpointState =
            serde_json::from_str(&cp.checkpoint_state_json).expect("must still deserialize");

        assert_eq!(patched.all_message_ids, ids, "the work queue must survive");
        assert_eq!(patched.start_date, "2023-01-01");
        assert_eq!(patched.processed_count, 42);
        assert_eq!(patched.transactions_found, 7);
        assert_eq!(patched.statements_found, 3);
        assert_eq!(patched.mandate_events_found, 2);
        assert_eq!(patched.non_financial, 11);
        assert_eq!(patched.errors, 1);
        assert_eq!(patched.pending_enrichment, 5);

        crate::db::processing_checkpoints::patch_scan_progress(
            &conn,
            "acct_missing",
            1,
            0,
            0,
            0,
            0,
            0,
            0,
        )
        .unwrap();
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

    #[tokio::test]
    async fn test_exhausted_fetch_failure_is_persisted_to_scan_failed_messages() {
        let app = mock_builder()
            .build(mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone();
        app.manage(test_queue_handles());

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test_scan_failed.db");
        let pool = init_db(db_path.clone()).await.expect("DB init failed");

        let account_id = "acc_failed_test".to_string();
        let state = ScanCheckpointState {
            start_date: "2023-01-01".into(),
            end_date: "2023-01-31".into(),
            all_message_ids: vec!["msg_a".to_string(), "msg_b".to_string()],
            processed_count: 0,
            ..Default::default()
        };

        let client = GmailClient::new("fake_token".into(), pool.clone(), None);

        run_scan_batches(app, pool.clone(), account_id.clone(), state, client)
            .await
            .expect("run_scan_batches failed");

        let conn = pool.get().await.unwrap();
        let account_id_for_query = account_id.clone();
        let failed = conn
            .interact(move |c| {
                crate::db::scan_failed_messages::select_by_account(c, &account_id_for_query)
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(failed.len(), 2);
        let mut msg_ids: Vec<String> = failed.iter().map(|r| r.msg_id.clone()).collect();
        msg_ids.sort();
        assert_eq!(msg_ids, vec!["msg_a".to_string(), "msg_b".to_string()]);
        assert!(
            failed[0].error.contains("401") || failed[0].error.contains("Unauthorized"),
            "expected a 401/Unauthorized error string, got: {}",
            failed[0].error
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

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

        let account_id = format!("gmail_{}", uuid::Uuid::new_v4());

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
            .interact(move |c| get_checkpoint(c, "historical_scan", &account_id).unwrap())
            .await
            .unwrap();

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
            channel: None,
            is_deleted: false,
            created_at: Some(Utc::now().naive_utc()),
            updated_at: Some(Utc::now().naive_utc()),
        };

        let conn = pool.get().await.unwrap();
        conn.interact({
            let row = row.clone();
            move |c| crate::db::transaction_observations::insert_observation(c, &row).unwrap()
        })
        .await
        .unwrap();

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
