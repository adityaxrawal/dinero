use chrono::Utc;
use deadpool_sqlite::Pool;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::db::processing_checkpoints::{
    claim_checkpoint_in_progress, get_checkpoint, upsert_checkpoint, ClaimOutcome,
    ProcessingCheckpointRow,
};
use crate::ingestion::gmail_client::GmailClient;
use crate::ingestion::message_processor::{MessageProcessor, ProcessResult};
use crate::ingestion::oauth::get_valid_access_token;

/// Doc 30 TASK-GMAIL-007: checkpoint every 5 processed messages.
const CHECKPOINT_INTERVAL: usize = 5;

/// Doc 19 §18 Scans group / Doc 30 TASK-API-009: `scans_cancel`. Keyed by
/// `account_id` since up to 10 accounts (Doc 03 §8.2) can have concurrent
/// scans. Checked once per checkpoint interval in `scans_historical`'s main
/// loop (the same cadence `wait_while_paused` already uses) -- didn't exist
/// at all before this task; a scan could previously only run to completion
/// or fail, never be stopped mid-flight.
fn cancelled_scans() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static CELL: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    CELL.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

fn is_scan_cancelled(account_id: &str) -> bool {
    cancelled_scans().lock().unwrap().contains(account_id)
}

fn clear_scan_cancellation(account_id: &str) {
    cancelled_scans().lock().unwrap().remove(account_id);
}

/// Blocks while the scan queue is paused. Returns `true` if a cancellation
/// arrived for `account_id` while waiting.
///
/// audit_01 #4: this used to poll `SCAN_QUEUE_PAUSED` alone. The report
/// called that a 5s "zombie window", but at the prefill call site it is
/// unbounded — that loop runs *before* the main loop's 1s `cancel_poll`
/// ticker exists, so pausing the queue and then cancelling left the scan
/// spinning here forever, with Cancel doing nothing until someone
/// un-paused. Returning on cancellation is what bounds it.
///
/// Deliberately does not call `clear_scan_cancellation` — it reports, and
/// the caller's existing cancellation path does the clearing, emits the
/// event, and sets `was_cancelled`. One place owns that sequence.
async fn wait_while_paused(account_id: &str) -> bool {
    loop {
        let paused = crate::commands::debug::SCAN_QUEUE_PAUSED
            .load(std::sync::atomic::Ordering::Relaxed);
        if !paused {
            return false;
        }
        if is_scan_cancelled(account_id) {
            return true;
        }
        // 1s, matching the main loop's `cancel_poll` ticker, so a paused scan
        // cancels on the same bound as a running one.
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

#[tauri::command]
pub async fn scans_cancel(account_id: String) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_account_id("account_id", &account_id)?;
    cancelled_scans().lock().unwrap().insert(account_id);
    Ok("cancel_requested".to_string())
}

/// Doc 19 §18 Scans group / Doc 30 TASK-API-009's `sync_get_scan_status`.
/// Named acceptance test: `test_scan_status_reflects_checkpoint_state`.
#[derive(Serialize)]
pub struct ScanStatusResponse {
    pub status: String,
    pub processed: usize,
    pub total: usize,
    pub transactions_found: usize,
    pub statements_found: usize,
    pub mandate_events_found: usize,
    pub errors: usize,
    pub pending_enrichment: usize,
}

fn checkpoint_to_status(checkpoint: Option<ProcessingCheckpointRow>) -> ScanStatusResponse {
    match checkpoint {
        None => ScanStatusResponse {
            status: "not_started".to_string(),
            processed: 0,
            total: 0,
            transactions_found: 0,
            statements_found: 0,
            mandate_events_found: 0,
            errors: 0,
            pending_enrichment: 0,
        },
        Some(cp) => {
            let state: ScanCheckpointState =
                serde_json::from_str(&cp.checkpoint_state_json).unwrap_or_default();
            ScanStatusResponse {
                status: cp.status,
                processed: state.processed_count,
                total: state.all_message_ids.len(),
                transactions_found: state.transactions_found,
                statements_found: state.statements_found,
                mandate_events_found: state.mandate_events_found,
                errors: state.errors,
                pending_enrichment: state.pending_enrichment,
            }
        }
    }
}

#[tauri::command]
pub async fn scans_status(
    account_id: String,
    pool: State<'_, Pool>,
) -> Result<ScanStatusResponse, crate::error::AppError> {
    crate::ipc::validation::validate_account_id("account_id", &account_id)?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let checkpoint = conn
        .interact(move |c| get_checkpoint(c, "historical_scan", &account_id))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    Ok(checkpoint_to_status(checkpoint))
}

/// Doc 19 §18 Scans group's `scans_resume`. `scans_historical`'s own
/// checkpoint-resume logic (a `"paused"`/`"failed"`/`"cancelled"` checkpoint
/// picks up from `state.all_message_ids`/`state.processed_count` rather than
/// re-querying Gmail) already makes re-invoking it with the same date range
/// a true resume, not a restart -- so this only needs to recover that
/// original `start_date`/`end_date` from the stored checkpoint before
/// delegating, rather than duplicating `scans_historical`'s own body.
#[tauri::command]
pub async fn scans_resume<R: tauri::Runtime>(
    app: AppHandle<R>,
    pool: State<'_, Pool>,
    account_id: String,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_account_id("account_id", &account_id)?;
    let account_id_clone = account_id.clone();
    let checkpoint = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .interact(move |c| get_checkpoint(c, "historical_scan", &account_id_clone))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let cp = checkpoint.ok_or_else(|| {
        crate::error::AppError::Validation(
            "no prior scan found for this account to resume".to_string(),
        )
    })?;
    let state: ScanCheckpointState = serde_json::from_str(&cp.checkpoint_state_json)
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    scans_historical(app, pool, account_id, state.start_date, state.end_date).await
}

fn should_checkpoint(batch_count: usize) -> bool {
    batch_count >= CHECKPOINT_INTERVAL
}

/// How many `process_message` tasks stay in flight at once.
///
/// This is no longer the throughput governor it once looked like: every
/// `users.messages.get` costs 5 Gmail quota units against
/// `gmail_quota_limiter()`'s 225 units/sec refill and 30-unit burst ceiling,
/// so the limiter — not this constant — sets the real fetch rate (~45
/// messages/sec). Raising it further just deepens the queue behind that
/// limiter. It stays at 25 to keep enough work pipelined that extraction and
/// DB writes overlap network waits.
///
/// Earlier revisions swung this between 50 and 12 trying to tame what looked
/// like connection contention; that was the `dev_review` runtime freeze
/// (see `NetworkClient::with_timeout`), not concurrency.
const MAX_CONCURRENT_FETCHES: usize = 25;

/// Longest `from:` clause we'll put in one Gmail `q` parameter, in chars.
/// The query is percent-encoded into a GET URL, so the encoded form runs
/// roughly 1.5x this; 1500 keeps a chunk comfortably inside Google's URL
/// limit even in the worst case, and the ~204 bundled domains split into
/// three requests.
const MAX_SENDER_CLAUSE_CHARS: usize = 1500;

/// Builds one or more Gmail search queries covering `domains`, each scoped to
/// `date_range`. Returns the bare `date_range` when `domains` is empty, so a
/// registry that somehow failed to load degrades to the old scan-everything
/// behavior rather than silently matching nothing.
fn build_sender_scoped_queries(date_range: &str, domains: &[String]) -> Vec<String> {
    if domains.is_empty() {
        return vec![date_range.to_string()];
    }

    let mut queries = Vec::new();
    let mut chunk: Vec<&str> = Vec::new();
    let mut chunk_len = 0usize;

    for domain in domains {
        // +9 for the "from:" prefix and the " OR " separator.
        let cost = domain.len() + 9;
        if !chunk.is_empty() && chunk_len + cost > MAX_SENDER_CLAUSE_CHARS {
            queries.push(format_sender_query(date_range, &chunk));
            chunk.clear();
            chunk_len = 0;
        }
        chunk.push(domain);
        chunk_len += cost;
    }
    if !chunk.is_empty() {
        queries.push(format_sender_query(date_range, &chunk));
    }
    queries.push(build_rescue_subject_query(date_range));
    queries
}

fn format_sender_query(date_range: &str, domains: &[&str]) -> String {
    let clause = domains
        .iter()
        .map(|d| format!("from:{d}"))
        .collect::<Vec<_>>()
        .join(" OR ");
    format!("({clause}) {date_range}")
}

/// The `from:` prefilter alone is NOT equivalent to Gate 1.
///
/// Gate 1 has a third acceptance path beyond "registry domain" and "approved
/// domain": a sender it would otherwise reject is promoted to
/// `VerifiedTransactionCandidate("Unknown Bank")` when the domain has a prior
/// `sender_reputation` sighting *and* its subject classifies as a transaction
/// or balance update. That is how a bank missing from the bundled registry
/// gets picked up at all. Filtering purely on `from:` would never fetch those
/// messages, so this adds a subject-scoped query alongside the sender ones.
///
/// Deliberately not restricted to previously-seen domains: that set lives in
/// the DB and can run to thousands of entries (every marketing sender ever
/// scanned), which would blow the query budget for no benefit. The subject
/// terms are the narrow half of the rescue condition; the local gate still
/// applies the `domain_previously_seen` half to whatever comes back.
fn build_rescue_subject_query(date_range: &str) -> String {
    let clause = crate::ingestion::content_classifier::RESCUE_SUBJECT_TERMS
        .iter()
        .map(|t| format!("subject:({t})"))
        .collect::<Vec<_>>()
        .join(" OR ");
    format!("({clause}) {date_range}")
}

/// Result of `audit_scan_coverage` — proof (or disproof) that the
/// server-side prefilter drops nothing Gate 1 would have accepted.
#[derive(Debug, Serialize)]
pub struct ScanCoverageAudit {
    /// Messages in the date range with no sender/subject filter at all.
    pub unfiltered_total: usize,
    /// Messages the real scan's filtered query set returns.
    pub filtered_total: usize,
    /// `unfiltered - filtered`: what the prefilter skipped.
    pub excluded_total: usize,
    /// Of those, how many were actually checked against Gate 1.
    pub excluded_checked: usize,
    /// **The number that matters.** Excluded messages that Gate 1 *would*
    /// have accepted. Anything above zero is mail the prefilter is losing.
    pub missed_total: usize,
    /// Up to 50 missed messages, as `msg_id | sender | subject`, so a
    /// non-zero `missed_total` can be investigated rather than just reported.
    pub missed_samples: Vec<String>,
}

/// Answers "how do I know the fast scan isn't missing real transactions?"
/// empirically, against the caller's actual mailbox.
///
/// Runs the old unfiltered date-range search AND the current filtered query
/// set, diffs the two ID sets, then fetches metadata for every excluded ID and
/// runs the real `evaluate_metadata_gate` over it — the same function the scan
/// uses. `missed_total == 0` means the prefilter provably lost nothing for
/// this mailbox and date range.
///
/// Deliberately expensive: it does the whole unfiltered metadata sweep the
/// optimisation exists to avoid. It is a verification tool, not part of a
/// scan.
pub async fn audit_scan_coverage(
    pool: &Pool,
    client: &GmailClient,
    start_date: &str,
    end_date: &str,
) -> anyhow::Result<ScanCoverageAudit> {
    let parsed_end = chrono::NaiveDate::parse_from_str(end_date, "%Y-%m-%d")
        .unwrap_or_else(|_| chrono::Utc::now().naive_utc().date());
    let inclusive_end = parsed_end + chrono::Duration::days(1);
    let date_range = format!(
        "after:{} before:{}",
        start_date,
        inclusive_end.format("%Y-%m-%d")
    );

    // Same domain set the real scan builds.
    let mut sender_domains =
        crate::ingestion::message_processor::get_sender_validator().registry_domains();
    if let Ok(conn) = pool.get().await {
        if let Ok(Ok(rows)) = conn
            .interact(|c| crate::db::sender_reputation::select_approved_domains(c))
            .await
        {
            sender_domains.extend(rows.into_iter().map(|r| r.domain.to_lowercase()));
        }
    }
    sender_domains.sort();
    sender_domains.dedup();

    let filtered: std::collections::HashSet<String> = {
        let mut acc = std::collections::HashSet::new();
        for q in build_sender_scoped_queries(&date_range, &sender_domains) {
            acc.extend(client.search_messages(&q, |_| {}).await?);
        }
        acc
    };

    let unfiltered = client.search_messages(&date_range, |_| {}).await?;
    let unfiltered_total = unfiltered.len();

    let excluded: Vec<String> = unfiltered
        .into_iter()
        .filter(|id| !filtered.contains(id))
        .collect();
    let excluded_total = excluded.len();

    let mut excluded_checked = 0usize;
    let mut missed_total = 0usize;
    let mut missed_samples = Vec::new();

    for msg_id in &excluded {
        let Ok(msg) = client.fetch_message(msg_id, crate::ingestion::gmail_client::FetchFormat::Metadata).await else {
            continue;
        };
        excluded_checked += 1;

        // Gate 1 reads reputation/approved state exactly this way.
        let domain = crate::ingestion::message_processor::MessageProcessor::extract_sender_domain(&msg);
        let (approved, overrides) = match (&domain, pool.get().await) {
            (Some(_), Ok(conn)) => conn
                .interact(move |c| {
                    (
                        crate::db::sender_reputation::select_approved_domains(c)
                            .unwrap_or_default(),
                        crate::db::sender_bank_overrides::select_active(c).unwrap_or_default(),
                    )
                })
                .await
                .unwrap_or((Vec::new(), Vec::new())),
            _ => (Vec::new(), Vec::new()),
        };

        let verdict = crate::ingestion::message_processor::MessageProcessor::evaluate_metadata_gate(
            &msg, &approved, &overrides,
        );
        if matches!(
            verdict,
            crate::ingestion::verified_senders::SenderVerificationResult::VerifiedTransactionCandidate(_)
                | crate::ingestion::verified_senders::SenderVerificationResult::VerifiedStatementCandidate(_)
        ) {
            missed_total += 1;
            if missed_samples.len() < 50 {
                missed_samples.push(format!(
                    "{} | {} | {}",
                    msg_id,
                    domain.unwrap_or_else(|| "?".into()),
                    crate::ingestion::message_processor::MessageProcessor::header_value(
                        &msg, "subject"
                    )
                ));
            }
        }
    }

    Ok(ScanCoverageAudit {
        unfiltered_total,
        filtered_total: filtered.len(),
        excluded_total,
        excluded_checked,
        missed_total,
        missed_samples,
    })
}

#[derive(Clone, Serialize)]
struct ScanProgressPayload {
    account_id: String,
    processed: usize,
    total: usize,
    transactions_found: usize,
    statements_found: usize,
    mandate_events_found: usize,
    non_financial: usize,
    errors: usize,
    pending_enrichment: usize,
    error_message: Option<String>,
}

/// `pub`/pub fields for the same reason as `run_scan_batches` above --
/// `tests/historical_scan_benchmark.rs` (Doc 30 TASK-QA-002) constructs this
/// directly to drive a scan against a pre-built synthetic message-id list.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ScanCheckpointState {
    pub start_date: String,
    pub end_date: String,
    pub all_message_ids: Vec<String>,
    pub processed_count: usize,
    #[serde(default)]
    pub transactions_found: usize,
    #[serde(default)]
    pub statements_found: usize,
    #[serde(default)]
    pub mandate_events_found: usize,
    #[serde(default)]
    pub non_financial: usize,
    #[serde(default)]
    pub errors: usize,
    #[serde(default)]
    pub pending_enrichment: usize,
}

#[tauri::command]
pub async fn scans_historical<R: tauri::Runtime>(
    app: AppHandle<R>,
    pool: State<'_, Pool>,
    account_id: String,
    start_date: String,
    end_date: String,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_account_id("account_id", &account_id)?;
    crate::ipc::validation::validate_date_range(&start_date, &end_date)?;
    // Doc 22 §11.5: LOCKED blocks "all writes, including new ingestion on both
    // queues" — a historical scan is new ingestion.
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let pool = pool.inner().clone();

    let access_token = get_valid_access_token(&app, &pool, &account_id)
        .await
        .map_err(|e| crate::error::AppError::Auth(e.to_string()))?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let parsed_start =
        chrono::NaiveDate::parse_from_str(&start_date, "%Y-%m-%d").map_err(|_| {
            crate::error::AppError::Unknown("Invalid start_date format. Use YYYY-MM-DD".into())
        })?;
    let parsed_end = chrono::NaiveDate::parse_from_str(&end_date, "%Y-%m-%d").map_err(|_| {
        crate::error::AppError::Unknown("Invalid end_date format. Use YYYY-MM-DD".into())
    })?;

    if parsed_start > parsed_end {
        return Err(crate::error::AppError::Unknown(
            "start_date cannot be after end_date".into(),
        ));
    }

    // Doc 30 TASK-API-009 / Document 19: "scans_historical -- dedupe active
    // scans via SCAN_ALREADY_RUNNING." Atomically checks-and-claims the
    // in_progress slot in one statement (see `claim_checkpoint_in_progress`)
    // rather than a separate read-then-later-write -- the old version only
    // wrote `status = 'in_progress'` deep inside `run_scan`, after an
    // awaited Gmail network round-trip, leaving a wide window in which two
    // near-simultaneous calls for the same account could both pass a
    // read-only check and both start scanning.
    let existing = conn
        .interact({
            let account_id_clone = account_id.clone();
            move |c| claim_checkpoint_in_progress(c, "historical_scan", &account_id_clone)
        })
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let existing = match existing {
        ClaimOutcome::AlreadyInProgress => {
            return Err(crate::error::AppError::ScanAlreadyRunning(
                "A historical scan is already in progress for this account".into(),
            ));
        }
        ClaimOutcome::Claimed(existing) => existing,
    };

    // TASK-DESK-003: register with the global background-task indicator
    // before the scan starts running. `account_id` is a stable task id here
    // -- Doc 19 §3.6 already guarantees only one active scan per account.
    if let Some(registry) =
        app.try_state::<crate::background_tasks::indicator::BackgroundTaskRegistry>()
    {
        registry.register_or_update(
            &app,
            &account_id,
            "historical_scan",
            &format!("Historical scan: {}", account_id),
            0,
            0,
            "Starting historical scan…",
        );
    }

    tokio::spawn(async move {
        let scan_result = run_scan(
            app.clone(),
            pool.clone(),
            account_id.clone(),
            start_date,
            end_date,
            access_token,
            existing,
        )
        .await;

        // TASK-DESK-003: deregister on completion, success or failure --
        // this is the signal the frontend removes the task on, not an
        // inferred `current == total`.
        if let Some(registry) =
            app.try_state::<crate::background_tasks::indicator::BackgroundTaskRegistry>()
        {
            use crate::background_tasks::indicator::TaskStatus;
            match &scan_result {
                Ok(_) => registry.deregister(
                    &app,
                    &account_id,
                    TaskStatus::Completed,
                    "Historical scan completed",
                ),
                Err(e) => {
                    registry.deregister(&app, &account_id, TaskStatus::Failed, &e.to_string())
                }
            }
        }

        if let Err(e) = scan_result {
            tracing::error!("Historical scan failed: {}", e);

            // Mark checkpoint as failed
            if let Ok(conn) = pool.get().await {
                let _ = conn
                    .interact({
                        let acc_id = account_id.clone();
                        move |c| {
                            if let Ok(Some(mut cp)) = get_checkpoint(c, "historical_scan", &acc_id)
                            {
                                cp.status = "failed".to_string();
                                cp.updated_at = Some(chrono::Utc::now().naive_utc());
                                let _ = upsert_checkpoint(c, &cp);
                            }
                        }
                    })
                    .await;
            }

            let _ = crate::ipc::events::emit_event(
                &app,
                crate::ipc::events::AppEvent::ScanFailed,
                ScanProgressPayload {
                    account_id,
                    processed: 0,
                    total: 0,
                    transactions_found: 0,
                    statements_found: 0,
                    mandate_events_found: 0,
                    non_financial: 0,
                    errors: 1,
                    pending_enrichment: 0,
                    error_message: Some(e.to_string()),
                },
            );
        }
    });

    Ok("Scan started".to_string())
}

async fn run_scan<R: tauri::Runtime>(
    app: AppHandle<R>,
    pool: Pool,
    account_id: String,
    start_date: String,
    end_date: String,
    access_token: String,
    existing_checkpoint: Option<ProcessingCheckpointRow>,
) -> anyhow::Result<()> {
    // Doc 19 §18 Scans group: `scans_resume`/a fresh `scans_historical` call
    // for this account must not inherit a stale cancellation from a
    // previous, already-finished run.
    clear_scan_cancellation(&account_id);

    let refresher = crate::ingestion::oauth::create_token_refresher(&app, &pool, &account_id);
    let client = GmailClient::new(access_token, pool.clone(), refresher);

    let mut state = if let Some(cp) = existing_checkpoint {
        if cp.status == "paused" || cp.status == "failed" || cp.status == "cancelled" {
            serde_json::from_str::<ScanCheckpointState>(&cp.checkpoint_state_json).unwrap_or_else(
                |_| ScanCheckpointState {
                    start_date: start_date.clone(),
                    end_date: end_date.clone(),
                    ..Default::default()
                },
            )
        } else {
            ScanCheckpointState {
                start_date: start_date.clone(),
                end_date: end_date.clone(),
                ..Default::default()
            }
        }
    } else {
        ScanCheckpointState {
            start_date: start_date.clone(),
            end_date: end_date.clone(),
            ..Default::default()
        }
    };

    if state.all_message_ids.is_empty() {
        // Gmail's `before:` operator is exclusive. To make `end_date` inclusive, we add 1 day.
        let parsed_end = chrono::NaiveDate::parse_from_str(&end_date, "%Y-%m-%d")
            .unwrap_or_else(|_| chrono::Utc::now().naive_utc().date());
        let inclusive_end = parsed_end + chrono::Duration::days(1);

        let date_range = format!(
            "after:{} before:{}",
            start_date,
            inclusive_end.format("%Y-%m-%d")
        );

        // Push Gate 1 server-side. Previously every message in the date range
        // was fetched at `format=metadata` just to read its From header, and
        // ~80% were then dropped as "Unknown sender domain" -- and Gmail
        // charges the same 5 quota units for a metadata fetch as a full one,
        // so that rejected majority was the single largest fixed cost in the
        // scan. Asking Gmail for `from:` the known-sender set instead means we
        // only pay for mail that can possibly matter.
        let mut sender_domains =
            crate::ingestion::message_processor::get_sender_validator().registry_domains();
        // User-approved domains live in the DB, not the bundled registry, and
        // Gate 1 honours them -- so the filter must too, or approving a sender
        // would silently stop working for historical scans.
        if let Ok(conn) = pool.get().await {
            if let Ok(approved) = conn
                .interact(|c| crate::db::sender_reputation::select_approved_domains(c))
                .await
            {
                if let Ok(rows) = approved {
                    sender_domains.extend(rows.into_iter().map(|r| r.domain.to_lowercase()));
                }
            }
        }
        sender_domains.sort();
        sender_domains.dedup();

        let queries = build_sender_scoped_queries(&date_range, &sender_domains);
        tracing::info!(
            domains = sender_domains.len(),
            queries = queries.len(),
            "Scoping scan to known sender domains"
        );
        // The search/pagination phase (`search_messages`) is one long
        // `await` from here -- for a wide date range on a large mailbox it
        // can take many sequential page fetches before returning at all,
        // and `run_scan_batches` doesn't emit its first `scan_progress`
        // until this whole phase is done and the real `total` is known.
        // Without this, the UI's counters sit frozen at "0 / 0" for however
        // long the search takes, indistinguishable from a genuine hang.
        // `on_page` fires after every page with the running count found so
        // far, so at minimum the numbers visibly move.
        // Domain chunks are searched sequentially and their results unioned.
        // `carried` keeps the UI's running total monotonic across chunks --
        // each `search_messages` call reports a count starting from zero.
        let mut ids: Vec<String> = Vec::new();
        let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for query in &queries {
            let account_id_for_search_progress = account_id.clone();
            let app_for_search_progress = app.clone();
            let carried = ids.len();
            tracing::info!("Starting search_messages with query: {}", query);
            let chunk = client
                .search_messages(query, move |found_so_far| {
                    let running = carried + found_so_far;
                    tracing::info!("search_messages found so far: {}", running);
                    let _ = crate::ipc::events::emit_event(
                        &app_for_search_progress,
                        crate::ipc::events::AppEvent::ScanProgress,
                        ScanProgressPayload {
                            account_id: account_id_for_search_progress.clone(),
                            processed: 0,
                            total: running,
                            transactions_found: 0,
                            statements_found: 0,
                            mandate_events_found: 0,
                            non_financial: 0,
                            errors: 0,
                            pending_enrichment: 0,
                            error_message: None,
                        },
                    );
                })
                .await?;
            // A message from a sender matching two chunks would otherwise be
            // processed (and counted) twice.
            for id in chunk {
                if seen_ids.insert(id.clone()) {
                    ids.push(id);
                }
            }
        }
        tracing::info!("search_messages completed. Total messages found: {}", ids.len());
        state.all_message_ids = ids;
        state.processed_count = 0;

        let initial_cp = ProcessingCheckpointRow {
            id: Uuid::new_v4().to_string(),
            job_type: "historical_scan".to_string(),
            job_key: account_id.clone(),
            checkpoint_state_json: serde_json::to_string(&state)?,
            last_processed_token: None,
            status: "in_progress".to_string(),
            updated_at: Some(Utc::now().naive_utc()),
        };

        let conn = pool.get().await?;
        conn.interact(move |c| upsert_checkpoint(c, &initial_cp))
            .await
            .map_err(|e| anyhow::anyhow!("Interact error: {}", e))??;
    } else {
        let cp = ProcessingCheckpointRow {
            id: Uuid::new_v4().to_string(),
            job_type: "historical_scan".to_string(),
            job_key: account_id.clone(),
            checkpoint_state_json: serde_json::to_string(&state)?,
            last_processed_token: None,
            status: "in_progress".to_string(),
            updated_at: Some(Utc::now().naive_utc()),
        };

        let conn = pool.get().await?;
        conn.interact(move |c| upsert_checkpoint(c, &cp))
            .await
            .map_err(|e| anyhow::anyhow!("Interact error: {}", e))??;
    }

    let _total = state.all_message_ids.len();

    run_scan_batches(app, pool, account_id, state, client).await
}

/// `pub` (rather than private, like `run_scan`) specifically so
/// `tests/historical_scan_benchmark.rs` (Doc 30 TASK-QA-002) can drive the
/// real batch/checkpoint/concurrency loop directly against a mocked
/// `GmailClient`, without needing a full `scans_historical` OAuth+licensing
/// call chain -- the exact seam this file's own internal tests already use.
pub async fn run_scan_batches<R: tauri::Runtime>(
    app: AppHandle<R>,
    pool: Pool,
    account_id: String,
    mut state: ScanCheckpointState,
    client: GmailClient,
) -> anyhow::Result<()> {
    let total = state.all_message_ids.len();
    let to_process = state.all_message_ids.clone();
    let mut processed_count = state.processed_count;

    let client = Arc::new(client);
    let pool_arc = Arc::new(pool.clone());

    // Doc 2026-07-28 mail scan performance: pre-filter bookkeeping (sender
    // sightings, audit-log rejections, ignored-noise rows) used to write one
    // row at a time from inside every spawned `process_message` call --
    // O(N) DB round-trips serialized through SQLite's single-writer lock.
    // Batched here and flushed at the same cadence as the scan checkpoint
    // (`should_checkpoint` below) instead.
    let scan_batcher: crate::ingestion::message_processor::ScanBatcherHandle =
        Arc::new(tokio::sync::Mutex::new(
            crate::ingestion::scan_db_batcher::ScanDbBatcher::new(),
        ));

    // TASK-TXN-001: resolved once per scan and threaded into every spawned
    // `process_message` call so Layer 5 (local LLM fallback) can actually
    // run during a historical scan — previously hardcoded to `None`.
    let app_dir = app.path().app_data_dir().ok();
    let llm_eligible = app
        .try_state::<crate::startup::LlmEligibility>()
        .map(|s| s.eligible)
        .unwrap_or(false);
    let layer6_tx = app
        .state::<crate::ingestion::queues::QueueHandles>()
        .layer6_tx
        .clone();

    let mut join_set: JoinSet<(String, anyhow::Result<Option<ProcessResult>>)> = JoinSet::new();

    let mut was_cancelled = false;
    let mut batch_count = 0;

    let mut batch_start_time = std::time::Instant::now();

    // Emit initial progress so the UI knows how many emails were fetched immediately
    let _ = crate::ipc::events::emit_event(
        &app,
        crate::ipc::events::AppEvent::ScanProgress,
        ScanProgressPayload {
            account_id: account_id.clone(),
            processed: processed_count,
            total,
            transactions_found: state.transactions_found,
            statements_found: state.statements_found,
            mandate_events_found: state.mandate_events_found,
            non_financial: state.non_financial,
            errors: state.errors,
            pending_enrichment: state.pending_enrichment,
            error_message: None,
        },
    );

    #[allow(clippy::too_many_arguments)]
    fn spawn_fetch(
        join_set: &mut JoinSet<(String, anyhow::Result<Option<ProcessResult>>)>,
        client: Arc<GmailClient>,
        pool_arc: Arc<Pool>,
        msg_id: String,
        app_dir: Option<std::path::PathBuf>,
        llm_eligible: bool,
        layer6_tx: tokio::sync::mpsc::Sender<crate::ingestion::queues::Layer6Job>,
        scan_batcher: crate::ingestion::message_processor::ScanBatcherHandle,
    ) {
        tracing::info!("Spawning fetch for msg_id='{}'", msg_id);
        join_set.spawn(async move {
            let res = MessageProcessor::process_message(
                &pool_arc,
                &client,
                &msg_id,
                app_dir,
                llm_eligible,
                Some(layer6_tx),
                Some(&scan_batcher),
            )
            .await;
            (msg_id, res)
        });
    }

    let mut ids_iter = to_process.into_iter().skip(processed_count);

    // Doc 30 TASK-GMAIL-007: keep up to MAX_CONCURRENT_FETCHES tasks in
    // flight at once. Priming here, then refilling one-for-one as each
    // completes below (rather than spawning a single task and draining it
    // to empty before spawning the next) is what actually makes the bound
    // meaningful — the previous spawn-then-immediately-drain pattern here
    // meant effective concurrency was always 1, regardless of the semaphore
    // that used to gate it.
    for _ in 0..MAX_CONCURRENT_FETCHES {
        // Cancelled while paused: stop prefilling and fall through to the main
        // loop, whose `cancel_poll` arm owns the actual cancellation sequence.
        if wait_while_paused(&account_id).await {
            break;
        }
        match ids_iter.next() {
            Some(msg_id) => spawn_fetch(
                &mut join_set,
                Arc::clone(&client),
                Arc::clone(&pool_arc),
                msg_id,
                app_dir.clone(),
                llm_eligible,
                layer6_tx.clone(),
                Arc::clone(&scan_batcher),
            ),
            None => break,
        }
    }

    // TASK-RT-CANCEL-RESPONSIVE: `join_set.join_next().await` alone only
    // gets a chance to notice `is_scan_cancelled` once some in-flight fetch
    // completes. If every currently in-flight task is stuck -- a Gmail
    // rate-limit backoff sleep (`execute_with_retry`'s up-to-~14s
    // exponential backoff), or a slow/unresponsive local LLM call during
    // Layer 6 classification -- a `scans_cancel` request could go
    // unnoticed for an unbounded amount of time, which is exactly the
    // "clicked Cancel and it never stops" symptom this fixes. Racing
    // `join_next()` against a 1s ticker bounds worst-case cancellation
    // latency to ~1s no matter how slow or stuck any individual in-flight
    // task is. The per-message check further down is left in place too --
    // this is additive, not a replacement.
    let mut cancel_poll = tokio::time::interval(std::time::Duration::from_secs(1));
    cancel_poll.tick().await; // first tick fires immediately; consume it up front

    loop {
        let join_res = tokio::select! {
            res = join_set.join_next() => match res {
                Some(r) => r,
                None => break,
            },
            _ = cancel_poll.tick() => {
                if is_scan_cancelled(&account_id) {
                    tracing::warn!("Scan cancelled by user for account_id='{}'", account_id);
                    clear_scan_cancellation(&account_id);
                    was_cancelled = true;
                    break;
                }
                continue;
            }
        };

        match join_res {
            Ok((msg_id, result)) => {
                tracing::info!("Finished processing msg_id='{}'", msg_id);
                match result {
                    Ok(Some(ProcessResult::TransactionAlert(extracted, boxed_obs, html, email_meta))) => {
                        tracing::info!("Classified msg_id='{}' as Transaction", msg_id);
                        // Doc 15 §2 principle 7 / Doc 12 §6.2a: route to the Transaction
                        // Queue rather than processing inline — no code path may write an
                        // observation directly, only via the queue's shared worker logic.
                        state.transactions_found += 1;
                        // TASK-DB-008 fix: was "historical_scan" -- that
                        // conflated *evidence origin* (what this field
                        // means, Document 18 §4.4) with *ingestion
                        // trigger mechanism* (already tracked separately
                        // by `processing_checkpoints.job_type`). A
                        // historical scan still reads Gmail transaction
                        // alert messages, exactly like live polling.
                        let job = crate::ingestion::queues::TransactionJob {
                            obs: *boxed_obs,
                            source_pipeline: "gmail_transaction".to_string(),
                            source_record_id: msg_id.clone(),
                            connected_account_id: account_id.clone(),
                            raw_body: extracted.text_body.clone(),
                            raw_html: html,
                            email_meta: Some(email_meta),
                        };
                        let tx = app
                            .state::<crate::ingestion::queues::QueueHandles>()
                            .transaction_tx
                            .clone();
                        if tx.send(job).await.is_err() {
                            tracing::error!(
                                "Transaction Queue closed — dropping job for msg_id='{}'",
                                msg_id
                            );
                        }
                    }
                    Ok(Some(ProcessResult::StatementEmail(extracted, email_meta))) => {
                        tracing::info!("Classified msg_id='{}' as Statement", msg_id);
                        // Doc 15 §2 principle 7 / Doc 12 §7.2: email-detected statements
                        // route onto the same Statement Queue as manual uploads — no
                        // lesser-validated path for either entry point.
                        state.statements_found += 1;
                        if extracted.pdf_attachments.is_empty() {
                            tracing::warn!(
                                "StatementEmail for msg_id='{}' has has_pdf_attachment=true \
                                     but no downloadable attachment_ids — skipping parse. \
                                     skipped_parts=[{}]",
                                msg_id,
                                extracted.skipped_pdf_parts.join("; ")
                            );
                        } else {
                            // Doc 2026-07-28 mail scan performance: attachment download
                            // and password resolution used to run inline in this loop --
                            // `resolve_statement_password` can spawn several `pdf_sidecar`
                            // processes in sequence (one per stored password, each with
                            // its own up-to-30s timeout), and this loop is the same
                            // single-threaded consumer that drains every other fetch and
                            // refills `MAX_CONCURRENT_FETCHES`. One slow password-protected
                            // statement stalled fetching, checkpointing, and cancellation
                            // for the *entire* scan, not just its own message -- real logs
                            // showed 60-200s wall-clock freezes lining up exactly with
                            // password prompts. Detaching it here lets this loop keep
                            // draining/refilling while the statement resolves in the
                            // background, the same fix already applied to Layer 6.
                            let client = Arc::clone(&client);
                            let pool = pool.clone();
                            let app = app.clone();
                            let msg_id = msg_id.clone();
                            let email_meta = email_meta.clone();
                            let attachments = extracted.pdf_attachments.clone();
                            tokio::spawn(async move {
                                for att in &attachments {
                                    let filename = &att.filename;
                                    // Prefer bytes Gmail already inlined in the payload
                                    // (`body.data`) — no network round-trip needed, and this
                                    // is exactly the case that used to be silently dropped
                                    // (see `PdfAttachmentMeta::inline_bytes`'s doc comment).
                                    let fetch_result: anyhow::Result<Vec<u8>> =
                                        if let Some(bytes) = &att.inline_bytes {
                                            Ok(bytes.clone())
                                        } else if let Some(att_id) = &att.attachment_id {
                                            client.fetch_attachment(&msg_id, att_id).await
                                        } else {
                                            // push_pdf_attachment never inserts an entry with
                                            // neither source, so this is unreachable in practice.
                                            continue;
                                        };
                                    match fetch_result {
                                        Ok(pdf_bytes) => {
                                            // audit_04 #4: the real content hash, checked for
                                            // a prior import before anything is created or the
                                            // user is prompted for a password.
                                            let file_hash = match crate::statements::duplicate_check::hash_email_attachment_if_new(
                                                &pdf_bytes, filename, &msg_id, &pool,
                                            )
                                            .await
                                            {
                                                Some(h) => h,
                                                None => continue,
                                            };

                                            // Doc 18 §4.7: the `statements` row must exist in
                                            // `queued` state before parsing begins, regardless
                                            // of entry point — same invariant as manual upload.
                                            let stmt_id = uuid::Uuid::new_v4().to_string();

                                            // TASK-GMAIL: this path previously skipped password
                                            // resolution entirely — an encrypted attachment was
                                            // enqueued with no password and died in pdfium with
                                            // PasswordError. Same choke point manual upload uses
                                            // (see `resolve_statement_password`'s doc comment).
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
                                            // audit_04 #1: stage the PDF into
                                            // encrypted on-disk storage instead
                                            // of carrying it in the job; the
                                            // worker reads it back under a
                                            // concurrency permit.
                                            if let Ok(dir) = app.path().app_data_dir() {
                                                if let Err(e) =
                                                    crate::statements::pdf_storage::store_pdf(
                                                        &dir, &stmt_id, &pdf_bytes,
                                                    )
                                                {
                                                    tracing::warn!(
                                                        "Failed to stage statement PDF for stmt_id='{}': {} — skipping",
                                                        stmt_id, e
                                                    );
                                                    continue;
                                                }
                                            } else {
                                                tracing::warn!(
                                                    "Could not resolve app data dir to stage statement PDF for stmt_id='{}' — skipping",
                                                    stmt_id
                                                );
                                                continue;
                                            }
                                            drop(pdf_bytes);

                                            let job = crate::ingestion::queues::StatementJob {
                                                filename: filename.clone(),
                                                file_hash,
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
                                                "Failed to fetch PDF attachment '{}' for \
                                                     msg_id='{}': {}",
                                                filename,
                                                msg_id,
                                                e
                                            );
                                        }
                                    }
                                }
                            });
                        }
                    }
                    Ok(Some(ProcessResult::MandateEvent(
                        extracted,
                        mandate_extraction,
                        event_type,
                    ))) => {
                        tracing::info!("Classified msg_id='{}' as Mandate", msg_id);
                        // docs/superpowers/specs/2026-07-18-mandate-tracking-design.md
                        // §4.2: route to the Mandate Queue, same shared worker logic
                        // as the live-poll entry point.
                        state.mandate_events_found += 1;
                        let job = crate::ingestion::queues::MandateJob {
                            extraction: mandate_extraction,
                            event_type,
                            source_pipeline: "gmail_transaction".to_string(),
                            source_record_id: msg_id.clone(),
                            connected_account_id: account_id.clone(),
                            raw_body: extracted.text_body.clone(),
                        };
                        let mandate_tx = app
                            .state::<crate::ingestion::queues::QueueHandles>()
                            .mandate_tx
                            .clone();
                        if mandate_tx.send(job).await.is_err() {
                            tracing::error!(
                                "Mandate Queue closed — dropping job for msg_id='{}'",
                                msg_id
                            );
                        }
                    }
                    Ok(None) => {
                        tracing::info!("Classified msg_id='{}' as Non-Financial", msg_id);
                        state.non_financial += 1;
                    }
                    Ok(Some(ProcessResult::EnqueuedForEnrichment)) => {
                        tracing::info!(
                            "msg_id='{}' enqueued for background Layer 6 enrichment",
                            msg_id
                        );
                        state.pending_enrichment += 1;
                    }
                    Err(e) => {
                        state.errors += 1;
                        tracing::error!("Failed to process message {}: {}", msg_id, e);

                        let failed_row = crate::db::scan_failed_messages::ScanFailedMessageRow {
                            id: Uuid::new_v4().to_string(),
                            account_id: account_id.clone(),
                            msg_id: msg_id.clone(),
                            error: e.to_string(),
                            failed_at: None,
                        };
                        if let Ok(conn) = pool.get().await {
                            let _ = conn
                                .interact(move |c| {
                                    crate::db::scan_failed_messages::insert(c, &failed_row)
                                })
                                .await;
                        }
                    }
                }
            }
            Err(e) => {
                state.errors += 1;
                tracing::error!("Join error: {}", e);
            }
        }

        processed_count += 1;
        batch_count += 1;

        if should_checkpoint(batch_count) {
            state.processed_count = processed_count;

            // audit_01 #2: patch the counters in place instead of
            // re-serializing the whole state — `all_message_ids` is in there
            // and never changes after the search phase, so rebuilding it
            // every 5 messages was pure write amplification. See
            // `patch_scan_progress`.
            let key = account_id.clone();
            let (p, t, s, m, n, e, pe) = (
                processed_count,
                state.transactions_found,
                state.statements_found,
                state.mandate_events_found,
                state.non_financial,
                state.errors,
                state.pending_enrichment,
            );
            if let Ok(conn) = pool.get().await {
                let _ = conn
                    .interact(move |c| {
                        crate::db::processing_checkpoints::patch_scan_progress(
                            c, &key, p, t, s, m, n, e, pe,
                        )
                    })
                    .await;
            }

            // Flush the batched pre-filter bookkeeping (sightings,
            // rejections, ignored-noise rows) at the same cadence as the
            // checkpoint above rather than every message.
            if let Err(e) = scan_batcher.lock().await.flush(&pool).await {
                tracing::warn!("scan_batcher flush failed (best-effort): {}", e);
            }

            batch_count = 0;
            let elapsed = batch_start_time.elapsed();
            tracing::info!(
                elapsed_ms = elapsed.as_millis(),
                batch_size = CHECKPOINT_INTERVAL,
                processed = processed_count,
                total = total,
                "Historical scan batch completed"
            );
            batch_start_time = std::time::Instant::now();

            let _ = crate::ipc::events::emit_event(
                &app,
                crate::ipc::events::AppEvent::ScanProgress,
                ScanProgressPayload {
                    account_id: account_id.clone(),
                    processed: processed_count,
                    total,
                    transactions_found: state.transactions_found,
                    statements_found: state.statements_found,
                    mandate_events_found: state.mandate_events_found,
                    non_financial: state.non_financial,
                    errors: state.errors,
                    pending_enrichment: state.pending_enrichment,
                    error_message: None,
                },
            );
        }

        // Returns early on cancellation, so the check below fires immediately
        // instead of after the pause poll's next wake-up.
        wait_while_paused(&account_id).await;

        // Doc 19 §18 Scans group / Doc 30 TASK-API-009: `scans_cancel`.
        // Checked at the same checkpoint cadence as the pause check above
        // rather than every single message, matching how `wait_while_paused`
        // itself is only consulted here, not per-message.
        if is_scan_cancelled(&account_id) {
            clear_scan_cancellation(&account_id);
            was_cancelled = true;
            break;
        }

        if let Some(next_id) = ids_iter.next() {
            spawn_fetch(
                &mut join_set,
                Arc::clone(&client),
                Arc::clone(&pool_arc),
                next_id,
                app_dir.clone(),
                llm_eligible,
                layer6_tx.clone(),
                Arc::clone(&scan_batcher),
            );
        }
    }

    // Flush whatever's left in the batcher (< CHECKPOINT_INTERVAL trailing
    // records, or anything buffered when the loop broke on cancellation)
    // rather than losing it -- every checkpoint-cadence flush above except
    // this final one is covered by `should_checkpoint`.
    if let Err(e) = scan_batcher.lock().await.flush(&pool).await {
        tracing::warn!("scan_batcher final flush failed (best-effort): {}", e);
    }

    // Root cause of slow cancellation: `is_scan_cancelled` breaks this loop
    // within ~1s (the `cancel_poll` race above), but up to
    // `MAX_CONCURRENT_FETCHES` (50) `process_message` tasks are still
    // running in the background at that point -- each potentially mid a
    // Gmail retry sequence that can take up to ~51s
    // (`gmail_client::execute_with_retry`'s 15s timeout x 3 attempts +
    // backoff) and each pulling connections from the same shared,
    // finite-size `pool` for its own DB work (sender reputation, audit log,
    // extraction-ladder lookups). Left running, they keep contending for
    // that pool right through the cancellation cleanup below (which also
    // needs a connection from it), so the `scan_cancelled` event the UI is
    // waiting on doesn't fire until that backlog happens to clear -- not
    // because cancellation was detected slowly, but because nothing ever
    // told those tasks to stop. `abort_all()` cuts them immediately (Tokio
    // interrupts each at its next await point), instead of leaving them to
    // run until `join_set`'s implicit drop at this function's end, which is
    // *after* cleanup and event emission.
    join_set.abort_all();

    state.processed_count = processed_count;

    if was_cancelled {
        // User requested to cancel and wipe progress so the next scan starts from scratch,
        // but keep any successfully extracted transactions/statements.
        if let Ok(conn) = pool.get().await {
            let acct_id = account_id.clone();
            let msg_ids = state.all_message_ids.clone();
            
            let _ = conn.interact(move |c| {
                if let Ok(tx) = c.transaction() {
                    // 1. Delete checkpoint so progress is completely wiped
                    let _ = tx.execute(
                        "DELETE FROM processing_checkpoints WHERE job_type = 'historical_scan' AND job_key = ?",
                        rusqlite::params![acct_id],
                    );
                    
                    // 2. Wipe unresolved items generated by THIS scan's fetched messages
                    // Chunking to stay well under SQLite's parameter limits
                    for chunk in msg_ids.chunks(900) {
                        let placeholders = vec!["?"; chunk.len()].join(", ");
                        
                        let sql_unassigned = format!(
                            "DELETE FROM unassigned_transactions WHERE observation_id IN (
                                SELECT id FROM transaction_observations WHERE source_record_id IN ({})
                            )",
                            placeholders
                        );
                        let _ = tx.execute(&sql_unassigned, rusqlite::params_from_iter(chunk.iter()));

                        let sql_obs = format!(
                            "DELETE FROM transaction_observations 
                             WHERE source_record_id IN ({}) 
                             AND canonical_transaction_id IS NULL
                             AND id NOT IN (SELECT observation_id FROM match_decisions)",
                            placeholders
                        );
                        let _ = tx.execute(&sql_obs, rusqlite::params_from_iter(chunk.iter()));

                        let sql_ignored = format!(
                            "DELETE FROM ignored_messages WHERE message_id IN ({})",
                            placeholders
                        );
                        let _ = tx.execute(&sql_ignored, rusqlite::params_from_iter(chunk.iter()));

                        let sql_unproc = format!(
                            "DELETE FROM unprocessed_statements WHERE json_extract(statement_source_json, '$.message_id') IN ({})",
                            placeholders
                        );
                        let _ = tx.execute(&sql_unproc, rusqlite::params_from_iter(chunk.iter()));
                    }
                    
                    let _ = tx.commit();
                }
            }).await;
        }
    } else {
        let final_cp = ProcessingCheckpointRow {
            id: Uuid::new_v4().to_string(),
            job_type: "historical_scan".to_string(),
            job_key: account_id.clone(),
            checkpoint_state_json: serde_json::to_string(&state).unwrap_or_default(),
            last_processed_token: None,
            status: "completed".to_string(),
            updated_at: Some(Utc::now().naive_utc()),
        };

        if let Ok(conn) = pool.get().await {
            let _ = conn
                .interact(move |c| upsert_checkpoint(c, &final_cp))
                .await;
        }
    }

    // A cancelled scan didn't actually complete -- `scan_completed` would
    // misrepresent it to the UI (progress bar shows 100%, "Sync Now" button
    // re-enables as if nothing is pending). Emits a dedicated `scan_cancelled`
    // event instead so the frontend has a definitive signal that the scan
    // has actually stopped -- previously nothing was emitted at all here,
    // leaving the UI's "Scanning..." state with no way to ever learn the
    // cancellation it requested had taken effect.
    let final_payload = ScanProgressPayload {
        account_id: account_id.clone(),
        processed: processed_count,
        total,
        transactions_found: state.transactions_found,
        statements_found: state.statements_found,
        mandate_events_found: state.mandate_events_found,
        non_financial: state.non_financial,
        errors: state.errors,
        pending_enrichment: state.pending_enrichment,
        error_message: None,
    };
    let _ = crate::ipc::events::emit_event(
        &app,
        if was_cancelled {
            crate::ipc::events::AppEvent::ScanCancelled
        } else {
            crate::ipc::events::AppEvent::ScanCompleted
        },
        final_payload,
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;
    use std::fs;
    use tauri::test::{mock_builder, mock_context};

    /// audit_01 #4: a scan that is paused *and* cancelled must stop. Before
    /// this, `wait_while_paused` polled only the pause flag, so the prefill
    /// loop — which runs before the main loop's `cancel_poll` ticker exists —
    /// spun here indefinitely and Cancel did nothing until someone un-paused.
    #[tokio::test]
    async fn paused_scan_still_observes_cancellation() {
        use std::sync::atomic::Ordering;
        let account_id = "acct_paused_cancel";
        let paused = &crate::commands::debug::SCAN_QUEUE_PAUSED;

        // Not paused: returns immediately, and does not report cancellation.
        paused.store(false, Ordering::Relaxed);
        assert!(!wait_while_paused(account_id).await);

        // Paused with a cancellation pending: must return `true` rather than
        // sleeping until the pause is lifted.
        paused.store(true, Ordering::Relaxed);
        cancelled_scans()
            .lock()
            .unwrap()
            .insert(account_id.to_string());
        let cancelled = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            wait_while_paused(account_id),
        )
        .await
        .expect("a cancelled scan must not block on the pause flag");
        assert!(cancelled);

        // Reporting cancellation must not consume it -- the caller's own
        // cancellation path is what clears the flag and emits the event.
        assert!(is_scan_cancelled(account_id));

        clear_scan_cancellation(account_id);
        paused.store(false, Ordering::Relaxed);
    }

    /// The sender-scoped query is what stops the scan paying Gmail quota for
    /// mail Gate 1 would reject anyway, so it has to cover every domain it was
    /// given, keep each chunk inside the URL budget, and never silently match
    /// nothing when the registry is empty.
    #[test]
    fn sender_scoped_queries_cover_every_domain_within_the_size_budget() {
        let date_range = "after:2026-05-28 before:2026-07-29";

        // Empty registry must degrade to scanning everything, not to a query
        // that matches no mail at all.
        assert_eq!(
            build_sender_scoped_queries(date_range, &[]),
            vec![date_range.to_string()]
        );

        let domains: Vec<String> = (0..204).map(|i| format!("bank{i:03}.example.com")).collect();
        let queries = build_sender_scoped_queries(date_range, &domains);

        assert!(queries.len() > 1, "204 domains must split into chunks");
        for q in &queries {
            assert!(
                q.len() <= MAX_SENDER_CLAUSE_CHARS + date_range.len() + 16,
                "chunk overshot the URL budget: {} chars",
                q.len()
            );
            assert!(q.ends_with(date_range), "every chunk keeps the date range");
        }
        for d in &domains {
            assert!(
                queries.iter().any(|q| q.contains(&format!("from:{d}"))),
                "domain {d} was dropped from the query set"
            );
        }

        // The subject rescue query must always be present -- without it, a
        // bank missing from the registry can never be discovered.
        assert!(
            queries.iter().any(|q| q.contains("subject:(debited)")),
            "sender-scoped queries must still include the Gate 1 subject rescue"
        );
    }

    /// Gate 1's subject rescue is the only way a bank absent from the bundled
    /// registry ever gets picked up. The server-side prefilter can only be
    /// lossless if its `subject:` terms cover every phrase that rescue keys
    /// on, so a phrase added to the classifier without adding it here would
    /// silently shrink what a scan can discover.
    #[test]
    fn subject_terms_cover_classifier_rescue_phrases() {
        use crate::ingestion::content_classifier::{ContentClass, ContentClassifier};

        for term in crate::ingestion::content_classifier::RESCUE_SUBJECT_TERMS {
            // Every listed term must actually trigger the rescue classes, or
            // it is dead weight in the query.
            let class = ContentClassifier::classify(term, "");
            assert!(
                matches!(
                    class,
                    ContentClass::TransactionAlert | ContentClass::BalanceUpdate
                ),
                "RESCUE_SUBJECT_TERMS lists {term:?} but classify() returns {class:?}"
            );
        }

        // And the query must actually carry them.
        let q = build_rescue_subject_query("after:2026-01-01 before:2026-07-29");
        for term in crate::ingestion::content_classifier::RESCUE_SUBJECT_TERMS {
            assert!(
                q.contains(&format!("subject:({term})")),
                "rescue query is missing {term:?}"
            );
        }
    }

    /// `run_scan_batches` now unconditionally sources `layer6_tx` from
    /// `QueueHandles` (Doc 2026-07-26 mail scan performance), so any test
    /// driving it through a bare `mock_builder()` app needs this managed or
    /// `app.state::<QueueHandles>()` panics with "state() called before
    /// manage()" even when no message in the test actually reaches Layer 6.
    fn test_queue_handles() -> crate::ingestion::queues::QueueHandles {
        let (transaction_tx, _) = tokio::sync::mpsc::channel(1);
        let (statement_tx, _) = tokio::sync::mpsc::channel(1);
        let (mandate_tx, _) = tokio::sync::mpsc::channel(1);
        let (layer6_tx, _) = tokio::sync::mpsc::channel(1);
        crate::ingestion::queues::QueueHandles {
            transaction_tx,
            statement_tx,
            mandate_tx,
            layer6_tx,
        }
    }

    /// Doc 30 TASK-GMAIL-007: pure, deterministic proof that the checkpoint
    /// cadence is every 5 (not 10, the value the code used to have before
    /// this fix — a wall-clock/DB-timing test can't reliably distinguish "a
    /// checkpoint fired at 5" from "only the final one fired" once fetches
    /// run concurrently, so this checks the actual threshold value directly).
    #[test]
    fn test_historical_scan_checkpoints_every_5() {
        for n in 0..CHECKPOINT_INTERVAL {
            assert!(
                !should_checkpoint(n),
                "must not checkpoint before {} processed",
                CHECKPOINT_INTERVAL
            );
        }
        assert!(should_checkpoint(CHECKPOINT_INTERVAL));
        assert!(should_checkpoint(CHECKPOINT_INTERVAL + 1));
    }

    /// Doc 30 TASK-API-009 acceptance test: `scans_status` reflects the
    /// real stored checkpoint state -- both "no checkpoint yet" and a
    /// genuine in-progress one with real progress numbers.
    #[test]
    fn test_scan_status_reflects_checkpoint_state() {
        let not_started = checkpoint_to_status(None);
        assert_eq!(not_started.status, "not_started");
        assert_eq!(not_started.processed, 0);

        let state = ScanCheckpointState {
            start_date: "2026-01-01".to_string(),
            end_date: "2026-02-01".to_string(),
            all_message_ids: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            processed_count: 2,
            transactions_found: 1,
            statements_found: 0,
            mandate_events_found: 0,
            non_financial: 1,
            errors: 0,
        pending_enrichment: 0,
        };
        let cp = ProcessingCheckpointRow {
            id: "cp_status".into(),
            job_type: "historical_scan".into(),
            job_key: "acc_1".into(),
            checkpoint_state_json: serde_json::to_string(&state).unwrap(),
            last_processed_token: None,
            status: "in_progress".into(),
            updated_at: None,
        };

        let status = checkpoint_to_status(Some(cp));
        assert_eq!(status.status, "in_progress");
        assert_eq!(status.processed, 2);
        assert_eq!(status.total, 3);
        assert_eq!(status.transactions_found, 1);
    }

    #[test]
    fn test_scan_status_reflects_pending_enrichment_count() {
        let state = ScanCheckpointState {
            start_date: "2026-01-01".to_string(),
            end_date: "2026-02-01".to_string(),
            all_message_ids: vec!["a".to_string()],
            processed_count: 1,
            pending_enrichment: 3,
            ..Default::default()
        };
        let cp = ProcessingCheckpointRow {
            id: "cp_pending".into(),
            job_type: "historical_scan".into(),
            job_key: "acc_1".into(),
            checkpoint_state_json: serde_json::to_string(&state).unwrap(),
            last_processed_token: None,
            status: "completed".into(),
            updated_at: None,
        };
        let status = checkpoint_to_status(Some(cp));
        assert_eq!(status.pending_enrichment, 3);
    }

    /// Doc 30 TASK-GMAIL-007 / Doc 19 §3.6 / TASK-API-009: only one active
    /// scan per account -- and the claim itself (not just a preceding
    /// read) is what enforces it, so a second claim while the first is
    /// still in_progress must be rejected even though both calls would see
    /// the same "not in progress" state if they read at the same time.
    #[tokio::test]
    async fn test_concurrent_scan_claim_rejected() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test_claim.db");
        let pool = init_db(db_path.clone()).await.expect("DB init failed");
        let conn = pool.get().await.unwrap();

        // First claim on a brand-new job_key: succeeds, no prior checkpoint.
        let first = conn
            .interact(|c| claim_checkpoint_in_progress(c, "historical_scan", "acc_1"))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(first, ClaimOutcome::Claimed(None)));

        // Second claim for the same job_key while still in_progress: rejected.
        let second = conn
            .interact(|c| claim_checkpoint_in_progress(c, "historical_scan", "acc_1"))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(second, ClaimOutcome::AlreadyInProgress));

        // Mark it completed, then a fresh claim succeeds again.
        conn.interact(|c| {
            upsert_checkpoint(
                c,
                &ProcessingCheckpointRow {
                    id: "cp1".into(),
                    job_type: "historical_scan".into(),
                    job_key: "acc_1".into(),
                    checkpoint_state_json: "{}".into(),
                    last_processed_token: None,
                    status: "completed".into(),
                    updated_at: None,
                },
            )
        })
        .await
        .unwrap()
        .unwrap();

        let third = conn
            .interact(|c| claim_checkpoint_in_progress(c, "historical_scan", "acc_1"))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(third, ClaimOutcome::Claimed(Some(_))));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    /// audit_01 #2: the incremental checkpoint stopped re-serializing the
    /// whole `ScanCheckpointState` — which carries every message id in the
    /// scan — just to bump some integers. The patched row has to stay
    /// byte-compatible with what `ScanCheckpointState` deserializes, and the
    /// id list has to survive untouched, or a resumed scan loses its work
    /// queue.
    #[test]
    fn patching_scan_progress_updates_counters_without_touching_the_id_list() {
        let conn = crate::db::test_helpers::setup_test_db();

        let ids: Vec<String> = (0..500).map(|i| format!("msg_{i}")).collect();
        let state = ScanCheckpointState {
            start_date: "2023-01-01".into(),
            end_date: "2023-01-31".into(),
            all_message_ids: ids.clone(),
            processed_count: 0,
            ..Default::default()
        };
        crate::db::processing_checkpoints::upsert_checkpoint(
            &conn,
            &ProcessingCheckpointRow {
                id: "cp_1".into(),
                job_type: "historical_scan".into(),
                job_key: "acct_1".into(),
                checkpoint_state_json: serde_json::to_string(&state).unwrap(),
                last_processed_token: None,
                status: "in_progress".into(),
                updated_at: None,
            },
        )
        .unwrap();

        crate::db::processing_checkpoints::patch_scan_progress(
            &conn, "acct_1", 42, 7, 3, 2, 11, 1, 5,
        )
        .unwrap();

        let cp = crate::db::processing_checkpoints::get_checkpoint(
            &conn,
            "historical_scan",
            "acct_1",
        )
        .unwrap()
        .unwrap();
        let patched: ScanCheckpointState =
            serde_json::from_str(&cp.checkpoint_state_json).expect("must still deserialize");

        assert_eq!(patched.all_message_ids, ids, "the work queue must survive");
        assert_eq!(patched.start_date, "2023-01-01");
        assert_eq!(patched.processed_count, 42);
        assert_eq!(patched.transactions_found, 7);
        assert_eq!(patched.statements_found, 3);
        assert_eq!(patched.mandate_events_found, 2);
        assert_eq!(patched.non_financial, 11);
        assert_eq!(patched.errors, 1);
        assert_eq!(patched.pending_enrichment, 5);

        // No row yet (e.g. a job key that never ran) must not error -- the
        // initial upsert at scan start is what creates it.
        crate::db::processing_checkpoints::patch_scan_progress(
            &conn, "acct_missing", 1, 0, 0, 0, 0, 0, 0,
        )
        .unwrap();
    }

    #[tokio::test]
    async fn test_historical_scan_completes_and_checkpoints_final_state() {
        let app = mock_builder()
            .build(mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone();
        app.manage(test_queue_handles());

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test_scan.db");
        let pool = init_db(db_path.clone()).await.expect("DB init failed");

        let account_id = "acc_test_1".to_string();

        let mut ids = vec![];
        for i in 0..7 {
            ids.push(format!("msg_{}", i));
        }

        let state = ScanCheckpointState {
            start_date: "2023-01-01".into(),
            end_date: "2023-01-31".into(),
            all_message_ids: ids,
            processed_count: 0,
            transactions_found: 0,
            statements_found: 0,
            mandate_events_found: 0,
            non_financial: 0,
            errors: 0,
        pending_enrichment: 0,
        };

        let client = GmailClient::new("fake_token".into(), pool.clone(), None);

        // This will process all 7 messages. It should checkpoint at 5, and at 7 (completion).
        run_scan_batches(app, pool.clone(), account_id.clone(), state, client)
            .await
            .expect("run_scan_batches failed");

        let conn = pool.get().await.unwrap();
        let cp = conn
            .interact(move |c| {
                get_checkpoint(c, "historical_scan", &account_id)
                    .unwrap()
                    .unwrap()
            })
            .await
            .unwrap();

        assert_eq!(cp.status, "completed");
        let final_state: ScanCheckpointState =
            serde_json::from_str(&cp.checkpoint_state_json).unwrap();
        assert_eq!(final_state.processed_count, 7);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    /// Doc 2026-07-28 dev-scan-log-issues: previously an exhausted-retry
    /// fetch failure was only ever logged (`tracing::error!`), never
    /// persisted anywhere queryable -- a transaction could vanish from a
    /// scan with nothing user-visible beyond an incremented `errors`
    /// counter. `GmailClient::new(..., None)` here has no mock server and
    /// no token refresher, so every fetch immediately fails with 401
    /// Unauthorized (no retry loop -- there's no refresher to retry with),
    /// driving every message through the exact failure path this fixes.
    #[tokio::test]
    async fn test_exhausted_fetch_failure_is_persisted_to_scan_failed_messages() {
        let app = mock_builder()
            .build(mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone();
        app.manage(test_queue_handles());

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test_scan_failed.db");
        let pool = init_db(db_path.clone()).await.expect("DB init failed");

        let account_id = "acc_failed_test".to_string();
        let state = ScanCheckpointState {
            start_date: "2023-01-01".into(),
            end_date: "2023-01-31".into(),
            all_message_ids: vec!["msg_a".to_string(), "msg_b".to_string()],
            processed_count: 0,
            ..Default::default()
        };

        let client = GmailClient::new("fake_token".into(), pool.clone(), None);

        run_scan_batches(app, pool.clone(), account_id.clone(), state, client)
            .await
            .expect("run_scan_batches failed");

        let conn = pool.get().await.unwrap();
        let account_id_for_query = account_id.clone();
        let failed = conn
            .interact(move |c| {
                crate::db::scan_failed_messages::select_by_account(c, &account_id_for_query)
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(failed.len(), 2);
        let mut msg_ids: Vec<String> = failed.iter().map(|r| r.msg_id.clone()).collect();
        msg_ids.sort();
        assert_eq!(msg_ids, vec!["msg_a".to_string(), "msg_b".to_string()]);
        assert!(
            failed[0].error.contains("401") || failed[0].error.contains("Unauthorized"),
            "expected a 401/Unauthorized error string, got: {}",
            failed[0].error
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    /// Regression test: a cancelled scan previously emitted nothing at all
    /// once its loop broke, leaving the frontend's "Scanning..." state with
    /// no definitive signal the cancellation actually took effect. Must now
    /// emit `scan_cancelled` (not `scan_completed`, which would misrepresent
    /// a stopped-early scan as having finished) and checkpoint `"cancelled"`.
    #[tokio::test]
    async fn test_historical_scan_cancellation_emits_scan_cancelled_not_scan_completed() {
        use tauri::Listener;

        let app = mock_builder()
            .build(mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone();
        app.manage(test_queue_handles());

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test_scan_cancel.db");
        let pool = init_db(db_path.clone()).await.expect("DB init failed");

        // `scans_cancel` validates the `gmail_<uuid>` shape (unlike the
        // other tests in this module, which construct a `ScanCheckpointState`
        // directly and never pass their plain "acc_..." id through that
        // validator).
        let account_id = format!("gmail_{}", uuid::Uuid::new_v4());

        // Pre-request cancellation. `run_scan_batches` is called directly
        // here (bypassing `run_scan`'s own `clear_scan_cancellation` at scan
        // start), so this flag is already set the first time the loop's
        // per-checkpoint cancellation check runs, after the first
        // `CHECKPOINT_INTERVAL` (5) messages are processed.
        scans_cancel(account_id.clone()).await.unwrap();

        let ids: Vec<String> = (0..7).map(|i| format!("msg_{i}")).collect();
        let state = ScanCheckpointState {
            start_date: "2023-01-01".into(),
            end_date: "2023-01-31".into(),
            all_message_ids: ids,
            processed_count: 0,
            transactions_found: 0,
            statements_found: 0,
            mandate_events_found: 0,
            non_financial: 0,
            errors: 0,
        pending_enrichment: 0,
        };

        let client = GmailClient::new("fake_token".into(), pool.clone(), None);

        let captured_events = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let captured_clone = captured_events.clone();
        app.listen_any("scan_cancelled", move |_| {
            captured_clone
                .lock()
                .unwrap()
                .push("scan_cancelled".to_string());
        });
        let captured_clone2 = captured_events.clone();
        app.listen_any("scan_completed", move |_| {
            captured_clone2
                .lock()
                .unwrap()
                .push("scan_completed".to_string());
        });

        run_scan_batches(app, pool.clone(), account_id.clone(), state, client)
            .await
            .expect("run_scan_batches failed");

        let conn = pool.get().await.unwrap();
        let cp_opt = conn
            .interact(move |c| {
                get_checkpoint(c, "historical_scan", &account_id)
                    .unwrap()
            })
            .await
            .unwrap();
        
        // Assert the checkpoint was completely deleted so progress starts from 0 next time
        assert!(cp_opt.is_none(), "Checkpoint should be deleted on cancel");

        let captured = captured_events.lock().unwrap();
        assert!(
            captured.contains(&"scan_cancelled".to_string()),
            "expected scan_cancelled to fire, got {:?}",
            *captured
        );
        assert!(
            !captured.contains(&"scan_completed".to_string()),
            "scan_completed must NOT fire for a cancelled scan, got {:?}",
            *captured
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_historical_scan_resumes_from_checkpoint() {
        let app = mock_builder()
            .build(mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone();
        app.manage(test_queue_handles());

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test_scan_resume.db");
        let pool = init_db(db_path.clone()).await.expect("DB init failed");

        let account_id = "acc_test_2".to_string();

        let mut ids = vec![];
        for i in 0..12 {
            ids.push(format!("msg_{}", i));
        }

        // Start from 5 processed
        let state = ScanCheckpointState {
            start_date: "2023-01-01".into(),
            end_date: "2023-01-31".into(),
            all_message_ids: ids,
            processed_count: 5,
            transactions_found: 0,
            statements_found: 0,
            mandate_events_found: 0,
            non_financial: 0,
            errors: 0,
        pending_enrichment: 0,
        };

        let client = GmailClient::new("fake_token".into(), pool.clone(), None);

        run_scan_batches(app, pool.clone(), account_id.clone(), state, client)
            .await
            .expect("run_scan_batches failed");

        let conn = pool.get().await.unwrap();
        let cp = conn
            .interact(move |c| {
                get_checkpoint(c, "historical_scan", &account_id)
                    .unwrap()
                    .unwrap()
            })
            .await
            .unwrap();

        assert_eq!(cp.status, "completed");
        let final_state: ScanCheckpointState =
            serde_json::from_str(&cp.checkpoint_state_json).unwrap();

        // Processed count should be 12 total, meaning it resumed and finished
        assert_eq!(final_state.processed_count, 12);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_overlapping_scan_dedupes_at_observation_layer() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test_scan_dedupe.db");
        let pool = init_db(db_path.clone()).await.expect("DB init failed");

        let msg_id = "msg_dup_123".to_string();
        let fingerprint = format!("hist_scan_{}", msg_id);

        let row = crate::db::transaction_observations::TransactionObservationsRow {
            id: Uuid::new_v4().to_string(),
            canonical_transaction_id: None,
            source_pipeline: Some("gmail_transaction".to_string()),
            source_record_id: Some(msg_id.clone()),
            source_message_id: Some(msg_id.clone()),
            source_thread_id: None,
            statement_id: None,
            statement_entry_id: None,
            instrument_id: None,
            direction: None,
            amount: None,
            amount_minor: Some(100),
            currency: None,
            event_time: None,
            event_time_confidence: None,
            posting_date: None,
            merchant_raw: Some("Test Merchant".to_string()),
            merchant_normalized: None,
            reference_id: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            timezone_at_ingestion: None,
            fingerprint: Some(fingerprint.clone()),
            extraction_method: Some("historical_scan".to_string()),
            confidence_score: None,
            raw_payload_json: None,
            parser_version: None,
            emi_total_installments: None,
            emi_installment_number: None,
            emi_original_amount_minor: None,
            channel: None,
            is_deleted: false,
            created_at: Some(Utc::now().naive_utc()),
            updated_at: Some(Utc::now().naive_utc()),
        };

        let conn = pool.get().await.unwrap();
        // Insert first time
        conn.interact({
            let row = row.clone();
            move |c| crate::db::transaction_observations::insert_observation(c, &row).unwrap()
        })
        .await
        .unwrap();

        // Insert second time should fail with unique constraint on fingerprint
        let res = conn
            .interact({
                let row2 = row.clone();
                move |c| crate::db::transaction_observations::insert_observation(c, &row2)
            })
            .await
            .unwrap();

        assert!(res.is_err());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
