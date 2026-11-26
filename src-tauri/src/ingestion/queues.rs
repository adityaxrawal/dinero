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
use futures_util::FutureExt as _;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::{mpsc, Mutex, Semaphore};

#[derive(serde::Serialize, serde::Deserialize)]
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
    /// `(finished, timed, total_ms)`, updated together under one lock: as
    /// independent atomics they tear under concurrent parsers, and an average built
    /// from a count and a duration belonging to different moments swings the ETA
    /// between absurd extremes. `timed` trails `finished` because an item rejected
    /// before it ever reaches a parser contributes no duration.
    progress: std::sync::Mutex<(usize, usize, u64)>,
}

impl BatchProgressTracker {
    /// Tracker for a batch of statement files, seeded with the expected total.
    pub fn new(total: usize) -> Self {
        Self {
            total,
            progress: std::sync::Mutex::new((0, 0, 0)),
        }
    }

    /// Records one completion and returns progress with an ETA.
    ///
    /// The estimate extrapolates from the average time per item so far, which drifts
    /// early in a batch and tightens as the average stabilises.
    fn record_completion(&self, elapsed: std::time::Duration) -> (usize, usize, u64) {
        self.record(Some(elapsed))
    }

    /// Records an item that will never be parsed -- rejected as a duplicate, held
    /// for a password, or failed before it was ever queued.
    ///
    /// The tracker is seeded with the file count, not the queued count, so without
    /// this the total is never reached and the batch's progress sits short of
    /// completion for the rest of the session.
    pub fn record_skipped(&self) -> (usize, usize, u64) {
        self.record(None)
    }

    fn record(&self, elapsed: Option<std::time::Duration>) -> (usize, usize, u64) {
        let (finished, timed, total_ms) = {
            // A panic in a parser must not poison batch progress for the rest of
            // the batch; the counters are plain numbers with no invariant to break.
            let mut progress = self
                .progress
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            progress.0 = progress.0.saturating_add(1);
            if let Some(elapsed) = elapsed {
                // A skipped item contributes no sample: folding a 0 ms entry into
                // the average would halve the ETA for every duplicate in the batch.
                progress.1 = progress.1.saturating_add(1);
                progress.2 = progress.2.saturating_add(elapsed.as_millis() as u64);
            }
            *progress
        };
        let remaining = self.total.saturating_sub(finished) as u64;
        // A batch that opens with a run of rejects has nothing timed yet, so there
        // is no average to extrapolate from and no honest estimate to give.
        let eta_seconds = if timed == 0 {
            0
        } else {
            (total_ms / timed as u64).saturating_mul(remaining) / 1000
        };
        // More completions than expected (a re-queued file) must not report 5 of 4.
        (finished.min(self.total), self.total, eta_seconds)
    }
}

/// Publishes one batch-progress tick on both the internal bus and the window.
///
/// Shared with the upload command so an item rejected before it reaches the queue
/// moves the same bar the parsed ones do.
pub fn emit_batch_progress<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    parsed: usize,
    total: usize,
    eta_seconds: u64,
) {
    let payload = serde_json::json!({
        "parsed": parsed,
        "total": total,
        "eta_seconds": eta_seconds,
    });
    crate::statements::events::emit(crate::statements::events::BATCH_PROGRESS, payload.clone());
    let _ = app.emit(crate::statements::events::BATCH_PROGRESS, payload);
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct MandateJob {
    pub extraction: crate::extraction::mandate_extractor::MandateExtraction,
    pub event_type: crate::ingestion::message_processor::MandateEventType,
    pub source_pipeline: String,
    pub source_record_id: String,
    pub connected_account_id: String,
    pub raw_body: Option<String>,
}

#[derive(Clone)]
pub struct Layer6Job {
    pub observation_id: String,
    pub unassigned_id: String,
    pub bank_name: String,
    pub body_text: String,
    pub app_dir: std::path::PathBuf,
    pub internal_date_seconds: Option<i64>,
}

pub(crate) const LAYER6_QUEUE_CAPACITY: usize = 256;

pub static ACTIVE_LAYER6_JOBS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Persists a job, then offers it: what `record_unassigned_transaction`
/// (with `ReviewRoute::AwaitLlm`) plus `offer_layer6_job` do in production.
#[cfg(test)]
pub(crate) async fn enqueue_layer6_job(pool: &Pool, tx: &mpsc::Sender<Layer6Job>, job: Layer6Job) {
    let pending = crate::db::layer6_jobs::PendingLayer6Job {
        id: job.unassigned_id.clone(),
        observation_id: job.observation_id.clone(),
        bank_name: job.bank_name.clone(),
        body_text: job.body_text.clone(),
        internal_date_seconds: job.internal_date_seconds,
    };
    pool.get()
        .await
        .unwrap()
        .interact(move |c| crate::db::layer6_jobs::insert(c, &pending))
        .await
        .unwrap()
        .unwrap();
    offer_layer6_job(tx, job);
}

/// Jobs queued or running in this session, by unassigned id, so a refill
/// never queues one twice. Removed when a job completes; a job that did not
/// complete stays claimed and waits for the next launch, as before.
static LAYER6_CLAIMED: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(Default::default);
/// Set when a durable job did not fit in the queue.
static LAYER6_OVERFLOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Queues a persisted job without waiting; returns false if it did not fit
/// (it stays in `layer6_pending_jobs` and the overflow flag is set).
pub(crate) fn offer_layer6_job(tx: &mpsc::Sender<Layer6Job>, job: Layer6Job) -> bool {
    let id = job.unassigned_id.clone();
    if let Ok(mut claimed) = LAYER6_CLAIMED.lock() {
        claimed.insert(id.clone());
    }
    match tx.try_send(job) {
        Ok(()) => true,
        Err(e) => {
            if let Ok(mut claimed) = LAYER6_CLAIMED.lock() {
                claimed.remove(&id);
            }
            match e {
                mpsc::error::TrySendError::Full(_) => {
                    LAYER6_OVERFLOW.store(true, std::sync::atomic::Ordering::SeqCst);
                    tracing::debug!("Layer 6 queue full — unassigned_id='{}' waits in the table", id);
                }
                mpsc::error::TrySendError::Closed(_) => tracing::error!("Layer 6 Queue closed"),
            }
            false
        }
    }
}

/// Moves overflowed jobs from the table into the queue until it is full
/// again (audit A-7). Only one worker refills at a time.
async fn refill_layer6_queue(pool: &Pool, tx: &mpsc::Sender<Layer6Job>, app_dir: &std::path::Path) {
    if !LAYER6_OVERFLOW.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let pending = match pool.get().await {
        Ok(conn) => conn
            .interact(|c| crate::db::layer6_jobs::select_all(c))
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    for job in pending {
        let claimed = LAYER6_CLAIMED.lock().map(|c| c.contains(&job.id)).unwrap_or(true);
        if claimed {
            continue;
        }
        let layer6_job = Layer6Job {
            observation_id: job.observation_id,
            unassigned_id: job.id,
            bank_name: job.bank_name,
            body_text: job.body_text,
            app_dir: app_dir.to_path_buf(),
            internal_date_seconds: job.internal_date_seconds,
        };
        if !offer_layer6_job(tx, layer6_job) {
            break;
        }
    }
}

/// Recovers statements left `queued` by a crash or quit before this launch.
///
/// The statement queue is in memory, so a `queued` row from an earlier run has
/// no job behind it; and because a `statements` row exists for its message,
/// dedupe would never let it be processed again. A row whose staged PDF still
/// exists is re-enqueued (its password resolved as on first ingest); one
/// without a PDF is deleted so the message can be reprocessed.
pub async fn recover_queued_statements<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &Pool,
    tx: &mpsc::Sender<StatementJob>,
    app_dir: &std::path::Path,
    launched_before: chrono::NaiveDateTime,
) {
    let cutoff = launched_before.format("%Y-%m-%d %H:%M:%S").to_string();
    type QueuedRow = (String, Option<String>, String, Option<String>);
    let rows: Vec<QueuedRow> = match pool.get().await {
        Ok(conn) => conn
            .interact(move |c| -> rusqlite::Result<Vec<QueuedRow>> {
                let mut stmt = c.prepare(
                    "SELECT id, file_hash, source_type, source_filename FROM statements
                     WHERE parse_status = 'queued' AND created_at < ?1",
                )?;
                let rows = stmt
                    .query_map([cutoff], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                    .collect();
                rows
            })
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or_default(),
        Err(_) => return,
    };
    if rows.is_empty() {
        return;
    }
    tracing::info!("Recovering {} statement(s) left queued before this launch", rows.len());

    for (stmt_id, file_hash, source_type, source_filename) in rows {
        let bytes = crate::statements::pdf_storage::read_pdf(app_dir, &stmt_id)
            .ok()
            .flatten();
        let Some(bytes) = bytes else {
            tracing::warn!("Queued statement '{}' has no staged PDF — removing so it can be reprocessed", stmt_id);
            delete_queued_statement(pool, &stmt_id).await;
            continue;
        };
        let file_hash = file_hash.unwrap_or_else(|| {
            use sha2::Digest;
            format!("{:x}", sha2::Sha256::digest(&bytes))
        });
        let filename = source_filename.unwrap_or_else(|| "statement.pdf".to_string());
        let password = match crate::statements::password::resolve_statement_password(
            &stmt_id, &bytes, &filename, &file_hash, pool, app, None,
        )
        .await
        {
            Ok(crate::statements::password::StatementPasswordResolution::Proceed(p)) => p,
            Ok(crate::statements::password::StatementPasswordResolution::PromptCreated) => {
                // Same as first ingest: an awaiting-password entry now owns the
                // staged PDF, and no statements row exists until it is unlocked.
                delete_queued_statement(pool, &stmt_id).await;
                continue;
            }
            Err(e) => {
                tracing::error!("Password resolution failed for queued statement '{}': {}", stmt_id, e);
                continue;
            }
        };
        let origin = if source_type == "manual_upload" { "manual_upload" } else { "email_scan" };
        let job = StatementJob {
            filename,
            file_hash,
            stmt_id,
            batch_progress: None,
            password,
            origin: origin.to_string(),
        };
        if tx.send(job).await.is_err() {
            tracing::error!("Statement Queue closed during queued-statement recovery");
            break;
        }
    }
}

async fn delete_queued_statement(pool: &Pool, stmt_id: &str) {
    let id = stmt_id.to_string();
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

/// Re-queues LLM jobs left pending by a previous session.
///
/// Runs at startup, which is what makes extraction survive a quit mid-scan.
pub async fn replay_pending_layer6_jobs(
    pool: &Pool,
    tx: &mpsc::Sender<Layer6Job>,
    app_dir: std::path::PathBuf,
) {
    // Without a model every replayed job would just fail again. They stay
    // pending (no attempt is spent) until a model is downloaded.
    if !crate::startup::any_model_downloaded(&app_dir) {
        tracing::info!("No LLM model downloaded — Layer 6 replay skipped");
        return;
    }
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
        Ok(Err(e)) => {
            tracing::error!("Failed to read persisted Layer 6 jobs: {}", e);
            return;
        }
        Err(e) => {
            tracing::error!("Failed to read persisted Layer 6 jobs: {}", e);
            return;
        }
    };
    // Sending below blocks once the queue is full, and the workers that drain it
    // need pool connections of their own -- holding one here can deadlock them.
    drop(conn);
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
        // What does not fit is refilled by the workers as they drain.
        if !offer_layer6_job(tx, layer6_job) {
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

/// Blocks while the transaction queue, or all ingestion (review P5), is paused.
async fn wait_while_transaction_queue_paused() {
    while TRANSACTION_QUEUE_PAUSED.load(std::sync::atomic::Ordering::Relaxed)
        || crate::security::incident_response::ingestion_paused()
    {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

/// Blocks while the statement queue, or all ingestion (review P5), is paused.
async fn wait_while_statement_queue_paused() {
    while STATEMENT_QUEUE_PAUSED.load(std::sync::atomic::Ordering::Relaxed)
        || crate::security::incident_response::ingestion_paused()
    {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
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
    /// Emails waiting on the LLM. Their review entries are hidden until the
    /// verdict, so the UI shows this count instead.
    pub llm_pending_jobs: i64,
    /// `ready`, `no_model` or `ineligible` (see `startup::llm_state`).
    pub llm_state: &'static str,
}

#[tauri::command]
/// Reports queue depth and pause state.
pub async fn pipeline_status(
    handles: tauri::State<'_, QueueHandles>,
    pool: tauri::State<'_, Pool>,
    app: tauri::AppHandle,
) -> Result<PipelineStatusResponse, crate::error::AppError> {
    let llm_pending_jobs = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .interact(|c| crate::db::layer6_jobs::count_pending(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
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
            // Read from the channel itself rather than the constant, so the figure
            // stays honest if the queue is ever built with a different capacity.
            queued_jobs: handles
                .transaction_tx
                .max_capacity()
                .saturating_sub(handles.transaction_tx.capacity()),
        },
        statement_queue: StatementQueueStatus {
            state: if stmt_paused { "paused" } else { "running" }.to_string(),
            active_parsers: if stmt_paused {
                0
            } else {
                STATEMENT_QUEUE_MAX_CONCURRENT
            },
            queued_jobs: handles
                .statement_tx
                .max_capacity()
                .saturating_sub(handles.statement_tx.capacity()),
        },
        llm_pending_jobs,
        llm_state: crate::startup::llm_state(&app),
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

    spawn_transaction_workers(transaction_rx, transaction_tx.clone(), pool.clone(), app.clone());
    spawn_statement_dispatcher(statement_rx, pool.clone(), app.clone());
    spawn_mandate_workers(mandate_rx, mandate_tx.clone(), pool.clone());
    spawn_layer6_workers(app.clone(), layer6_rx, layer6_tx.clone(), pool, learning);

    let handles = QueueHandles {
        transaction_tx,
        statement_tx,
        mandate_tx,
        layer6_tx,
    };

    spawn_queue_monitor(app.clone(), handles.clone());

    handles
}

/// Spawns a monitor that reads the queue depths and reports them to the background task registry.
fn spawn_queue_monitor<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    handles: QueueHandles,
) {
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        let mut tx_peak = 0;
        let mut layer6_peak = 0;

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;

            let tx_queued = handles
                .transaction_tx
                .max_capacity()
                .saturating_sub(handles.transaction_tx.capacity());
            let layer6_queued = handles
                .layer6_tx
                .max_capacity()
                .saturating_sub(handles.layer6_tx.capacity())
                + ACTIVE_LAYER6_JOBS.load(std::sync::atomic::Ordering::Relaxed);

            if let Some(registry) =
                app.try_state::<crate::background_tasks::indicator::BackgroundTaskRegistry>()
            {
                if tx_queued > 0 {
                    tx_peak = tx_peak.max(tx_queued);
                    let processed = tx_peak.saturating_sub(tx_queued);
                    registry.register_or_update(
                        &app,
                        "transaction_queue",
                        "system",
                        "Transaction Ingestion",
                        processed as u64,
                        tx_peak as u64,
                        &format!("Processing transactions ({} pending)", tx_queued),
                    );
                } else if tx_peak > 0 {
                    registry.deregister(
                        &app,
                        "transaction_queue",
                        crate::background_tasks::indicator::TaskStatus::Completed,
                        "Transaction Ingestion Complete",
                    );
                    tx_peak = 0;
                }

                if layer6_queued > 0 {
                    layer6_peak = layer6_peak.max(layer6_queued);
                    let processed = layer6_peak.saturating_sub(layer6_queued);
                    registry.register_or_update(
                        &app,
                        "layer6_queue",
                        "normalization",
                        "AI Transaction Extraction",
                        processed as u64,
                        layer6_peak as u64,
                        &format!("Extracting fields ({} pending)", layer6_queued),
                    );
                } else if layer6_peak > 0 {
                    registry.deregister(
                        &app,
                        "layer6_queue",
                        crate::background_tasks::indicator::TaskStatus::Completed,
                        "AI Extraction Complete",
                    );
                    layer6_peak = 0;
                }
            }
        }
    });
}

fn spawn_layer6_workers<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    rx: mpsc::Receiver<Layer6Job>,
    tx: mpsc::Sender<Layer6Job>,
    pool: Pool,
    learning: crate::learning::LearningHandle,
) {
    // A fixed pool (review M4). One task per job let an unbounded number pile
    // up behind the sidecar's own semaphore; now the channel's capacity is
    // the backpressure, and at most LAYER6_WORKERS jobs are in flight.
    let rx = Arc::new(Mutex::new(rx));
    for _ in 0..LAYER6_WORKERS {
        let rx = Arc::clone(&rx);
        let pool = pool.clone();
        let learning = learning.clone();
        let tx = tx.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let job = { rx.lock().await.recv().await };
                let Some(job) = job else { break };
                crate::security::incident_response::wait_while_ingestion_paused().await;
                ACTIVE_LAYER6_JOBS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let (id, app_dir) = (job.unassigned_id.clone(), job.app_dir.clone());
                let retry_job = job.clone();
                // Held, not failed, while this device cannot run the model (no
                // download yet, or under the RAM floor): re-checked every few
                // minutes without using an attempt (owner decision 2026-11-25:
                // incomplete mail always waits for the LLM).
                let done = if crate::startup::llm_ready(&app) {
                    process_layer6_job(job, &pool, &learning).await
                } else if !review_entry_open(&pool, &id).await {
                    // The user finished or dismissed the entry while it was
                    // held (owner decision Q-2 lets them): drop the job
                    // instead of re-offering it forever.
                    delete_persisted_layer6_job(&pool, &id).await;
                    Layer6Done { completed: true, booked: false, retry_after: None }
                } else {
                    Layer6Done { completed: false, booked: false, retry_after: Some(LAYER6_NO_MODEL_RECHECK) }
                };
                // Offered again after the delay; the claim is kept meanwhile so
                // a refill cannot queue it twice (owner decision, audit N-A3).
                if let Some(delay) = done.retry_after {
                    let tx = tx.clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(delay).await;
                        offer_layer6_job(&tx, retry_job);
                    });
                }
                if done.completed {
                    if let Ok(mut claimed) = LAYER6_CLAIMED.lock() {
                        claimed.remove(&id);
                    }
                }
                // Tell the UI (audit B-1): any outcome can change the review
                // entry; a booking adds a ledger row.
                let payload = serde_json::json!({ "unassigned_id": id });
                if done.booked {
                    let _ = crate::ipc::events::emit_event(
                        &app,
                        crate::ipc::events::AppEvent::TransactionCreated,
                        payload.clone(),
                    );
                }
                // Only when something changed: a held job re-offered every
                // five minutes used to refetch the review queue each time
                // (audit F7-3).
                if done.completed || done.booked {
                    let _ = crate::ipc::events::emit_event(
                        &app,
                        crate::ipc::events::AppEvent::UnassignedChanged,
                        payload,
                    );
                }
                ACTIVE_LAYER6_JOBS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                refill_layer6_queue(&pool, &tx, &app_dir).await;
            }
        });
    }
}

/// Layer 6 workers, matching the documented queue design.
const LAYER6_WORKERS: usize = 6;

const LAYER6_AUTO_RESOLVE_CONFIDENCE_THRESHOLD: f64 = crate::extraction::ladder::MIN_LEDGER_CONFIDENCE;

/// Whether a review entry is still open. A read failure counts as open, so
/// a job is never dropped on a transient error.
async fn review_entry_open(pool: &Pool, unassigned_id: &str) -> bool {
    let id = unassigned_id.to_string();
    let Ok(conn) = pool.get().await else { return true };
    conn.interact(move |c| crate::db::unassigned_transactions::is_open(c, &id))
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or(true)
}

/// Clears the durable row for a job that has genuinely finished.
///
/// Called only once the outcome is committed. A failed delete leaves the job to be
/// replayed at the next launch, which `apply_layer6_success` absorbs rather than
/// double-applying.
async fn delete_persisted_layer6_job(pool: &Pool, unassigned_id: &str) {
    let id = unassigned_id.to_string();
    let deleted = match pool.get().await {
        Ok(conn) => conn
            .interact(move |c| crate::db::layer6_jobs::delete(c, &id))
            .await
            .map_err(|e| anyhow::anyhow!("Interact error: {}", e))
            .and_then(|inner| inner),
        Err(e) => Err(anyhow::anyhow!("DB pool error: {}", e)),
    };
    if let Err(e) = deleted {
        tracing::error!(
            "Failed to delete completed Layer 6 job for unassigned_id='{}': {:#}",
            unassigned_id,
            e
        );
    }
}

/// What a Layer-6 job did: whether it is finished (its row deleted) and
/// whether it booked a transaction.
struct Layer6Done {
    completed: bool,
    booked: bool,
    /// When to offer the job again this session (audit N-A3); `None` when it
    /// is finished or dead.
    retry_after: Option<std::time::Duration>,
}

/// How long a job that found no model waits before checking again; no
/// attempt is used (owner decision, audit N-A3).
const LAYER6_NO_MODEL_RECHECK: std::time::Duration = std::time::Duration::from_secs(300);

/// Delay before the next attempt after `attempts` failed ones: 1 minute,
/// then 5, then every 15 for as long as it takes (owner decision 2026-11-25:
/// an infrastructure failure is never final).
fn layer6_retry_delay(attempts: i64) -> std::time::Duration {
    std::time::Duration::from_secs(match attempts {
        ..=1 => 60,
        2 => 300,
        _ => 900,
    })
}

/// Processes one LLM extraction job.
///
/// The durable row survives until the outcome is committed: extraction runs for a
/// long time, and deleting up front means a quit or crash mid-run loses the work
/// the persistence exists to protect.
async fn process_layer6_job(
    job: Layer6Job,
    pool: &Pool,
    learning: &crate::learning::LearningHandle,
) -> Layer6Done {
    use crate::extraction::llm::Layer6Outcome;

    let layer = crate::extraction::ladder::Layer6LlmLayer {
        app_dir: Some(job.app_dir.clone()),
        fallback_event_time: job.internal_date_seconds,
    };
    let result = layer.run(pool, &job.bank_name, &job.body_text).await;
    let mut booked = false;
    let completed = match result {
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

            let confirmed = enriched.clone();
            match apply_layer6_success(
                pool,
                &job.observation_id,
                &job.unassigned_id,
                enriched,
                job.internal_date_seconds,
            )
            .await
            {
                Ok(did_book) => {
                    booked = did_book;
                    // A committed, grounded LLM value is the evidence pending
                    // drift rules need to be promoted or retired (M10).
                    let (bank, body) = (job.bank_name.clone(), job.body_text.clone());
                    if let Ok(conn) = pool.get().await {
                        let _ = conn
                            .interact(move |c| {
                                crate::db::field_rules::shadow_evaluate_pending(
                                    c, &bank, "email", &body, &confirmed,
                                )
                            })
                            .await;
                    }
                    true
                }
                Err(e) => {
                    tracing::error!(
                        "Layer 6 background worker: failed to apply success for observation_id='{}': {}",
                        job.observation_id, e
                    );
                    false
                }
            }
        }
        // The model answered, but with a value the email does not contain,
        // even after the correction retry: content-caused, so final (audit
        // A-8), and the entry goes to review (owner decision 2026-11-25).
        Layer6Outcome::Rejected => mark_llm_rejected(pool, &job.unassigned_id).await,
        // A timeout says nothing about the message; retried until it answers.
        Layer6Outcome::TimedOut => false,
        Layer6Outcome::NotATransaction => {
            let mark_result: anyhow::Result<()> = async {
                let unassigned_id = job.unassigned_id.clone();
                let conn = pool.get().await?;
                conn.interact(move |c| -> anyhow::Result<()> {
                    crate::db::unassigned_transactions::update_reason(
                        c,
                        &unassigned_id,
                        "not_a_transaction",
                    )?;
                    crate::db::unassigned_transactions::update_status(
                        c,
                        &unassigned_id,
                        "ignored",
                    )?;
                    Ok(())
                })
                .await
                .map_err(|e| anyhow::anyhow!("Interact error: {}", e))??;
                Ok(())
            }
            .await;
            match mark_result {
                Ok(()) => true,
                Err(e) => {
                    tracing::error!(
                        "Layer 6 background worker: failed to ignore unassigned_id='{}': {}",
                        job.unassigned_id, e
                    );
                    false
                }
            }
        }
        Layer6Outcome::Failed => {
            tracing::info!(
                "Layer 6 background worker: infra failure for observation_id='{}' — will retry",
                job.observation_id
            );
            false
        }
        // No model to run: re-checked every few minutes without using an
        // attempt, so a missing download never kills it (audit A-8) and a model
        // downloaded mid-session drains the backlog (audit N-A3).
        Layer6Outcome::Unavailable => {
            tracing::info!(
                "Layer 6 background worker: no model available for observation_id='{}' — waiting",
                job.observation_id
            );
            return Layer6Done { completed: false, booked: false, retry_after: Some(LAYER6_NO_MODEL_RECHECK) };
        }
    };

    if completed {
        delete_persisted_layer6_job(pool, &job.unassigned_id).await;
        return Layer6Done { completed: true, booked, retry_after: None };
    } else {
        // A timeout, a dead sidecar or a failed write says nothing about the
        // message: keep the row and retry it this session after a growing
        // delay, with no attempt cap (owner decision 2026-11-25). Launch
        // replay still covers a quit.
        let id = job.unassigned_id.clone();
        let attempts = match pool.get().await {
            Ok(conn) => conn
                .interact(move |c| crate::db::layer6_jobs::record_failed_attempt(c, &id))
                .await
                .ok()
                .and_then(|r| r.ok())
                .unwrap_or(0),
            Err(_) => 0,
        };
        let delay = layer6_retry_delay(attempts);
        tracing::warn!(
            "Layer 6 job for unassigned_id='{}' did not complete — retrying in {:?}",
            job.unassigned_id,
            delay
        );
        return Layer6Done { completed: false, booked: false, retry_after: Some(delay) };
    }
}

/// Review reason for an LLM answer that named a value absent from the email.
pub(crate) const LLM_VALUE_NOT_IN_EMAIL: &str = "llm_value_not_in_email";

/// Re-labels a review entry still waiting on enrichment as
/// `llm_value_not_in_email`; returns whether the write succeeded.
async fn mark_llm_rejected(pool: &Pool, unassigned_id: &str) -> bool {
    let id = unassigned_id.to_string();
    let result = match pool.get().await {
        Ok(conn) => conn
            .interact(move |c| -> anyhow::Result<()> {
                use rusqlite::OptionalExtension;
                let current_reason: Option<String> = c
                    .query_row(
                        "SELECT reason FROM unassigned_transactions WHERE id = ?1",
                        rusqlite::params![id],
                        |row| row.get(0),
                    )
                    .optional()?;
                if current_reason.as_deref() == Some("pending_llm_enrichment") {
                    crate::db::unassigned_transactions::update_reason(c, &id, LLM_VALUE_NOT_IN_EMAIL)?;
                }
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!("Interact error: {}", e))
            .and_then(|r| r),
        Err(e) => Err(anyhow::anyhow!("DB pool error: {}", e)),
    };
    if let Err(e) = &result {
        tracing::error!(
            "Layer 6 background worker: failed to update reason for unassigned_id='{}': {}",
            unassigned_id, e
        );
    }
    result.is_ok()
}

/// Applies a successful LLM extraction.
///
/// The caller clears the persisted job only once this has committed, so a crash
/// between the two replays the job rather than losing it -- which is why applying
/// an already-promoted observation is a no-op rather than a second promotion.
pub(crate) async fn apply_layer6_success(
    pool: &Pool,
    observation_id: &str,
    unassigned_id: &str,
    enriched: ExtractionResult,
    internal_date_seconds: Option<i64>,
) -> anyhow::Result<bool> {
    let observation_id = observation_id.to_string();
    let unassigned_id = unassigned_id.to_string();
    let conn = pool.get().await?;
    let booked = conn.interact(move |c| -> anyhow::Result<bool> {
        // Extraction takes minutes, so the user may have resolved or dismissed
        // the entry while it ran. Booking a transaction for an entry they
        // dismissed overrules them with money, so the job stops here -- and
        // stops *successfully*, since there is genuinely nothing left to do.
        if !crate::db::unassigned_transactions::is_open(c, &unassigned_id)? {
            tracing::info!(
                "Layer 6 result for observation '{}' arrived after the entry left the queue — dropping it",
                observation_id
            );
            return Ok(false);
        }

        let row_opt = crate::db::transaction_observations::get_observation(c, &observation_id)?;
        // The observation can be absent when the user manually cleared the unassigned
        // transaction while Layer 6 was still running. There is nothing left to do, but
        // this is not an infrastructure failure — treat it as successfully completed so
        // the persisted job row is deleted and the job is never replayed again.
        let mut row = match row_opt {
            Some(r) => r,
            None => {
                tracing::info!(
                    "Layer 6 result for observation '{}' — observation no longer exists (cleared by user?); resolving job",
                    observation_id
                );
                return Ok(false);
            }
        };

        // A job replayed after a crash between the commit and the row's deletion
        // arrives here with the observation already promoted. Reconciling it a
        // second time would mint a duplicate transaction.
        if row.canonical_transaction_id.is_some() {
            tracing::info!(
                "Layer 6 result for observation '{}' was already applied — resolving the replayed job",
                observation_id
            );
            crate::db::unassigned_transactions::settle_if_open(c, &unassigned_id, "resolved")?;
            return Ok(false);
        }

        // Layer 6 exists to fill in what layers 1-5 could not, so a field the model
        // left empty has to keep whatever they did extract. Assigning straight
        // through wiped a regex-recovered amount whenever the LLM returned only a
        // merchant, and the review queue then showed an entry with no amount at all
        // -- strictly worse than before the enrichment ran.
        row.amount_minor = enriched.amount_minor.or(row.amount_minor);
        row.currency = enriched.currency.or(row.currency.take());
        row.direction = enriched.direction.or(row.direction.take());
        row.merchant_raw = enriched.merchant_raw.or(row.merchant_raw.take());
        row.reference_id = enriched.reference_id.or(row.reference_id.take());
        // Written to the row, not just consulted below: the Layer 1-5 path persists
        // the detected channel, and dropping it here left every LLM-enriched
        // observation with a NULL channel and handed reconciliation a null too.
        row.channel = enriched.channel.or(row.channel.take());

        let is_self_transfer = row.channel.as_deref() == Some("internal_transfer");
        if is_self_transfer {
            if let Some(dest_account) =
                crate::ingestion::message_processor::MessageProcessor::self_transfer_destination_account(
                    row.merchant_raw.as_deref(),
                )
            {
                row.merchant_normalized = Some(format!("Internal Transfer (A/c {dest_account})"));
            }
        }
        // The LLM supplies event_time, so out-of-range values are expected input,
        // not an impossibility -- unwrapping one panics the worker thread. Fall back
        // to the message's own timestamp when the model's is unusable.
        use chrono::TimeZone;
        let to_utc = |ts: i64| chrono::Utc.timestamp_opt(ts, 0).single();
        if let Some(dt_utc) = enriched
            .event_time
            .and_then(to_utc)
            .or_else(|| internal_date_seconds.and_then(to_utc))
        {
            let ist_offset = chrono::FixedOffset::east_opt(5 * 3600 + 30 * 60)
                .expect("IST offset is a valid fixed offset");
            row.event_time = Some(dt_utc.with_timezone(&ist_offset).naive_local());
        } else if enriched.event_time.is_some() || internal_date_seconds.is_some() {
            tracing::warn!(
                "Layer 6 returned an out-of-range event_time ({:?}) for observation '{}' — leaving event_time unchanged",
                enriched.event_time.or(internal_date_seconds),
                observation_id
            );
        }
        // The layer that produced the values: the LLM, or the rule ladder
        // when a re-check (`ingestion::recheck`) applies through here.
        row.extraction_method = Some(enriched.extraction_method.clone())
            .filter(|m| !m.is_empty())
            .or(row.extraction_method.take());
        row.confidence_score = enriched.confidence_score;
        // An untrusted sender (audit E-3 / N-E1) is only ever pre-filled.
        let review_only = row
            .raw_payload_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
            .is_some_and(|v| v["review_only"] == serde_json::Value::Bool(true));

        let resolved = crate::db::instruments::resolve_instrument(
            c,
            enriched.instrument_type.as_deref(),
            enriched.issuer_name.as_deref(),
            enriched.masked_identifier.as_deref(),
            enriched.network.as_deref(),
        );
        let instrument_archived = matches!(&resolved, Err(e)
            if e.downcast_ref::<crate::db::instruments::InstrumentArchived>().is_some());
        let instrument_id = match resolved {
            Ok(id) => id,
            Err(_) if instrument_archived => None,
            Err(e) => return Err(e),
        };
        // An instrument the earlier layers already resolved stands when the
        // model named none: the enrichment fills gaps, it does not erase.
        let instrument_id = if instrument_archived {
            None
        } else {
            instrument_id.or(row.instrument_id.take())
        };
        row.instrument_id = instrument_id.clone();

        let confident_enough = is_self_transfer
            || enriched
                .confidence_score
                .map(|s| s >= LAYER6_AUTO_RESOLVE_CONFIDENCE_THRESHOLD)
                .unwrap_or(false);

        let has_instrument = instrument_id.is_some();
        // A balance-update placeholder carries amount_minor = 0 and merchant_raw =
        // "Balance Update". evaluate_gate3 sees both as present and would auto-reconcile
        // it as a real ₹0 transaction. Guard against that: only promote observations
        // whose amount is actually positive.
        let has_nonzero_amount = row.amount_minor.map(|a| a > 0).unwrap_or(false);
        // Booking defaults a missing direction to debit and a missing currency
        // to INR, so both must be present: a guess is never written to the
        // ledger, the entry goes to review instead.
        let ready = !review_only
            && has_instrument
            && has_nonzero_amount
            && row.direction.is_some()
            && row.currency.is_some()
            && confident_enough
            && crate::extraction::ladder::evaluate_gate3(
                row.amount_minor.is_some(),
                row.merchant_raw.is_some(),
                true,
            );
        // Same order of blame as `gate3_failure_reason`, which is what wrote
        // the reason in the first place. Without the amount arm an entry the
        // LLM left with no amount was filed as "low_confidence", sending the
        // user looking for a confidence problem that was not the reason.
        let review_reason = if ready {
            None
        } else if row.amount_minor.is_none() {
            Some("gate3_failed:missing_amount")
        } else if row.merchant_raw.is_none() {
            Some("gate3_failed:missing_counterparty")
        } else if row.direction.is_none() {
            Some("gate3_failed:missing_direction")
        } else if row.currency.is_none() {
            Some("gate3_failed:missing_currency")
        } else if instrument_archived {
            Some("instrument_archived")
        } else if !has_instrument {
            Some("gate3_failed:missing_instrument")
        } else if review_only {
            Some("unverified_sender")
        } else {
            Some("gate3_failed:low_confidence")
        };
        // One transaction for update + reconcile + settle (audit A-9).
        let decision = crate::db::retry_on_busy(c, || {
            crate::reconciliation::engine::complete_observation(c, &row, &unassigned_id, review_reason)
        })?;
        Ok(decision.is_some())
    })
    .await
    .map_err(|e| anyhow::anyhow!("Interact error: {}", e))??;
    Ok(booked)
}

fn transaction_job_key(job: &TransactionJob) -> String {
    format!("transaction:{}", job.source_record_id)
}

fn mandate_job_key(job: &MandateJob) -> String {
    format!("mandate:{}", job.source_record_id)
}

/// Saves a job to `pending_ingest_jobs`. A failure is returned, not just
/// logged (audit N-A2): the caller must not move past a message whose job
/// is not durable.
async fn persist_job(
    pool: &Pool,
    id: String,
    kind: &'static str,
    payload: anyhow::Result<String>,
) -> anyhow::Result<()> {
    let payload = payload.map_err(|e| anyhow::anyhow!("could not serialise {kind} job '{id}': {e}"))?;
    let conn = pool.get().await.map_err(|e| anyhow::anyhow!("pool error: {e}"))?;
    // Retried while a long statement commit holds the write lock (audit N-D2).
    conn.interact(move |c| {
        crate::db::retry_on_busy(c, || crate::db::pending_ingest_jobs::upsert(c, &id, kind, &payload))
    })
        .await
        .map_err(|e| anyhow::anyhow!("interact error: {e}"))?
        .map_err(|e| e.context(format!("could not persist {kind} job")))
}

/// Counts a failed attempt for a persisted job; after MAX_ATTEMPTS it is
/// marked dead and no longer replayed (H4 follow-up). A dead job is also
/// recorded in `scan_failed_messages` for its Gmail message, so it shows up
/// under "Retry failed" instead of disappearing (audit A-1).
async fn record_failed_job_attempt(pool: &Pool, id: String, account_id: String, msg_id: String) -> bool {
    let Ok(conn) = pool.get().await else { return false };
    let key = id.clone();
    let dead = conn
        .interact(move |c| -> anyhow::Result<bool> {
            let dead = crate::db::pending_ingest_jobs::record_failed_attempt(c, &key)?;
            if dead {
                crate::db::scan_failed_messages::delete_for_message(c, &account_id, &msg_id)?;
                crate::db::scan_failed_messages::insert(
                    c,
                    &crate::db::scan_failed_messages::ScanFailedMessageRow {
                        id: uuid::Uuid::new_v4().to_string(),
                        account_id,
                        msg_id,
                        error: format!(
                            "ingest failed {} times",
                            crate::db::pending_ingest_jobs::MAX_ATTEMPTS
                        ),
                        failed_at: None,
                    },
                )?;
            }
            Ok(dead)
        })
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or(false);
    if dead {
        tracing::warn!(
            "Ingest job '{}' failed {} times — marked dead, listed under Retry failed",
            id,
            crate::db::pending_ingest_jobs::MAX_ATTEMPTS
        );
    }
    dead
}

/// Delay before a failed ingest job is retried in the same session.
const INGEST_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(30);

/// Re-queues a failed, still-live job after `delay` (audit B-5); it used to
/// wait for the next launch. Read back from `pending_ingest_jobs`, so a job
/// finished, retried or marked dead meanwhile is not sent again. If the app
/// quits first, launch replay picks it up as before.
fn schedule_ingest_retry<J>(pool: Pool, tx: mpsc::Sender<J>, key: String, delay: std::time::Duration)
where
    J: serde::de::DeserializeOwned + Send + 'static,
{
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        let payload = match pool.get().await {
            Ok(conn) => {
                let id = key.clone();
                conn.interact(move |c| crate::db::pending_ingest_jobs::select_live_payload(c, &id))
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .flatten()
            }
            Err(_) => None,
        };
        match payload.map(|p| serde_json::from_str::<J>(&p)) {
            Some(Ok(job)) => {
                if tx.send(job).await.is_err() {
                    tracing::warn!("Queue closed — job '{}' left for launch replay", key);
                }
            }
            Some(Err(e)) => tracing::error!("Unreadable persisted job '{}' on retry: {}", key, e),
            None => {}
        }
    });
}

async fn delete_persisted_job(pool: &Pool, id: String) {
    if let Ok(conn) = pool.get().await {
        let _ = conn
            .interact(move |c| crate::db::pending_ingest_jobs::delete(c, &id))
            .await;
    }
}

/// Persists a transaction job, then queues it (review H4). Nothing is queued
/// when the job could not be persisted (audit N-A2).
pub async fn send_transaction_job(
    pool: &Pool,
    tx: &mpsc::Sender<TransactionJob>,
    job: TransactionJob,
) -> anyhow::Result<()> {
    let payload = serde_json::to_string(&job).map_err(anyhow::Error::from);
    persist_job(pool, transaction_job_key(&job), "transaction", payload).await?;
    if tx.send(job).await.is_err() {
        tracing::error!("Transaction Queue closed — job kept for replay at next launch");
    }
    Ok(())
}

/// Queues a Gmail transaction alert: the one copy shared by live polling and
/// historical scans (audit F-3).
pub async fn dispatch_transaction_alert<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &Pool,
    account_id: &str,
    msg_id: &str,
    raw_body: Option<String>,
    obs: crate::extraction::ladder::ExtractionResult,
    email_meta: crate::ingestion::message_processor::EmailMetadata,
) -> anyhow::Result<()> {
    let job = TransactionJob {
        obs,
        source_pipeline: "gmail_transaction".to_string(),
        source_record_id: msg_id.to_string(),
        connected_account_id: account_id.to_string(),
        raw_body,
        email_meta: Some(email_meta),
    };
    let tx = tauri::Manager::state::<QueueHandles>(app).transaction_tx.clone();
    send_transaction_job(pool, &tx, job).await
}

/// Queues a Gmail mandate event; shared like `dispatch_transaction_alert`.
pub async fn dispatch_mandate_event<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &Pool,
    account_id: &str,
    msg_id: &str,
    raw_body: Option<String>,
    extraction: crate::extraction::mandate_extractor::MandateExtraction,
    event_type: crate::ingestion::message_processor::MandateEventType,
) -> anyhow::Result<()> {
    let job = MandateJob {
        extraction,
        event_type,
        source_pipeline: "gmail_transaction".to_string(),
        source_record_id: msg_id.to_string(),
        connected_account_id: account_id.to_string(),
        raw_body,
    };
    let tx = tauri::Manager::state::<QueueHandles>(app).mandate_tx.clone();
    send_mandate_job(pool, &tx, job).await
}

/// Persists a mandate job, then queues it (review H4). Nothing is queued when
/// the job could not be persisted (audit N-A2).
pub async fn send_mandate_job(
    pool: &Pool,
    tx: &mpsc::Sender<MandateJob>,
    job: MandateJob,
) -> anyhow::Result<()> {
    let payload = serde_json::to_string(&job).map_err(anyhow::Error::from);
    persist_job(pool, mandate_job_key(&job), "mandate", payload).await?;
    if tx.send(job).await.is_err() {
        tracing::error!("Mandate Queue closed — job kept for replay at next launch");
    }
    Ok(())
}

/// Re-queues transaction and mandate jobs persisted before this launch.
pub async fn replay_pending_ingest_jobs(
    pool: &Pool,
    transaction_tx: &mpsc::Sender<TransactionJob>,
    mandate_tx: &mpsc::Sender<MandateJob>,
    launched_before: chrono::NaiveDateTime,
) {
    let cutoff = launched_before.format("%Y-%m-%d %H:%M:%S").to_string();
    const PAGE: usize = 100;
    let mut after: Option<(String, String)> = None;
    let mut replayed = 0usize;
    // Paged (audit D-3): a fresh connection per page, released before the
    // sends, which can wait on workers that need connections of their own.
    loop {
        let page = match pool.get().await {
            Ok(conn) => {
                let (cutoff, after) = (cutoff.clone(), after.clone());
                conn.interact(move |c| {
                    crate::db::pending_ingest_jobs::select_before(c, &cutoff, after.as_ref(), PAGE)
                })
                .await
                .ok()
                .and_then(|r| r.ok())
                .unwrap_or_default()
            }
            Err(_) => return,
        };
        let Some(last) = page.last() else { break };
        after = Some((last.created_at.clone(), last.id.clone()));
        let full_page = page.len() == PAGE;
        for job in page {
            let (id, kind, payload) = (job.id, job.kind, job.payload_json);
            let sent = match kind.as_str() {
                "transaction" => match serde_json::from_str::<TransactionJob>(&payload) {
                    Ok(job) => transaction_tx.send(job).await.is_ok(),
                    Err(e) => {
                        tracing::error!("Unreadable persisted job '{}' ({}), discarding", id, e);
                        delete_persisted_job(pool, id).await;
                        continue;
                    }
                },
                "mandate" => match serde_json::from_str::<MandateJob>(&payload) {
                    Ok(job) => mandate_tx.send(job).await.is_ok(),
                    Err(e) => {
                        tracing::error!("Unreadable persisted job '{}' ({}), discarding", id, e);
                        delete_persisted_job(pool, id).await;
                        continue;
                    }
                },
                _ => continue,
            };
            if !sent {
                tracing::error!("Queue closed during ingest job replay");
                return;
            }
            replayed += 1;
        }
        if !full_page {
            break;
        }
    }
    if replayed > 0 {
        tracing::info!("Replayed {} ingest job(s) persisted before the last restart", replayed);
    }
}

/// Spawns the mandate-processing workers.
fn spawn_mandate_workers(mut rx: mpsc::Receiver<MandateJob>, tx: mpsc::Sender<MandateJob>, pool: Pool) {
    tauri::async_runtime::spawn(async move {
        while let Some(job) = rx.recv().await {
            crate::security::incident_response::wait_while_ingestion_paused().await;
            let key = mandate_job_key(&job);
            let account_id = job.connected_account_id.clone();
            let msg_id = job.source_record_id.clone();
            if process_mandate_job(job, &pool).await {
                delete_persisted_job(&pool, key).await;
            } else if !record_failed_job_attempt(&pool, key.clone(), account_id, msg_id).await {
                schedule_ingest_retry(pool.clone(), tx.clone(), key, INGEST_RETRY_DELAY);
            }
        }
    });
}

/// Processes one mandate event.
async fn process_mandate_job(
    job: MandateJob,
    pool: &Pool,
) -> bool {
    let extraction = job.extraction.clone();
    let event_type = job.event_type.clone();

    // False when the mandate could not be recorded, so the job is kept and
    // replayed (recording is idempotent on source_record_id).
    let mut recorded = true;
    let conn = match pool.get().await {
        Ok(conn) => Some(conn),
        Err(e) => {
            // The job is kept for replay; log it, since silence would hide
            // that the mandate was not recorded.
            tracing::error!(
                "Mandate Queue: DB pool unavailable for source_record_id='{}' — mandate not recorded: {}",
                job.source_record_id,
                e
            );
            recorded = false;
            None
        }
    };
    if let Some(conn) = conn {
        let outcome = conn
            .interact(move |c| -> bool {
                // Errors are logged and reported instead of swallowed (M5),
                // so the job is kept for replay.
                let mut ok = true;
                let mut check = |what: &str, r: anyhow::Result<()>| {
                    if let Err(e) = r {
                        tracing::error!("Mandate Queue: {} failed: {:#}", what, e);
                        ok = false;
                    }
                };
                let instrument_id = crate::db::instruments::resolve_instrument(
                    c,
                    extraction.instrument_type.as_deref(),
                    extraction.issuer_name.as_deref(),
                    extraction.masked_identifier.as_deref(),
                    None,
                ).unwrap_or(None);
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
                            check(
                                "recording the mandate",
                                crate::db::recurring_payments::upsert_explicit(
                                    c,
                                    instrument_id,
                                    merchant_entity_id,
                                    extraction.max_limit_amount,
                                    "INR",
                                    extraction.cadence.as_deref(),
                                    extraction.external_mandate_id.as_deref(),
                                )
                                .map(|_| ())
                                .map_err(anyhow::Error::from),
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
                                check(
                                    "cancelling the mandate",
                                    crate::db::recurring_payments::mark_cancelled(c, &candidates[0].id)
                                        .map(|_| ())
                                        .map_err(anyhow::Error::from),
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
                                check(
                                    "recording an unresolved cancellation",
                                    crate::db::unresolved_mandate_cancellations::insert_unresolved(
                                        c,
                                        &raw_signal,
                                        &candidate_ids,
                                    )
                                    .map(|_| ())
                                    .map_err(anyhow::Error::from),
                                );
                            }
                        }
                    }
                }
                ok
            })
            .await;
        if matches!(outcome, Ok(false)) {
            recorded = false;
        }
        if let Err(e) = outcome {
            recorded = false;
            tracing::error!(
                "Mandate Queue: DB interact failed for source_record_id='{}' — mandate not recorded: {}",
                job.source_record_id,
                e
            );
        }
    }

    // Mandate events are recurring-payment markers (recurring_payments and
    // processed_messages), not ledger entries: no ₹0 transaction is booked
    // any more (review M5).
    recorded
}

/// Spawns the transaction-processing workers.
fn spawn_transaction_workers<R: tauri::Runtime>(
    rx: mpsc::Receiver<TransactionJob>,
    tx: mpsc::Sender<TransactionJob>,
    pool: Pool,
    app: tauri::AppHandle<R>,
) {
    let rx = Arc::new(Mutex::new(rx));
    for _ in 0..TRANSACTION_QUEUE_WORKERS {
        let rx = Arc::clone(&rx);
        let tx = tx.clone();
        let pool = pool.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let job = { rx.lock().await.recv().await };
                match job {
                    Some(job) => {
                        // Checked after the receive, not before: a worker already
                        // parked in recv() when the pause arrives would otherwise
                        // process one more job while the queue reports "paused".
                        wait_while_transaction_queue_paused().await;
                        let key = transaction_job_key(&job);
                        let account_id = job.connected_account_id.clone();
                        let msg_id = job.source_record_id.clone();
                        if process_transaction_job(job, &pool, &app).await {
                            delete_persisted_job(&pool, key).await;
                        } else if !record_failed_job_attempt(&pool, key.clone(), account_id, msg_id).await {
                            schedule_ingest_retry(pool.clone(), tx.clone(), key, INGEST_RETRY_DELAY);
                        }
                    }
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
                    Ok(dir) => Some(dir),
                    Err(e) => {
                        tracing::error!(
                            "Statement Queue job failed (file='{}'): could not resolve app data dir: {}",
                            job.filename, e
                        );
                        None
                    }
                };

                // Every exit path has to fall out here rather than returning early:
                // a job that skips the progress record below strands the batch's
                // ETA on a total that is never reached. A panic inside the parser
                // -- a malformed PDF is attacker-supplied input as far as the
                // parsing crates are concerned -- is such an exit path, so it is
                // caught and turned into an ordinary failure instead of killing
                // the task with the staged PDF and the batch's progress still owed.
                let parse = async {
                    let app_data_dir = app_data_dir
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("could not resolve app data dir"))?;
                    let bytes =
                        crate::statements::pdf_storage::read_pdf(app_data_dir, &job.stmt_id)
                            .map_err(|e| anyhow::anyhow!("could not read staged PDF: {}", e))?
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "staged PDF for stmt_id='{}' is missing",
                                    job.stmt_id
                                )
                            })?;
                    let outcome = crate::commands::stage_parse_pipeline(
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
                    outcome
                };
                let result: anyhow::Result<crate::commands::PipelineOutcome> =
                    match std::panic::AssertUnwindSafe(parse).catch_unwind().await {
                        Ok(outcome) => outcome,
                        Err(_) => Err(anyhow::anyhow!("statement parser panicked")),
                    };

                // A staged draft keeps the PDF under this same id --
                // statements_get_draft_pdf reads it back for review -- so the staged
                // copy is only swept up when nothing downstream owns it any more.
                if !matches!(result, Ok(crate::commands::PipelineOutcome::Staged(_))) {
                    if let Some(dir) = &app_data_dir {
                        let _ = crate::statements::pdf_storage::delete_pdf(dir, &job.stmt_id);
                    }
                }

                if let Some(tracker) = &job.batch_progress {
                    let (parsed, total, eta_seconds) = tracker.record_completion(start.elapsed());
                    emit_batch_progress(&app, parsed, total, eta_seconds);
                }

                use crate::statements::events;
                match &result {
                    Ok(crate::commands::PipelineOutcome::Staged(_draft_id)) => {}
                    Ok(crate::commands::PipelineOutcome::BlockedAwaitingInstrument(
                        _unprocessed_id,
                    )) => {}
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
) -> bool {
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
    let source_record_id = job.source_record_id;

    let notify_amount_minor = row.amount_minor;
    let notify_direction = row.direction.clone();
    let notify_merchant = row.merchant_raw.clone();

    // Dropping a job because the pool or the interact failed is a silently lost
    // transaction; it has to leave a trace.
    let conn = match pool.get().await {
        Ok(conn) => conn,
        Err(e) => {
            tracing::error!(
                "Transaction Queue: DB pool unavailable — keeping observation for replay, source_record_id='{}': {}",
                source_record_id,
                e
            );
            return false;
        }
    };

    let outcome = conn
        .interact(
            move |c| -> Result<Option<(crate::reconciliation::audit::DecisionType, String)>, String> {
                let mut unresolved_reason = crate::db::unassigned_transactions::MISSING_INSTRUMENT;
                match crate::db::instruments::resolve_instrument(
                    c,
                    instrument_type.as_deref(),
                    issuer_name.as_deref(),
                    masked_identifier.as_deref(),
                    network.as_deref(),
                ) {
                    Ok(Some(instr_id)) => {
                        row.instrument_id = Some(instr_id);
                    }
                    Ok(None) => {}
                    // An archived card's new alert goes to review (owner
                    // decision, audit A-2); ingest_observation files it.
                    Err(e) if e.downcast_ref::<crate::db::instruments::InstrumentArchived>().is_some() => {
                        unresolved_reason = "instrument_archived";
                    }
                    Err(e) => return Err(format!("instrument resolution failed: {e}")),
                }

                if let (Some(ref instrument_id), Some(ref direction), Some(amount_minor)) =
                    (&row.instrument_id, &row.direction, row.amount_minor)
                {
                    let event_bucket = row
                        .event_time
                        .map(|dt| dt.format("%Y-%m-%dT%H:%M").to_string())
                        .unwrap_or_default();
                    row.fingerprint = Some(crate::extraction::fingerprint::compute_fingerprint(
                        instrument_id,
                        direction,
                        amount_minor,
                        &event_bucket,
                        &connected_account_id,
                    ));
                }

                match crate::db::retry_on_busy(c, || {
                    crate::reconciliation::engine::ingest_observation(c, &row, unresolved_reason)
                }) {
                    Ok((_, Some(decision))) => {
                        tracing::debug!(
                            "Reconciliation decision for obs '{}': {:?}",
                            row.id,
                            decision
                        );
                        Ok(Some((decision, row.id.clone())))
                    }
                    Ok((_, None)) => Ok(None),
                    // Nothing was stored (the observation insert is inside the
                    // failed transaction), so this is not "done": keep the job
                    // so it is retried rather than lost (audit A-1).
                    Err(e) => Err(e.to_string()),
                }
            },
        )
        .await;

    let outcome = match outcome {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(e)) => {
            tracing::error!(
                "Transaction Queue: observation ingest failed — keeping job for retry, source_record_id='{}': {}",
                source_record_id,
                e
            );
            return false;
        }
        Err(e) => {
            tracing::error!(
                "Transaction Queue: DB interact failed — keeping observation for replay, source_record_id='{}': {}",
                source_record_id,
                e
            );
            return false;
        }
    };
    if outcome.is_none() {
        // Stored without a decision: it went to review (or was a duplicate).
        let _ = crate::ipc::events::emit_event(
            app,
            crate::ipc::events::AppEvent::UnassignedChanged,
            serde_json::json!({ "source_record_id": source_record_id }),
        );
    }
    if let Some((decision, obs_id)) = outcome {
        if let crate::reconciliation::audit::DecisionType::AmbiguousPending(cluster_id) = &decision
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
                        let merchant = notify_merchant.unwrap_or_else(|| "a merchant".to_string());
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
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};


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

    /// The tracker is seeded with the file count, but duplicates and
    /// password-protected files never reach a parser. They still have to move the
    /// bar, or a batch containing one never reaches its total.
    #[test]
    fn skipped_files_still_carry_the_batch_to_its_total() {
        let tracker = BatchProgressTracker::new(3);

        let (parsed, total, eta) = tracker.record_skipped();
        assert_eq!((parsed, total), (1, 3));
        assert_eq!(
            eta, 0,
            "nothing has been timed yet, so there is no estimate"
        );

        let (parsed, _, eta) = tracker.record_completion(std::time::Duration::from_secs(2));
        assert_eq!(parsed, 2);
        assert_eq!(
            eta, 2,
            "the one real sample must set the ETA -- a skipped file is not a 0ms parse"
        );

        let (parsed, total, eta) = tracker.record_skipped();
        assert_eq!((parsed, total), (3, 3), "the batch must reach its total");
        assert_eq!(eta, 0);
    }

    #[test]
    fn tracker_with_no_files_reports_no_estimate_rather_than_dividing_by_zero() {
        let tracker = BatchProgressTracker::new(0);
        assert_eq!(tracker.record_skipped(), (0, 0, 0));
        assert_eq!(
            tracker.record_completion(std::time::Duration::from_secs(9)),
            (0, 0, 0)
        );
    }
}

#[cfg(test)]
mod layer6_tests {
    use super::*;
    use crate::db::init_db;
    use std::fs;

    /// Seeds the observation + open unassigned row a Layer 6 job is applied against.
    async fn seed_pending_observation(
        conn: &deadpool_sqlite::Object,
        observation_id: &str,
        unassigned_id: &str,
        msg_id: &str,
    ) {
        let row = crate::db::transaction_observations::TransactionObservationsRow {
            id: observation_id.to_string(),
            source_pipeline: Some("gmail_transaction".to_string()),
            source_record_id: Some(msg_id.to_string()),
            source_message_id: Some(msg_id.to_string()),
            fingerprint: Some(format!("pending_{}", observation_id)),
            extraction_method: Some("pending_llm_enrichment".to_string()),
            created_at: Some(chrono::Utc::now().naive_utc()),
            updated_at: Some(chrono::Utc::now().naive_utc()),
            ..Default::default()
        };
        conn.interact(move |c| crate::db::transaction_observations::insert_observation(c, &row))
            .await
            .unwrap()
            .unwrap();

        let unassigned = crate::db::unassigned_transactions::UnassignedTransactionRow {
            id: unassigned_id.to_string(),
            observation_id: observation_id.to_string(),
            reason: "pending_llm_enrichment".to_string(),
            status: "open".to_string(),
            created_at: None,
        };
        conn.interact(move |c| crate::db::unassigned_transactions::insert(c, &unassigned))
            .await
            .unwrap()
            .unwrap();
    }

    async fn reason_of(conn: &deadpool_sqlite::Object, unassigned_id: &str) -> String {
        let id = unassigned_id.to_string();
        conn.interact(move |c| {
            c.query_row("SELECT reason FROM unassigned_transactions WHERE id = ?1", [id], |r| r.get(0))
        })
        .await
        .unwrap()
        .unwrap()
    }

    async fn visible_ids(conn: &deadpool_sqlite::Object) -> Vec<String> {
        conn.interact(|c| crate::db::unassigned_transactions::select_open(c))
            .await
            .unwrap()
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect()
    }

    /// Owner decision 2026-11-25: an entry waiting on its LLM job is not in
    /// the review queue; it appears only with the model's verdict, here
    /// "no direction in this email". Booking would default the direction to
    /// debit, so it must not book.
    #[tokio::test]
    async fn an_llm_verdict_of_missing_direction_reviews_after_the_job_and_never_books() {
        let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let pool = init_db(dir.join("test.db")).await.unwrap();
        let conn = pool.get().await.unwrap();
        seed_pending_observation(&conn, "obs-dir", "u-dir", "msg-dir").await;
        conn.interact(|c| {
            crate::db::layer6_jobs::insert(
                c,
                &crate::db::layer6_jobs::PendingLayer6Job {
                    id: "u-dir".into(),
                    observation_id: "obs-dir".into(),
                    bank_name: "HDFC Bank".into(),
                    body_text: "Rs 500 at Amazon on card XX1234".into(),
                    internal_date_seconds: None,
                },
            )
        })
        .await
        .unwrap()
        .unwrap();
        assert!(!visible_ids(&conn).await.contains(&"u-dir".to_string()), "hidden while the job is live");

        let enriched = crate::extraction::ladder::ExtractionResult {
            amount_minor: Some(50000),
            currency: Some("INR".into()),
            direction: None,
            merchant_raw: Some("Amazon".into()),
            event_time: Some(1_704_412_200),
            extraction_method: "llm_layer6".into(),
            confidence_score: Some(0.95),
            instrument_type: Some("credit_card".into()),
            issuer_name: Some("HDFC Bank".into()),
            masked_identifier: Some("1234".into()),
            ..Default::default()
        };
        let booked = apply_layer6_success(&pool, "obs-dir", "u-dir", enriched, None).await.unwrap();
        assert!(!booked, "a missing direction must never be booked as a default debit");
        assert_eq!(reason_of(&conn, "u-dir").await, "gate3_failed:missing_direction");

        delete_persisted_layer6_job(&pool, "u-dir").await;
        assert!(visible_ids(&conn).await.contains(&"u-dir".to_string()), "visible once the verdict is in");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Owner decision 2026-11-25: an answer naming a value the email does not
    /// contain (after the correction retry) goes to review under its own reason.
    #[tokio::test]
    async fn a_rejected_llm_answer_is_reviewed_as_value_not_in_email() {
        let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let pool = init_db(dir.join("test.db")).await.unwrap();
        let conn = pool.get().await.unwrap();
        seed_pending_observation(&conn, "obs-rej", "u-rej", "msg-rej").await;
        assert!(mark_llm_rejected(&pool, "u-rej").await);
        assert_eq!(reason_of(&conn, "u-rej").await, LLM_VALUE_NOT_IN_EMAIL);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Jobs left dead by the old 3-attempt cap no longer hide their entry.
    #[tokio::test]
    async fn an_entry_behind_a_dead_job_is_visible() {
        let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let pool = init_db(dir.join("test.db")).await.unwrap();
        let conn = pool.get().await.unwrap();
        seed_pending_observation(&conn, "obs-dead", "u-dead", "msg-dead").await;
        conn.interact(|c| {
            c.execute(
                "INSERT INTO layer6_pending_jobs (id, observation_id, bank_name, body_text, dead_at)
                 VALUES ('u-dead', 'obs-dead', 'HDFC Bank', 'x', datetime('now'))",
                [],
            )
        })
        .await
        .unwrap()
        .unwrap();
        assert!(visible_ids(&conn).await.contains(&"u-dead".to_string()));
        let _ = fs::remove_dir_all(&dir);
    }

    /// A `missing_counterparty` entry reaches Layer 6 with an amount, a currency
    /// and a direction that layers 1-5 already recovered; only the merchant is
    /// missing. A model that answers with the merchant alone must not cost the
    /// entry everything else it had.
    #[tokio::test]
    async fn layer6_fills_the_gaps_without_wiping_what_earlier_layers_extracted() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let pool = init_db(temp_dir.join("test.db"))
            .await
            .expect("DB init failed");
        let conn = pool.get().await.unwrap();

        let observation_id = uuid::Uuid::new_v4().to_string();
        let unassigned_id = uuid::Uuid::new_v4().to_string();

        let row = crate::db::transaction_observations::TransactionObservationsRow {
            id: observation_id.clone(),
            source_pipeline: Some("gmail_transaction".to_string()),
            source_record_id: Some("msg_partial".to_string()),
            source_message_id: Some("msg_partial".to_string()),
            amount_minor: Some(12_345), // ₹123.45
            currency: Some("INR".to_string()),
            direction: Some("debit".to_string()),
            reference_id: Some("REF-FROM-REGEX".to_string()),
            fingerprint: Some(format!("pending_{}", observation_id)),
            extraction_method: Some("regex_layer2".to_string()),
            created_at: Some(chrono::Utc::now().naive_utc()),
            updated_at: Some(chrono::Utc::now().naive_utc()),
            ..Default::default()
        };
        conn.interact(move |c| crate::db::transaction_observations::insert_observation(c, &row))
            .await
            .unwrap()
            .unwrap();
        conn.interact({
            let unassigned = crate::db::unassigned_transactions::UnassignedTransactionRow {
                id: unassigned_id.clone(),
                observation_id: observation_id.clone(),
                reason: "gate3_failed:missing_counterparty".to_string(),
                status: "open".to_string(),
                created_at: None,
            };
            move |c| crate::db::unassigned_transactions::insert(c, &unassigned)
        })
        .await
        .unwrap()
        .unwrap();

        let enriched = crate::extraction::ladder::ExtractionResult {
            merchant_raw: Some("Blue Tokai".to_string()),
            channel: Some("upi".to_string()),
            extraction_method: "llm_layer6".to_string(),
            confidence_score: Some(0.9),
            ..Default::default()
        };

        apply_layer6_success(&pool, &observation_id, &unassigned_id, enriched, None)
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
        assert_eq!(
            updated.merchant_raw.as_deref(),
            Some("Blue Tokai"),
            "the gap the model was asked to fill must be filled"
        );
        assert_eq!(
            updated.amount_minor,
            Some(12_345),
            "an amount the model did not mention must survive the enrichment"
        );
        assert_eq!(updated.currency.as_deref(), Some("INR"));
        assert_eq!(updated.direction.as_deref(), Some("debit"));
        assert_eq!(updated.reference_id.as_deref(), Some("REF-FROM-REGEX"));
        assert_eq!(
            updated.channel.as_deref(),
            Some("upi"),
            "the detected channel must be persisted, not merely consulted"
        );

        let reason: String = conn
            .interact({
                let id = unassigned_id.clone();
                move |c| {
                    c.query_row(
                        "SELECT reason FROM unassigned_transactions WHERE id = ?1",
                        rusqlite::params![id],
                        |row| row.get(0),
                    )
                }
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            reason, "gate3_failed:missing_instrument",
            "with amount and merchant both present, the instrument is what is still missing"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

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
            rfc822_message_id: None,
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

    /// Extraction runs for minutes, so the user can dismiss an entry while its
    /// Layer 6 job is still in flight. Applying the result anyway overrules that
    /// decision, and reporting the clash as an error left the durable job row
    /// undeleted -- so the whole LLM run replayed at every launch, forever.
    #[tokio::test]
    async fn layer6_result_for_a_dismissed_entry_converges_without_overruling_the_user() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let pool = init_db(temp_dir.join("test.db"))
            .await
            .expect("DB init failed");
        let conn = pool.get().await.unwrap();

        let observation_id = uuid::Uuid::new_v4().to_string();
        let unassigned_id = uuid::Uuid::new_v4().to_string();
        seed_pending_observation(&conn, &observation_id, &unassigned_id, "msg_dismissed").await;

        conn.interact({
            let id = unassigned_id.clone();
            move |c| crate::db::unassigned_transactions::update_status(c, &id, "ignored")
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
            confidence_score: Some(0.95),
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
        .expect("a dismissed entry leaves the job nothing to do -- that is success, not a retry");

        let status: String = conn
            .interact({
                let id = unassigned_id.clone();
                move |c| {
                    c.query_row(
                        "SELECT status FROM unassigned_transactions WHERE id = ?1",
                        rusqlite::params![id],
                        |row| row.get(0),
                    )
                }
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status, "ignored", "the user's dismissal must stand");

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
            updated.canonical_transaction_id.is_none(),
            "a dismissed entry must not be booked as a transaction behind the user's back"
        );
        assert!(
            updated.amount_minor.is_none(),
            "the observation must be left as the user last saw it"
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
            rfc822_message_id: None,
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
    async fn replayed_layer6_job_does_not_promote_the_same_observation_twice() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let pool = init_db(temp_dir.join("test.db"))
            .await
            .expect("DB init failed");
        let conn = pool.get().await.unwrap();

        let observation_id = uuid::Uuid::new_v4().to_string();
        let unassigned_id = uuid::Uuid::new_v4().to_string();
        seed_pending_observation(&conn, &observation_id, &unassigned_id, "msg_replay").await;

        let enriched = |merchant: &str, amount: i64| crate::extraction::ladder::ExtractionResult {
            amount_minor: Some(amount),
            currency: Some("INR".to_string()),
            direction: Some("debit".to_string()),
            merchant_raw: Some(merchant.to_string()),
            event_time: Some(1704412200),
            extraction_method: "llm_layer6".to_string(),
            confidence_score: Some(0.9),
            instrument_type: Some("credit_card".to_string()),
            issuer_name: Some("HDFC Bank".to_string()),
            masked_identifier: Some("4321".to_string()),
            ..Default::default()
        };

        apply_layer6_success(
            &pool,
            &observation_id,
            &unassigned_id,
            enriched("Test Merchant", 50000),
            Some(1_780_000_000),
        )
        .await
        .unwrap();

        let first = conn
            .interact({
                let id = observation_id.clone();
                move |c| crate::db::transaction_observations::get_observation(c, &id)
            })
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let canonical_id = first
            .canonical_transaction_id
            .clone()
            .expect("first apply must promote");

        // A crash between the commit and the durable row's deletion replays the job,
        // and a re-run of the model need not reproduce its earlier answer. The
        // committed result is the one the canonical transaction was built from, so
        // it has to win.
        apply_layer6_success(
            &pool,
            &observation_id,
            &unassigned_id,
            enriched("Replayed Merchant", 99999),
            Some(1_780_000_000),
        )
        .await
        .unwrap();

        let after = conn
            .interact({
                let id = observation_id.clone();
                move |c| crate::db::transaction_observations::get_observation(c, &id)
            })
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            after.canonical_transaction_id.as_deref(),
            Some(canonical_id.as_str()),
            "a replayed job must not re-reconcile into a second canonical transaction"
        );
        assert_eq!(
            (after.amount_minor, after.merchant_raw.as_deref()),
            (Some(50000), Some("Test Merchant")),
            "the committed result must survive the replay, not be overwritten by it"
        );

        let count: i64 = conn
            .interact(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM transactions WHERE is_deleted = 0",
                    [],
                    |r| r.get(0),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(count, 1, "replay must not mint a duplicate transaction");

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
            rfc822_message_id: None,
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
        // The label matches the Layer 1-5 path, but lives in
        // merchant_normalized; merchant_raw keeps the model's span (audit A-6).
        assert_eq!(
            updated.merchant_normalized.as_deref(),
            Some("Internal Transfer (A/c 1527)"),
            "the self-transfer label must match the Layer 1-5 path"
        );
        assert_eq!(updated.merchant_raw.as_deref(), Some("account 1527"));
        let display: String = conn
            .interact({
                let id = updated.canonical_transaction_id.clone().unwrap();
                move |c| {
                    c.query_row("SELECT merchant_display_name FROM transactions WHERE id = ?1", [id], |r| r.get(0))
                }
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(display, "Internal Transfer (A/c 1527)", "the ledger shows the label");

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

        // Without a model replay is skipped (review H8).
        let (replay_tx, mut replay_rx) = mpsc::channel::<Layer6Job>(4);
        replay_pending_layer6_jobs(&pool, &replay_tx, temp_dir.clone()).await;
        assert!(replay_rx.try_recv().is_err(), "no model: nothing may be replayed");

        // A downloaded, verified model enables replay.
        let models = temp_dir.join("models");
        fs::create_dir_all(&models).unwrap();
        fs::write(models.join("gemma4_e4b.gguf"), b"stub").unwrap();
        fs::write(models.join("gemma4_e4b.gguf.verified"), b"").unwrap();
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

#[cfg(test)]
mod queued_statement_recovery_tests {
    use super::*;
    use std::fs;
    use tauri::test::{mock_builder, mock_context};

    /// Review H6: a statement left `queued` by a crash is re-enqueued when its
    /// staged PDF exists, released (row deleted) when it does not, and rows
    /// created after launch are never touched.
    #[tokio::test]
    async fn queued_statements_from_before_launch_are_recovered_or_released() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();
        let app = mock_builder().build(mock_context(tauri::test::noop_assets())).unwrap();

        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            for id in ["with_pdf", "without_pdf", "fresh"] {
                crate::db::statements::insert_queued(c, id, "gmail_email", Some(id), None).unwrap();
            }
            c.execute(
                "UPDATE statements SET created_at = datetime('now', '-1 hour') WHERE id != 'fresh'",
                [],
            )
            .unwrap();
        })
        .await
        .unwrap();
        let pdf = fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/statements/hdfc_plain.pdf"),
        )
        .unwrap();
        crate::statements::pdf_storage::store_pdf(&temp_dir, "with_pdf", &pdf).unwrap();
        conn.interact(|c| crate::db::statements::set_source_filename(c, "with_pdf", "HDFC_Nov.pdf"))
            .await
            .unwrap()
            .unwrap();

        let (tx, mut rx) = mpsc::channel::<StatementJob>(4);
        let launched = chrono::Utc::now().naive_utc() - chrono::Duration::minutes(1);
        recover_queued_statements(app.handle(), &pool, &tx, &temp_dir, launched).await;

        let remaining: Vec<String> = conn
            .interact(|c| {
                let mut s = c.prepare("SELECT id FROM statements ORDER BY id").unwrap();
                s.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect()
            })
            .await
            .unwrap();

        // Password resolution needs the PDF sidecar. Where it can read the PDF
        // the statement is re-enqueued; where it cannot (the PDF looks locked),
        // it is handed to the awaiting-password flow like a first ingest.
        if crate::statements::password::is_pdf_unencrypted(&pdf).await {
            let job = rx.try_recv().expect("statement with a staged PDF must be re-enqueued");
            assert_eq!(job.stmt_id, "with_pdf");
            assert_eq!(job.origin, "email_scan");
            assert_eq!(job.filename, "HDFC_Nov.pdf", "original filename is kept");
            assert_eq!(remaining, vec!["fresh".to_string(), "with_pdf".to_string()]);
        } else {
            assert_eq!(remaining, vec!["fresh".to_string()]);
            assert!(
                crate::statements::pdf_storage::read_pdf(&temp_dir, "with_pdf").unwrap().is_some(),
                "the staged PDF must stay for the awaiting-password entry"
            );
        }
        assert!(rx.try_recv().is_err(), "at most one job expected");
        // "without_pdf" is gone in both cases; "fresh" (after launch) is untouched.
        assert!(!remaining.contains(&"without_pdf".to_string()));
        let _ = fs::remove_dir_all(&temp_dir);
    }
}

#[cfg(test)]
mod ingest_failure_tests {
    use super::*;
    use tauri::test::{mock_builder, mock_context};

    fn job() -> TransactionJob {
        TransactionJob {
            obs: crate::extraction::ladder::ExtractionResult {
                amount_minor: Some(50000),
                direction: Some("debit".into()),
                merchant_raw: Some("CAFE".into()),
                extraction_method: "layer1".into(),
                ..Default::default()
            },
            source_pipeline: "gmail_transaction".into(),
            source_record_id: "msg_fail".into(),
            connected_account_id: "acc".into(),
            raw_body: None,
            email_meta: None,
        }
    }

    /// Audit A-1: an `ingest_observation` error must not count as success, so
    /// the persisted job survives; after MAX_ATTEMPTS it is listed for retry.
    #[tokio::test]
    async fn failed_ingest_keeps_the_job_and_dead_jobs_are_listed_for_retry() {
        let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = crate::db::init_db(dir.join("t.db")).await.unwrap();
        let app = mock_builder().build(mock_context(tauri::test::noop_assets())).unwrap();
        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            c.execute_batch(
                "CREATE TRIGGER force_fail BEFORE INSERT ON transaction_observations
                 BEGIN SELECT RAISE(ABORT, 'forced failure'); END;",
            )
        })
        .await
        .unwrap()
        .unwrap();

        assert!(
            !process_transaction_job(job(), &pool, app.handle()).await,
            "a failed ingest must not report success"
        );

        let (tx, _rx) = mpsc::channel::<TransactionJob>(4);
        send_transaction_job(&pool, &tx, job()).await.unwrap();
        let key = transaction_job_key(&job());
        for _ in 0..crate::db::pending_ingest_jobs::MAX_ATTEMPTS {
            assert!(!process_transaction_job(job(), &pool, app.handle()).await);
            record_failed_job_attempt(&pool, key.clone(), "acc".into(), "msg_fail".into()).await;
        }
        let (jobs, failed): (i64, Vec<String>) = conn
            .interact(|c| {
                let jobs = c.query_row("SELECT COUNT(*) FROM pending_ingest_jobs", [], |r| r.get(0))?;
                let failed = crate::db::scan_failed_messages::distinct_msg_ids(c, "acc").unwrap();
                Ok::<_, rusqlite::Error>((jobs, failed))
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(jobs, 1, "the job is kept, not deleted");
        assert_eq!(failed, vec!["msg_fail".to_string()], "a dead job is listed under Retry failed");

        // A retry re-enqueues the job with a fresh attempt count, and once the
        // cause is gone it is ingested.
        send_transaction_job(&pool, &tx, job()).await.unwrap();
        let (attempts, dead): (i64, bool) = conn
            .interact(|c| {
                c.query_row("SELECT attempts, dead_at IS NOT NULL FROM pending_ingest_jobs", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!((attempts, dead), (0, false));
        conn.interact(|c| c.execute_batch("DROP TRIGGER force_fail")).await.unwrap().unwrap();
        assert!(process_transaction_job(job(), &pool, app.handle()).await);
        let stored: i64 = conn
            .interact(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM transaction_observations WHERE source_record_id = 'msg_fail'",
                    [],
                    |r| r.get(0),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Audit B-5: a failed job that is still live is re-queued in the same
    /// session; one marked dead is not.
    #[tokio::test]
    async fn failed_job_is_retried_in_session_unless_dead() {
        let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = crate::db::init_db(dir.join("t.db")).await.unwrap();
        let (tx, mut rx) = mpsc::channel::<TransactionJob>(4);
        send_transaction_job(&pool, &tx, job()).await.unwrap();
        rx.try_recv().unwrap();
        let key = transaction_job_key(&job());

        assert!(!record_failed_job_attempt(&pool, key.clone(), "acc".into(), "msg_fail".into()).await);
        schedule_ingest_retry(pool.clone(), tx.clone(), key.clone(), std::time::Duration::from_millis(10));
        let retried = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("retried in session")
            .unwrap();
        assert_eq!(retried.source_record_id, "msg_fail");

        for _ in 0..crate::db::pending_ingest_jobs::MAX_ATTEMPTS {
            record_failed_job_attempt(&pool, key.clone(), "acc".into(), "msg_fail".into()).await;
        }
        schedule_ingest_retry(pool.clone(), tx.clone(), key, std::time::Duration::from_millis(10));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await.is_err(),
            "a dead job is not retried"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Audit A-2 (owner decision: review): an alert for an archived card is
    /// stored and filed for review, never left as an invisible orphan.
    #[tokio::test]
    async fn alert_for_an_archived_instrument_goes_to_review() {
        let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = crate::db::init_db(dir.join("t.db")).await.unwrap();
        let app = mock_builder().build(mock_context(tauri::test::noop_assets())).unwrap();
        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            let id = crate::db::instruments::get_or_create_instrument(c, "credit_card", "HDFC", "1234", None)?;
            crate::db::instruments::delete_instrument(c, &id)
        })
        .await
        .unwrap()
        .unwrap();

        let mut archived = job();
        archived.obs.instrument_type = Some("credit_card".into());
        archived.obs.issuer_name = Some("HDFC Bank".into());
        archived.obs.masked_identifier = Some("XXXX1234".into());
        assert!(process_transaction_job(archived, &pool, app.handle()).await);

        let (instrument, reason): (Option<String>, String) = conn
            .interact(|c| {
                c.query_row(
                    "SELECT o.instrument_id, u.reason FROM transaction_observations o
                     JOIN unassigned_transactions u ON u.observation_id = o.id
                     WHERE o.source_record_id = 'msg_fail'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(instrument, None);
        assert_eq!(reason, "instrument_archived");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;
    use tauri::test::{mock_builder, mock_context};
    use tauri::Manager;

    /// Audit F-3: the shared alert dispatch persists the job before queueing
    /// it, keyed by the Gmail message, for both polling and scans.
    #[tokio::test]
    async fn transaction_alert_is_persisted_and_queued() {
        let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = crate::db::init_db(dir.join("t.db")).await.unwrap();
        let app = mock_builder().build(mock_context(tauri::test::noop_assets())).unwrap();
        let (transaction_tx, mut transaction_rx) = mpsc::channel(1);
        let (statement_tx, _) = mpsc::channel(1);
        let (mandate_tx, _) = mpsc::channel(1);
        let (layer6_tx, _) = mpsc::channel(1);
        app.manage(QueueHandles { transaction_tx, statement_tx, mandate_tx, layer6_tx });

        dispatch_transaction_alert(
            app.handle(),
            &pool,
            "acc",
            "msg_f3",
            Some("body".into()),
            crate::extraction::ladder::ExtractionResult { amount_minor: Some(100), ..Default::default() },
            crate::ingestion::message_processor::EmailMetadata {
                sender: "alerts@hdfcbank.net".into(),
                recipient: "me@example.com".into(),
                subject: "Alert".into(),
                date: "Mon, 1 Jun 2026".into(),
                snippet: "Rs 100".into(),
                html: None,
                sender_email: None,
                sender_domain: None,
                recipient_email: None,
                recipient_domain: None,
                rfc822_message_id: None,
            },
        )
        .await
        .unwrap();

        let job = transaction_rx.try_recv().expect("queued");
        assert_eq!((job.source_record_id.as_str(), job.connected_account_id.as_str()), ("msg_f3", "acc"));
        let persisted: i64 = pool
            .get()
            .await
            .unwrap()
            .interact(|c| {
                c.query_row("SELECT COUNT(*) FROM pending_ingest_jobs WHERE id = 'transaction:msg_f3'", [], |r| r.get(0))
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(persisted, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod layer6_overflow_tests {
    use super::*;

    fn job(id: &str, dir: &std::path::Path) -> Layer6Job {
        Layer6Job {
            observation_id: format!("obs-{id}"),
            unassigned_id: id.to_string(),
            bank_name: "HDFC Bank".into(),
            body_text: "Rs 100 debited".into(),
            app_dir: dir.to_path_buf(),
            internal_date_seconds: None,
        }
    }

    /// Audit A-7: a full Layer-6 queue never blocks the producer (the scan);
    /// the overflow waits in the table and is refilled once there is room.
    #[tokio::test]
    async fn full_queue_does_not_block_and_overflow_is_refilled() {
        let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = crate::db::init_db(dir.join("t.db")).await.unwrap();
        let (tx, mut rx) = mpsc::channel::<Layer6Job>(1);

        enqueue_layer6_job(&pool, &tx, job("a7-first", &dir)).await;
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            enqueue_layer6_job(&pool, &tx, job("a7-overflow", &dir)),
        )
        .await
        .expect("enqueue must not wait for a queue slot");

        assert_eq!(rx.recv().await.unwrap().unassigned_id, "a7-first");
        assert!(rx.try_recv().is_err(), "the overflow job is not in the queue yet");
        refill_layer6_queue(&pool, &tx, &dir).await;
        assert_eq!(rx.try_recv().unwrap().unassigned_id, "a7-overflow");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod layer6_attempt_policy_tests {
    use super::*;

    /// Audit A-8: with no model to run, a Layer-6 job waits without using an
    /// attempt or giving up on the review entry; audit N-A3: it is re-checked
    /// this session rather than only at the next launch.
    #[tokio::test]
    async fn missing_model_does_not_use_up_attempts() {
        let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = crate::db::init_db(dir.join("t.db")).await.unwrap();
        pool.get()
            .await
            .unwrap()
            .interact(|c| {
                c.execute_batch(
                    "PRAGMA foreign_keys = OFF;
                     INSERT INTO unassigned_transactions (id, observation_id, reason, status)
                     VALUES ('u1', 'o1', 'pending_llm_enrichment', 'open');",
                )?;
                crate::db::layer6_jobs::insert(
                    c,
                    &crate::db::layer6_jobs::PendingLayer6Job {
                        id: "u1".into(),
                        observation_id: "o1".into(),
                        bank_name: "HDFC Bank".into(),
                        body_text: "Rs 500 spent".into(),
                        internal_date_seconds: None,
                    },
                )
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))
            })
            .await
            .unwrap()
            .unwrap();
        let (tx, _rx) = mpsc::channel(1);
        let learning = crate::learning::LearningHandle { tx };
        let job = Layer6Job {
            observation_id: "o1".into(),
            unassigned_id: "u1".into(),
            bank_name: "HDFC Bank".into(),
            body_text: "Rs 500 spent".into(),
            app_dir: dir.clone(), // no model downloaded here
            internal_date_seconds: None,
        };

        let done = process_layer6_job(job, &pool, &learning).await;
        assert!(!done.completed);
        assert_eq!(done.retry_after, Some(LAYER6_NO_MODEL_RECHECK));

        let (attempts, dead, reason): (i64, bool, String) = pool
            .get()
            .await
            .unwrap()
            .interact(|c| {
                c.query_row(
                    "SELECT j.attempts, j.dead_at IS NOT NULL, u.reason FROM layer6_pending_jobs j
                     JOIN unassigned_transactions u ON u.id = j.id WHERE j.id = 'u1'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!((attempts, dead), (0, false), "no attempt is used without a model");
        assert_eq!(reason, "pending_llm_enrichment");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Owner decision 2026-11-25: retries wait 1 minute, then 5, then 15
    /// for as long as it takes; no attempt is final.
    #[test]
    fn failed_attempts_retry_after_one_then_five_then_fifteen_minutes() {
        assert_eq!(layer6_retry_delay(1), std::time::Duration::from_secs(60));
        assert_eq!(layer6_retry_delay(2), std::time::Duration::from_secs(300));
        assert_eq!(layer6_retry_delay(3), std::time::Duration::from_secs(900));
        assert_eq!(layer6_retry_delay(50), std::time::Duration::from_secs(900));
    }
}

#[cfg(test)]
mod pending_ingest_job_tests {
    use super::*;

    /// Audit D-3: replay pages through a large backlog, delivering every job
    /// exactly once.
    #[tokio::test]
    async fn replay_pages_through_a_large_backlog() {
        let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = crate::db::init_db(dir.join("t.db")).await.unwrap();
        let (tx, _hold) = mpsc::channel::<TransactionJob>(300);
        for i in 0..250 {
            let job = TransactionJob {
                obs: Default::default(),
                source_pipeline: "gmail_transaction".into(),
                source_record_id: format!("m{i:03}"),
                connected_account_id: "acc".into(),
                raw_body: None,
                email_meta: None,
            };
            send_transaction_job(&pool, &tx, job).await.unwrap();
        }
        drop(_hold);
        let (tx2, mut rx2) = mpsc::channel::<TransactionJob>(300);
        let (mtx, _mrx) = mpsc::channel::<MandateJob>(4);
        let later = chrono::Utc::now().naive_utc() + chrono::Duration::minutes(1);
        replay_pending_ingest_jobs(&pool, &tx2, &mtx, later).await;
        let mut seen = std::collections::HashSet::new();
        while let Ok(job) = rx2.try_recv() {
            assert!(seen.insert(job.source_record_id), "no job is replayed twice");
        }
        assert_eq!(seen.len(), 250);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Review H4: a queued transaction job is persisted before it is sent and
    /// replayed after a restart until a worker finishes it.
    #[tokio::test]
    async fn queued_transaction_job_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = crate::db::init_db(dir.join("t.db")).await.unwrap();

        // The app "quits" with the job still in the channel.
        let (tx, rx) = mpsc::channel::<TransactionJob>(4);
        let job = TransactionJob {
            obs: crate::extraction::ladder::ExtractionResult {
                amount_minor: Some(12345),
                merchant_raw: Some("CAFE".into()),
                extraction_method: "layer1".into(),
                ..Default::default()
            },
            source_pipeline: "gmail_transaction".into(),
            source_record_id: "msg_1".into(),
            connected_account_id: "acc".into(),
            raw_body: None,
            email_meta: None,
        };
        send_transaction_job(&pool, &tx, job).await.unwrap();
        drop(rx);

        let (tx2, mut rx2) = mpsc::channel::<TransactionJob>(4);
        let (mtx, _mrx) = mpsc::channel::<MandateJob>(4);
        let later = chrono::Utc::now().naive_utc() + chrono::Duration::minutes(1);
        replay_pending_ingest_jobs(&pool, &tx2, &mtx, later).await;
        let replayed = rx2.try_recv().expect("job must be replayed");
        assert_eq!(replayed.source_record_id, "msg_1");
        assert_eq!(replayed.obs.amount_minor, Some(12345));

        // A job that keeps failing is marked dead after MAX_ATTEMPTS and no
        // longer replayed, but kept for diagnosis.
        for _ in 0..crate::db::pending_ingest_jobs::MAX_ATTEMPTS {
            record_failed_job_attempt(
                &pool,
                transaction_job_key(&replayed),
                replayed.connected_account_id.clone(),
                replayed.source_record_id.clone(),
            )
            .await;
        }
        let (txd, mut rxd) = mpsc::channel::<TransactionJob>(4);
        replay_pending_ingest_jobs(&pool, &txd, &mtx, later).await;
        assert!(rxd.try_recv().is_err(), "dead jobs are not replayed");
        let kept: i64 = pool
            .get()
            .await
            .unwrap()
            .interact(|c| c.query_row("SELECT COUNT(*) FROM pending_ingest_jobs", [], |r| r.get(0)))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(kept, 1, "dead jobs are kept for diagnosis");

        // Once a worker finishes it, it is gone.
        delete_persisted_job(&pool, transaction_job_key(&replayed)).await;
        let (tx3, mut rx3) = mpsc::channel::<TransactionJob>(4);
        replay_pending_ingest_jobs(&pool, &tx3, &mtx, later).await;
        assert!(rx3.try_recv().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod mandate_ledger_migration_tests {
    /// Review M5: the migration hides mandate-only ₹0 transactions and their
    /// observations, and leaves a transaction a real observation points to.
    #[test]
    fn migration_soft_deletes_only_mandate_only_zero_transactions() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute_batch(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO transactions (id, amount_minor) VALUES ('t_mandate', 0), ('t_shared', 0), ('t_real', 500);
             INSERT INTO transaction_observations (id, source_pipeline, source_record_id, canonical_transaction_id, extraction_method, fingerprint)
             VALUES ('o1', 'gmail_transaction', 'm1', 't_mandate', 'mandate_event', 'f1'),
                    ('o2', 'gmail_transaction', 'm2', 't_shared', 'mandate_event', 'f2'),
                    ('o3', 'gmail_transaction', 'm3', 't_shared', 'layer1', 'f3'),
                    ('o4', 'gmail_transaction', 'm4', 't_real', 'layer1', 'f4');",
        )
        .unwrap();
        conn.execute_batch(include_str!(
            "../../migrations/20260101000064_hide_mandate_zero_transactions.sql"
        ))
        .unwrap();
        let deleted = |table: &str, id: &str| -> bool {
            conn.query_row(
                &format!("SELECT is_deleted FROM {table} WHERE id = ?1"),
                [id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert!(deleted("transactions", "t_mandate"));
        assert!(!deleted("transactions", "t_shared"));
        assert!(!deleted("transactions", "t_real"));
        assert!(deleted("transaction_observations", "o1"));
        assert!(!deleted("transaction_observations", "o3"));
    }
}
