use chrono::Utc;
use deadpool_sqlite::Pool;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::db::processing_checkpoints::{
    get_checkpoint, upsert_checkpoint, ProcessingCheckpointRow,
};
use crate::ingestion::gmail_client::GmailClient;
use crate::ingestion::message_processor::{MessageProcessor, ProcessResult};
use crate::ingestion::oauth::get_valid_access_token;

/// Doc 30 TASK-GMAIL-007 / Doc 19 §3.6: "only one active historical scan per
/// connected account is allowed unless explicitly resumed." Split out as a
/// pure function so the rejection behavior is testable without a live OAuth
/// token / Keychain access, which `scans_historical` itself requires.
fn reject_if_scan_in_progress(
    existing: &Option<ProcessingCheckpointRow>,
) -> Result<(), crate::error::AppError> {
    if let Some(cp) = existing {
        if cp.status == "in_progress" {
            // Document 19's dedicated `SCAN_ALREADY_RUNNING` code isn't on
            // `AppError` yet — that catalog-wide mapping is TASK-API-010's
            // explicit scope (see error.rs's own doc comment). `Validation`
            // is the closest generic code available now.
            return Err(crate::error::AppError::Validation(
                "A historical scan is already in progress for this account".into(),
            ));
        }
    }
    Ok(())
}

/// Doc 30 TASK-GMAIL-007: checkpoint every 5 processed messages.
const CHECKPOINT_INTERVAL: usize = 5;

fn should_checkpoint(batch_count: usize) -> bool {
    batch_count >= CHECKPOINT_INTERVAL
}

/// Doc 30 TASK-GMAIL-007 / TASK-GMAIL-002: bounded concurrent batches, max 50
/// full-message fetches in flight at once — the same figure the shared Gmail
/// quota semaphore in `gmail_client.rs` enforces globally across both the
/// live poll worker and historical scans.
const MAX_CONCURRENT_FETCHES: usize = 50;

#[derive(Clone, Serialize)]
struct ScanProgressPayload {
    account_id: String,
    processed: usize,
    total: usize,
    transactions_found: usize,
    statements_found: usize,
    non_financial: usize,
    errors: usize,
    error_message: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct ScanCheckpointState {
    start_date: String,
    end_date: String,
    all_message_ids: Vec<String>,
    processed_count: usize,
    #[serde(default)]
    transactions_found: usize,
    #[serde(default)]
    statements_found: usize,
    #[serde(default)]
    non_financial: usize,
    #[serde(default)]
    errors: usize,
}

#[tauri::command]
pub async fn scans_historical<R: tauri::Runtime>(
    app: AppHandle<R>,
    pool: State<'_, Pool>,
    account_id: String,
    start_date: String,
    end_date: String,
) -> Result<String, crate::error::AppError> {
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

    let existing = conn
        .interact({
            let account_id_clone = account_id.clone();
            move |c| get_checkpoint(c, "historical_scan", &account_id_clone)
        })
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    reject_if_scan_in_progress(&existing)?;

    tokio::spawn(async move {
        if let Err(e) = run_scan(
            app.clone(),
            pool.clone(),
            account_id.clone(),
            start_date,
            end_date,
            access_token,
            existing,
        )
        .await
        {
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

            let _ = app.emit(
                "scan_failed",
                ScanProgressPayload {
                    account_id,
                    processed: 0,
                    total: 0,
                    transactions_found: 0,
                    statements_found: 0,
                    non_financial: 0,
                    errors: 1,
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
    let client = GmailClient::new(access_token, pool.clone());

    let mut state = if let Some(cp) = existing_checkpoint {
        if cp.status == "paused" || cp.status == "failed" {
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

        let query = format!(
            "after:{} before:{}",
            start_date,
            inclusive_end.format("%Y-%m-%d")
        );
        let ids = client.search_messages(&query).await?;
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

async fn run_scan_batches<R: tauri::Runtime>(
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

    // TASK-TXN-001: resolved once per scan and threaded into every spawned
    // `process_message` call so Layer 5 (local LLM fallback) can actually
    // run during a historical scan — previously hardcoded to `None`.
    let app_dir = app.path().app_data_dir().ok();
    let llm_eligible = app
        .try_state::<crate::startup::LlmEligibility>()
        .map(|s| s.eligible)
        .unwrap_or(false);

    let mut join_set: JoinSet<(String, anyhow::Result<Option<ProcessResult>>)> = JoinSet::new();

    let mut batch_count = 0;
    
    let mut batch_start_time = std::time::Instant::now();

    // Emit initial progress so the UI knows how many emails were fetched immediately
    let _ = app.emit(
        "scan_progress",
        ScanProgressPayload {
            account_id: account_id.clone(),
            processed: processed_count,
            total,
            transactions_found: state.transactions_found,
            statements_found: state.statements_found,
            non_financial: state.non_financial,
            errors: state.errors,
            error_message: None,
        },
    );

    async fn wait_while_paused() {
        loop {
            let paused =
                crate::commands::debug::SCAN_QUEUE_PAUSED.load(std::sync::atomic::Ordering::Relaxed);
            if !paused {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }

    fn spawn_fetch(
        join_set: &mut JoinSet<(String, anyhow::Result<Option<ProcessResult>>)>,
        client: Arc<GmailClient>,
        pool_arc: Arc<Pool>,
        msg_id: String,
        app_dir: Option<std::path::PathBuf>,
        llm_eligible: bool,
    ) {
        join_set.spawn(async move {
            let res = MessageProcessor::process_message(
                &pool_arc,
                &client,
                &msg_id,
                app_dir,
                llm_eligible,
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
        wait_while_paused().await;
        match ids_iter.next() {
            Some(msg_id) => spawn_fetch(
                &mut join_set,
                Arc::clone(&client),
                Arc::clone(&pool_arc),
                msg_id,
                app_dir.clone(),
                llm_eligible,
            ),
            None => break,
        }
    }

    while let Some(join_res) = join_set.join_next().await {
        match join_res {
            Ok((msg_id, result)) => {
                match result {
                        Ok(Some(ProcessResult::TransactionAlert(_, boxed_obs))) => {
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
                            };
                            let tx = app
                                .state::<crate::ingestion::queues::QueueHandles>()
                                .transaction_tx
                                .clone();
                            if tx.send(job).await.is_err() {
                                tracing::error!("Transaction Queue closed — dropping job for msg_id='{}'", msg_id);
                            }
                        }
                        Ok(Some(ProcessResult::StatementEmail(extracted))) => {
                            // Doc 15 §2 principle 7 / Doc 12 §7.2: email-detected statements
                            // route onto the same Statement Queue as manual uploads — no
                            // lesser-validated path for either entry point.
                            state.statements_found += 1;
                            if extracted.pdf_attachments.is_empty() {
                                tracing::warn!(
                                    "StatementEmail for msg_id='{}' has has_pdf_attachment=true \
                                     but no downloadable attachment_ids — skipping parse",
                                    msg_id
                                );
                            } else {
                                for att in &extracted.pdf_attachments {
                                    let att_id = &att.attachment_id;
                                    let filename = &att.filename;
                                    match client.fetch_attachment(&msg_id, att_id).await {
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
                                                // Use message_id as the source_record_id /
                                                // file_hash proxy for email-sourced statements.
                                                file_hash: msg_id.clone(),
                                                stmt_id,
                                                // Doc 30 TASK-STMT-009: batch progress is a
                                                // manual-upload-batch concept only.
                                                batch_progress: None,
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
                                                att_id, msg_id, e
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        Ok(None) => {
                            state.non_financial += 1;
                        }
                        Err(e) => {
                            state.errors += 1;
                            tracing::error!("Failed to process message {}: {}", msg_id, e);
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

                let cp = ProcessingCheckpointRow {
                    id: Uuid::new_v4().to_string(),
                    job_type: "historical_scan".to_string(),
                    job_key: account_id.clone(),
                    checkpoint_state_json: serde_json::to_string(&state).unwrap_or_default(),
                    last_processed_token: None,
                    status: "in_progress".to_string(),
                    updated_at: Some(Utc::now().naive_utc()),
                };

                if let Ok(conn) = pool.get().await {
                    let _ = conn.interact(move |c| upsert_checkpoint(c, &cp)).await;
                }

                batch_count = 0;
                let elapsed = batch_start_time.elapsed();
                tracing::info!(elapsed_ms = elapsed.as_millis(), batch_size = CHECKPOINT_INTERVAL, "Historical scan batch completed");
                batch_start_time = std::time::Instant::now();

                let _ = app.emit(
                    "scan_progress",
                    ScanProgressPayload {
                        account_id: account_id.clone(),
                        processed: processed_count,
                        total,
                        transactions_found: state.transactions_found,
                        statements_found: state.statements_found,
                        non_financial: state.non_financial,
                        errors: state.errors,
                        error_message: None,
                    },
                );
            }

        wait_while_paused().await;
        if let Some(next_id) = ids_iter.next() {
            spawn_fetch(
                &mut join_set,
                Arc::clone(&client),
                Arc::clone(&pool_arc),
                next_id,
                app_dir.clone(),
                llm_eligible,
            );
        }
    }

    state.processed_count = processed_count;

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

    let _ = app.emit(
        "scan_completed",
        ScanProgressPayload {
            account_id: account_id.clone(),
            processed: processed_count,
            total,
            transactions_found: state.transactions_found,
            statements_found: state.statements_found,
            non_financial: state.non_financial,
            errors: state.errors,
            error_message: None,
        },
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;
    use std::fs;
    use tauri::test::{mock_builder, mock_context};

    /// Doc 30 TASK-GMAIL-007: pure, deterministic proof that the checkpoint
    /// cadence is every 5 (not 10, the value the code used to have before
    /// this fix — a wall-clock/DB-timing test can't reliably distinguish "a
    /// checkpoint fired at 5" from "only the final one fired" once fetches
    /// run concurrently, so this checks the actual threshold value directly).
    #[test]
    fn test_historical_scan_checkpoints_every_5() {
        for n in 0..CHECKPOINT_INTERVAL {
            assert!(!should_checkpoint(n), "must not checkpoint before {} processed", CHECKPOINT_INTERVAL);
        }
        assert!(should_checkpoint(CHECKPOINT_INTERVAL));
        assert!(should_checkpoint(CHECKPOINT_INTERVAL + 1));
    }

    /// Doc 30 TASK-GMAIL-007 / Doc 19 §3.6: only one active scan per account.
    #[test]
    fn test_concurrent_scan_rejected() {
        let in_progress = ProcessingCheckpointRow {
            id: "cp1".into(),
            job_type: "historical_scan".into(),
            job_key: "acc_1".into(),
            checkpoint_state_json: "{}".into(),
            last_processed_token: None,
            status: "in_progress".into(),
            updated_at: None,
        };
        assert!(reject_if_scan_in_progress(&Some(in_progress)).is_err());

        let completed = ProcessingCheckpointRow {
            id: "cp2".into(),
            job_type: "historical_scan".into(),
            job_key: "acc_1".into(),
            checkpoint_state_json: "{}".into(),
            last_processed_token: None,
            status: "completed".into(),
            updated_at: None,
        };
        assert!(reject_if_scan_in_progress(&Some(completed)).is_ok());
        assert!(reject_if_scan_in_progress(&None).is_ok());
    }

    #[tokio::test]
    async fn test_historical_scan_completes_and_checkpoints_final_state() {
        let app = mock_builder()
            .build(mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone();

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
            non_financial: 0,
            errors: 0,
        };

        let client = GmailClient::new("fake_token".into(), pool.clone());

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

    #[tokio::test]
    async fn test_historical_scan_resumes_from_checkpoint() {
        let app = mock_builder()
            .build(mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone();

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
            non_financial: 0,
            errors: 0,
        };

        let client = GmailClient::new("fake_token".into(), pool.clone());

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
