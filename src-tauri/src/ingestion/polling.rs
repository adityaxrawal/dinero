//! The background loop that discovers new mail.
//!
//! Polls incrementally using Gmail's history id, so each cycle asks only what
//! has changed since the last one rather than re-listing the mailbox. The saved
//! history id is what makes that resumable across restarts.
//!
//! Cadence adapts to power state, since polling aggressively on battery costs
//! the user real runtime for mail that will still be there in ten minutes.
use deadpool_sqlite::Pool;
use reqwest::StatusCode;
use serde::Deserialize;
use std::time::Duration;
use tauri::Manager;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::db::connected_accounts::{self, ConnectedAccountsRow};
use crate::db::processing_checkpoints::{self, ProcessingCheckpointRow};
use crate::ingestion::gmail_client::{gmail_gate, quota_cost};
use crate::ingestion::oauth::{get_valid_access_token, handle_invalid_history_id};
use crate::network_client::NetworkClient;

#[derive(Deserialize, Debug)]
#[allow(non_snake_case)]
struct HistoryResponse {
    historyId: Option<String>,
    nextPageToken: Option<String>,
    history: Option<Vec<HistoryRecord>>,
}

#[derive(Deserialize, Debug)]
#[allow(non_snake_case)]
struct HistoryRecord {
    messagesAdded: Option<Vec<HistoryMessageAdded>>,
}

#[derive(Deserialize, Debug)]
#[allow(non_snake_case)]
struct HistoryMessageAdded {
    message: HistoryMessage,
}

#[derive(Deserialize, Debug)]
struct HistoryMessage {
    id: String,
}

/// Doubles the backoff interval, capped at a minute.
///
/// The cap matters: without it repeated failures would push the retry interval
/// out to hours, and a transient outage would look like a permanently stopped
/// sync. `saturating_mul` prevents overflow at extreme values.
pub(crate) fn next_backoff(current: Duration) -> Duration {
    current.saturating_mul(2).min(Duration::from_secs(60))
}

/// Applies +/-15% random jitter to an interval.
///
/// Stops multiple accounts that failed together from retrying in lockstep and
/// hammering the API at the same instant. Entropy comes from the clock's
/// sub-second component, which is sufficient here and avoids pulling in an RNG.
pub(crate) fn jittered(d: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.subsec_nanos())
        .unwrap_or(0);
    let factor = 0.85 + (nanos as f64 / 1_000_000_000.0) * 0.30;
    Duration::from_secs_f64((d.as_secs_f64() * factor).max(0.0)).min(Duration::from_secs(60))
}

/// When Gmail was last checked successfully and when the next background
/// check is due, for the "last checked / next check" line (audit F5-6). The
/// dashboard used to show the time its own component mounted instead.
#[derive(Clone, Default, serde::Serialize)]
pub struct PollStatus {
    pub last_checked_at: Option<String>,
    pub next_check_at: Option<String>,
    pub last_check_failed: bool,
    pub paused: bool,
}

fn poll_status() -> &'static std::sync::Mutex<PollStatus> {
    static CELL: std::sync::OnceLock<std::sync::Mutex<PollStatus>> = std::sync::OnceLock::new();
    CELL.get_or_init(|| std::sync::Mutex::new(PollStatus::default()))
}

fn update_poll_status(f: impl FnOnce(&mut PollStatus)) {
    f(&mut poll_status().lock().unwrap_or_else(std::sync::PoisonError::into_inner));
}

/// Records the outcome of a finished check.
pub(crate) fn record_poll_result(ok: bool) {
    update_poll_status(|s| {
        if ok {
            s.last_checked_at = Some(chrono::Utc::now().to_rfc3339());
        }
        s.last_check_failed = !ok;
    });
}

/// Reports when Gmail was last checked and when the next check is due.
#[tauri::command]
pub async fn sync_poll_status() -> Result<PollStatus, crate::error::AppError> {
    let mut status = poll_status().lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    status.paused = crate::commands::debug::GMAIL_POLL_PAUSED.load(std::sync::atomic::Ordering::Relaxed);
    Ok(status)
}

/// The long-running loop that discovers new mail.
///
/// Runs until cancelled at shutdown. Failures back off exponentially with jitter
/// rather than retrying tightly against an API that is already refusing.
pub async fn start_polling_loop<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    pool: Pool,
    cancel_token: CancellationToken,
) {
    tracing::info!("Starting Gmail smart polling loop...");

    loop {
        let interval_secs = app
            .try_state::<crate::lifecycle::launch_agent::PollingIntervalState>()
            .map(|s| s.load_secs())
            .unwrap_or(60);
        let due = chrono::Utc::now() + chrono::Duration::seconds(interval_secs as i64);
        update_poll_status(|s| s.next_check_at = Some(due.to_rfc3339()));
        tokio::select! {
            _ = cancel_token.cancelled() => {
                tracing::info!("Polling loop cancelled.");
                break;
            }
            _ = sleep(Duration::from_secs(interval_secs)) => {
                let paused = crate::commands::debug::GMAIL_POLL_PAUSED.load(std::sync::atomic::Ordering::Relaxed);
                if paused {
                    tracing::info!("Gmail polling is currently paused. Skipping cycle.");
                    continue;
                }

                let pool_clone = pool.clone();
                let app_clone = app.clone();
                let cycle_start = std::time::Instant::now();
                let result = poll_all_accounts(
                    &app_clone,
                    &pool_clone,
                    crate::ingestion::gmail_client::GMAIL_API_BASE_URL,
                )
                .await;
                crate::ingestion::gmail_telemetry::gmail_telemetry()
                    .record_poll_cycle_duration(cycle_start.elapsed());
                record_poll_result(result.is_ok());
                if let Err(e) = result {
                    tracing::error!("Error during polling cycle: {}", e);
                }
            }
        }
    }
}

const FORCE_POLL_DEBOUNCE: std::time::Duration = std::time::Duration::from_secs(10);

/// Timestamp of the last manual poll, for rate limiting.
fn last_force_poll_at() -> &'static std::sync::Mutex<Option<std::time::Instant>> {
    static CELL: std::sync::OnceLock<std::sync::Mutex<Option<std::time::Instant>>> =
        std::sync::OnceLock::new();
    CELL.get_or_init(|| std::sync::Mutex::new(None))
}

/// Whether a manual poll may run, given how recently one did.
///
/// Rate-limited because the button is user-facing and repeated presses would
/// otherwise consume API quota to no benefit.
pub(crate) fn is_force_poll_allowed(
    now: std::time::Instant,
    last: Option<std::time::Instant>,
) -> bool {
    match last {
        None => true,
        Some(last) => now.duration_since(last) >= FORCE_POLL_DEBOUNCE,
    }
}

#[tauri::command]
/// Triggers an immediate poll, subject to the rate limit.
pub async fn sync_force_poll_now<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    pool: tauri::State<'_, Pool>,
) -> Result<String, crate::error::AppError> {
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    crate::security::incident_response::assert_ingestion_allowed()?;

    let now = std::time::Instant::now();
    {
        let mut last = last_force_poll_at().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !is_force_poll_allowed(now, *last) {
            return Err(crate::error::AppError::Validation(
                "Sync Now was used too recently -- please wait a few seconds and try again"
                    .to_string(),
            ));
        }
        *last = Some(now);
    }

    let pool = pool.inner().clone();
    let result = poll_all_accounts(
        &app,
        &pool,
        crate::ingestion::gmail_client::GMAIL_API_BASE_URL,
    )
    .await;
    record_poll_result(result.is_ok());
    result.map_err(|e| crate::error::AppError::Network(e.to_string()))?;

    Ok("synced".to_string())
}

/// Polls every connected account.
///
/// A failure on one account is isolated, so one broken connection does not stop
/// the others syncing.
pub(crate) async fn poll_all_accounts<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &Pool,
    base_url: &str,
) -> anyhow::Result<()> {
    // A lapsed licence stops new ingestion (reads stay available). The
    // background poller never checked this before, so only manual actions
    // were actually blocked.
    if let Err(e) = crate::licensing::gate::assert_write_allowed(pool).await {
        tracing::info!("Gmail polling skipped: {}", e);
        return Ok(());
    }
    if let Err(e) = crate::security::incident_response::assert_ingestion_allowed() {
        tracing::warn!("Gmail polling skipped: {}", e);
        return Ok(());
    }

    let accounts: Vec<ConnectedAccountsRow> = {
        let conn = pool.get().await?;
        conn.interact(|c| connected_accounts::get_all_accounts(c))
            .await
            .map_err(|e| anyhow::anyhow!("Interact error: {}", e))??
    };

    let active_accounts = accounts.into_iter().filter(|acc| {
        acc.account_status.as_deref().unwrap_or("") == "ACTIVE"
            || acc.account_status.as_deref().unwrap_or("") == "active"
    });

    for account in active_accounts {
        run_pending_catch_up(app, pool, &account.id, base_url).await;
        if let Err(e) = poll_single_account(app, pool, &account, base_url).await {
            tracing::error!("Failed to poll account {}: {}", account.id, e);
        }
    }

    Ok(())
}

/// Polls one account incrementally from its stored history id.
pub(crate) async fn poll_single_account<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &Pool,
    account: &ConnectedAccountsRow,
    base_url: &str,
) -> anyhow::Result<()> {
    let token = match get_valid_access_token(app, pool, &account.id).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("Failed to get token for account {}: {}", account.id, e);
            return Ok(());
        }
    };

    let mut current_history_id = None;
    if let Ok(conn) = pool.get().await {
        let acc_id = account.id.clone();
        if let Ok(Some(checkpoint)) = conn
            .interact(move |c| processing_checkpoints::get_checkpoint(c, "gmail_history", &acc_id))
            .await
            .unwrap_or(Ok(None))
        {
            current_history_id = checkpoint.last_processed_token;
        }
    }

    if current_history_id.is_none() {
        current_history_id = account.last_history_id.clone();
    }

    let start_history_id = match current_history_id {
        Some(ref id) => id,
        None => {
            tracing::warn!(
                "No history ID found for account {}, skipping until full sync completes.",
                account.id
            );
            return Ok(());
        }
    };

    let current_history_id_str = start_history_id.clone();
    let mut page_token: Option<String> = None;
    let mut latest_history_id_received = None;
    let mut token = token;
    let mut refreshed_after_401 = false;

    let refresher = crate::ingestion::oauth::create_token_refresher(app, pool, &account.id);
    let mut gmail_client = crate::ingestion::gmail_client::GmailClient::new_with_base_url(
        token.clone(),
        pool.clone(),
        base_url.to_string(),
        refresher.clone(),
    );
    let network = NetworkClient::new(pool.clone());

    let app_dir = app.path().app_data_dir().ok();
    let layer6_tx = app
        .state::<crate::ingestion::queues::QueueHandles>()
        .layer6_tx
        .clone();

    loop {
        let mut url = format!(
            "{}/gmail/v1/users/me/history?startHistoryId={}",
            base_url, current_history_id_str
        );
        if let Some(token) = &page_token {
            url.push_str(&format!("&pageToken={}", token));
        }

        let mut retry_count = 0;
        let max_retries = 8;
        let mut backoff = Duration::from_secs(1);

        let response = loop {
            let builder = network.client().get(&url).bearer_auth(&token);
            // Shares the scan's Gmail budget (one gate for every caller).
            let resp = {
                let _permit = gmail_gate().acquire(quota_cost::HISTORY_LIST).await;
                network.execute("gmail_api", builder).await
            };

            match resp {
                Ok(res) => {
                    let status = res.status();
                    if status.is_success() {
                        gmail_gate().on_success().await;
                        break Ok::<reqwest::Response, anyhow::Error>(res);
                    } else if status == StatusCode::NOT_FOUND || status == StatusCode::BAD_REQUEST {
                        tracing::warn!(
                            "History ID too old or invalid for account {}. Code: {}",
                            account.id,
                            status
                        );
                        recover_from_expired_history(app, pool, &account.id, base_url, &token)
                            .await?;
                        return Ok(());
                    } else if status == StatusCode::UNAUTHORIZED && !refreshed_after_401 {
                        refreshed_after_401 = true;
                        tracing::warn!(
                            "Gmail API returned 401 for account {} — forcing token refresh and retrying once",
                            account.id
                        );
                        match crate::ingestion::oauth::force_refresh_access_token(
                            app,
                            pool,
                            &account.id,
                        )
                        .await
                        {
                            Ok(new_token) => {
                                token = new_token;
                                gmail_client =
                                    crate::ingestion::gmail_client::GmailClient::new_with_base_url(
                                        token.clone(),
                                        pool.clone(),
                                        base_url.to_string(),
                                        refresher.clone(),
                                    );
                            }
                            Err(e) => {
                                return Err(anyhow::anyhow!(
                                    "401 from Gmail and refresh failed for account {}: {}",
                                    account.id,
                                    e
                                ));
                            }
                        }
                    } else if let Some(rate_limited) = retryable_failure(res).await {
                        if rate_limited {
                            crate::ingestion::gmail_telemetry::gmail_telemetry()
                                .record_quota_exhausted();
                        } else {
                            crate::ingestion::gmail_telemetry::gmail_telemetry()
                                .record_5xx(status.as_u16());
                        }
                        if retry_count >= max_retries {
                            if rate_limited {
                                crate::ipc::system_warnings::emit_system_warning(
                                    app,
                                    crate::ipc::system_warnings::SystemWarningPayload {
                                        warning_type: "gmail_quota_exhausted".to_string(),
                                        message:
                                            "Gmail is temporarily rate-limiting this account. \
                                        New email sync is paused and will resume automatically."
                                                .to_string(),
                                        severity:
                                            crate::ipc::system_warnings::WarningSeverity::Degraded,
                                        action_hint: None,
                                    },
                                );
                            }
                            return Err(anyhow::anyhow!(
                                "Max retries reached for polling account {}",
                                account.id
                            ));
                        }
                        retry_count += 1;
                        tracing::warn!(
                            "Rate limited or server error, retrying in {}s...",
                            backoff.as_secs()
                        );
                        sleep(jittered(backoff)).await;
                        backoff = next_backoff(backoff);
                    } else {
                        return Err(anyhow::anyhow!("Unexpected response status: {}", status));
                    }
                }
                Err(e) => {
                    if retry_count >= max_retries {
                        return Err(anyhow::anyhow!("Network error, max retries reached: {}", e));
                    }
                    retry_count += 1;
                    tracing::warn!(
                        "Network error, retrying in {}s... ({})",
                        backoff.as_secs(),
                        e
                    );
                    sleep(jittered(backoff)).await;
                    backoff = next_backoff(backoff);
                }
            }
        }?;

        let data: HistoryResponse = response.json().await?;

        if let Some(history_records) = &data.history {
            tracing::info!(
                target: "ingestion_extraction",
                "\n=== Starting Sync for {:?} ===\nFound {} history records.",
                account.email_address,
                history_records.len()
            );
        }

        if let Some(history_records) = data.history {
            for record in history_records {
                if let Some(messages_added) = record.messagesAdded {
                    for added in messages_added {
                        let msg_id = added.message.id;
                        tracing::info!(
                            target: "ingestion_extraction",
                            "→ Discovered new message in inbox: [ID: {}]",
                            msg_id
                        );
                        match crate::ingestion::message_processor::MessageProcessor::process_message(
                            pool,
                            &gmail_client,
                            &msg_id,
                            app_dir.clone(),
                            Some(layer6_tx.clone()),
                            None,
                        ).await {
                            Ok(Some(crate::ingestion::message_processor::ProcessResult::TransactionAlert(extracted, boxed_obs, email_meta))) => {
                                // A job that is not durable would be lost on
                                // quit once the cursor moves past it: stop the
                                // cycle and keep the old cursor (owner decision,
                                // audit N-A2); the next cycle re-reads it.
                                crate::ingestion::queues::dispatch_transaction_alert(
                                    app, pool, &account.id, &msg_id, extracted.text_body, *boxed_obs, email_meta,
                                )
                                .await?;
                            }
                            Ok(Some(crate::ingestion::message_processor::ProcessResult::StatementEmail(extracted, email_meta))) => {
                            if extracted.pdf_attachments.is_empty() {
                                tracing::warn!(
                                    "Realtime poll: StatementEmail msg_id='{}' has no \
                                         downloadable attachment_ids — skipping parse. \
                                         skipped_parts=[{}]",
                                        msg_id,
                                        extracted.skipped_pdf_parts.join("; ")
                                    );
                                // Recorded so it is traceable and rescans skip it
                                // (audit N-F3); a failed write keeps the cursor
                                // (audit N-A2).
                                crate::db::processed_messages::record_statement_without_attachment(pool, &msg_id)
                                    .await?;
                                } else {
                                    crate::ingestion::statement_intake::intake_email_statement(
                                        app,
                                        pool,
                                        &gmail_client,
                                        &account.id,
                                        &msg_id,
                                        &extracted.pdf_attachments,
                                        email_meta.clone(),
                                    )
                                    .await;
                                }
                            }
                            Ok(Some(crate::ingestion::message_processor::ProcessResult::MandateEvent(extracted, mandate_extraction, event_type))) => {
                                // A job that is not durable would be lost on
                                // quit once the cursor moves past it: stop the
                                // cycle and keep the old cursor (owner decision,
                                // audit N-A2); the next cycle re-reads it.
                                crate::ingestion::queues::dispatch_mandate_event(
                                    app, pool, &account.id, &msg_id, extracted.text_body, mandate_extraction, event_type,
                                )
                                .await?;
                            }
                            Ok(None) => {}
                            Ok(Some(crate::ingestion::message_processor::ProcessResult::EnqueuedForEnrichment)) => {
                                tracing::info!("Realtime poll: msg_id='{}' enqueued for background Layer 6 enrichment", msg_id);
                                notify_unassigned_changed(app, &msg_id);
                            }
                            Ok(Some(crate::ingestion::message_processor::ProcessResult::SentToReview)) => {
                                notify_unassigned_changed(app, &msg_id);
                            }
                            Err(e) => {
                                tracing::error!("Failed to process message {} for account {}: {}", msg_id, account.id, e);
                                // The cursor still advances past this message,
                                // so record it for retry ("Retry failed" in
                                // Settings) instead of losing it (review H4).
                                let row = crate::db::scan_failed_messages::ScanFailedMessageRow {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    account_id: account.id.clone(),
                                    msg_id: msg_id.clone(),
                                    error: e.to_string(),
                                    failed_at: None,
                                };
                                // If even that record cannot be written, keep the
                                // old cursor instead (owner decision, audit N-A2).
                                pool.get()
                                    .await
                                    .map_err(|e| anyhow::anyhow!("pool error: {e}"))?
                                    .interact(move |c| crate::db::scan_failed_messages::insert(c, &row))
                                    .await
                                    .map_err(|e| anyhow::anyhow!("interact error: {e}"))?
                                    .map_err(|e| e.context("could not record the failed message; cursor kept"))?;
                            }
                        }
                    }
                }
            }
        }

        if let Some(new_history_id) = data.historyId {
            latest_history_id_received = Some(new_history_id);
        }

        page_token = data.nextPageToken;
        if page_token.is_none() {
            break;
        }
    }

    if let Some(new_history_id) = latest_history_id_received {
        save_history_id(pool, &account.id, new_history_id).await?;
    }
    // A poll that got through means Gmail is no longer throttling us.
    crate::ipc::system_warnings::clear_system_warning(app, "gmail_quota_exhausted");

    Ok(())
}

/// Recovers an account whose stored Gmail history id has expired.
///
/// Fetches Gmail's current history id, re-seeds the cursor so polling resumes
/// from now, then starts a catch-up scan from one day before the cursor was
/// last known good up to today (dedupe drops anything already ingested). If
/// the profile fetch fails nothing is changed, so the next poll retries.
/// Whether a failed history.list response should be retried: `Some(true)`
/// for a Gmail rate limit (429, or 403 rateLimitExceeded /
/// userRateLimitExceeded), `Some(false)` for a 5xx, `None` for anything else.
/// Only a 403 or 429 body is read: that is where the rate-limit reason and
/// Gmail's "Retry after" time are.
/// A rate limit is also reported to the shared gate, so the scan pauses too.
async fn retryable_failure(res: reqwest::Response) -> Option<bool> {
    let status = res.status();
    let headers = res.headers().clone();
    let body = if status == StatusCode::FORBIDDEN || status == StatusCode::TOO_MANY_REQUESTS {
        res.text().await.unwrap_or_default()
    } else {
        String::new()
    };
    if crate::ingestion::gmail_client::is_rate_limited(status, &body) {
        gmail_gate()
            .on_rate_limited(crate::ingestion::gmail_client::retry_after_hint(&headers, &body))
            .await;
        Some(true)
    } else if status.is_server_error() {
        Some(false)
    } else {
        None
    }
}

async fn recover_from_expired_history<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &Pool,
    account_id: &str,
    base_url: &str,
    token: &str,
) -> anyhow::Result<()> {
    let network = NetworkClient::new(pool.clone());
    let builder = network
        .client()
        .get(format!("{}/gmail/v1/users/me/profile", base_url))
        .bearer_auth(token);
    let res = {
        let _permit = gmail_gate().acquire(quota_cost::GET_PROFILE).await;
        network.execute("gmail_api", builder).await?
    };
    if !res.status().is_success() {
        anyhow::bail!("Gmail profile fetch failed with {}", res.status());
    }
    let profile: serde_json::Value = res.json().await?;
    let new_history_id = match &profile["historyId"] {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => anyhow::bail!("Gmail profile response had no historyId"),
    };

    let since = handle_invalid_history_id(pool, account_id, &new_history_id).await?;
    let start = (since - chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
    let end = chrono::Utc::now().date_naive().format("%Y-%m-%d").to_string();
    match crate::ingestion::historical_scan::start_historical_scan(
        app.clone(),
        pool.clone(),
        account_id.to_string(),
        start.clone(),
        end.clone(),
        base_url.to_string(),
    )
    .await
    {
        Ok(_) => tracing::info!(
            "Started catch-up scan {}..{} for account {}",
            start,
            end,
            account_id
        ),
        Err(crate::error::AppError::ScanAlreadyRunning(_)) => {
            // Saved and retried by the poller once the running scan ends,
            // rather than skipped (follow-up to review C2).
            tracing::info!(
                "Catch-up scan for account {} deferred: a scan is already running",
                account_id
            );
            save_pending_catch_up(pool, account_id, &start).await;
        }
        Err(e) => tracing::error!("Could not start catch-up scan for {}: {}", account_id, e),
    }
    Ok(())
}

const PENDING_CATCH_UP: &str = "gmail_catch_up";

/// Remembers a catch-up scan that could not start yet (start date only; it
/// always runs up to today).
async fn save_pending_catch_up(pool: &Pool, account_id: &str, start: &str) {
    let row = ProcessingCheckpointRow {
        id: uuid::Uuid::new_v4().to_string(),
        job_type: PENDING_CATCH_UP.to_string(),
        job_key: account_id.to_string(),
        checkpoint_state_json: serde_json::json!({ "start": start }).to_string(),
        last_processed_token: None,
        status: "pending".to_string(),
        updated_at: None,
    };
    if let Ok(conn) = pool.get().await {
        let _ = conn
            .interact(move |c| processing_checkpoints::upsert_checkpoint(c, &row))
            .await;
    }
}

/// Starts a deferred catch-up scan if one is pending and no scan is running.
async fn run_pending_catch_up<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &Pool,
    account_id: &str,
    base_url: &str,
) {
    let Ok(conn) = pool.get().await else { return };
    let acc = account_id.to_string();
    let pending = conn
        .interact(move |c| processing_checkpoints::get_checkpoint(c, PENDING_CATCH_UP, &acc))
        .await
        .ok()
        .and_then(|r| r.ok())
        .flatten();
    let Some(start) = pending.and_then(|cp| {
        serde_json::from_str::<serde_json::Value>(&cp.checkpoint_state_json)
            .ok()
            .and_then(|v| v["start"].as_str().map(String::from))
    }) else {
        return;
    };
    let end = chrono::Utc::now().date_naive().format("%Y-%m-%d").to_string();
    match crate::ingestion::historical_scan::start_historical_scan(
        app.clone(),
        pool.clone(),
        account_id.to_string(),
        start.clone(),
        end,
        base_url.to_string(),
    )
    .await
    {
        Err(crate::error::AppError::ScanAlreadyRunning(_)) => return,
        Ok(_) => tracing::info!("Started deferred catch-up scan from {} for {}", start, account_id),
        Err(e) => tracing::error!("Deferred catch-up scan for {} failed to start: {}", account_id, e),
    }
    let acc = account_id.to_string();
    let _ = conn
        .interact(move |c| {
            c.execute(
                "DELETE FROM processing_checkpoints WHERE job_type = ?1 AND job_key = ?2",
                rusqlite::params![PENDING_CATCH_UP, acc],
            )
        })
        .await;
}

/// Persists the Gmail history id marking how far this account has been read.
///
/// Written only after messages are successfully processed, so an interrupted poll
/// resumes rather than skipping the mail it never handled.
pub async fn save_history_id(
    pool: &Pool,
    account_id: &str,
    history_id: String,
) -> anyhow::Result<()> {
    let conn = pool.get().await?;
    let checkpoint = ProcessingCheckpointRow {
        id: uuid::Uuid::new_v4().to_string(),
        job_type: "gmail_history".to_string(),
        job_key: account_id.to_string(),
        checkpoint_state_json: "{}".to_string(),
        last_processed_token: Some(history_id.clone()),
        status: "success".to_string(),
        updated_at: None,
    };

    let acc_id = account_id.to_string();
    conn.interact(move |c| {
        // One transaction, and only the cursor column: rewriting the whole
        // account row from an earlier read could revert a concurrent status
        // change, e.g. `degraded` set by a token refresh (audit B-4).
        let tx = c.unchecked_transaction()?;
        processing_checkpoints::upsert_checkpoint(&tx, &checkpoint)?;
        tx.execute(
            "UPDATE connected_accounts SET last_history_id = ?1 WHERE id = ?2",
            rusqlite::params![history_id, acc_id],
        )?;
        tx.commit()?;
        Ok::<(), anyhow::Error>(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("Interact error: {}", e))??;

    Ok(())
}

/// Tells the UI a review entry was filed for this message (audit B-1).
pub(crate) fn notify_unassigned_changed<R: tauri::Runtime>(app: &tauri::AppHandle<R>, msg_id: &str) {
    let _ = crate::ipc::events::emit_event(
        app,
        crate::ipc::events::AppEvent::UnassignedChanged,
        serde_json::json!({ "source_record_id": msg_id }),
    );
}

#[cfg(test)]
mod cursor_mirror_tests {
    use super::*;

    /// Audit B-4: saving the cursor touches only `last_history_id` (with the
    /// checkpoint), so it cannot overwrite a status set by someone else.
    #[tokio::test]
    async fn saving_the_cursor_leaves_account_status_alone() {
        let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = crate::db::init_db(dir.join("t.db")).await.unwrap();
        pool.get()
            .await
            .unwrap()
            .interact(|c| {
                c.execute(
                    "INSERT INTO connected_accounts (id, email_address, account_status, last_history_id)
                     VALUES ('acc', 'me@example.com', 'degraded', '1')",
                    [],
                )
            })
            .await
            .unwrap()
            .unwrap();

        save_history_id(&pool, "acc", "42".into()).await.unwrap();

        let (status, cursor, checkpoint): (String, String, String) = pool
            .get()
            .await
            .unwrap()
            .interact(|c| {
                c.query_row(
                    "SELECT a.account_status, a.last_history_id, p.last_processed_token
                     FROM connected_accounts a JOIN processing_checkpoints p
                       ON p.job_key = a.id AND p.job_type = 'gmail_history'
                     WHERE a.id = 'acc'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!((status.as_str(), cursor.as_str(), checkpoint.as_str()), ("degraded", "42", "42"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
