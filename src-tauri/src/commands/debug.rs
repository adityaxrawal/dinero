//! Read-only introspection commands for the development debug screen.
//!
//! Surfaces parse errors, unprocessed statements, audit rows and clusters as raw
//! records. The route that reaches these is compiled out of release builds.
use crate::db;
use crate::error::AppError;
use deadpool_sqlite::Pool;
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
pub struct DebugDashboardState {
    pub gmail_poll_paused: bool,
    pub scan_queue_paused: bool,
}

#[tauri::command]
/// Lists extraction failures with their raw context.
pub async fn debug_fetch_parse_errors(
    pool: State<'_, Pool>,
) -> Result<Vec<db::transaction_observations::TransactionObservationsRow>, AppError> {
    let conn = pool.get().await.map_err(|e| AppError::Db(e.to_string()))?;
    let rows = conn.interact(|c| {
        let mut stmt = c.prepare("SELECT * FROM transaction_observations WHERE extraction_method = 'failed' ORDER BY created_at DESC")?;
        let iter = stmt.query_map([], db::transaction_observations::row_to_observation)?;
        let mut results = Vec::new();
        for r in iter {
            results.push(r?);
        }
        Ok::<_, rusqlite::Error>(results)
    }).await.map_err(|e| AppError::Unknown(e.to_string()))?.map_err(|e| AppError::Db(e.to_string()))?;

    Ok(rows)
}

#[tauri::command]
/// Lists statements that failed to process.
pub async fn debug_fetch_unprocessed_statements(
    pool: State<'_, Pool>,
) -> Result<Vec<db::unprocessed_statements::UnprocessedStatementRow>, AppError> {
    let conn = pool.get().await.map_err(|e| AppError::Db(e.to_string()))?;
    let rows = conn
        .interact(|c| db::unprocessed_statements::select_pending(c))
        .await
        .map_err(|e| AppError::Unknown(e.to_string()))?
        .map_err(|e| AppError::Db(e.to_string()))?;

    Ok(rows)
}

#[tauri::command]
/// Returns raw audit-log rows.
pub async fn debug_fetch_audit_log(
    pool: State<'_, Pool>,
    resource_type_filter: Option<String>,
    limit: u32,
    offset: u32,
) -> Result<Vec<db::audit_log::AuditLogRow>, AppError> {
    let conn = pool.get().await.map_err(|e| AppError::Db(e.to_string()))?;
    let rows = conn
        .interact(move |c| db::audit_log::fetch_all(c, resource_type_filter, limit, offset))
        .await
        .map_err(|e| AppError::Unknown(e.to_string()))?
        .map_err(|e| AppError::Db(e.to_string()))?;

    Ok(rows)
}

#[tauri::command]
/// Returns reconciliation clusters in raw form.
pub async fn debug_fetch_reconciliation_clusters(
    pool: State<'_, Pool>,
) -> Result<Vec<db::reconciliation_clusters::ReconciliationClustersRow>, AppError> {
    let conn = pool.get().await.map_err(|e| AppError::Db(e.to_string()))?;
    let rows = conn
        .interact(|c| db::reconciliation_clusters::select_all(c))
        .await
        .map_err(|e| AppError::Unknown(e.to_string()))?
        .map_err(|e| AppError::Db(e.to_string()))?;

    Ok(rows)
}

use std::sync::atomic::{AtomicBool, Ordering};

pub static GMAIL_POLL_PAUSED: AtomicBool = AtomicBool::new(false);
pub static SCAN_QUEUE_PAUSED: AtomicBool = AtomicBool::new(false);

#[tauri::command]
/// Reports the pipeline's current pause and queue state.
pub async fn debug_get_pipeline_state() -> Result<DebugDashboardState, AppError> {
    Ok(DebugDashboardState {
        gmail_poll_paused: GMAIL_POLL_PAUSED.load(Ordering::Relaxed),
        scan_queue_paused: SCAN_QUEUE_PAUSED.load(Ordering::Relaxed),
    })
}

#[tauri::command]
/// Pauses or resumes Gmail polling, for debugging.
pub async fn debug_set_gmail_poll_paused(paused: bool) -> Result<(), AppError> {
    GMAIL_POLL_PAUSED.store(paused, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
/// Pauses or resumes the scan queue, for debugging.
pub async fn debug_set_scan_queue_paused(paused: bool) -> Result<(), AppError> {
    SCAN_QUEUE_PAUSED.store(paused, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
/// Audits whether a scan genuinely covered its full date range.
pub async fn debug_audit_scan_coverage<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    pool: State<'_, Pool>,
    account_id: String,
    start_date: String,
    end_date: String,
) -> Result<crate::ingestion::historical_scan::ScanCoverageAudit, AppError> {
    crate::ipc::validation::validate_account_id("account_id", &account_id)?;
    crate::ipc::validation::validate_date_range(&start_date, &end_date)?;

    let pool = pool.inner().clone();
    let access_token = crate::ingestion::oauth::get_valid_access_token(&app, &pool, &account_id)
        .await
        .map_err(|e| AppError::Auth(e.to_string()))?;
    let refresher = crate::ingestion::oauth::create_token_refresher(&app, &pool, &account_id);
    let client =
        crate::ingestion::gmail_client::GmailClient::new(access_token, pool.clone(), refresher);

    crate::ingestion::historical_scan::audit_scan_coverage(&pool, &client, &start_date, &end_date)
        .await
        .map_err(|e| AppError::Unknown(e.to_string()))
}
