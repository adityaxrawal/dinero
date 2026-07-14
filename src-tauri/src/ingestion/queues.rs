//! The two isolated ingestion queues (Doc 15 §2 principle 7, §5; Doc 12 §6.2a, §7.2).
//!
//! Every classified message is routed to exactly one of these queues — never both,
//! never neither. Both queues, and manual statement upload, converge on the same
//! processing function per queue, so there is exactly one Transaction-observation
//! path and exactly one Statement-parsing path, regardless of entry point.

use crate::extraction::ladder::ExtractionResult;
use crate::statements::pending_bytes::PendingStatementBytes;
use deadpool_sqlite::Pool;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::{mpsc, Mutex, Semaphore};

/// One classified, Gate-3-passed transaction-alert observation, ready for
/// instrument resolution, persistence, and reconciliation (Doc 12 §6.2a/§6.3).
pub struct TransactionJob {
    pub obs: ExtractionResult,
    pub source_pipeline: String,
    pub source_record_id: String,
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
pub struct StatementJob {
    pub bytes: Vec<u8>,
    pub filename: String,
    pub file_hash: String,
    pub stmt_id: String,
}

/// Senders for both queues, stored as Tauri managed state so every entry point
/// (Gmail polling, historical scan, manual upload) reaches the same two queues.
#[derive(Clone)]
pub struct QueueHandles {
    pub transaction_tx: mpsc::Sender<TransactionJob>,
    pub statement_tx: mpsc::Sender<StatementJob>,
}

/// Multi-parallel worker pool size for the Transaction Queue (Doc 15 §5: 2–8 workers).
const TRANSACTION_QUEUE_WORKERS: usize = 4;
/// Bounded concurrent PDF parses for the Statement Queue (Doc 15 §5, Doc 12 §6.2a/§7).
const STATEMENT_QUEUE_MAX_CONCURRENT: usize = 5;

const TRANSACTION_QUEUE_CAPACITY: usize = 256;
const STATEMENT_QUEUE_CAPACITY: usize = 64;

/// Spawns both ingestion queues and their worker pools. Called once at app startup.
pub fn spawn_queues<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    pool: Pool,
    pending_bytes: PendingStatementBytes,
) -> QueueHandles {
    let (transaction_tx, transaction_rx) = mpsc::channel::<TransactionJob>(TRANSACTION_QUEUE_CAPACITY);
    let (statement_tx, statement_rx) = mpsc::channel::<StatementJob>(STATEMENT_QUEUE_CAPACITY);

    spawn_transaction_workers(transaction_rx, pool.clone());
    spawn_statement_dispatcher(statement_rx, pool, app, pending_bytes);

    QueueHandles {
        transaction_tx,
        statement_tx,
    }
}

/// Spawns `TRANSACTION_QUEUE_WORKERS` persistent tasks pulling from the shared
/// receiver (wrapped for multi-consumer access, since `mpsc::Receiver` has exactly
/// one owner natively) — this is the "multi-parallel worker pool" of Doc 15 §5.
fn spawn_transaction_workers(rx: mpsc::Receiver<TransactionJob>, pool: Pool) {
    let rx = Arc::new(Mutex::new(rx));
    for _ in 0..TRANSACTION_QUEUE_WORKERS {
        let rx = Arc::clone(&rx);
        let pool = pool.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let job = { rx.lock().await.recv().await };
                match job {
                    Some(job) => process_transaction_job(job, &pool).await,
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
            let permit = Arc::clone(&semaphore).acquire_owned().await.unwrap();
            let pool = pool.clone();
            let app = app.clone();
            let pending_bytes = pending_bytes.clone();
            tauri::async_runtime::spawn(async move {
                let _permit = permit;
                let result = crate::commands::run_parse_pipeline(
                    &job.bytes,
                    &job.filename,
                    &job.file_hash,
                    &pool,
                    &app,
                    &pending_bytes,
                    None,
                    None,
                    Some(job.stmt_id),
                )
                .await;

                // Doc 19 §9.1/§3.6: fire-and-forget — the IPC call already
                // returned an intake status. The real outcome is reported
                // here, once processing actually finishes, via the same
                // statement_parsed/statement.parse_failed events the
                // Statement-Instrument-Gate-resume commands already emit.
                use crate::statements::events;
                match &result {
                    Ok(stmt_id) => {
                        events::emit(
                            events::PARSED,
                            serde_json::json!({ "statement_id": stmt_id, "filename": job.filename }),
                        );
                        let _ = app.emit(
                            events::PARSED,
                            serde_json::json!({ "statement_id": stmt_id, "filename": job.filename }),
                        );
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
async fn process_transaction_job(job: TransactionJob, pool: &Pool) {
    let obs = job.obs;
    let instrument_type = obs.instrument_type.clone();
    let issuer_name = obs.issuer_name.clone();
    let masked_identifier = obs.masked_identifier.clone();
    let network = obs.network.clone();
    let mut row = crate::extraction::normalization::normalize_observation(
        obs,
        &job.source_pipeline,
        &job.source_record_id,
    );

    if let Ok(conn) = pool.get().await {
        let _ = conn
            .interact(move |c| {
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

                if let Err(e) = crate::db::transaction_observations::insert_observation(c, &row) {
                    tracing::warn!("Observation insert failed (possibly deduped): {}", e);
                } else {
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
                    };

                    let candidates =
                        crate::reconciliation::engine::fetch_candidates(c, &incoming_obs)
                            .unwrap_or_default();
                    match crate::reconciliation::engine::reconcile(c, &incoming_obs, candidates) {
                        Ok(decision) => {
                            tracing::debug!(
                                "Reconciliation decision for obs '{}': {:?}",
                                incoming_obs.id,
                                decision
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Reconciliation failed for obs '{}': {}",
                                incoming_obs.id,
                                e
                            );
                        }
                    }
                }
            })
            .await;
    }
}
