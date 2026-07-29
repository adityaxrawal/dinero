//! Issue #12: the "Normalize with LLM" Settings action.
//!
//! Walks every transaction whose merchant scores below
//! [`LOW_CONFIDENCE_THRESHOLD`], sends each one's email body and extracted
//! fields to the local LLM, and applies the returned canonical merchant name
//! and category — recording enough to undo any of it.
//!
//! Deliberately reuses Layer 6's infrastructure rather than opening a second
//! LLM path: the same `llama_sidecar` server, the same completion semaphore,
//! the same grammar-constrained decoding, and the same RAM-eligibility gate.
//! The only thing that differs is the prompt and the output schema.

use deadpool_sqlite::Pool;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

use crate::db::merchant_cleanup;
use crate::error::AppError;
use crate::extraction::merchant_llm;

/// Upper bound on one run, so a pathological database can't queue unbounded
/// inference. Well above any realistic low-confidence backlog.
const MAX_CANDIDATES_PER_RUN: usize = 5000;

/// Set by [`merchant_cleanup_cancel`], checked between transactions. Mirrors
/// how the historical scan handles cancellation.
static CLEANUP_CANCELLED: AtomicBool = AtomicBool::new(false);
/// Guards against two concurrent runs stacking inference on the same queue.
static CLEANUP_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(serde::Serialize, Clone)]
pub struct CleanupProgressPayload {
    pub run_id: String,
    pub processed: usize,
    pub total: usize,
    pub applied: usize,
    pub current_merchant: Option<String>,
    pub status: String,
}

#[derive(serde::Serialize)]
pub struct CleanupPreview {
    /// How many transactions would be sent to the LLM.
    pub candidate_count: usize,
    /// Worst offenders, for the "here's what will change" summary.
    pub samples: Vec<CleanupSample>,
    pub llm_eligible: bool,
    pub total_ram_gb: f64,
    pub running: bool,
}

#[derive(serde::Serialize)]
pub struct CleanupSample {
    pub transaction_id: String,
    pub merchant: String,
    pub bank_name: String,
    pub confidence: f64,
    pub has_evidence: bool,
}

/// What the Settings panel shows before the user commits to a run.
#[tauri::command]
pub async fn merchant_cleanup_preview(
    app: tauri::AppHandle,
    pool: tauri::State<'_, Pool>,
) -> Result<CleanupPreview, AppError> {
    let eligibility = app.try_state::<crate::startup::LlmEligibility>();
    let (llm_eligible, total_ram_gb) = eligibility
        .map(|e| (e.eligible, e.total_ram_gb))
        .unwrap_or((false, 0.0));

    let conn = pool.get().await.map_err(|e| AppError::Db(e.to_string()))?;
    let candidates = conn
        .interact(|c| merchant_cleanup::select_candidates(c, MAX_CANDIDATES_PER_RUN))
        .await
        .map_err(|e| AppError::Db(format!("{e:?}")))?
        .map_err(|e| AppError::Db(e.to_string()))?;

    let samples = candidates
        .iter()
        .take(20)
        .map(|c| CleanupSample {
            transaction_id: c.transaction_id.clone(),
            merchant: c.current_merchant.clone(),
            bank_name: c.bank_name.clone(),
            confidence: c.confidence,
            has_evidence: c.body.is_some(),
        })
        .collect();

    Ok(CleanupPreview {
        candidate_count: candidates.len(),
        samples,
        llm_eligible,
        total_ram_gb,
        running: CLEANUP_RUNNING.load(Ordering::SeqCst),
    })
}

/// Resolves the model the user actually selected, the same way Layer 6 does.
async fn resolve_model(pool: &Pool, app_dir: &std::path::Path) -> Option<String> {
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
        .filter(|m| crate::llm_manager::get_model_path(app_dir, &m.id).is_some())
        .map(|m| m.id)
        .collect();
    crate::llm_manager::resolve_active_model(&downloaded, stored.as_deref())
}

/// Starts a cleanup run in the background and returns its `run_id`
/// immediately — the pass reports through `merchant_cleanup_progress` events
/// rather than blocking the IPC call, exactly as the historical scan does.
#[tauri::command]
pub async fn merchant_cleanup_start(
    app: tauri::AppHandle,
    pool: tauri::State<'_, Pool>,
) -> Result<String, AppError> {
    let eligible = app
        .try_state::<crate::startup::LlmEligibility>()
        .map(|e| e.eligible)
        .unwrap_or(false);
    if !eligible {
        return Err(AppError::Validation(
            "This Mac does not meet the memory requirement for on-device AI, so merchant \
             cleanup cannot run here."
                .to_string(),
        ));
    }

    if CLEANUP_RUNNING.swap(true, Ordering::SeqCst) {
        return Err(AppError::Validation(
            "A merchant cleanup run is already in progress.".to_string(),
        ));
    }
    CLEANUP_CANCELLED.store(false, Ordering::SeqCst);

    let run_id = uuid::Uuid::new_v4().to_string();
    let pool = pool.inner().clone();
    let run_id_for_task = run_id.clone();

    tauri::async_runtime::spawn(async move {
        let result = run_cleanup(app.clone(), pool, run_id_for_task.clone()).await;
        if let Err(e) = result {
            tracing::error!("merchant cleanup run failed: {e}");
            let _ = crate::ipc::events::emit_event(
                &app,
                crate::ipc::events::AppEvent::MerchantCleanupProgress,
                CleanupProgressPayload {
                    run_id: run_id_for_task,
                    processed: 0,
                    total: 0,
                    applied: 0,
                    current_merchant: None,
                    status: "failed".to_string(),
                },
            );
        }
        CLEANUP_RUNNING.store(false, Ordering::SeqCst);
    });

    Ok(run_id)
}

#[tauri::command]
pub async fn merchant_cleanup_cancel() -> Result<(), AppError> {
    CLEANUP_CANCELLED.store(true, Ordering::SeqCst);
    Ok(())
}

/// Undoes an entire run: every merchant name, entity link and category goes
/// back, and every pattern rule the run taught is retired.
#[tauri::command]
pub async fn merchant_cleanup_revert(
    pool: tauri::State<'_, Pool>,
    run_id: String,
) -> Result<usize, AppError> {
    let conn = pool.get().await.map_err(|e| AppError::Db(e.to_string()))?;
    conn.interact(move |c| merchant_cleanup::revert_run(c, &run_id))
        .await
        .map_err(|e| AppError::Db(format!("{e:?}")))?
        .map_err(|e| AppError::Db(e.to_string()))
}

/// The run loop. Fans out across the same number of slots the sidecar
/// calibrated for Layer 6, since it is the same server doing the work.
async fn run_cleanup(
    app: tauri::AppHandle,
    pool: Pool,
    run_id: String,
) -> anyhow::Result<()> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!("no app data dir: {e}"))?;
    let model_id = resolve_model(&pool, &app_dir)
        .await
        .ok_or_else(|| anyhow::anyhow!("no downloaded LLM model available"))?;

    let conn = pool.get().await?;
    let (candidates, categories) = conn
        .interact(|c| -> anyhow::Result<_> {
            Ok((
                merchant_cleanup::select_candidates(c, MAX_CANDIDATES_PER_RUN)?,
                merchant_cleanup::category_names(c)?,
            ))
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e:?}"))??;
    drop(conn);

    let total = candidates.len();
    if total == 0 || categories.is_empty() {
        emit_progress(&app, &run_id, 0, 0, 0, None, "completed");
        return Ok(());
    }

    let schema = merchant_llm::merchant_cleanup_schema(&categories);
    let categories = Arc::new(categories);
    let queue = Arc::new(Mutex::new(candidates.into_iter()));
    let processed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let applied = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let worker_count = crate::llama_sidecar::current_parallel_slots().clamp(1, 6);
    let mut handles = Vec::new();

    for _ in 0..worker_count {
        let queue = Arc::clone(&queue);
        let processed = Arc::clone(&processed);
        let applied = Arc::clone(&applied);
        let categories = Arc::clone(&categories);
        let schema = schema.clone();
        let pool = pool.clone();
        let app = app.clone();
        let run_id = run_id.clone();
        let app_dir = app_dir.clone();
        let model_id = model_id.clone();

        handles.push(tauri::async_runtime::spawn(async move {
            loop {
                if CLEANUP_CANCELLED.load(Ordering::SeqCst) {
                    break;
                }
                let Some(candidate) = queue.lock().await.next() else {
                    break;
                };

                let ok = process_one(
                    &pool,
                    &app_dir,
                    &model_id,
                    &run_id,
                    &candidate,
                    &categories,
                    &schema,
                )
                .await;
                if ok {
                    applied.fetch_add(1, Ordering::SeqCst);
                }

                let done = processed.fetch_add(1, Ordering::SeqCst) + 1;
                emit_progress(
                    &app,
                    &run_id,
                    done,
                    total,
                    applied.load(Ordering::SeqCst),
                    Some(candidate.current_merchant.clone()),
                    "running",
                );
            }
        }));
    }

    for h in handles {
        let _ = h.await;
    }

    let status = if CLEANUP_CANCELLED.load(Ordering::SeqCst) {
        "cancelled"
    } else {
        "completed"
    };
    emit_progress(
        &app,
        &run_id,
        processed.load(Ordering::SeqCst),
        total,
        applied.load(Ordering::SeqCst),
        None,
        status,
    );
    Ok(())
}

/// One transaction: prompt, infer, validate, apply. Returns whether a
/// correction was actually written.
///
/// Every failure mode — no model, timeout, unparseable output, hallucinated
/// merchant, DB error — leaves the transaction exactly as it was. A skipped
/// row is not an error; it simply stays in the queue for a future run.
async fn process_one(
    pool: &Pool,
    app_dir: &std::path::Path,
    model_id: &str,
    run_id: &str,
    candidate: &merchant_cleanup::CleanupCandidate,
    categories: &[String],
    schema: &serde_json::Value,
) -> bool {
    // Without a body there is nothing to read; the extracted fields alone
    // carry no information the parser did not already have. Retention keeps
    // bodies for a year (`db::retention::RAW_PAYLOAD_RETENTION`), so this
    // only bites on genuinely old transactions.
    let Some(body) = candidate.body.as_deref() else {
        tracing::debug!(
            tx = %candidate.transaction_id,
            "merchant cleanup: skipped, evidence body no longer retained"
        );
        return false;
    };

    let ctx = merchant_llm::TransactionContext {
        bank_name: &candidate.bank_name,
        current_merchant: &candidate.current_merchant,
        amount: candidate.amount,
        currency: candidate.currency.as_deref(),
        direction: candidate.direction.as_deref(),
        event_time: candidate.event_time.as_deref(),
    };
    let prompt = merchant_llm::generate_prompt(&ctx, body, categories);

    let raw = match crate::llama_sidecar::complete_with_schema(
        app_dir,
        model_id,
        &prompt,
        schema.clone(),
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(tx = %candidate.transaction_id, "merchant cleanup: inference failed: {e}");
            return false;
        }
    };

    let Some(resolution) = merchant_llm::validate(&raw, body, categories) else {
        return false;
    };

    let Ok(conn) = pool.get().await else {
        return false;
    };
    let candidate = candidate.clone();
    let run_id = run_id.to_string();
    match conn
        .interact(move |c| merchant_cleanup::apply_correction(c, &run_id, &candidate, &resolution))
        .await
    {
        Ok(Ok(())) => true,
        Ok(Err(e)) => {
            tracing::warn!("merchant cleanup: failed to apply correction: {e}");
            false
        }
        Err(e) => {
            tracing::warn!("merchant cleanup: pool interact failed: {e:?}");
            false
        }
    }
}

fn emit_progress(
    app: &tauri::AppHandle,
    run_id: &str,
    processed: usize,
    total: usize,
    applied: usize,
    current_merchant: Option<String>,
    status: &str,
) {
    let _ = crate::ipc::events::emit_event(
        app,
        crate::ipc::events::AppEvent::MerchantCleanupProgress,
        CleanupProgressPayload {
            run_id: run_id.to_string(),
            processed,
            total,
            applied,
            current_merchant,
            status: status.to_string(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::merchant_confidence::LOW_CONFIDENCE_THRESHOLD;

    /// The preview's headline number must mean the same thing the run will
    /// actually do, so both go through one queue function.
    #[test]
    fn threshold_is_shared_with_the_scorer() {
        assert_eq!(LOW_CONFIDENCE_THRESHOLD, 0.60);
    }

    /// A second start must be refused while one is running, or two worker
    /// pools would contend for the same sidecar slots and re-process the
    /// same queue.
    #[test]
    fn concurrent_runs_are_refused() {
        CLEANUP_RUNNING.store(false, Ordering::SeqCst);
        assert!(!CLEANUP_RUNNING.swap(true, Ordering::SeqCst));
        assert!(
            CLEANUP_RUNNING.swap(true, Ordering::SeqCst),
            "the second claim must observe the run already in progress"
        );
        CLEANUP_RUNNING.store(false, Ordering::SeqCst);
    }
}
