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
pub async fn debug_get_pipeline_state() -> Result<DebugDashboardState, AppError> {
    Ok(DebugDashboardState {
        gmail_poll_paused: GMAIL_POLL_PAUSED.load(Ordering::Relaxed),
        scan_queue_paused: SCAN_QUEUE_PAUSED.load(Ordering::Relaxed),
    })
}

#[tauri::command]
pub async fn debug_set_gmail_poll_paused(paused: bool) -> Result<(), AppError> {
    GMAIL_POLL_PAUSED.store(paused, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn debug_set_scan_queue_paused(paused: bool) -> Result<(), AppError> {
    SCAN_QUEUE_PAUSED.store(paused, Ordering::Relaxed);
    Ok(())
}

/// Proves whether the historical scan's server-side prefilter is dropping any
/// mail Gate 1 would have accepted.
///
/// Runs the old unfiltered date-range search alongside the current filtered
/// one, then runs the real Gate 1 over every message the filter excluded.
/// `missed_total == 0` is the answer to "how do I know the fast scan didn't
/// skip a real transaction?" for this mailbox and date range.
///
/// Slow on purpose — it performs exactly the full-mailbox metadata sweep the
/// prefilter exists to avoid, so run it deliberately, not on a schedule.
#[tauri::command]
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
    let access_token =
        crate::ingestion::oauth::get_valid_access_token(&app, &pool, &account_id)
            .await
            .map_err(|e| AppError::Auth(e.to_string()))?;
    let refresher = crate::ingestion::oauth::create_token_refresher(&app, &pool, &account_id);
    let client = crate::ingestion::gmail_client::GmailClient::new(
        access_token,
        pool.clone(),
        refresher,
    );

    crate::ingestion::historical_scan::audit_scan_coverage(
        &pool,
        &client,
        &start_date,
        &end_date,
    )
    .await
    .map_err(|e| AppError::Unknown(e.to_string()))
}


