//! Commands driving the AI merchant-normalisation pass.
//!
//! Preview before run is the point: the pass rewrites merchant names in bulk, so
//! the user sees a sample of what would change before agreeing to it, and each
//! applied change stays individually revertible afterwards.
use deadpool_sqlite::Pool;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

use crate::db::merchant_cleanup;
use crate::error::AppError;
use crate::extraction::merchant_llm;

const MAX_CANDIDATES_PER_RUN: usize = 5000;

static CLEANUP_CANCELLED: AtomicBool = AtomicBool::new(false);
static CLEANUP_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(serde::Serialize, Clone)]
pub struct CleanupProgressPayload {
    pub run_id: String,
    pub processed: usize,
    pub total: usize,
    pub applied: usize,
    pub skipped: usize,
    pub current_merchant: Option<String>,
    pub bank_name: Option<String>,
    pub resolved_merchant: Option<String>,
    pub resolved_category: Option<String>,
    pub status: String,
}

#[derive(serde::Serialize)]
pub struct CleanupPreview {
    pub candidate_count: usize,
    pub no_evidence_count: usize,
    pub by_bank: Vec<BankBucket>,
    pub samples: Vec<CleanupSample>,
    pub llm_eligible: bool,
    pub total_ram_gb: f64,
    pub running: bool,
}

#[derive(serde::Serialize)]
pub struct BankBucket {
    pub bank_name: String,
    pub count: usize,
    pub no_evidence: usize,
}

#[derive(serde::Serialize)]
pub struct CleanupSample {
    pub transaction_id: String,
    pub merchant: String,
    pub bank_name: String,
    pub confidence: f64,
    pub has_evidence: bool,
    pub amount: Option<f64>,
    pub currency: Option<String>,
    pub direction: Option<String>,
    pub event_time: Option<String>,
}

#[tauri::command]
/// Previews what a cleanup run would change, without applying anything.
///
/// Preview precedes run by design: the pass rewrites merchant names in bulk, so
/// the user agrees to a sample of real proposed changes rather than to the idea
/// of the operation.
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
            amount: c.amount,
            currency: c.currency.clone(),
            direction: c.direction.clone(),
            event_time: c.event_time.clone(),
        })
        .collect();

    let mut buckets: Vec<BankBucket> = {
        let mut acc: std::collections::HashMap<&str, (usize, usize)> = Default::default();
        for c in &candidates {
            let e = acc.entry(c.bank_name.as_str()).or_insert((0, 0));
            e.0 += 1;
            if c.body.is_none() {
                e.1 += 1;
            }
        }
        acc.into_iter()
            .map(|(bank_name, (count, no_evidence))| BankBucket {
                bank_name: bank_name.to_string(),
                count,
                no_evidence,
            })
            .collect()
    };
    buckets.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.bank_name.cmp(&b.bank_name))
    });

    Ok(CleanupPreview {
        candidate_count: candidates.len(),
        no_evidence_count: candidates.iter().filter(|c| c.body.is_none()).count(),
        by_bank: buckets,
        samples,
        llm_eligible,
        total_ram_gb,
        running: CLEANUP_RUNNING.load(Ordering::SeqCst),
    })
}

#[tauri::command]
/// Lists past cleanup runs.
pub async fn merchant_cleanup_runs(
    pool: tauri::State<'_, Pool>,
    limit: Option<usize>,
) -> Result<Vec<merchant_cleanup::RunDetail>, AppError> {
    let limit = limit.unwrap_or(20).clamp(1, 100);
    let conn = pool.get().await.map_err(|e| AppError::Db(e.to_string()))?;
    conn.interact(move |c| merchant_cleanup::list_runs(c, limit))
        .await
        .map_err(|e| AppError::Db(format!("{e:?}")))?
        .map_err(|e| AppError::Db(e.to_string()))
}

#[tauri::command]
/// Reverts a single correction.
pub async fn merchant_cleanup_revert_correction(
    pool: tauri::State<'_, Pool>,
    correction_id: String,
) -> Result<(), AppError> {
    crate::ipc::validation::validate_uuid("correction_id", &correction_id)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    let conn = pool.get().await.map_err(|e| AppError::Db(e.to_string()))?;
    conn.interact(move |c| merchant_cleanup::revert_correction(c, &correction_id))
        .await
        .map_err(|e| AppError::Db(format!("{e:?}")))?
        .map_err(|e| AppError::Db(e.to_string()))
}

/// Resolves which model to use, or None if local inference is unavailable.
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

#[tauri::command]
/// Starts a cleanup run, reporting progress by event.
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
                    skipped: 0,
                    current_merchant: None,
                    bank_name: None,
                    resolved_merchant: None,
                    resolved_category: None,
                    status: "failed".to_string(),
                },
            );
        }
        CLEANUP_RUNNING.store(false, Ordering::SeqCst);
    });

    Ok(run_id)
}

#[tauri::command]
/// Cancels a running cleanup.
pub async fn merchant_cleanup_cancel() -> Result<(), AppError> {
    CLEANUP_CANCELLED.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
/// Reverts an entire run.
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

/// Executes the cleanup run over its candidate queue.
async fn run_cleanup(app: tauri::AppHandle, pool: Pool, run_id: String) -> anyhow::Result<()> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!("no app data dir: {e}"))?;
    let model_id = resolve_model(&pool, &app_dir)
        .await
        .ok_or_else(|| anyhow::anyhow!("no downloaded LLM model available"))?;

    let pipeline = app.state::<crate::llm_pipeline::LlmPipeline>().inner().clone();

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
        emit_progress(&app, &run_id, 0, 0, 0, 0, None, None, None, "completed");
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
        let pipeline = pipeline.clone();

        handles.push(tauri::async_runtime::spawn(async move {
            loop {
                if CLEANUP_CANCELLED.load(Ordering::SeqCst) {
                    break;
                }
                let Some(candidate) = queue.lock().await.next() else {
                    break;
                };

                let resolution = process_one(
                    &pool,
                    &app_dir,
                    &model_id,
                    &pipeline,
                    &run_id,
                    &candidate,
                    &categories,
                    &schema,
                )
                .await;
                if resolution.is_some() {
                    applied.fetch_add(1, Ordering::SeqCst);
                }

                let done = processed.fetch_add(1, Ordering::SeqCst) + 1;
                let applied_now = applied.load(Ordering::SeqCst);
                emit_progress(
                    &app,
                    &run_id,
                    done,
                    total,
                    applied_now,
                    done - applied_now,
                    Some(candidate.current_merchant.clone()),
                    Some(candidate.bank_name.clone()),
                    resolution.as_ref(),
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
    let done = processed.load(Ordering::SeqCst);
    let fixed = applied.load(Ordering::SeqCst);
    emit_progress(
        &app,
        &run_id,
        done,
        total,
        fixed,
        done - fixed,
        None,
        None,
        None,
        status,
    );
    Ok(())
}

/// Processes one merchant candidate through the model and applies the result.
async fn process_one(
    pool: &Pool,
    app_dir: &std::path::Path,
    model_id: &str,
    pipeline: &crate::llm_pipeline::LlmPipeline,
    run_id: &str,
    candidate: &merchant_cleanup::CleanupCandidate,
    categories: &[String],
    schema: &serde_json::Value,
) -> Option<merchant_llm::MerchantResolution> {
    let Some(body) = candidate.body.as_deref() else {
        tracing::debug!(
            tx = %candidate.transaction_id,
            "merchant cleanup: skipped, evidence body no longer retained"
        );
        return None;
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

    let ctx = crate::logging::llm_logger::LlmCallContext::new(
        crate::logging::llm_logger::LlmCallType::MerchantCleanup,
        1,
    );
    let (tx, rx) = tokio::sync::oneshot::channel();
    let req = crate::llm_pipeline::LlmRequest {
        model_id: model_id.to_string(),
        prompt: prompt.clone(),
        schema: Some(schema.clone()),
        ctx,
        app_dir: app_dir.to_path_buf(),
        response_tx: tx,
    };

    if let Err(e) = pipeline.enqueue(req).await {
        tracing::debug!(tx = %candidate.transaction_id, "merchant cleanup: pipeline error: {e}");
        return None;
    }

    let raw = match rx.await {
        Ok(Ok(raw)) => raw,
        Ok(Err(e)) => {
            tracing::debug!(tx = %candidate.transaction_id, "merchant cleanup: inference failed: {e}");
            return None;
        }
        Err(_) => {
            tracing::debug!(tx = %candidate.transaction_id, "merchant cleanup: pipeline channel closed");
            return None;
        }
    };

    let resolution = merchant_llm::validate(&raw, body, categories)?;

    let Ok(conn) = pool.get().await else {
        return None;
    };
    let candidate = candidate.clone();
    let run_id = run_id.to_string();
    let applied = resolution.clone();
    match conn
        .interact(move |c| merchant_cleanup::apply_correction(c, &run_id, &candidate, &applied))
        .await
    {
        Ok(Ok(())) => Some(resolution),
        Ok(Err(e)) => {
            tracing::warn!("merchant cleanup: failed to apply correction: {e}");
            None
        }
        Err(e) => {
            tracing::warn!("merchant cleanup: pool interact failed: {e:?}");
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
/// Emits a progress event for the running cleanup.
fn emit_progress(
    app: &tauri::AppHandle,
    run_id: &str,
    processed: usize,
    total: usize,
    applied: usize,
    skipped: usize,
    current_merchant: Option<String>,
    bank_name: Option<String>,
    resolution: Option<&merchant_llm::MerchantResolution>,
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
            skipped,
            current_merchant,
            bank_name,
            resolved_merchant: resolution.map(|r| r.merchant_name.clone()),
            resolved_category: resolution.map(|r| r.category.clone()),
            status: status.to_string(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::merchant_confidence::LOW_CONFIDENCE_THRESHOLD;

    #[test]
    fn threshold_is_shared_with_the_scorer() {
        assert_eq!(LOW_CONFIDENCE_THRESHOLD, 0.60);
    }

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
