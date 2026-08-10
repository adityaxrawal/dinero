//! Worker queues connecting the stages of ingestion.
//!
//! Stages have very different costs -- fetching is I/O bound, LLM extraction is
//! compute bound -- so they are decoupled by queue rather than run inline. That
//! lets fetching continue at full speed while extraction works through a backlog
//! at whatever rate the hardware allows.
//!
//! LLM jobs are persisted before being queued, so work still pending when the app
//! exits is replayed at the next launch instead of being lost.
use crate::extraction::ladder::ExtractionResult;

use deadpool_sqlite::Pool;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::{mpsc, Mutex, Semaphore};

pub struct TransactionJob {
    pub obs: ExtractionResult,
    pub source_pipeline: String,
    pub source_record_id: String,
    pub connected_account_id: String,
    pub raw_body: Option<String>,
    pub email_meta: Option<crate::ingestion::message_processor::EmailMetadata>,
}

pub struct StatementJob {
    pub filename: String,
    pub file_hash: String,
    pub stmt_id: String,
    pub batch_progress: Option<Arc<BatchProgressTracker>>,
    pub password: Option<String>,
    pub origin: String,
}

pub struct BatchProgressTracker {
    total: usize,
    parsed: AtomicUsize,
    total_duration_ms: AtomicU64,
}

impl BatchProgressTracker {
    /// Tracker for a batch of statement files, seeded with the expected total.
    pub fn new(total: usize) -> Self {
        Self {
            total,
            parsed: AtomicUsize::new(0),
            total_duration_ms: AtomicU64::new(0),
        }
    }

    /// Records one completion and returns progress with an ETA.
    ///
    /// The estimate extrapolates from the average time per item so far, which drifts
    /// early in a batch and tightens as the average stabilises.
    fn record_completion(&self, elapsed: std::time::Duration) -> (usize, usize, u64) {
        let parsed = self.parsed.fetch_add(1, Ordering::SeqCst) + 1;
        let total_ms = self
            .total_duration_ms
            .fetch_add(elapsed.as_millis() as u64, Ordering::SeqCst)
            + elapsed.as_millis() as u64;
        let avg_ms = total_ms / parsed as u64;
        let remaining = self.total.saturating_sub(parsed) as u64;
        let eta_seconds = (avg_ms * remaining) / 1000;
        (parsed, self.total, eta_seconds)
    }
}

pub struct MandateJob {
    pub extraction: crate::extraction::mandate_extractor::MandateExtraction,
    pub event_type: crate::ingestion::message_processor::MandateEventType,
    pub source_pipeline: String,
    pub source_record_id: String,
    pub connected_account_id: String,
    pub raw_body: Option<String>,
}

pub struct Layer6Job {
    pub observation_id: String,
    pub unassigned_id: String,
    pub bank_name: String,
    pub body_text: String,
    pub app_dir: std::path::PathBuf,
    pub internal_date_seconds: Option<i64>,
}

pub(crate) const LAYER6_QUEUE_CAPACITY: usize = 256;

/// Persists an LLM job before queueing it.
///
/// Written to the database first, so work still pending when the app exits is
/// replayed at the next launch rather than lost with the in-memory queue.
pub(crate) async fn enqueue_layer6_job(pool: &Pool, tx: &mpsc::Sender<Layer6Job>, job: Layer6Job) {
    let pending = crate::db::layer6_jobs::PendingLayer6Job {
        id: job.unassigned_id.clone(),
        observation_id: job.observation_id.clone(),
        bank_name: job.bank_name.clone(),
        body_text: job.body_text.clone(),
        internal_date_seconds: job.internal_date_seconds,
    };
    if let Ok(conn) = pool.get().await {
        if let Err(e) = conn
            .interact(move |c| crate::db::layer6_jobs::insert(c, &pending))
            .await
        {
            tracing::error!(
                "Failed to persist Layer 6 job for unassigned_id='{}': {:?}",
                job.unassigned_id,
                e
            );
        }
    }
    if tx.send(job).await.is_err() {
        tracing::error!("Layer 6 Queue closed — dropping job");
    }
}

/// Re-queues LLM jobs left pending by a previous session.
///
/// Runs at startup, which is what makes extraction survive a quit mid-scan.
pub async fn replay_pending_layer6_jobs(
    pool: &Pool,
    tx: &mpsc::Sender<Layer6Job>,
    app_dir: std::path::PathBuf,
) {
    let conn = match pool.get().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to get DB connection for Layer 6 job replay: {}", e);
            return;
        }
    };
    let pending = match conn
        .interact(|c| crate::db::layer6_jobs::select_all(c))
        .await
    {
        Ok(Ok(jobs)) => jobs,
        _ => {
            tracing::error!("Failed to read persisted Layer 6 jobs");
            return;
        }
    };
    if pending.is_empty() {
        return;
    }
    tracing::info!(
        "Replaying {} Layer 6 job(s) persisted before the last restart",
        pending.len()
    );
    for job in pending {
        let layer6_job = Layer6Job {
            observation_id: job.observation_id,
            unassigned_id: job.id,
            bank_name: job.bank_name,
            body_text: job.body_text,
            app_dir: app_dir.clone(),
            internal_date_seconds: job.internal_date_seconds,
        };
        if tx.send(layer6_job).await.is_err() {
            tracing::error!("Layer 6 Queue closed during startup replay");
            break;
        }
    }
}

#[derive(Clone)]
pub struct QueueHandles {
    pub transaction_tx: mpsc::Sender<TransactionJob>,
    pub statement_tx: mpsc::Sender<StatementJob>,
    pub mandate_tx: mpsc::Sender<MandateJob>,
    pub layer6_tx: mpsc::Sender<Layer6Job>,
}

const TRANSACTION_QUEUE_WORKERS: usize = 4;
const STATEMENT_QUEUE_MAX_CONCURRENT: usize = 5;

pub(crate) const TRANSACTION_QUEUE_CAPACITY: usize = 256;
pub(crate) const STATEMENT_QUEUE_CAPACITY: usize = 64;
pub(crate) const MANDATE_QUEUE_CAPACITY: usize = 64;

pub static TRANSACTION_QUEUE_PAUSED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
pub static STATEMENT_QUEUE_PAUSED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Blocks while the transaction queue is paused.
async fn wait_while_transaction_queue_paused() {
    while TRANSACTION_QUEUE_PAUSED.load(std::sync::atomic::Ordering::Relaxed) {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

/// Blocks while the statement queue is paused.
async fn wait_while_statement_queue_paused() {
    while STATEMENT_QUEUE_PAUSED.load(std::sync::atomic::Ordering::Relaxed) {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

/// Validates a queue name from an IPC caller.
///
/// The name arrives from the frontend, so it is checked against the known queues
/// rather than used directly.
fn validate_queue_name(queue: &str) -> Result<(), crate::error::AppError> {
    if queue != "transaction_queue" && queue != "statement_queue" {
        return Err(crate::error::AppError::Validation(format!(
            "invalid queue '{}': must be 'transaction_queue' or 'statement_queue'",
            queue
        )));
    }
    Ok(())
}

#[tauri::command]
/// Pauses a queue, used for debugging and controlled shutdown.
pub async fn pipeline_pause(queue: String) -> Result<serde_json::Value, crate::error::AppError> {
    validate_queue_name(&queue)?;
    match queue.as_str() {
        "transaction_queue" => {
            TRANSACTION_QUEUE_PAUSED.store(true, std::sync::atomic::Ordering::Relaxed)
        }
        "statement_queue" => {
            STATEMENT_QUEUE_PAUSED.store(true, std::sync::atomic::Ordering::Relaxed)
        }
        _ => unreachable!(),
    }
    Ok(serde_json::json!({ "status": "paused", "queue": queue }))
}

#[tauri::command]
/// Resumes a paused queue.
pub async fn pipeline_resume(queue: String) -> Result<serde_json::Value, crate::error::AppError> {
    validate_queue_name(&queue)?;
    match queue.as_str() {
        "transaction_queue" => {
            TRANSACTION_QUEUE_PAUSED.store(false, std::sync::atomic::Ordering::Relaxed)
        }
        "statement_queue" => {
            STATEMENT_QUEUE_PAUSED.store(false, std::sync::atomic::Ordering::Relaxed)
        }
        _ => unreachable!(),
    }
    Ok(serde_json::json!({ "status": "running", "queue": queue }))
}

#[derive(serde::Serialize)]
pub struct TransactionQueueStatus {
    pub state: String,
    pub active_workers: usize,
    pub queued_jobs: usize,
}

#[derive(serde::Serialize)]
pub struct StatementQueueStatus {
    pub state: String,
    pub active_parsers: usize,
    pub queued_jobs: usize,
}

#[derive(serde::Serialize)]
pub struct PipelineStatusResponse {
    pub transaction_queue: TransactionQueueStatus,
    pub statement_queue: StatementQueueStatus,
}

#[tauri::command]
/// Reports queue depth and pause state.
pub async fn pipeline_status(
    handles: tauri::State<'_, QueueHandles>,
) -> Result<PipelineStatusResponse, crate::error::AppError> {
    let tx_paused = TRANSACTION_QUEUE_PAUSED.load(std::sync::atomic::Ordering::Relaxed);
    let stmt_paused = STATEMENT_QUEUE_PAUSED.load(std::sync::atomic::Ordering::Relaxed);
    Ok(PipelineStatusResponse {
        transaction_queue: TransactionQueueStatus {
            state: if tx_paused { "paused" } else { "running" }.to_string(),
            active_workers: if tx_paused {
                0
            } else {
                TRANSACTION_QUEUE_WORKERS
            },
            queued_jobs: TRANSACTION_QUEUE_CAPACITY
                .saturating_sub(handles.transaction_tx.capacity()),
        },
        statement_queue: StatementQueueStatus {
            state: if stmt_paused { "paused" } else { "running" }.to_string(),
            active_parsers: if stmt_paused {
                0
            } else {
                STATEMENT_QUEUE_MAX_CONCURRENT
            },
            queued_jobs: STATEMENT_QUEUE_CAPACITY.saturating_sub(handles.statement_tx.capacity()),
        },
    })
}

/// Spawns every worker queue and returns their handles.
///
/// Stages are decoupled by queue because their costs differ sharply: fetching is
/// I/O bound while LLM extraction is compute bound, so running them inline would
/// let the slowest stage throttle the fastest.
pub fn spawn_queues<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    pool: Pool,
    learning: crate::learning::LearningHandle,
) -> QueueHandles {
    let (transaction_tx, transaction_rx) =
        mpsc::channel::<TransactionJob>(TRANSACTION_QUEUE_CAPACITY);
    let (statement_tx, statement_rx) = mpsc::channel::<StatementJob>(STATEMENT_QUEUE_CAPACITY);
    let (mandate_tx, mandate_rx) = mpsc::channel::<MandateJob>(MANDATE_QUEUE_CAPACITY);
    let (layer6_tx, layer6_rx) = mpsc::channel::<Layer6Job>(LAYER6_QUEUE_CAPACITY);

    spawn_transaction_workers(transaction_rx, pool.clone(), app.clone());
    spawn_statement_dispatcher(statement_rx, pool.clone(), app);
    spawn_mandate_workers(mandate_rx, pool.clone(), transaction_tx.clone());
    spawn_layer6_workers(layer6_rx, pool, learning);

    QueueHandles {
        transaction_tx,
        statement_tx,
        mandate_tx,
        layer6_tx,
    }
}

/// ponytail: fixed clamp to 6 rather than plumbing the runtime-calibrated
fn spawn_layer6_workers(
    rx: mpsc::Receiver<Layer6Job>,
    pool: Pool,
    learning: crate::learning::LearningHandle,
) {
    const LAYER6_WORKER_COUNT: usize = 6;
    let worker_count = crate::llama_sidecar::current_parallel_slots().clamp(1, LAYER6_WORKER_COUNT);
    let rx = Arc::new(Mutex::new(rx));
    for _ in 0..worker_count {
        let rx = Arc::clone(&rx);
        let pool = pool.clone();
        let learning = learning.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let job = { rx.lock().await.recv().await };
                match job {
                    Some(job) => process_layer6_job(job, &pool, &learning).await,
                    None => break,
                }
            }
        });
    }
}

const LAYER6_AUTO_RESOLVE_CONFIDENCE_THRESHOLD: f64 = 0.75;

/// Processes one LLM extraction job.
async fn process_layer6_job(
    job: Layer6Job,
    pool: &Pool,
    learning: &crate::learning::LearningHandle,
) {
    use crate::extraction::llm::Layer6Outcome;

    {
        let unassigned_id = job.unassigned_id.clone();
        if let Ok(conn) = pool.get().await {
            if let Err(e) = conn
                .interact(move |c| crate::db::layer6_jobs::delete(c, &unassigned_id))
                .await
            {
                tracing::error!("Failed to delete persisted Layer 6 job: {:?}", e);
            }
        }
    }
    let layer = crate::extraction::ladder::Layer6LlmLayer {
        app_dir: Some(job.app_dir.clone()),
        fallback_event_time: job.internal_date_seconds,
    };
    let result = layer.run(pool, &job.bank_name, &job.body_text).await;
    match result {
        Layer6Outcome::Extracted(enriched) => {
            let mut enriched = *enriched;
            crate::extraction::ladder::enqueue_drift_candidates_if_drifted(
                pool,
                learning,
                &job.bank_name,
                &job.body_text,
                &enriched,
                Some(job.app_dir.clone()),
            )
            .await;

            crate::extraction::ladder::apply_instrument_signals(
                &mut enriched,
                &job.bank_name,
                &job.body_text,
            );
            enriched.channel = crate::extraction::ladder::detect_channel(&enriched, &job.body_text);

            if let Err(e) = apply_layer6_success(
                pool,
                &job.observation_id,
                &job.unassigned_id,
                enriched,
                job.internal_date_seconds,
            )
            .await
            {
                tracing::error!(
                    "Layer 6 background worker: failed to apply success for observation_id='{}': {}",
                    job.observation_id, e
                );
            }
        }
        Layer6Outcome::Rejected => {
            let mark_result: anyhow::Result<()> = async {
                let unassigned_id = job.unassigned_id.clone();
                let conn = pool.get().await?;
                conn.interact(move |c| {
                    crate::db::unassigned_transactions::update_status(
                        c,
                        &unassigned_id,
                        "no_transaction_found",
                    )
                })
                .await
                .map_err(|e| anyhow::anyhow!("Interact error: {}", e))??;
                Ok(())
            }
            .await;
            if let Err(e) = mark_result {
                tracing::error!(
                    "Layer 6 background worker: failed to mark unassigned_id='{}' as no_transaction_found: {}",
                    job.unassigned_id, e
                );
            }
        }
        Layer6Outcome::TimedOut | Layer6Outcome::Failed => {
            tracing::info!(
                "Layer 6 background worker: no extraction for observation_id='{}' — leaving as unassigned",
                job.observation_id
            );
        }
    }
}

/// Applies a successful LLM extraction and clears the persisted job.
///
/// The job row is deleted only after its result is committed, so a crash between
/// the two replays the job rather than losing it.
async fn apply_layer6_success(
    pool: &Pool,
    observation_id: &str,
    unassigned_id: &str,
    enriched: ExtractionResult,
    internal_date_seconds: Option<i64>,
) -> anyhow::Result<()> {
    let observation_id = observation_id.to_string();
    let unassigned_id = unassigned_id.to_string();
    let conn = pool.get().await?;
    conn.interact(move |c| -> anyhow::Result<()> {
        let mut row = crate::db::transaction_observations::get_observation(c, &observation_id)?
            .ok_or_else(|| anyhow::anyhow!("observation {} not found", observation_id))?;
        row.amount_minor = enriched.amount_minor;
        row.currency = enriched.currency;
        row.direction = enriched.direction;
        row.merchant_raw = enriched.merchant_raw;
        row.reference_id = enriched.reference_id;

        let is_self_transfer = enriched.channel.as_deref() == Some("internal_transfer");
        if is_self_transfer {
            if let Some(dest_account) =
                crate::ingestion::message_processor::MessageProcessor::self_transfer_destination_account(
                    row.merchant_raw.as_deref(),
                )
            {
                row.merchant_raw = Some(format!("Internal Transfer (A/c {dest_account})"));
            }
        }
        if let Some(ts) = enriched.event_time.or(internal_date_seconds) {
            use chrono::TimeZone;
            let dt_utc = chrono::Utc.timestamp_opt(ts, 0).unwrap();
            let ist_offset = chrono::FixedOffset::east_opt(5 * 3600 + 30 * 60).unwrap();
            row.event_time = Some(dt_utc.with_timezone(&ist_offset).naive_local());
        }
        row.extraction_method = Some("llm_layer6".to_string());
        row.confidence_score = enriched.confidence_score;

        let instrument_id = match (
            &enriched.instrument_type,
            &enriched.issuer_name,
            &enriched.masked_identifier,
        ) {
            (Some(itype), Some(iname), Some(masked)) => {
                crate::db::instruments::get_or_create_instrument(
                    c,
                    itype,
                    iname,
                    masked,
                    enriched.network.as_deref(),
                )
                .ok()
            }
            _ => enriched
                .issuer_name
                .as_deref()
                .and_then(|iname| {
                    crate::db::instruments::resolve_single_instrument_by_issuer(c, iname).ok()
                })
                .flatten(),
        };
        row.instrument_id = instrument_id.clone();

        let confident_enough = is_self_transfer
            || enriched
                .confidence_score
                .map(|s| s >= LAYER6_AUTO_RESOLVE_CONFIDENCE_THRESHOLD)
                .unwrap_or(false);

        crate::db::transaction_observations::update_observation(c, &row)?;

        let has_instrument = instrument_id.is_some();
        let ready_instrument_id = instrument_id.filter(|_| {
            confident_enough && row.amount_minor.is_some() && row.merchant_raw.is_some()
        });
        if let Some(instrument_id) = ready_instrument_id {
            let incoming_obs = crate::reconciliation::engine::IncomingObservation {
                id: row.id.clone(),
                instrument_id,
                amount_minor: row.amount_minor.unwrap_or(0),
                currency: row.currency.clone().unwrap_or_else(|| "INR".to_string()),
                direction: row.direction.clone().unwrap_or_else(|| "debit".to_string()),
                event_time: row
                    .event_time
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_default(),
                reference_id: row.reference_id.clone(),
                merchant_raw: row.merchant_raw.clone(),
                source_pipeline: row
                    .source_pipeline
                    .clone()
                    .unwrap_or_else(|| "gmail_transaction".to_string()),
                source_record_id: row.source_record_id.clone().unwrap_or_default(),
                emi_total_installments: row.emi_total_installments,
                emi_original_amount_minor: row.emi_original_amount_minor,
                fingerprint: row.fingerprint.clone(),
                confidence_score: row.confidence_score,
                event_time_confidence: row.event_time_confidence.clone(),
                channel: row.channel.clone(),
            };
            crate::reconciliation::engine::reconcile_transactionally(c, &incoming_obs)?;
            crate::db::unassigned_transactions::update_status(c, &unassigned_id, "resolved")?;
        } else {
            let reason = if row.merchant_raw.is_none() {
                "gate3_failed:missing_counterparty"
            } else if !has_instrument {
                "gate3_failed:missing_instrument"
            } else {
                "gate3_failed:low_confidence"
            };
            crate::db::unassigned_transactions::update_reason(c, &unassigned_id, reason)?;
        }
        Ok(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("Interact error: {}", e))??;
    Ok(())
}

/// Spawns the mandate-processing workers.
fn spawn_mandate_workers(
    mut rx: mpsc::Receiver<MandateJob>,
    pool: Pool,
    transaction_tx: mpsc::Sender<TransactionJob>,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(job) = rx.recv().await {
            process_mandate_job(job, &pool, &transaction_tx).await;
        }
    });
}

/// Processes one mandate event.
async fn process_mandate_job(
    job: MandateJob,
    pool: &Pool,
    transaction_tx: &mpsc::Sender<TransactionJob>,
) {
    let extraction = job.extraction.clone();
    let event_type = job.event_type.clone();
    let merchant_raw = extraction.merchant.clone();

    if let Ok(conn) = pool.get().await {
        let _ = conn
            .interact(move |c| -> Option<String> {
                let instrument_id = if let (Some(itype), Some(iname), Some(masked)) = (
                    &extraction.instrument_type,
                    &extraction.issuer_name,
                    &extraction.masked_identifier,
                ) {
                    crate::db::instruments::get_or_create_instrument(c, itype, iname, masked, None)
                        .ok()
                } else {
                    None
                };
                let merchant_entity_id = extraction
                    .merchant
                    .as_deref()
                    .and_then(|m| {
                        crate::extraction::merchant_normalizer::normalize_merchant_sync(c, m).ok()
                    })
                    .map(|(entity_id, _)| entity_id)
                    .filter(|id| !id.is_empty());

                match event_type {
                    crate::ingestion::message_processor::MandateEventType::Registration => {
                        if let (Some(instrument_id), Some(merchant_entity_id)) =
                            (&instrument_id, &merchant_entity_id)
                        {
                            let _ = crate::db::recurring_payments::upsert_explicit(
                                c,
                                instrument_id,
                                merchant_entity_id,
                                extraction.max_limit_amount,
                                "INR",
                                extraction.cadence.as_deref(),
                                extraction.external_mandate_id.as_deref(),
                            );
                        }
                    }
                    crate::ingestion::message_processor::MandateEventType::Cancellation => {
                        let candidates =
                            crate::db::recurring_payments::find_active_candidates_for_cancellation(
                                c,
                                instrument_id.as_deref(),
                                merchant_entity_id.as_deref(),
                                extraction.external_mandate_id.as_deref(),
                            )
                            .unwrap_or_default();
                        match candidates.len() {
                            1 => {
                                let _ = crate::db::recurring_payments::mark_cancelled(
                                    c,
                                    &candidates[0].id,
                                );
                            }
                            _ => {
                                let raw_signal = serde_json::json!({
                                    "merchant": extraction.merchant,
                                    "external_mandate_id": extraction.external_mandate_id,
                                    "instrument_id": instrument_id,
                                })
                                .to_string();
                                let candidate_ids: Vec<String> =
                                    candidates.iter().map(|r| r.id.clone()).collect();
                                let _ =
                                    crate::db::unresolved_mandate_cancellations::insert_unresolved(
                                        c,
                                        &raw_signal,
                                        &candidate_ids,
                                    );
                            }
                        }
                    }
                }
                instrument_id
            })
            .await;
    }

    let tx_job = TransactionJob {
        obs: crate::extraction::ladder::ExtractionResult {
            amount_minor: Some(0),
            currency: Some("INR".to_string()),
            direction: Some("debit".to_string()),
            merchant_raw,
            extraction_method: "mandate_event".to_string(),
            instrument_type: job.extraction.instrument_type.clone(),
            issuer_name: job.extraction.issuer_name.clone(),
            masked_identifier: job.extraction.masked_identifier.clone(),
            ..Default::default()
        },
        source_pipeline: job.source_pipeline,
        source_record_id: job.source_record_id,
        connected_account_id: job.connected_account_id,
        raw_body: job.raw_body,
        email_meta: None,
    };
    if transaction_tx.send(tx_job).await.is_err() {
        tracing::error!("Transaction Queue closed — dropping mandate-generated ₹0 transaction job");
    }
}

/// Spawns the transaction-processing workers.
fn spawn_transaction_workers<R: tauri::Runtime>(
    rx: mpsc::Receiver<TransactionJob>,
    pool: Pool,
    app: tauri::AppHandle<R>,
) {
    let rx = Arc::new(Mutex::new(rx));
    for _ in 0..TRANSACTION_QUEUE_WORKERS {
        let rx = Arc::clone(&rx);
        let pool = pool.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                wait_while_transaction_queue_paused().await;
                let job = { rx.lock().await.recv().await };
                match job {
                    Some(job) => process_transaction_job(job, &pool, &app).await,
                    None => break,
                }
            }
        });
    }
}

/// Spawns the statement dispatcher.
///
/// A dispatcher rather than a worker pool: statement parsing runs through the
/// sidecar, which serialises the work anyway.
fn spawn_statement_dispatcher<R: tauri::Runtime>(
    mut rx: mpsc::Receiver<StatementJob>,
    pool: Pool,
    app: tauri::AppHandle<R>,
) {
    let semaphore = Arc::new(Semaphore::new(STATEMENT_QUEUE_MAX_CONCURRENT));
    tauri::async_runtime::spawn(async move {
        while let Some(job) = rx.recv().await {
            wait_while_statement_queue_paused().await;
            let permit = Arc::clone(&semaphore).acquire_owned().await.unwrap();
            let pool = pool.clone();
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let _permit = permit;
                let start = std::time::Instant::now();

                use tauri::Manager as _;
                let app_data_dir = match app.path().app_data_dir() {
                    Ok(dir) => dir,
                    Err(e) => {
                        tracing::error!(
                            "Statement Queue job failed (file='{}'): could not resolve app data dir: {}",
                            job.filename, e
                        );
                        return;
                    }
                };
                let bytes = match crate::statements::pdf_storage::read_pdf(
                    &app_data_dir,
                    &job.stmt_id,
                ) {
                    Ok(Some(b)) => b,
                    Ok(None) => {
                        tracing::error!(
                            "Statement Queue job failed (file='{}'): staged PDF for stmt_id='{}' is missing",
                            job.filename, job.stmt_id
                        );
                        return;
                    }
                    Err(e) => {
                        tracing::error!(
                            "Statement Queue job failed (file='{}'): could not read staged PDF: {}",
                            job.filename,
                            e
                        );
                        return;
                    }
                };

                let result = crate::commands::stage_parse_pipeline(
                    &bytes,
                    &job.filename,
                    &job.file_hash,
                    &pool,
                    &app,
                    None,
                    job.password.as_deref(),
                    &job.origin,
                    Some(job.stmt_id.clone()),
                )
                .await;
                drop(bytes);

                let _ = crate::statements::pdf_storage::delete_pdf(&app_data_dir, &job.stmt_id);

                if let Some(tracker) = &job.batch_progress {
                    let (parsed, total, eta_seconds) = tracker.record_completion(start.elapsed());
                    let payload = serde_json::json!({
                        "parsed": parsed,
                        "total": total,
                        "eta_seconds": eta_seconds,
                    });
                    crate::statements::events::emit(
                        crate::statements::events::BATCH_PROGRESS,
                        payload.clone(),
                    );
                    let _ = app.emit(crate::statements::events::BATCH_PROGRESS, payload);
                }

                use crate::statements::events;
                match &result {
                    Ok(crate::commands::PipelineOutcome::Staged(_draft_id)) => {
                    }
                    Ok(crate::commands::PipelineOutcome::BlockedAwaitingInstrument(
                        _unprocessed_id,
                    )) => {
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Statement Queue job failed (file='{}'): {}",
                            job.filename,
                            e
                        );
                        events::emit(
                            events::PARSE_FAILED,
                            serde_json::json!({ "reason": e.to_string(), "filename": job.filename }),
                        );
                        let _ = app.emit(
                            events::PARSE_FAILED,
                            serde_json::json!({ "reason": e.to_string(), "filename": job.filename }),
                        );
                    }
                }
            });
        }
    });
}

/// Processes one transaction job through reconciliation and persistence.
async fn process_transaction_job<R: tauri::Runtime>(
    job: TransactionJob,
    pool: &Pool,
    app: &tauri::AppHandle<R>,
) {
    let obs = job.obs;
    let instrument_type = obs.instrument_type.clone();
    let issuer_name = obs.issuer_name.clone();
    let masked_identifier = obs.masked_identifier.clone();
    let network = obs.network.clone();
    let mut row = crate::extraction::normalization::normalize_observation(
        obs,
        &job.source_pipeline,
        &job.source_record_id,
        job.raw_body.as_deref(),
        job.email_meta.as_ref(),
    );

    let connected_account_id = job.connected_account_id;

    let notify_amount_minor = row.amount_minor;
    let notify_direction = row.direction.clone();
    let notify_merchant = row.merchant_raw.clone();

    if let Ok(conn) = pool.get().await {
        let outcome = conn
            .interact(
                move |c| -> Option<(crate::reconciliation::audit::DecisionType, String)> {
                    if let (Some(ref itype), Some(ref iname), Some(ref masked)) =
                        (instrument_type, issuer_name, masked_identifier)
                    {
                        match crate::db::instruments::get_or_create_instrument(
                            c,
                            itype,
                            iname,
                            masked,
                            network.as_deref(),
                        ) {
                            Ok(instr_id) => {
                                row.instrument_id = Some(instr_id);
                            }
                            Err(e) => {
                                tracing::warn!("Failed to resolve instrument: {}", e);
                            }
                        }
                    }

                    if let (Some(ref instrument_id), Some(ref direction), Some(amount_minor)) =
                        (&row.instrument_id, &row.direction, row.amount_minor)
                    {
                        let event_bucket = row
                            .event_time
                            .map(|dt| dt.format("%Y-%m-%dT%H:%M").to_string())
                            .unwrap_or_default();
                        row.fingerprint =
                            Some(crate::extraction::fingerprint::compute_fingerprint(
                                instrument_id,
                                direction,
                                amount_minor,
                                &event_bucket,
                                &connected_account_id,
                            ));
                    }

                    use crate::db::transaction_observations::InsertObservationOutcome;
                    match crate::db::transaction_observations::insert_observation_idempotent(
                        c, &row,
                    ) {
                        Err(e) => {
                            tracing::warn!("Observation insert failed: {}", e);
                            None
                        }
                        Ok(InsertObservationOutcome::DuplicateSkipped) => {
                            None
                        }
                        Ok(InsertObservationOutcome::Inserted) => {
                            let incoming_obs = crate::reconciliation::engine::IncomingObservation {
                                id: row.id.clone(),
                                instrument_id: row
                                    .instrument_id
                                    .clone()
                                    .unwrap_or_else(|| "unknown".to_string()),
                                amount_minor: row.amount_minor.unwrap_or(0),
                                currency: row.currency.clone().unwrap_or_else(|| "INR".to_string()),
                                direction: row
                                    .direction
                                    .clone()
                                    .unwrap_or_else(|| "debit".to_string()),
                                event_time: row
                                    .event_time
                                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                                    .unwrap_or_default(),
                                reference_id: row.reference_id.clone(),
                                merchant_raw: row.merchant_raw.clone(),
                                source_pipeline: row
                                    .source_pipeline
                                    .clone()
                                    .unwrap_or_else(|| "unknown".to_string()),
                                source_record_id: row.source_record_id.clone().unwrap_or_default(),
                                emi_total_installments: row.emi_total_installments,
                                emi_original_amount_minor: row.emi_original_amount_minor,
                                fingerprint: row.fingerprint.clone(),
                                confidence_score: row.confidence_score,
                                event_time_confidence: row.event_time_confidence.clone(),
                                channel: row.channel.clone(),
                            };

                            match crate::reconciliation::engine::reconcile_transactionally(
                                c,
                                &incoming_obs,
                            ) {
                                Ok(decision) => {
                                    tracing::debug!(
                                        "Reconciliation decision for obs '{}': {:?}",
                                        incoming_obs.id,
                                        decision
                                    );
                                    Some((decision, incoming_obs.id.clone()))
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Reconciliation failed for obs '{}': {}",
                                        incoming_obs.id,
                                        e
                                    );
                                    None
                                }
                            }
                        }
                    }
                },
            )
            .await;

        if let Ok(Some((decision, obs_id))) = outcome {
            if let crate::reconciliation::audit::DecisionType::AmbiguousPending(cluster_id) =
                &decision
            {
                let _ = crate::ipc::events::emit_event(
                    app,
                    crate::ipc::events::AppEvent::ReconciliationCluster,
                    serde_json::json!({ "cluster_id": cluster_id, "observation_id": obs_id }),
                );
            } else {
                let _ = crate::ipc::events::emit_event(
                    app,
                    crate::ipc::events::AppEvent::TransactionCreated,
                    serde_json::json!({ "observation_id": obs_id }),
                );

                let is_confirmed = matches!(
                    decision,
                    crate::reconciliation::audit::DecisionType::NewCanonical
                        | crate::reconciliation::audit::DecisionType::AutoMatchedExact
                        | crate::reconciliation::audit::DecisionType::AutoMatchedScored
                );
                if is_confirmed && notify_direction.as_deref() == Some("debit") {
                    if let Some(amount_minor) = notify_amount_minor {
                        if crate::notifications::should_notify_transaction(
                            amount_minor,
                            crate::notifications::DEFAULT_TRANSACTION_NOTIFICATION_THRESHOLD_MINOR,
                        ) {
                            let merchant =
                                notify_merchant.unwrap_or_else(|| "a merchant".to_string());
                            crate::notifications::send_notification(
                                app,
                                crate::notifications::NotificationKind::TransactionAboveThreshold,
                                "New Transaction",
                                &format!("₹{:.2} at {}", amount_minor as f64 / 100.0, merchant),
                                None,
                            );
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_queue_name_rejects_unknown_queue() {
        assert!(validate_queue_name("transaction_queue").is_ok());
        assert!(validate_queue_name("statement_queue").is_ok());
        assert!(validate_queue_name("not_a_real_queue").is_err());
        assert!(validate_queue_name("").is_err());
    }

    #[tokio::test]
    async fn test_concurrency_cap_enforced_at_5() {
        let semaphore = Arc::new(Semaphore::new(STATEMENT_QUEUE_MAX_CONCURRENT));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..12 {
            let semaphore = Arc::clone(&semaphore);
            let in_flight = Arc::clone(&in_flight);
            let max_seen = Arc::clone(&max_seen);
            let completed = Arc::clone(&completed);
            handles.push(tokio::spawn(async move {
                let _permit = semaphore.acquire_owned().await.unwrap();
                let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                max_seen.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                in_flight.fetch_sub(1, Ordering::SeqCst);
                completed.fetch_add(1, Ordering::SeqCst);
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(
            max_seen.load(Ordering::SeqCst),
            STATEMENT_QUEUE_MAX_CONCURRENT,
            "cap must be exactly 5, not more (and, given 12 tasks contending, not less either)"
        );
        assert_eq!(
            completed.load(Ordering::SeqCst),
            12,
            "every task beyond the cap must still eventually run (FIFO wait), never be dropped"
        );
    }

    #[test]
    fn test_eta_calculation_uses_rolling_average() {
        let tracker = BatchProgressTracker::new(4);

        let (parsed, total, eta) = tracker.record_completion(std::time::Duration::from_millis(100));
        assert_eq!((parsed, total), (1, 4));
        assert_eq!(eta, 0, "300ms rounds down to 0 whole seconds");

        let (parsed, _, eta) = tracker.record_completion(
            std::time::Duration::from_secs(2) - std::time::Duration::from_millis(100),
        );
        assert_eq!(parsed, 2);
        assert_eq!(
            eta, 2,
            "rolling average must reflect both samples, not just the first"
        );
    }
}

#[cfg(test)]
mod layer6_tests {
    use super::*;
    use crate::db::init_db;
    use std::fs;

    #[tokio::test]
    async fn low_confidence_layer6_result_stays_open_and_prefilled() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let pool = init_db(temp_dir.join("test.db"))
            .await
            .expect("DB init failed");
        let conn = pool.get().await.unwrap();

        let observation_id = uuid::Uuid::new_v4().to_string();
        let unassigned_id = uuid::Uuid::new_v4().to_string();

        let base_row = crate::db::transaction_observations::TransactionObservationsRow {
            id: observation_id.clone(),
            canonical_transaction_id: None,
            source_pipeline: Some("gmail_transaction".to_string()),
            source_record_id: Some("msg_1".to_string()),
            source_message_id: Some("msg_1".to_string()),
            source_thread_id: None,
            statement_id: None,
            statement_entry_id: None,
            instrument_id: None,
            direction: None,
            amount: None,
            amount_minor: None,
            currency: None,
            event_time: None,
            event_time_confidence: None,
            posting_date: None,
            merchant_raw: None,
            merchant_normalized: None,
            reference_id: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            timezone_at_ingestion: None,
            fingerprint: Some(format!("pending_{}", observation_id)),
            extraction_method: Some("pending_llm_enrichment".to_string()),
            confidence_score: None,
            raw_payload_json: None,
            parser_version: None,
            emi_total_installments: None,
            emi_installment_number: None,
            emi_original_amount_minor: None,
            channel: None,
            is_deleted: false,
            created_at: Some(chrono::Utc::now().naive_utc()),
            updated_at: Some(chrono::Utc::now().naive_utc()),
        };
        conn.interact({
            let row = base_row.clone();
            move |c| crate::db::transaction_observations::insert_observation(c, &row)
        })
        .await
        .unwrap()
        .unwrap();
        conn.interact({
            let unassigned_id = unassigned_id.clone();
            let observation_id = observation_id.clone();
            move |c| {
                crate::db::unassigned_transactions::insert(
                    c,
                    &crate::db::unassigned_transactions::UnassignedTransactionRow {
                        id: unassigned_id,
                        observation_id,
                        reason: "pending_llm_enrichment".to_string(),
                        status: "open".to_string(),
                        created_at: None,
                    },
                )
            }
        })
        .await
        .unwrap()
        .unwrap();

        let enriched = crate::extraction::ladder::ExtractionResult {
            amount_minor: Some(50000),
            currency: Some("INR".to_string()),
            direction: Some("debit".to_string()),
            merchant_raw: Some("Test Merchant".to_string()),
            extraction_method: "llm_layer6".to_string(),
            confidence_score: Some(0.7),
            ..Default::default()
        };

        apply_layer6_success(
            &pool,
            &observation_id,
            &unassigned_id,
            enriched,
            Some(1_780_000_000),
        )
        .await
        .unwrap();

        let updated = conn
            .interact({
                let id = observation_id.clone();
                move |c| crate::db::transaction_observations::get_observation(c, &id)
            })
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(updated.amount_minor, Some(50000));
        assert_eq!(updated.merchant_raw.as_deref(), Some("Test Merchant"));
        assert!(
            updated.canonical_transaction_id.is_none(),
            "a low-confidence, instrument-less result must not become a transaction"
        );

        let open = conn
            .interact(|c| crate::db::unassigned_transactions::select_open(c))
            .await
            .unwrap()
            .unwrap();
        assert!(
            open.iter().any(|r| r.id == unassigned_id),
            "a low-confidence result must stay visible in the review queue, pre-filled"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn high_confidence_verified_layer6_result_promotes_to_transaction() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let pool = init_db(temp_dir.join("test.db"))
            .await
            .expect("DB init failed");
        let conn = pool.get().await.unwrap();

        let observation_id = uuid::Uuid::new_v4().to_string();
        let unassigned_id = uuid::Uuid::new_v4().to_string();

        let base_row = crate::db::transaction_observations::TransactionObservationsRow {
            id: observation_id.clone(),
            canonical_transaction_id: None,
            source_pipeline: Some("gmail_transaction".to_string()),
            source_record_id: Some("msg_2".to_string()),
            source_message_id: Some("msg_2".to_string()),
            source_thread_id: None,
            statement_id: None,
            statement_entry_id: None,
            instrument_id: None,
            direction: None,
            amount: None,
            amount_minor: None,
            currency: None,
            event_time: None,
            event_time_confidence: None,
            posting_date: None,
            merchant_raw: None,
            merchant_normalized: None,
            reference_id: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            timezone_at_ingestion: None,
            fingerprint: Some(format!("pending_{}", observation_id)),
            extraction_method: Some("pending_llm_enrichment".to_string()),
            confidence_score: None,
            raw_payload_json: None,
            parser_version: None,
            emi_total_installments: None,
            emi_installment_number: None,
            emi_original_amount_minor: None,
            channel: None,
            is_deleted: false,
            created_at: Some(chrono::Utc::now().naive_utc()),
            updated_at: Some(chrono::Utc::now().naive_utc()),
        };
        conn.interact({
            let row = base_row.clone();
            move |c| crate::db::transaction_observations::insert_observation(c, &row)
        })
        .await
        .unwrap()
        .unwrap();
        conn.interact({
            let unassigned_id = unassigned_id.clone();
            let observation_id = observation_id.clone();
            move |c| {
                crate::db::unassigned_transactions::insert(
                    c,
                    &crate::db::unassigned_transactions::UnassignedTransactionRow {
                        id: unassigned_id,
                        observation_id,
                        reason: "gate3_failed:missing_instrument".to_string(),
                        status: "open".to_string(),
                        created_at: None,
                    },
                )
            }
        })
        .await
        .unwrap()
        .unwrap();

        let enriched = crate::extraction::ladder::ExtractionResult {
            amount_minor: Some(50000),
            currency: Some("INR".to_string()),
            direction: Some("debit".to_string()),
            merchant_raw: Some("Test Merchant".to_string()),
            event_time: Some(1704412200),
            extraction_method: "llm_layer6".to_string(),
            confidence_score: Some(0.9),
            instrument_type: Some("credit_card".to_string()),
            issuer_name: Some("HDFC Bank".to_string()),
            masked_identifier: Some("1234".to_string()),
            ..Default::default()
        };

        apply_layer6_success(
            &pool,
            &observation_id,
            &unassigned_id,
            enriched,
            Some(1_780_000_000),
        )
        .await
        .unwrap();

        let updated = conn
            .interact({
                let id = observation_id.clone();
                move |c| crate::db::transaction_observations::get_observation(c, &id)
            })
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(
            updated.canonical_transaction_id.is_some(),
            "a high-confidence, instrument-resolved result must be promoted to a transaction"
        );
        assert!(updated.instrument_id.is_some());

        let open = conn
            .interact(|c| crate::db::unassigned_transactions::select_open(c))
            .await
            .unwrap()
            .unwrap();
        assert!(
            open.iter().all(|r| r.id != unassigned_id),
            "resolved unassigned row must no longer appear in select_open"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn low_confidence_self_transfer_still_promotes_with_placeholder_merchant() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let pool = init_db(temp_dir.join("test.db"))
            .await
            .expect("DB init failed");
        let conn = pool.get().await.unwrap();

        let observation_id = uuid::Uuid::new_v4().to_string();
        let unassigned_id = uuid::Uuid::new_v4().to_string();

        let base_row = crate::db::transaction_observations::TransactionObservationsRow {
            id: observation_id.clone(),
            canonical_transaction_id: None,
            source_pipeline: Some("gmail_transaction".to_string()),
            source_record_id: Some("msg_3".to_string()),
            source_message_id: Some("msg_3".to_string()),
            source_thread_id: None,
            statement_id: None,
            statement_entry_id: None,
            instrument_id: None,
            direction: None,
            amount: None,
            amount_minor: None,
            currency: None,
            event_time: None,
            event_time_confidence: None,
            posting_date: None,
            merchant_raw: None,
            merchant_normalized: None,
            reference_id: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            timezone_at_ingestion: None,
            fingerprint: Some(format!("pending_{}", observation_id)),
            extraction_method: Some("pending_llm_enrichment".to_string()),
            confidence_score: None,
            raw_payload_json: None,
            parser_version: None,
            emi_total_installments: None,
            emi_installment_number: None,
            emi_original_amount_minor: None,
            channel: None,
            is_deleted: false,
            created_at: Some(chrono::Utc::now().naive_utc()),
            updated_at: Some(chrono::Utc::now().naive_utc()),
        };
        conn.interact({
            let row = base_row.clone();
            move |c| crate::db::transaction_observations::insert_observation(c, &row)
        })
        .await
        .unwrap()
        .unwrap();
        conn.interact({
            let unassigned_id = unassigned_id.clone();
            let observation_id = observation_id.clone();
            move |c| {
                crate::db::unassigned_transactions::insert(
                    c,
                    &crate::db::unassigned_transactions::UnassignedTransactionRow {
                        id: unassigned_id,
                        observation_id,
                        reason: "gate3_failed:low_confidence".to_string(),
                        status: "open".to_string(),
                        created_at: None,
                    },
                )
            }
        })
        .await
        .unwrap()
        .unwrap();

        let enriched = crate::extraction::ladder::ExtractionResult {
            amount_minor: Some(8216400),
            currency: Some("INR".to_string()),
            direction: Some("debit".to_string()),
            merchant_raw: Some("account 1527".to_string()),
            extraction_method: "llm_layer6".to_string(),
            confidence_score: Some(0.70),
            instrument_type: Some("bank_account".to_string()),
            issuer_name: Some("HDFC Bank".to_string()),
            masked_identifier: Some("4691".to_string()),
            channel: Some("internal_transfer".to_string()),
            ..Default::default()
        };

        apply_layer6_success(
            &pool,
            &observation_id,
            &unassigned_id,
            enriched,
            Some(1_780_000_000),
        )
        .await
        .unwrap();

        let updated = conn
            .interact({
                let id = observation_id.clone();
                move |c| crate::db::transaction_observations::get_observation(c, &id)
            })
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(
            updated.canonical_transaction_id.is_some(),
            "a detected self-transfer must promote despite sub-threshold confidence"
        );
        assert!(
            updated.event_time.is_some(),
            "a promoted Layer 6 observation must carry a real event_time, not NULL"
        );
        assert_eq!(
            updated.merchant_raw.as_deref(),
            Some("Internal Transfer (A/c 1527)"),
            "merchant_raw must get the same placeholder as the Layer 1-5 path, not the raw account string"
        );

        let open = conn
            .interact(|c| crate::db::unassigned_transactions::select_open(c))
            .await
            .unwrap()
            .unwrap();
        assert!(open.iter().all(|r| r.id != unassigned_id));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn persisted_layer6_job_survives_until_dequeued() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let pool = init_db(temp_dir.join("test.db"))
            .await
            .expect("DB init failed");
        let conn = pool.get().await.unwrap();

        let (tx, mut rx) = mpsc::channel::<Layer6Job>(4);
        let job = Layer6Job {
            observation_id: "obs-durability".to_string(),
            unassigned_id: "unassigned-durability".to_string(),
            bank_name: "HDFC Bank".to_string(),
            body_text: "Rs. 100 debited".to_string(),
            app_dir: temp_dir.clone(),
            internal_date_seconds: None,
        };
        enqueue_layer6_job(&pool, &tx, job).await;

        let pending = conn
            .interact(|c| crate::db::layer6_jobs::select_all(c))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            pending.len(),
            1,
            "job must be durably persisted before it's ever dequeued"
        );
        assert_eq!(pending[0].id, "unassigned-durability");

        let (replay_tx, mut replay_rx) = mpsc::channel::<Layer6Job>(4);
        replay_pending_layer6_jobs(&pool, &replay_tx, temp_dir.clone()).await;
        let replayed = replay_rx.recv().await.expect("replayed job must arrive");
        assert_eq!(replayed.unassigned_id, "unassigned-durability");

        {
            let unassigned_id = replayed.unassigned_id.clone();
            conn.interact(move |c| crate::db::layer6_jobs::delete(c, &unassigned_id))
                .await
                .unwrap()
                .unwrap();
        }
        let pending = conn
            .interact(|c| crate::db::layer6_jobs::select_all(c))
            .await
            .unwrap()
            .unwrap();
        assert!(
            pending.is_empty(),
            "durable row must be gone once the job has been dequeued for processing"
        );

        assert!(rx.recv().await.is_some());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
