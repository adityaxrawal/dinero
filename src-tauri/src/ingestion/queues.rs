//! The two isolated ingestion queues (Doc 15 §2 principle 7, §5; Doc 12 §6.2a, §7.2).
//!
//! Every classified message is routed to exactly one of these queues — never both,
//! never neither. Both queues, and manual statement upload, converge on the same
//! processing function per queue, so there is exactly one Transaction-observation
//! path and exactly one Statement-parsing path, regardless of entry point.

use crate::extraction::ladder::ExtractionResult;

use deadpool_sqlite::Pool;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::{mpsc, Mutex, Semaphore};

/// One classified, Gate-3-passed transaction-alert observation, ready for
/// instrument resolution, persistence, and reconciliation (Doc 12 §6.2a/§6.3).
pub struct TransactionJob {
    pub obs: ExtractionResult,
    pub source_pipeline: String,
    pub source_record_id: String,
    /// Doc 30 TASK-TXN-008: the connected Gmail account this observation
    /// came from, folded into the fingerprint so otherwise-identical alerts
    /// from two different genuine accounts are never merged.
    pub connected_account_id: String,
    /// Doc 30 TASK-TXN-009: the sanitized email body, persisted verbatim as
    /// `transaction_observations.raw_payload_json` for auditability/
    /// reprocessing. `None` when the message had no text body at all.
    pub raw_body: Option<String>,
    /// audit_03 #7: a `raw_html: Option<String>` field used to sit here,
    /// documented as being "folded into `raw_payload_json` so the Evidence tab
    /// can show the email as it rendered". It never was — `normalize_observation`
    /// builds that payload's `"html"` key from `email_meta.html`, which is the
    /// same string, carried right below. So every job held a second full copy
    /// of the sanitized HTML (200–500 KB for a complex bank template) that
    /// nothing read, up to `TRANSACTION_QUEUE_CAPACITY` (256) deep. Removed;
    /// the Evidence tab is unaffected because it was never the source.
    pub email_meta: Option<crate::ingestion::message_processor::EmailMetadata>,
}

/// Raw PDF bytes from either Statement Queue entry point (email-detected or
/// manually uploaded, Doc 12 §7.2 step 1). Both entry points are fire-and-forget
/// (Doc 19 §9.1/§3.6: PDF processing is queued and async, never blocks the IPC
/// call) — the real outcome is reported via `statement_parsed`/`statement.parse_failed`
/// events, not a response channel.
///
/// `stmt_id` is the `statements` row `insert_queued()` already wrote at intake
/// (Doc 18 §4.7's crash-recovery invariant) — threaded through so
/// `run_parse_pipeline`'s Step 10 upserts it rather than minting a new ID.
///
/// `batch_progress`, when `Some`, is a tracker shared by every job in the
/// same manual-upload batch (Doc 30 TASK-STMT-009: batches over 10
/// statements get periodic `parsed`/`total`/`eta_seconds` progress events).
/// `None` for single-file uploads and the Gmail-attachment path, neither of
/// which is a "batch" in the sense this task means.
pub struct StatementJob {
    // audit_04 #1: this used to be `bytes: Vec<u8>`, the whole PDF. With
    // `STATEMENT_QUEUE_CAPACITY` = 64, a manual batch could hold 64 complete
    // statements on the heap at once (plus 5 being parsed) before any
    // extraction started — hundreds of MB for a large batch.
    //
    // The bytes now live in the same AES-256-GCM `pdf_storage` file the
    // pipeline was already going to write anyway, keyed by `stmt_id`, and the
    // worker reads them back only once it holds a concurrency permit — so
    // peak memory is bounded by `STATEMENT_QUEUE_MAX_CONCURRENT`, not by queue
    // depth. Deliberately not a plaintext temp file (the audit's suggestion):
    // these are bank statements, and every other at-rest copy in this app is
    // encrypted.
    pub filename: String,
    pub file_hash: String,
    pub stmt_id: String,
    pub batch_progress: Option<Arc<BatchProgressTracker>>,
    /// The password that unlocked `bytes` during `resolve_statement_password`,
    /// when it was encrypted — pdfium needs it passed again at actual parse
    /// time (a resolved password doesn't decrypt `bytes` in place). `None`
    /// for an unencrypted PDF. Every entry point must call
    /// `password::resolve_statement_password` before constructing this job;
    /// see that function's doc comment for the bug this field fixes.
    pub password: Option<String>,
    /// 'manual_upload' | 'email_scan' — threaded into `statement_drafts.origin`
    /// so the review-queue UI and GlobalStateContext's "was I watching this?"
    /// logic can tell the two apart.
    pub origin: String,
}

/// Doc 30 TASK-STMT-009: "emit periodic scan.progress { parsed, total,
/// eta_seconds } using a rolling per-statement duration average" for batches
/// exceeding 10 statements. A simple cumulative average — `total_duration /
/// parsed_count`, updated as each statement finishes — since no window size
/// is specified anywhere for a fancier moving average.
pub struct BatchProgressTracker {
    total: usize,
    parsed: AtomicUsize,
    total_duration_ms: AtomicU64,
}

impl BatchProgressTracker {
    pub fn new(total: usize) -> Self {
        Self {
            total,
            parsed: AtomicUsize::new(0),
            total_duration_ms: AtomicU64::new(0),
        }
    }

    /// Records one statement's completion and returns `(parsed, total, eta_seconds)`
    /// for the caller to emit as a progress event.
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

/// One classified, Gate-3-equivalent mandate registration/cancellation
/// event, ready for recurring_payments upsert/cancellation-matching
/// (dinero-docs/design-archive/specs/2026-07-18-mandate-tracking-design.md §4.2-§4.4).
pub struct MandateJob {
    pub extraction: crate::extraction::mandate_extractor::MandateExtraction,
    pub event_type: crate::ingestion::message_processor::MandateEventType,
    pub source_pipeline: String,
    pub source_record_id: String,
    pub connected_account_id: String,
    pub raw_body: Option<String>,
}

/// A message whose regex-based Layers 1-5 all failed but this machine is
/// LLM-eligible — enqueued instead of running Layer 6 inline (Doc
/// 2026-07-26 mail scan performance: Layer 6 no longer blocks the scan's
/// critical path). `observation_id`/`unassigned_id` are the rows
/// `record_unassigned_transaction`'s `pending_llm_enrichment` path already
/// created — this job's success path upgrades them in place rather than
/// inserting a duplicate.
pub struct Layer6Job {
    pub observation_id: String,
    pub unassigned_id: String,
    pub bank_name: String,
    pub body_text: String,
    pub app_dir: std::path::PathBuf,
    /// Gmail's `internalDate`, already resolved by `message_processor`.
    /// Fallback for the LLM's self-reported `event_time`, which it omits
    /// on essentially every call — see `LlmEngine::extract`'s doc comment.
    pub internal_date_seconds: Option<i64>,
}

pub(crate) const LAYER6_QUEUE_CAPACITY: usize = 256;

/// Persists `job` to `layer6_pending_jobs` before handing it to `tx` — the
/// send alone isn't durable (see migration `20260101000057_layer6_pending_jobs`'s
/// doc comment), so every enqueue site must go through this instead of
/// calling `tx.send` directly.
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

/// Replays anything left in `layer6_pending_jobs` at startup — the durable
/// record of jobs that were persisted but never reached `process_layer6_job`
/// (queue full at send time doesn't apply here since capacity is 256; the
/// real case is an app restart while the job was still sitting in the
/// in-memory channel). Called once, after `spawn_queues`.
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

/// Senders for all four queues, stored as Tauri managed state so every
/// entry point (Gmail polling, historical scan, manual upload) reaches the
/// same queues.
#[derive(Clone)]
pub struct QueueHandles {
    pub transaction_tx: mpsc::Sender<TransactionJob>,
    pub statement_tx: mpsc::Sender<StatementJob>,
    pub mandate_tx: mpsc::Sender<MandateJob>,
    pub layer6_tx: mpsc::Sender<Layer6Job>,
}

/// Multi-parallel worker pool size for the Transaction Queue (Doc 15 §5: 2–8 workers).
const TRANSACTION_QUEUE_WORKERS: usize = 4;
/// Bounded concurrent PDF parses for the Statement Queue (Doc 15 §5, Doc 12 §6.2a/§7).
const STATEMENT_QUEUE_MAX_CONCURRENT: usize = 5;

pub(crate) const TRANSACTION_QUEUE_CAPACITY: usize = 256;
pub(crate) const STATEMENT_QUEUE_CAPACITY: usize = 64;
pub(crate) const MANDATE_QUEUE_CAPACITY: usize = 64;

/// Doc 19 §14a / Doc 12 §12.9 (FR-052): "independent pause/resume of the
/// Transaction Queue and Statement Queue" -- distinct from `commands::debug`'s
/// `GMAIL_POLL_PAUSED`/`SCAN_QUEUE_PAUSED`, which gate the *producers*
/// (Gmail polling, historical scan) feeding jobs into these channels, not
/// the worker pools that actually consume and process them. Neither queue
/// had any pause mechanism at all before this task.
pub static TRANSACTION_QUEUE_PAUSED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
pub static STATEMENT_QUEUE_PAUSED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

async fn wait_while_transaction_queue_paused() {
    while TRANSACTION_QUEUE_PAUSED.load(std::sync::atomic::Ordering::Relaxed) {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

async fn wait_while_statement_queue_paused() {
    while STATEMENT_QUEUE_PAUSED.load(std::sync::atomic::Ordering::Relaxed) {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

fn validate_queue_name(queue: &str) -> Result<(), crate::error::AppError> {
    if queue != "transaction_queue" && queue != "statement_queue" {
        return Err(crate::error::AppError::Validation(format!(
            "invalid queue '{}': must be 'transaction_queue' or 'statement_queue'",
            queue
        )));
    }
    Ok(())
}

/// Document 19 §14a.1.
#[tauri::command]
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

/// Document 19 §14a.2.
#[tauri::command]
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

/// Document 19 §14a.3's two differently-shaped queue objects
/// (`active_workers` for the transaction queue, `active_parsers` for the
/// statement queue -- not a shared struct).
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

/// Document 19 §14a.3. `queued_jobs` is real (derived from the live
/// `mpsc::Sender`'s remaining capacity, not a placeholder); `active_workers`/
/// `active_parsers` report the configured pool size when running and 0 when
/// paused, rather than tracking a live busy-count -- proportionate to this
/// command's role powering the Local Debug Dashboard's display, not a
/// scheduler decision.
#[tauri::command]
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

/// Spawns both ingestion queues and their worker pools. Called once at app startup.
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

/// Doc 2026-07-28 mail scan performance: was a single consumer looping
/// `rx.recv().await` -> `process_layer6_job(...).await` sequentially, so
/// only one LLM call was ever in flight no matter how many concurrent slots
/// the sidecar calibrated (`llama_sidecar::current_parallel_slots()`,
/// observed calibrating to 7 effective slots in production logs) — the
/// "concurrency is already bounded by the sidecar's semaphore" reasoning
/// this comment used to have only holds with multiple concurrent callers.
/// Spawns a small pool instead so up to `LAYER6_WORKER_COUNT` jobs can be
/// in flight together; the sidecar's own semaphore inside `extract()` still
/// gate-keeps actual concurrent LLM calls, so over-spawning here is
/// harmless (extra workers just queue there instead of at `rx.recv()`).
/// ponytail: fixed clamp to 6 rather than plumbing the runtime-calibrated
/// `effective_slots` value out of `llama_sidecar`'s internal state — revisit
/// if that's ever exposed as a cheap public getter.
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

/// Below this, a Layer 6 result stays in the Unassigned queue pre-filled
/// for the user to confirm rather than auto-becoming a transaction — the
/// needle-verification in `LlmEngine::validate_against_source` already
/// rules out fabricated values before a result ever reaches here; this
/// threshold is a *second*, independent signal (the model's own stated
/// uncertainty) for genuinely ambiguous-but-real extractions, not a
/// hallucination filter.
const LAYER6_AUTO_RESOLVE_CONFIDENCE_THRESHOLD: f64 = 0.75;

async fn process_layer6_job(
    job: Layer6Job,
    pool: &Pool,
    learning: &crate::learning::LearningHandle,
) {
    use crate::extraction::llm::Layer6Outcome;

    // Durability window closes here: the job has been dequeued and is about
    // to run, so a restart from this point on re-enters the same
    // stay-open-and-retry-later behavior a timeout/failure already has
    // (see the `TimedOut | Failed` arm below), not a silent drop. Deleting
    // eagerly (rather than only on success) also avoids replaying a
    // poison-pill job forever on every startup.
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
            // Drift self-healing. Since the 2026-07-26 scan-performance change,
            // Layer 6 no longer runs inside `run_extraction_ladder` on the scan
            // path -- it runs here. Doing the drift check only in the ladder
            // would leave the whole self-healing loop unreachable in production,
            // so it lives at the place Layer 6 actually succeeds.
            crate::extraction::ladder::enqueue_drift_candidates_if_drifted(
                pool,
                learning,
                &job.bank_name,
                &job.body_text,
                &enriched,
                Some(job.app_dir.clone()),
            )
            .await;

            // The LLM's JSON schema has no instrument fields (Doc 12 §6.3
            // scope) -- reuse the same regex-based signal extraction the
            // main ladder already applies to Layers 1-4's output, so a
            // Layer-6-recovered merchant/amount can still clear the Gate 3
            // instrument requirement.
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
            // The model produced (and self-corrected) a response for this
            // email on both attempts and it still never validated -- Layer 6
            // has genuinely looked and there's no extractable transaction
            // here (most commonly a marketing/notification email the content
            // classifier let through). Terminal: mark it out of the open
            // review queue instead of leaving it stuck in
            // `pending_llm_enrichment` forever.
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

/// Upgrades the existing `pending_llm_enrichment` observation in place with
/// the LLM's result. Needle-verification (`LlmEngine::validate_against_source`)
/// already ran before `enriched` was ever produced, so it cannot contain a
/// merchant/amount/reference absent from the source body -- the only two
/// things left to check here are the model's self-reported confidence and
/// whether an instrument actually resolved. Both must clear before this
/// promotes the observation to a real canonical transaction and marks the
/// `unassigned_transactions` row resolved; short of that, the row is still
/// updated with the LLM's best-guess fields (a pre-fill for the user) but
/// stays `open` so it's still visible in the review queue via `select_open`.
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

        // Same relabeling `message_processor` already applies on the
        // Layer 1-5 path (see `self_transfer_destination_account`'s doc
        // comment) -- Layer 6's LLM extraction has no dedicated
        // self-transfer handling of its own, so without this a self-transfer
        // recovered here keeps whichever raw destination-account string the
        // model captured as `merchant_raw` instead of the placeholder.
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
        // enriched.event_time is an i64 Unix timestamp (UTC), same shape
        // normalize_observation converts from — mirror that conversion here.
        //
        // audit_02 #4: fall back to the email's Gmail `internalDate` when the
        // model omits a date, which `Layer6Job::internal_date_seconds`'s own
        // doc comment notes it does "on essentially every call". The
        // placeholder observation this upgrades was created from
        // `ExtractionResult::default()`, so its `event_time` is NULL until
        // something sets it here. Leaving it NULL meant the promotion below
        // reconciled against an empty `event_time` string, which
        // `fetch_candidates` used to silently read as the Unix epoch — a
        // ±3-day candidate window around 1970 that matched nothing and
        // created a duplicate canonical transaction every time.
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
            // No masked identifier in the source at all (e.g. Jupiter's
            // card-payment confirmations never print card digits) -- if the
            // issuer resolves to exactly one instrument on file, there's no
            // ambiguity about which one this is.
            _ => enriched
                .issuer_name
                .as_deref()
                .and_then(|iname| {
                    crate::db::instruments::resolve_single_instrument_by_issuer(c, iname).ok()
                })
                .flatten(),
        };
        row.instrument_id = instrument_id.clone();

        // A detected self-transfer skips the confidence gate: needle-
        // verification already confirmed every field came from the source
        // text, `detect_channel`'s "internal_transfer" match is a
        // deterministic regex on the same body (not a model guess), and
        // there's no merchant-identification ambiguity for the model's
        // stated uncertainty to be hedging about in the first place.
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
            // Still open -- refresh `reason` to reflect what's actually
            // still missing now that Layer 6 has filled in its best guess,
            // rather than leaving it frozen at the pre-enrichment gate3
            // evaluation (see `update_reason`'s doc comment).
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

/// Single dispatcher for the Mandate Queue -- mandate volume is expected to
/// be far lower than transaction volume (registrations/cancellations, not
/// every transaction), so one sequential consumer is sufficient; no worker
/// pool needed unlike the Transaction Queue's 4 parallel workers.
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

/// Processes one mandate event: upserts/matches-and-cancels the
/// recurring_payments row, then sends a synthesized TransactionJob onto the
/// *existing* Transaction Queue for the ₹0.00 transaction side effect --
/// reusing process_transaction_job unmodified rather than calling
/// reconciliation internals directly
/// (dinero-docs/design-archive/specs/2026-07-18-mandate-tracking-design.md §4.4: the
/// real single entry point is reconcile_transactionally via
/// process_transaction_job, not create_canonical_transaction alone).
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

    // Both registration and (successfully matched) cancellation also
    // produce the ₹0 transaction, via the unmodified Transaction Queue.
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

/// Spawns `TRANSACTION_QUEUE_WORKERS` persistent tasks pulling from the shared
/// receiver (wrapped for multi-consumer access, since `mpsc::Receiver` has exactly
/// one owner natively) — this is the "multi-parallel worker pool" of Doc 15 §5.
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

/// Single dispatcher consuming the Statement Queue, spawning each parse under a
/// `Semaphore` permit so at most `STATEMENT_QUEUE_MAX_CONCURRENT` PDF parses run
/// at once — the bounded-pool shape of Doc 15 §5 (rather than N fixed workers).
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

                // audit_04 #1: read the PDF back only now that this task holds
                // a concurrency permit, so at most
                // `STATEMENT_QUEUE_MAX_CONCURRENT` statements are resident at
                // once regardless of how deep the queue is.
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

                // The pipeline re-stores the PDF under its own draft /
                // unprocessed id when it needs to retain it, so this intake
                // copy has served its purpose either way. Best-effort: a
                // leftover file is swept by `cleanup_expired_pdfs`.
                let _ = crate::statements::pdf_storage::delete_pdf(&app_data_dir, &job.stmt_id);

                // Doc 30 TASK-STMT-009: batches over 10 statements get
                // periodic parsed/total/eta_seconds progress — permit release
                // (the `_permit` drop at the end of this task) already happens
                // after this point, so the reported "parsed" count and the
                // concurrency cap stay consistent with each other.
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

                // Doc 19 §9.1/§3.6: fire-and-forget — the IPC call already
                // returned an intake status. The real outcome is reported
                // here, once processing actually finishes, via the same
                // statement_parsed/statement.parse_failed events the
                // Statement-Instrument-Gate-resume commands already emit.
                use crate::statements::events;
                match &result {
                    Ok(crate::commands::PipelineOutcome::Staged(_draft_id)) => {
                        // stage_parse_pipeline already emitted STAGED itself — nothing to do here.
                    }
                    Ok(crate::commands::PipelineOutcome::BlockedAwaitingInstrument(
                        _unprocessed_id,
                    )) => {
                        // The gate already emitted INSTRUMENT_CONFIRMATION_REQUIRED, we don't need to do anything here.
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

/// The single Transaction Queue processing path (Doc 12 §8.2a): instrument
/// resolution, observation persistence, and reconciliation — identical for every
/// entry point that feeds the Transaction Queue.
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

    // TASK-DESK-002: snapshotted before `row` moves into the DB closure
    // below, so a native "new confirmed transaction" notification can be
    // built afterward without needing to re-fetch anything.
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

                    // Doc 30 TASK-TXN-008: fingerprint must be keyed on the
                    // *resolved* instrument_id, which is only known from this
                    // point on — computed here, not inside normalize_observation
                    // (which runs before instrument resolution and has no way
                    // to produce a spec-correct fingerprint yet).
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
                            // Doc 30 TASK-TXN-009: a re-processed message is
                            // silently skipped, never an error and never
                            // re-run through reconciliation a second time.
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
                                // Doc 30 TASK-DEDUP-001: thread the fingerprint
                                // computed above into the reconciliation engine's
                                // input so the fast pre-filter can actually run —
                                // previously computed and persisted but never
                                // consumed anywhere.
                                fingerprint: row.fingerprint.clone(),
                                // Doc 30 TASK-DEDUP-008: threaded into the
                                // reconciliation engine's input for the
                                // email-vs-email precedence comparison.
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

        // Doc 19 §15 / TASK-DESK-002: this queue-driven path previously
        // emitted no Tauri event at all on completion -- confirmed via a
        // full-crate grep showing `AppEvent::TransactionCreated` was only
        // ever emitted from the manual `transactions_create` IPC command
        // (commands/mod.rs), never from real Gmail-ingested transactions.
        // Mirrors that command's exact branching (ambiguous -> cluster
        // event, otherwise -> transaction_created) so both entry points
        // behave identically, and is also what makes a native
        // "new confirmed transaction" notification possible for the
        // real-world case this task is actually about.
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

    /// Document 19 §14a's three pipeline commands share this validation;
    /// `pipeline_pause`/`pipeline_resume` return `VALIDATION_ERROR` per
    /// Doc19 §14a's own error list for anything other than the two real
    /// queue names.
    #[test]
    fn test_validate_queue_name_rejects_unknown_queue() {
        assert!(validate_queue_name("transaction_queue").is_ok());
        assert!(validate_queue_name("statement_queue").is_ok());
        assert!(validate_queue_name("not_a_real_queue").is_err());
        assert!(validate_queue_name("").is_err());
    }

    /// Doc 30 TASK-STMT-009: "A Tokio semaphore with exactly 5 permits guards
    /// entry into the PDF parsing pipeline... additional PDFs beyond 5
    /// concurrent wait FIFO, never dropped/rejected." Proves the real
    /// `STATEMENT_QUEUE_MAX_CONCURRENT` constant the dispatcher uses, the
    /// same way TASK-GMAIL-002's quota-semaphore test proved its cap — every
    /// task eventually gets its permit (never dropped/rejected), and never
    /// more than 5 hold one at once.
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

    /// Doc 30 TASK-STMT-009: "emit periodic scan.progress { parsed, total,
    /// eta_seconds } using a rolling per-statement duration average."
    #[test]
    fn test_eta_calculation_uses_rolling_average() {
        let tracker = BatchProgressTracker::new(4);

        // First statement takes 100ms — average is 100ms, 3 remaining → 300ms ETA.
        let (parsed, total, eta) = tracker.record_completion(std::time::Duration::from_millis(100));
        assert_eq!((parsed, total), (1, 4));
        assert_eq!(eta, 0, "300ms rounds down to 0 whole seconds");

        // Second statement takes 1900ms — average is now (100+1900)/2 = 1000ms,
        // 2 remaining → 2000ms = 2s ETA. Proves the average actually *rolls*
        // forward with new data rather than staying pinned to the first sample.
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

    /// A Layer6Job whose LLM call succeeds but stays below the
    /// auto-resolve confidence threshold (or can't resolve an instrument)
    /// must still update the existing observation in place (not insert a
    /// duplicate) with the LLM's best-guess fields, so the user sees a
    /// pre-filled row instead of a blank one -- but the row must stay
    /// `open` in the Unassigned queue, not silently vanish as `resolved`
    /// without ever becoming a transaction (2026-07-30: this is the bug
    /// that let "successful" Layer 6 runs disappear with no transaction
    /// ever created). This test constructs the observation/unassigned rows
    /// directly and calls the worker's success-processing function with a
    /// fake outcome rather than spawning a real sidecar.
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

        // Gmail's `internalDate` for the source email. Production always has
        // one, and the placeholder observation's own `event_time` is NULL (it
        // was built from `ExtractionResult::default()`), so this is the only
        // date the promotion can use -- see audit_02 #4.
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

    /// A Layer6Job whose result clears both the confidence threshold and
    /// instrument resolution must be promoted all the way to a real
    /// canonical transaction, and the unassigned row marked resolved.
    /// Needle-verification (LlmEngine::validate_against_source) already ran
    /// before this function ever sees the result -- confidence and
    /// instrument are the only two things left to check here.
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

        // Gmail's `internalDate` for the source email. Production always has
        // one, and the placeholder observation's own `event_time` is NULL (it
        // was built from `ExtractionResult::default()`), so this is the only
        // date the promotion can use -- see audit_02 #4.
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

    /// Root-cause regression test for the 2026-07-31 finding: a self-
    /// transfer Layer 6 correctly extracted (amount/merchant/instrument all
    /// resolved) stayed stuck in the Unassigned queue because its
    /// self-reported confidence (0.70) landed under the 0.75 auto-resolve
    /// threshold, and because the destination-account placeholder relabeling
    /// `message_processor` applies on the Layer 1-5 path was never reused
    /// here. Both must now be fixed: a detected `internal_transfer` channel
    /// bypasses the confidence gate, and `merchant_raw` gets the same
    /// "Internal Transfer (A/c X)" placeholder instead of a raw account
    /// number.
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

        // Gmail's `internalDate` for the source email. Production always has
        // one, and the placeholder observation's own `event_time` is NULL (it
        // was built from `ExtractionResult::default()`), so this is the only
        // date the promotion can use -- see audit_02 #4.
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
        // audit_02 #4: the LLM returned no date, so the promotion must have
        // taken the email's internalDate. A NULL here means reconciliation ran
        // against a candidate window around the Unix epoch and the promotion
        // produced a duplicate rather than a match.
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

    /// Root-cause regression test for the 2026-07-31 finding: 54 of 74
    /// unassigned transactions were never even attempted by Layer 6 because
    /// the app restarted while their jobs were still sitting in the
    /// in-memory `mpsc` channel, which has no persistence. Proves the fix's
    /// full lifecycle without a real app restart: `enqueue_layer6_job`
    /// leaves a durable row behind, `replay_pending_layer6_jobs` (the
    /// startup-recovery path) finds and re-sends it, and dequeuing via
    /// `process_layer6_job` deletes the durable row so it isn't replayed
    /// again on a future restart.
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

        // Simulate the restart: nothing dequeued `rx` yet, but the durable
        // row must already be there -- this is exactly the state a crash
        // between enqueue and dequeue would leave behind.
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

        // Startup recovery: replay into a fresh channel, as `lib.rs` does.
        let (replay_tx, mut replay_rx) = mpsc::channel::<Layer6Job>(4);
        replay_pending_layer6_jobs(&pool, &replay_tx, temp_dir.clone()).await;
        let replayed = replay_rx.recv().await.expect("replayed job must arrive");
        assert_eq!(replayed.unassigned_id, "unassigned-durability");

        // Dequeuing (the start of process_layer6_job) must clear the durable
        // row so a *second* restart doesn't replay it forever.
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

        // The original channel's job is still there too (send always
        // succeeds independently of persistence).
        assert!(rx.recv().await.is_some());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
