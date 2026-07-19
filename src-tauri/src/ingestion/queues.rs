//! The two isolated ingestion queues (Doc 15 §2 principle 7, §5; Doc 12 §6.2a, §7.2).
//!
//! Every classified message is routed to exactly one of these queues — never both,
//! never neither. Both queues, and manual statement upload, converge on the same
//! processing function per queue, so there is exactly one Transaction-observation
//! path and exactly one Statement-parsing path, regardless of entry point.

use crate::extraction::ladder::ExtractionResult;
use crate::statements::pending_bytes::PendingStatementBytes;
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
    pub bytes: Vec<u8>,
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
/// (docs/superpowers/specs/2026-07-18-mandate-tracking-design.md §4.2-§4.4).
pub struct MandateJob {
    pub extraction: crate::extraction::mandate_extractor::MandateExtraction,
    pub event_type: crate::ingestion::message_processor::MandateEventType,
    pub source_pipeline: String,
    pub source_record_id: String,
    pub connected_account_id: String,
    pub raw_body: Option<String>,
}

/// Senders for all three queues, stored as Tauri managed state so every
/// entry point (Gmail polling, historical scan, manual upload) reaches the
/// same queues.
#[derive(Clone)]
pub struct QueueHandles {
    pub transaction_tx: mpsc::Sender<TransactionJob>,
    pub statement_tx: mpsc::Sender<StatementJob>,
    pub mandate_tx: mpsc::Sender<MandateJob>,
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
    pending_bytes: PendingStatementBytes,
) -> QueueHandles {
    let (transaction_tx, transaction_rx) =
        mpsc::channel::<TransactionJob>(TRANSACTION_QUEUE_CAPACITY);
    let (statement_tx, statement_rx) = mpsc::channel::<StatementJob>(STATEMENT_QUEUE_CAPACITY);
    let (mandate_tx, mandate_rx) = mpsc::channel::<MandateJob>(MANDATE_QUEUE_CAPACITY);

    spawn_transaction_workers(transaction_rx, pool.clone(), app.clone());
    spawn_statement_dispatcher(statement_rx, pool.clone(), app, pending_bytes);
    spawn_mandate_workers(mandate_rx, pool, transaction_tx.clone());

    QueueHandles {
        transaction_tx,
        statement_tx,
        mandate_tx,
    }
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
/// (docs/superpowers/specs/2026-07-18-mandate-tracking-design.md §4.4: the
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
                    .map(|(entity_id, _)| entity_id);

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
    pending_bytes: PendingStatementBytes,
) {
    let semaphore = Arc::new(Semaphore::new(STATEMENT_QUEUE_MAX_CONCURRENT));
    tauri::async_runtime::spawn(async move {
        while let Some(job) = rx.recv().await {
            wait_while_statement_queue_paused().await;
            let permit = Arc::clone(&semaphore).acquire_owned().await.unwrap();
            let pool = pool.clone();
            let app = app.clone();
            let pending_bytes = pending_bytes.clone();
            tauri::async_runtime::spawn(async move {
                let _permit = permit;
                let start = std::time::Instant::now();
                let result = crate::commands::run_parse_pipeline(
                    &job.bytes,
                    &job.filename,
                    &job.file_hash,
                    &pool,
                    &app,
                    &pending_bytes,
                    None,
                    job.password.as_deref(),
                    Some(job.stmt_id),
                )
                .await;

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
                    Ok(crate::commands::PipelineOutcome::Parsed(stmt_id)) => {
                        events::emit(
                            events::PARSED,
                            serde_json::json!({ "statement_id": stmt_id, "filename": job.filename }),
                        );
                        let _ = app.emit(
                            events::PARSED,
                            serde_json::json!({ "statement_id": stmt_id, "filename": job.filename }),
                        );
                    }
                    Ok(crate::commands::PipelineOutcome::BlockedAwaitingInstrument(_unprocessed_id)) => {
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
