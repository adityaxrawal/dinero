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

/// Background task that polls Gmail for new history events every 60 seconds.
pub async fn start_polling_loop<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    pool: Pool,
    cancel_token: CancellationToken,
) {
    tracing::info!("Starting Gmail smart polling loop...");

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                tracing::info!("Polling loop cancelled.");
                break;
            }
            _ = sleep(Duration::from_secs(60)) => {
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

    let mut gmail_client = crate::ingestion::gmail_client::GmailClient::new_with_base_url(
        token.clone(),
        pool.clone(),
        base_url.to_string(),
    );
    // Doc 01 §10.4 (BG-02): the history-poll loop below built its own bare
    // client, invisible to the Network Activity audit trail — routed through
    // NetworkClient like every other Gmail call in this module.
    let network = NetworkClient::new(pool.clone());

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
            let resp = network.execute(builder).await;

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
                        match crate::ingestion::oauth::force_refresh_access_token(app, pool, &account.id).await {
                            Ok(new_token) => {
                                token = new_token;
                                gmail_client = crate::ingestion::gmail_client::GmailClient::new_with_base_url(
                                    token.clone(),
                                    pool.clone(),
                                    base_url.to_string(),
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
                        ).await {
                            Ok(Some(crate::ingestion::message_processor::ProcessResult::TransactionAlert(_, boxed_obs))) => {
                                // Doc 15 §2 principle 7 / Doc 12 §6.2a: route to the Transaction
                                // Queue rather than processing inline — same shared worker logic
                                // as the historical-scan entry point.
                                let job = crate::ingestion::queues::TransactionJob {
                                    obs: *boxed_obs,
                                    source_pipeline: "realtime_poll".to_string(),
                                    source_record_id: msg_id.clone(),
                                };
                                let tx = app
                                    .state::<crate::ingestion::queues::QueueHandles>()
                                    .transaction_tx
                                    .clone();
                                if tx.send(job).await.is_err() {
                                    tracing::error!("Transaction Queue closed — dropping job for msg_id='{}'", msg_id);
                                }
                            }
                            Ok(Some(crate::ingestion::message_processor::ProcessResult::StatementEmail(extracted))) => {
                                // Doc 15 §2 principle 7 / Doc 12 §7.2: email-detected statements
                                // route onto the same Statement Queue as manual uploads.
                                if extracted.pdf_attachments.is_empty() {
                                    tracing::warn!(
                                        "Realtime poll: StatementEmail msg_id='{}' has no \
                                         downloadable PDF attachment_ids — skipping parse",
                                        msg_id
                                    );
                                } else {
                                    for att in &extracted.pdf_attachments {
                                        let att_id = &att.attachment_id;
                                        let filename = &att.filename;
                                        match gmail_client.fetch_attachment(&msg_id, att_id).await {
                                            Ok(pdf_bytes) => {
                                                // Doc 18 §4.7: the `statements` row must exist in
                                                // `queued` state before parsing begins, regardless
                                                // of entry point — same invariant as manual upload.
                                                let stmt_id = uuid::Uuid::new_v4().to_string();
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
                                                    att_id, msg_id, e
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(_) => {}
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
