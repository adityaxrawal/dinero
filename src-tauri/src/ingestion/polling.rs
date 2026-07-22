use deadpool_sqlite::Pool;
use reqwest::StatusCode;
use serde::Deserialize;
use std::time::Duration;
use tauri::Manager;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::db::connected_accounts::{self, ConnectedAccountsRow};
use crate::db::processing_checkpoints::{self, ProcessingCheckpointRow};
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

/// Doubles a backoff duration, capped at 60s (Doc 30 TASK-GMAIL-001: "1s initial, 60s max, jittered").
pub(crate) fn next_backoff(current: Duration) -> Duration {
    current.saturating_mul(2).min(Duration::from_secs(60))
}

/// Applies +/-15% jitter to a backoff duration so concurrent retries don't synchronize.
fn jittered(d: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.subsec_nanos())
        .unwrap_or(0);
    let factor = 0.85 + (nanos as f64 / 1_000_000_000.0) * 0.30; // in [0.85, 1.15]
    Duration::from_secs_f64((d.as_secs_f64() * factor).max(0.0)).min(Duration::from_secs(60))
}

/// Background task that polls Gmail for new history events, normally every
/// 60 seconds. TASK-DESK-010: this interval is throttled to 5 minutes while
/// operating in background-only mode ("continue syncing when closed") on
/// battery power below the configured threshold -- see
/// `lifecycle::launch_agent::PollingIntervalState`, refreshed independently
/// of this loop's own cycle.
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
                // Doc 30 TASK-GMAIL-010: gmail_poll_cycle_duration_ms — timed
                // regardless of success/failure, since a stalled cycle is
                // exactly the case this latency signal exists to catch.
                crate::ingestion::gmail_telemetry::gmail_telemetry()
                    .record_poll_cycle_duration(cycle_start.elapsed());
                if let Err(e) = result {
                    tracing::error!("Error during polling cycle: {}", e);
                }
            }
        }
    }
}

/// Doc 30 TASK-API-009: "a manual 'Sync Now' bypassing the normal 30-90s
/// wait, debounced to at most once per 10 seconds to prevent Gmail-quota
/// abuse." No Document 19 contract exists for this command under any name
/// -- built verbatim under Doc 30's own name, same documentation-gap
/// precedent as several other TASK-API-005 through 008 commands.
const FORCE_POLL_DEBOUNCE: std::time::Duration = std::time::Duration::from_secs(10);

fn last_force_poll_at() -> &'static std::sync::Mutex<Option<std::time::Instant>> {
    static CELL: std::sync::OnceLock<std::sync::Mutex<Option<std::time::Instant>>> =
        std::sync::OnceLock::new();
    CELL.get_or_init(|| std::sync::Mutex::new(None))
}

/// Pure, directly testable: given "now" and the last-poll instant, decides
/// whether a new force-poll is allowed. Split out from the `#[tauri::command]`
/// below (which also needs a live `AppHandle`/`Pool` to actually poll) so
/// `test_force_poll_debounced` doesn't need to fabricate either.
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
pub async fn sync_force_poll_now<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    pool: tauri::State<'_, Pool>,
) -> Result<String, crate::error::AppError> {
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let now = std::time::Instant::now();
    {
        let mut last = last_force_poll_at().lock().unwrap();
        if !is_force_poll_allowed(now, *last) {
            return Err(crate::error::AppError::Validation(
                "Sync Now was used too recently -- please wait a few seconds and try again"
                    .to_string(),
            ));
        }
        *last = Some(now);
    }

    let pool = pool.inner().clone();
    poll_all_accounts(
        &app,
        &pool,
        crate::ingestion::gmail_client::GMAIL_API_BASE_URL,
    )
    .await
    .map_err(|e| crate::error::AppError::Network(e.to_string()))?;

    Ok("synced".to_string())
}

/// Doc 30 TASK-GMAIL-009: iterates every active connected account
/// independently — each account's checkpoint is keyed on its own `id`
/// (`poll_single_account`), and one account's failure (caught here, not
/// propagated) never blocks the next account in the loop from being polled.
pub(crate) async fn poll_all_accounts<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &Pool,
    base_url: &str,
) -> anyhow::Result<()> {
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
        if let Err(e) = poll_single_account(app, pool, &account, base_url).await {
            tracing::error!("Failed to poll account {}: {}", account.id, e);
        }
    }

    Ok(())
}

pub(crate) async fn poll_single_account<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &Pool,
    account: &ConnectedAccountsRow,
    base_url: &str,
) -> anyhow::Result<()> {
    // 1. Fetch valid token
    let token = match get_valid_access_token(app, pool, &account.id).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("Failed to get token for account {}: {}", account.id, e);
            return Ok(());
        }
    };

    // 2. Get last_history_id
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
    // Doc 01 §10.4 (BG-02): the history-poll loop below built its own bare
    // client, invisible to the Network Activity audit trail — routed through
    // NetworkClient like every other Gmail call in this module.
    let network = NetworkClient::new(pool.clone());

    // TASK-TXN-001: resolved once per poll cycle and threaded down into
    // `MessageProcessor::process_message` so Layer 5 (local LLM fallback)
    // can actually run — the previous hardcoded `None` app_dir meant Layer 5
    // never fired in production regardless of RAM eligibility.
    let app_dir = app.path().app_data_dir().ok();
    let llm_eligible = app
        .try_state::<crate::startup::LlmEligibility>()
        .map(|s| s.eligible)
        .unwrap_or(false);

    loop {
        let mut url = format!(
            "{}/gmail/v1/users/me/history?startHistoryId={}",
            base_url, current_history_id_str
        );
        if let Some(token) = &page_token {
            url.push_str(&format!("&pageToken={}", token));
        }

        let mut retry_count = 0;
        // Doc 30 TASK-GMAIL-001: backoff must reach and hold a 60s cap before
        // giving up for this cycle — 8 retries covers 1,2,4,8,16,32,60,60.
        let max_retries = 8;
        let mut backoff = Duration::from_secs(1);

        let response = loop {
            let builder = network.client().get(&url).bearer_auth(&token);
            let resp = network.execute("gmail_api", builder).await;

            match resp {
                Ok(res) => {
                    let status = res.status();
                    if status.is_success() {
                        break Ok::<reqwest::Response, anyhow::Error>(res);
                    } else if status == StatusCode::NOT_FOUND || status == StatusCode::BAD_REQUEST {
                        tracing::warn!(
                            "History ID too old or invalid for account {}. Code: {}",
                            account.id,
                            status
                        );
                        handle_invalid_history_id(pool, &account.id).await?;
                        return Ok(());
                    } else if status == StatusCode::UNAUTHORIZED && !refreshed_after_401 {
                        // TASK-AUTH-004: on HTTP 401, force a refresh and
                        // retry once — the cached token looked valid by our
                        // local expiry math but Google rejected it anyway
                        // (external revocation, clock skew). Only one retry
                        // per cycle: if the freshly refreshed token still
                        // gets a 401, something deeper is wrong and this
                        // should surface as an error, not loop forever.
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
                    } else if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                        // Doc 30 TASK-GMAIL-010: aggregate counts only — no
                        // response body, no account/email content.
                        if status == StatusCode::TOO_MANY_REQUESTS {
                            crate::ingestion::gmail_telemetry::gmail_telemetry()
                                .record_quota_exhausted();
                        } else {
                            crate::ingestion::gmail_telemetry::gmail_telemetry()
                                .record_5xx(status.as_u16());
                        }
                        if retry_count >= max_retries {
                            if status == StatusCode::TOO_MANY_REQUESTS {
                                crate::ipc::system_warnings::emit_system_warning(
                                    app,
                                    crate::ipc::system_warnings::SystemWarningPayload {
                                        warning_type: "gmail_quota_exhausted".to_string(),
                                        message: "Gmail is temporarily rate-limiting this account. \
                                        New email sync is paused and will resume automatically."
                                            .to_string(),
                                        severity: crate::ipc::system_warnings::WarningSeverity::Degraded,
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

        if let Some(history_records) = data.history {
            for record in history_records {
                if let Some(messages_added) = record.messagesAdded {
                    for added in messages_added {
                        let msg_id = added.message.id;
                        match crate::ingestion::message_processor::MessageProcessor::process_message(
                            pool,
                            &gmail_client,
                            &msg_id,
                            app_dir.clone(),
                            llm_eligible,
                        ).await {
                            Ok(Some(crate::ingestion::message_processor::ProcessResult::TransactionAlert(extracted, boxed_obs, html))) => {
                                // Doc 15 §2 principle 7 / Doc 12 §6.2a: route to the Transaction
                                // Queue rather than processing inline — same shared worker logic
                                // as the historical-scan entry point.
                                let job = crate::ingestion::queues::TransactionJob {
                                    obs: *boxed_obs,
                                    // Doc 18's CHECK constraint only allows
                                    // 'gmail_transaction'/'statement_pdf'/'manual'
                                    // -- "realtime_poll" isn't one of them and
                                    // would fail every insert (same class of
                                    // bug already fixed in historical_scan.rs
                                    // at TASK-DB-008: this field is evidence
                                    // origin, not ingestion trigger mechanism,
                                    // and live polling reads the same Gmail
                                    // transaction alerts a historical scan does).
                                    source_pipeline: "gmail_transaction".to_string(),
                                    source_record_id: msg_id.clone(),
                                    connected_account_id: account.id.clone(),
                                    raw_body: extracted.text_body.clone(),
                                    raw_html: html,
                                };
                                let tx = app
                                    .state::<crate::ingestion::queues::QueueHandles>()
                                    .transaction_tx
                                    .clone();
                                if tx.send(job).await.is_err() {
                                    tracing::error!("Transaction Queue closed — dropping job for msg_id='{}'", msg_id);
                                }
                            }
                            Ok(Some(crate::ingestion::message_processor::ProcessResult::StatementEmail(extracted, email_meta))) => {
                            // Doc 15 §2 principle 7 / Doc 12 §7.2: route to the Statement Queue.
                            if extracted.pdf_attachments.is_empty() {
                                tracing::warn!(
                                    "Realtime poll: StatementEmail msg_id='{}' has no \
                                         downloadable attachment_ids — skipping parse. \
                                         skipped_parts=[{}]",
                                        msg_id,
                                        extracted.skipped_pdf_parts.join("; ")
                                    );
                                } else {
                                    for att in &extracted.pdf_attachments {
                                        let filename = &att.filename;
                                        // Prefer bytes Gmail already inlined in the payload
                                        // (`body.data`) — no network round-trip needed, and
                                        // this is exactly the case that used to be silently
                                        // dropped (see `PdfAttachmentMeta::inline_bytes`'s
                                        // doc comment).
                                        let fetch_result: anyhow::Result<Vec<u8>> =
                                            if let Some(bytes) = &att.inline_bytes {
                                                Ok(bytes.clone())
                                            } else if let Some(att_id) = &att.attachment_id {
                                                gmail_client.fetch_attachment(&msg_id, att_id).await
                                            } else {
                                                // push_pdf_attachment never inserts an entry
                                                // with neither source, so this is
                                                // unreachable in practice.
                                                continue;
                                            };
                                        match fetch_result {
                                            Ok(pdf_bytes) => {
                                                // Doc 18 §4.7: the `statements` row must exist in
                                                // `queued` state before parsing begins, regardless
                                                // of entry point — same invariant as manual upload.
                                                let stmt_id = uuid::Uuid::new_v4().to_string();

                                                // TASK-GMAIL: this path previously skipped
                                                // password resolution entirely — an encrypted
                                                // attachment was enqueued with no password and
                                                // died in pdfium with PasswordError. Same choke
                                                // point manual upload uses (see
                                                // `resolve_statement_password`'s doc comment).
                                                let password = match crate::statements::password::resolve_statement_password(
                                                    &stmt_id,
                                                    &pdf_bytes,
                                                    filename,
                                                    &msg_id,
                                                    &pool,
                                                    &app,
                                                    email_meta.clone(),
                                                )
                                                .await
                                                {
                                                    Ok(crate::statements::password::StatementPasswordResolution::Proceed(password)) => password,
                                                    Ok(crate::statements::password::StatementPasswordResolution::PromptCreated) => {
                                                        continue;
                                                    }
                                                    Err(e) => {
                                                        tracing::error!(
                                                            "Password resolution failed for msg_id='{}' file='{}': {}",
                                                            msg_id, filename, e
                                                        );
                                                        continue;
                                                    }
                                                };

                                                if let Ok(conn) = pool.get().await {
                                                    let id = stmt_id.clone();
                                                    let msg_id_for_row = msg_id.clone();
                                                    let _ = conn
                                                        .interact(move |c| {
                                                            crate::db::statements::insert_queued(
                                                                c,
                                                                &id,
                                                                "gmail_email",
                                                                Some(&msg_id_for_row),
                                                                None,
                                                            )
                                                        })
                                                        .await;
                                                }
                                                let job = crate::ingestion::queues::StatementJob {
                                                    bytes: pdf_bytes,
                                                    filename: filename.clone(),
                                                    file_hash: msg_id.clone(),
                                                    stmt_id,
                                                    // Doc 30 TASK-STMT-009: batch progress is a
                                                    // manual-upload-batch concept only.
                                                    batch_progress: None,
                                                    password,
                                                    origin: "email_scan".to_string(),
                                                };
                                                let st_tx = app
                                                    .state::<crate::ingestion::queues::QueueHandles>()
                                                    .statement_tx
                                                    .clone();
                                                if st_tx.send(job).await.is_err() {
                                                    tracing::error!(
                                                        "Statement Queue closed — dropping job for msg_id='{}' file='{}'",
                                                        msg_id, filename
                                                    );
                                                }
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    "Realtime poll: failed to fetch attachment \
                                                     '{}' for msg_id='{}': {}",
                                                    filename, msg_id, e
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(Some(crate::ingestion::message_processor::ProcessResult::MandateEvent(extracted, mandate_extraction, event_type))) => {
                                // docs/superpowers/specs/2026-07-18-mandate-tracking-design.md
                                // §4.2: route to the Mandate Queue, same shared worker logic
                                // as the historical-scan entry point.
                                let job = crate::ingestion::queues::MandateJob {
                                    extraction: mandate_extraction,
                                    event_type,
                                    source_pipeline: "gmail_transaction".to_string(),
                                    source_record_id: msg_id.clone(),
                                    connected_account_id: account.id.clone(),
                                    raw_body: extracted.text_body.clone(),
                                };
                                let mandate_tx = app
                                    .state::<crate::ingestion::queues::QueueHandles>()
                                    .mandate_tx
                                    .clone();
                                if mandate_tx.send(job).await.is_err() {
                                    tracing::error!("Mandate Queue closed — dropping job for msg_id='{}'", msg_id);
                                }
                            }
                            Ok(None) => {}
                            Err(e) => {
                                tracing::error!("Failed to process message {} for account {}: {}", msg_id, account.id, e);
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

    Ok(())
}

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
        processing_checkpoints::upsert_checkpoint(c, &checkpoint)?;
        if let Ok(Some(mut acc)) = connected_accounts::get_account(c, &acc_id) {
            acc.last_history_id = Some(history_id);
            let _ = connected_accounts::update_account(c, &acc);
        }
        Ok::<(), anyhow::Error>(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("Interact error: {}", e))??;

    Ok(())
}
