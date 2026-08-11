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
    // Both the pool handout and the insert itself can fail, and the insert's own
    // error sits inside the interact result -- checking only the outer one reports
    // a job as durable when nothing was written.
    let persisted = match pool.get().await {
        Ok(conn) => conn
            .interact(move |c| crate::db::layer6_jobs::insert(c, &pending))
            .await
            .map_err(|e| anyhow::anyhow!("Interact error: {}", e))
            .and_then(|inner| inner),
        Err(e) => Err(anyhow::anyhow!("DB pool error: {}", e)),
    };
    if let Err(e) = persisted {
        // Still queued: processing it in this session beats dropping it outright,
        // but it is no longer replayable if the app exits first.
        tracing::error!(
            "Failed to persist Layer 6 job for unassigned_id='{}' — queueing without durability: {:#}",
            job.unassigned_id,
            e
        );
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

/// Resolves a queue name from an IPC caller to the flag that pauses it.
///
/// The name arrives from the frontend, so it is matched against the known queues
/// rather than used directly. Validation and dispatch are the same lookup so the
/// two cannot drift apart into an unreachable arm that is suddenly reachable.
fn queue_pause_flag(
    queue: &str,
) -> Result<&'static std::sync::atomic::AtomicBool, crate::error::AppError> {
    match queue {
        "transaction_queue" => Ok(&TRANSACTION_QUEUE_PAUSED),
        "statement_queue" => Ok(&STATEMENT_QUEUE_PAUSED),
        _ => Err(crate::error::AppError::Validation(format!(
            "invalid queue '{}': must be 'transaction_queue' or 'statement_queue'",
            queue
        ))),
    }
}

#[tauri::command]
/// Pauses a queue, used for debugging and controlled shutdown.
pub async fn pipeline_pause(queue: String) -> Result<serde_json::Value, crate::error::AppError> {
    queue_pause_flag(&queue)?.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(serde_json::json!({ "status": "paused", "queue": queue }))
}

#[tauri::command]
/// Resumes a paused queue.
pub async fn pipeline_resume(queue: String) -> Result<serde_json::Value, crate::error::AppError> {
    queue_pause_flag(&queue)?.store(false, std::sync::atomic::Ordering::Relaxed);
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

/// Spawns the Layer 6 worker pool.
///
/// The pool is a fixed size because the sidecar already caps real inference
/// concurrency with its own semaphore, sized from the slot count calibrated once
/// the server is up. Sizing the pool from `current_parallel_slots()` here read
/// that counter before anything had set it -- it still held its default of 1 --
/// so Layer 6 ran single-file for the whole session, and a slot count the user
/// raised later could not reach workers that were already spawned.
// ponytail: fixed ceiling of 6 -- raise it if a machine can genuinely feed more.
fn spawn_layer6_workers(
    rx: mpsc::Receiver<Layer6Job>,
    pool: Pool,
    learning: crate::learning::LearningHandle,
) {
    const LAYER6_WORKER_COUNT: usize = 6;
    let rx = Arc::new(Mutex::new(rx));
    for _ in 0..LAYER6_WORKER_COUNT {
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

/// Processes one LLM extraction job.
///
/// The durable row survives until the outcome is committed: extraction runs for a
/// long time, and deleting up front means a quit or crash mid-run loses the work
/// the persistence exists to protect.
async fn process_layer6_job(
    job: Layer6Job,
    pool: &Pool,
    learning: &crate::learning::LearningHandle,
) {
    use crate::extraction::llm::Layer6Outcome;

    let layer = crate::extraction::ladder::Layer6LlmLayer {
        app_dir: Some(job.app_dir.clone()),
        fallback_event_time: job.internal_date_seconds,
    };
    let result = layer.run(pool, &job.bank_name, &job.body_text).await;
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

            match apply_layer6_success(
                pool,
                &job.observation_id,
                &job.unassigned_id,
                enriched,
                job.internal_date_seconds,
            )
            .await
            {
                Ok(()) => true,
                Err(e) => {
                    tracing::error!(
                        "Layer 6 background worker: failed to apply success for observation_id='{}': {}",
                        job.observation_id, e
                    );
                    false
                }
            }
        }
        Layer6Outcome::Rejected => {
            let mark_result: anyhow::Result<()> = async {
                let unassigned_id = job.unassigned_id.clone();
                let conn = pool.get().await?;
                conn.interact(move |c| {
                    crate::db::unassigned_transactions::settle_if_open(
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
            match mark_result {
                Ok(()) => true,
                Err(e) => {
                    tracing::error!(
                        "Layer 6 background worker: failed to mark unassigned_id='{}' as no_transaction_found: {}",
                        job.unassigned_id, e
                    );
                    false
                }
            }
        }
        Layer6Outcome::TimedOut | Layer6Outcome::Failed => {
            tracing::info!(
                "Layer 6 background worker: no extraction for observation_id='{}' — leaving as unassigned",
                job.observation_id
            );
            false
        }
    };

    if completed {
        delete_persisted_layer6_job(pool, &job.unassigned_id).await;
    } else {
        // A timeout, a dead sidecar or a failed write is exactly what the durable
        // queue is for: keep the row so the next launch retries it.
        tracing::warn!(
            "Layer 6 job for unassigned_id='{}' did not complete — kept for replay at next launch",
            job.unassigned_id
        );
    }
}

/// Applies a successful LLM extraction.
///
/// The caller clears the persisted job only once this has committed, so a crash
/// between the two replays the job rather than losing it -- which is why applying
/// an already-promoted observation is a no-op rather than a second promotion.
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
        // Extraction takes minutes, so the user may have resolved or dismissed
        // the entry while it ran. Booking a transaction for an entry they
        // dismissed overrules them with money, so the job stops here -- and
        // stops *successfully*, since there is genuinely nothing left to do.
        if !crate::db::unassigned_transactions::is_open(c, &unassigned_id)? {
            tracing::info!(
                "Layer 6 result for observation '{}' arrived after the entry left the queue — dropping it",
                observation_id
            );
            return Ok(());
        }

        let mut row = crate::db::transaction_observations::get_observation(c, &observation_id)?
            .ok_or_else(|| anyhow::anyhow!("observation {} not found", observation_id))?;

        // A job replayed after a crash between the commit and the row's deletion
        // arrives here with the observation already promoted. Reconciling it a
        // second time would mint a duplicate transaction.
        if row.canonical_transaction_id.is_some() {
            tracing::info!(
                "Layer 6 result for observation '{}' was already applied — resolving the replayed job",
                observation_id
            );
            crate::db::unassigned_transactions::settle_if_open(c, &unassigned_id, "resolved")?;
            return Ok(());
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
                row.merchant_raw = Some(format!("Internal Transfer (A/c {dest_account})"));
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
            crate::db::unassigned_transactions::settle_if_open(c, &unassigned_id, "resolved")?;
        } else {
            // Same order of blame as `gate3_failure_reason`, which is what wrote
            // the reason in the first place. Without the amount arm an entry the
            // LLM left with no amount was filed as "low_confidence", sending the
            // user looking for a confidence problem that was not the reason.
            let reason = if row.amount_minor.is_none() {
                "gate3_failed:missing_amount"
            } else if row.merchant_raw.is_none() {
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

    let conn = match pool.get().await {
        Ok(conn) => Some(conn),
        Err(e) => {
            // The ₹0 transaction job below is still worth emitting; only the
            // mandate bookkeeping is lost, and silence would hide that.
            tracing::error!(
                "Mandate Queue: DB pool unavailable for source_record_id='{}' — mandate not recorded: {}",
                job.source_record_id,
                e
            );
            None
        }
    };
    if let Some(conn) = conn {
        let outcome = conn
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
        if let Err(e) = outcome {
            tracing::error!(
                "Mandate Queue: DB interact failed for source_record_id='{}' — mandate not recorded: {}",
                job.source_record_id,
                e
            );
        }
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
                let job = { rx.lock().await.recv().await };
                match job {
                    Some(job) => {
                        // Checked after the receive, not before: a worker already
                        // parked in recv() when the pause arrives would otherwise
                        // process one more job while the queue reports "paused".
                        wait_while_transaction_queue_paused().await;
                        process_transaction_job(job, &pool, &app).await
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
                "Transaction Queue: DB pool unavailable — dropping observation for source_record_id='{}': {}",
                source_record_id,
                e
            );
            return;
        }
    };

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
                    row.fingerprint = Some(crate::extraction::fingerprint::compute_fingerprint(
                        instrument_id,
                        direction,
                        amount_minor,
                        &event_bucket,
                        &connected_account_id,
                    ));
                }

                use crate::db::transaction_observations::InsertObservationOutcome;
                match crate::db::transaction_observations::insert_observation_idempotent(c, &row) {
                    Err(e) => {
                        tracing::warn!("Observation insert failed: {}", e);
                        None
                    }
                    Ok(InsertObservationOutcome::DuplicateSkipped) => None,
                    Ok(InsertObservationOutcome::Inserted) => {
                        let incoming_obs = crate::reconciliation::engine::IncomingObservation {
                            id: row.id.clone(),
                            instrument_id: row
                                .instrument_id
                                .clone()
                                .unwrap_or_else(|| "unknown".to_string()),
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

    if let Err(e) = &outcome {
        tracing::error!(
                "Transaction Queue: DB interact failed — dropping observation for source_record_id='{}': {}",
                source_record_id,
                e
            );
    }
    if let Ok(Some((decision, obs_id))) = outcome {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_queue_pause_flag_rejects_unknown_queue() {
        assert!(std::ptr::eq(
            queue_pause_flag("transaction_queue").unwrap(),
            &TRANSACTION_QUEUE_PAUSED
        ));
        assert!(std::ptr::eq(
            queue_pause_flag("statement_queue").unwrap(),
            &STATEMENT_QUEUE_PAUSED
        ));
        assert!(queue_pause_flag("not_a_real_queue").is_err());
        assert!(queue_pause_flag("").is_err());
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
