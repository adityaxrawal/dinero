//! Doc 30 TASK-QA-002: Build Historical Scan Performance Harness.
//!
//! Drives the real `run_scan_batches` (the same fetch/classify/checkpoint
//! loop `scans_historical` spawns in production) against a mocked Gmail API
//! server, with the real Transaction Queue worker pool (`spawn_queues`)
//! running behind it -- so extraction/dedup happen through the genuine
//! production path, not a re-implemented shortcut.
//!
//! `clippy::await_holding_lock` is allowed file-wide on purpose: `test_lock()`
//! (below) exists precisely to hold a guard across each test's `.await`s, so
//! only one test at a time drives the process-wide `SCAN_QUEUE_PAUSED` static.
//! Dropping the guard before awaiting would defeat the serialization it exists
//! for. Nothing here contends the lock across threads except the test harness.
#![allow(clippy::await_holding_lock)]

use base64::{engine::general_purpose::URL_SAFE, Engine as _};
use dinero_app_lib::commands::debug::SCAN_QUEUE_PAUSED;
use dinero_app_lib::db;
use dinero_app_lib::db::processing_checkpoints::get_checkpoint;
use dinero_app_lib::ingestion::gmail_client::GmailClient;
use dinero_app_lib::ingestion::historical_scan::{run_scan_batches, ScanCheckpointState};
use dinero_app_lib::ingestion::queues::spawn_queues;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::{AppHandle, Manager};

/// `run_scan_batches`'s `wait_while_paused` checks a process-wide static
/// (`commands::debug::SCAN_QUEUE_PAUSED`). `cargo test` runs every test
/// function in this binary concurrently by default, so two tests toggling
/// it at once would corrupt each other's timing/pause assertions -- same
/// class of race TASK-OPS-007 guarded its `DINERO_LOG_RETENTION_DAYS` env
/// var test against. Every test in this file locks this for its duration.
fn test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn mock_app() -> AppHandle<tauri::test::MockRuntime> {
    mock_builder()
        .build(mock_context(noop_assets()))
        .unwrap()
        .handle()
        .clone()
}

async fn migrated_pool(label: &str) -> deadpool_sqlite::Pool {
    let dir =
        std::env::temp_dir().join(format!("dinero_hs_bench_{label}_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    db::init_db(dir.join("test.db"))
        .await
        .expect("DB init failed")
}

fn b64(s: &str) -> String {
    URL_SAFE.encode(s)
}

fn p95(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    let idx = ((samples.len() as f64) * 0.95).ceil() as usize - 1;
    samples[idx.min(samples.len() - 1)]
}

/// One realistic HDFC-shaped transaction body per index. Doc 30 TASK-QA-002
/// wants "template variants, duplicates, a small malformed-message
/// percentage": every 10th non-zero index reuses index 0's exact body (a
/// genuine duplicate -- same merchant/amount/date, so it produces the same
/// fingerprint and must dedup), and every 50th index is malformed (no
/// From/Subject headers, so it fails Gate 1 outright).
fn synthetic_message_json(idx: usize) -> serde_json::Value {
    if idx % 50 == 49 {
        return serde_json::json!({
            "id": format!("msg_{idx}"),
            "threadId": format!("thread_{idx}"),
            "historyId": "1000",
            "internalDate": "1750000000000",
            "snippet": "malformed",
            "payload": {
                "mimeType": "text/plain",
                "headers": [],
                "body": { "data": b64("garbled, no usable headers") }
            }
        });
    }

    let effective_idx = if idx.is_multiple_of(10) && idx != 0 {
        0
    } else {
        idx
    };
    let amount = 100 + (effective_idx * 7 % 5000);
    let day = 1 + (effective_idx % 27);
    let body = format!(
        "Rs {amount}.00 spent on your HDFC Bank CREDIT Card ending 1234 at Merchant{} on {day:02}-Jun-26.",
        effective_idx % 37
    );
    serde_json::json!({
        "id": format!("msg_{idx}"),
        "threadId": format!("thread_{idx}"),
        "historyId": "1000",
        "internalDate": "1750000000000",
        "snippet": body,
        "payload": {
            "mimeType": "text/plain",
            "headers": [
                {"name": "From", "value": "\"HDFC Bank\" <alerts@hdfcbank.net>"},
                {"name": "Subject", "value": "Payment Notification"},
                {"name": "Date", "value": "Mon, 1 Jun 2026 12:00:00 +0000"}
            ],
            "body": { "data": b64(&body) }
        }
    })
}

/// Returns the mock server plus a live hit counter -- counting requests
/// ourselves (rather than relying on mockito's own `.expect(n)`/
/// `.assert_async()` bookkeeping, which requires asserting against the
/// exact `Mock` handle returned by `.create_async()`) is what lets every
/// caller check "how many fetches actually happened" with an assertion of
/// its own choosing, including mid-scan (the quota-pause test needs to
/// check the count *while the scan is still paused*, before any final
/// mock-side assertion would even run).
async fn mock_gmail_server() -> (
    mockito::ServerGuard,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let hits_for_closure = hits.clone();
    let mut server = mockito::Server::new_async().await;
    server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"^/gmail/v1/users/me/messages/msg_\d+$".to_string()),
        )
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body_from_request(move |request| {
            hits_for_closure.fetch_add(1, Ordering::SeqCst);
            let path = request.path();
            let idx: usize = path
                .rsplit('_')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            serde_json::to_vec(&synthetic_message_json(idx)).unwrap()
        })
        .expect_at_least(1)
        .create_async()
        .await;
    (server, hits)
}

/// Polls `transaction_observations` until its count stops changing (the
/// Transaction Queue's workers drain asynchronously after `run_scan_batches`
/// returns -- the fetch/classify loop and the reconciliation loop are two
/// separate concurrent stages, per Doc 15 Core Principle 7).
async fn wait_for_queue_drain(pool: &deadpool_sqlite::Pool) {
    let mut last = -1i64;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let conn = pool.get().await.unwrap();
        let count: i64 = conn
            .interact(|c| {
                c.query_row("SELECT COUNT(*) FROM transaction_observations", [], |r| {
                    r.get(0)
                })
            })
            .await
            .unwrap()
            .unwrap();
        if count == last {
            return;
        }
        last = count;
    }
}

/// Doc 30 TASK-QA-002 acceptance: `test_1000_email_scan_p95_under_target`.
///
/// Runs 20 independently-timed sub-scans of 50 synthetic messages each
/// (1,000 total) against the mocked Gmail API, computes the p95 wall-clock
/// time of those 20 samples, and asserts the scaled total lands under the
/// documented 15-minutes-per-1,000-emails target -- with the real
/// Transaction Queue worker pool running behind it so extraction throughput
/// and dedup rate reflect genuine end-to-end behavior, not just the
/// fetch/classify stage.
#[tokio::test]
async fn test_1000_email_scan_p95_under_target() {
    let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
    SCAN_QUEUE_PAUSED.store(false, Ordering::Relaxed);

    let pool = migrated_pool("perf").await;
    let app = mock_app();
    let handles = spawn_queues(
        app.clone(),
        pool.clone(),
        dinero_app_lib::learning::spawn_learning_worker(pool.clone()),
    );
    app.manage(handles);

    let (server, _hits) = mock_gmail_server().await;

    const BATCHES: usize = 20;
    const BATCH_SIZE: usize = 50;
    const TOTAL: usize = BATCHES * BATCH_SIZE;
    let account_id = "acc_perf".to_string();

    let mut samples = Vec::with_capacity(BATCHES);
    for batch in 0..BATCHES {
        let ids: Vec<String> = (0..BATCH_SIZE)
            .map(|i| format!("msg_{}", batch * BATCH_SIZE + i))
            .collect();
        let state = ScanCheckpointState {
            start_date: "2026-01-01".into(),
            end_date: "2026-06-30".into(),
            all_message_ids: ids,
            processed_count: 0,
            ..Default::default()
        };

        let client = GmailClient::new_with_base_url(
            "fake_token".to_string(),
            pool.clone(),
            server.url(),
            None,
        );
        let start = Instant::now();
        run_scan_batches(app.clone(), pool.clone(), account_id.clone(), state, client)
            .await
            .expect("run_scan_batches failed");
        samples.push(start.elapsed());
    }

    let p95_per_batch = p95(samples.clone());
    let extrapolated_total = p95_per_batch * (TOTAL / BATCH_SIZE) as u32;
    // Was 15 minutes, which a 48-minute real scan still "passed" because the
    // regression it needed to catch (`dev_review`'s O(n^2) rewrite pinning
    // every tokio worker) lived behind `cfg!(debug_assertions)` and an
    // unbounded on-disk buffer this harness never grew. 60s is the actual
    // product target for 1000 mails. This runs against a local mock, so it
    // measures fetch-loop + extraction + DB, not real Gmail latency or quota
    // pacing -- treat it as the floor the code must not regress past, not as
    // a prediction of end-to-end wall time.
    const TARGET: Duration = Duration::from_secs(60);
    println!(
        "1000-email p95 extrapolated total: {:?} (per-batch p95 {:?}, target {:?})",
        extrapolated_total, p95_per_batch, TARGET
    );
    assert!(
        extrapolated_total <= TARGET,
        "p95 extrapolated total ({:?}) exceeds the 60s-per-1000-email target ({:?}); per-batch samples: {:?}",
        extrapolated_total,
        TARGET,
        samples
    );

    // Extraction throughput / dedup rate: real reconciliation, not asserted
    // inline above so it can't skew the timed portion.
    wait_for_queue_drain(&pool).await;
    let conn = pool.get().await.unwrap();
    let (observation_count, canonical_count): (i64, i64) = conn
        .interact(|c| {
            let obs: i64 = c
                .query_row("SELECT COUNT(*) FROM transaction_observations", [], |r| {
                    r.get(0)
                })
                .unwrap();
            let canon: i64 = c
                .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
                .unwrap();
            Ok::<_, rusqlite::Error>((obs, canon))
        })
        .await
        .unwrap()
        .unwrap();

    // Every non-malformed message (49/50 of TOTAL) passes Gates 1-3 and
    // becomes an observation; malformed ones (idx % 50 == 49) fail Gate 1
    // (no From header at all) and never reach the ladder.
    let expected_observations = TOTAL - (TOTAL / 50);
    assert_eq!(
        observation_count, expected_observations as i64,
        "extraction throughput: expected {expected_observations} observations from {TOTAL} messages"
    );
    // Every 10th non-zero index (within each batch of 50) duplicates index 0
    // of that same batch -- real reconciliation must collapse those into the
    // same canonical row rather than creating a new one per duplicate.
    assert!(
        canonical_count < observation_count,
        "dedup rate: canonical count ({canonical_count}) must be lower than observation count ({observation_count}) given the corpus's intentional duplicates"
    );
}

/// Doc 30 TASK-QA-002 acceptance: `test_checkpoint_resume_after_interruption`.
///
/// Simulates an interruption partway through a scan (a checkpoint with
/// `processed_count` already at 15 of 40) and verifies `run_scan_batches`
/// both completes correctly AND genuinely resumes -- proven via the mock's
/// own hit-count, not just the final `processed_count` (which would read the
/// same even if the resume path wrongly re-fetched from the start).
#[tokio::test]
async fn test_checkpoint_resume_after_interruption() {
    let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
    SCAN_QUEUE_PAUSED.store(false, Ordering::Relaxed);

    let pool = migrated_pool("resume").await;
    let app = mock_app();
    let handles = spawn_queues(
        app.clone(),
        pool.clone(),
        dinero_app_lib::learning::spawn_learning_worker(pool.clone()),
    );
    app.manage(handles);

    let (server, hits) = mock_gmail_server().await;
    let client =
        GmailClient::new_with_base_url("fake_token".to_string(), pool.clone(), server.url(), None);

    const TOTAL: usize = 40;
    const ALREADY_PROCESSED: usize = 15;
    let ids: Vec<String> = (0..TOTAL).map(|i| format!("msg_{i}")).collect();
    let account_id = "acc_resume".to_string();

    let state = ScanCheckpointState {
        start_date: "2026-01-01".into(),
        end_date: "2026-06-30".into(),
        all_message_ids: ids,
        processed_count: ALREADY_PROCESSED,
        ..Default::default()
    };

    run_scan_batches(app.clone(), pool.clone(), account_id.clone(), state, client)
        .await
        .expect("run_scan_batches failed");

    let conn = pool.get().await.unwrap();
    let cp = conn
        .interact(move |c| get_checkpoint(c, "historical_scan", &account_id))
        .await
        .unwrap()
        .unwrap()
        .expect("checkpoint must exist after a completed scan");
    assert_eq!(cp.status, "completed");

    let final_state: ScanCheckpointState = serde_json::from_str(&cp.checkpoint_state_json).unwrap();
    assert_eq!(
        final_state.processed_count, TOTAL,
        "a resumed scan must finish at the true total, not just re-report where it left off"
    );

    // The real proof of resume, not just a plausible-looking final count:
    // only the (TOTAL - ALREADY_PROCESSED) messages after the interruption
    // point should ever have been fetched from Gmail -- each well-formed
    // message is hit twice (metadata fetch, then full fetch once Gate 1
    // passes), per `MessageProcessor::process_message`.
    let expected_fetches = (TOTAL - ALREADY_PROCESSED) * 2;
    assert_eq!(
        hits.load(Ordering::SeqCst),
        expected_fetches,
        "resume must only fetch the messages after the interruption point, not re-fetch from the start"
    );
}

/// Doc 30 TASK-QA-002 acceptance: `test_quota_pause_and_resume_behavior`.
///
/// Sets `SCAN_QUEUE_PAUSED` before starting a scan (simulating an operator/
/// backoff response to Gmail quota exhaustion), proves genuinely zero
/// fetches happen while paused (checked against the mock's own hit count,
/// not an inferred timing gap), then clears it and proves the scan resumes
/// and completes -- exercising the real `wait_while_paused` primitive
/// `run_scan_batches` checks before every single spawn (both the initial
/// priming batch and every refill), not a separate mechanism invented for
/// this test.
#[tokio::test]
async fn test_quota_pause_and_resume_behavior() {
    let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());

    let pool = migrated_pool("quota_pause").await;
    let app = mock_app();
    let handles = spawn_queues(
        app.clone(),
        pool.clone(),
        dinero_app_lib::learning::spawn_learning_worker(pool.clone()),
    );
    app.manage(handles);

    let (server, hits) = mock_gmail_server().await;
    let client =
        GmailClient::new_with_base_url("fake_token".to_string(), pool.clone(), server.url(), None);

    const TOTAL: usize = 10;
    let ids: Vec<String> = (0..TOTAL).map(|i| format!("msg_{i}")).collect();
    let account_id = "acc_quota_pause".to_string();

    // Pause BEFORE the scan starts -- `wait_while_paused` is checked before
    // the very first spawn in the priming loop, so nothing should be
    // fetched at all until this clears.
    SCAN_QUEUE_PAUSED.store(true, Ordering::Relaxed);

    let state = ScanCheckpointState {
        start_date: "2026-01-01".into(),
        end_date: "2026-06-30".into(),
        all_message_ids: ids,
        processed_count: 0,
        ..Default::default()
    };

    let scan_pool = pool.clone();
    let scan_app = app.clone();
    let scan_account = account_id.clone();
    let handle = tokio::spawn(async move {
        run_scan_batches(scan_app, scan_pool, scan_account, state, client).await
    });

    // wait_while_paused polls every 5s -- give it a moment to prove it's
    // genuinely blocked (zero fetches), not just slow to start.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "no fetch may happen while SCAN_QUEUE_PAUSED is set"
    );

    SCAN_QUEUE_PAUSED.store(false, Ordering::Relaxed);

    // 60s (not 15s): when this test runs right after the 1000-message perf
    // test in the same process, `spawn_queues`'s background workers from
    // earlier tests are still alive on Tauri's shared async runtime and
    // compete for scheduling -- observed flaky at 15s under that load, even
    // though the pause/resume mechanism itself was already proven correct
    // in isolation. This margin absorbs contention, not a logic change.
    tokio::time::timeout(Duration::from_secs(60), handle)
        .await
        .expect("scan did not resume and complete within 60s of unpausing")
        .expect("task panicked")
        .expect("run_scan_batches failed");

    let conn = pool.get().await.unwrap();
    let cp = conn
        .interact(move |c| get_checkpoint(c, "historical_scan", &account_id))
        .await
        .unwrap()
        .unwrap()
        .expect("checkpoint must exist after the scan resumes and completes");
    assert_eq!(cp.status, "completed");
    let final_state: ScanCheckpointState = serde_json::from_str(&cp.checkpoint_state_json).unwrap();
    assert_eq!(final_state.processed_count, TOTAL);
    // Each well-formed message is hit twice (metadata, then full) -- see the
    // resume test's identical note.
    assert_eq!(
        hits.load(Ordering::SeqCst),
        TOTAL * 2,
        "all messages must be fetched once resumed"
    );

    SCAN_QUEUE_PAUSED.store(false, Ordering::Relaxed);
}

/// Root-cause regression for the reported "clicking Cancel doesn't stop the
/// scan" bug. The pre-existing cancellation test
/// (`historical_scan.rs`'s `test_historical_scan_cancellation_emits_scan_cancelled_not_scan_completed`)
/// only proves the flag-check mechanism works when cancellation is requested
/// *before* the scan starts (nothing is ever in flight). It never exercises
/// the real `scans_cancel` command against a scan with genuinely concurrent
/// in-flight fetches (up to `MAX_CONCURRENT_FETCHES` = 50 in production) --
/// the actual scenario a user hits clicking Cancel mid-scan. This drives the
/// real command against a large corpus and proves the scan halts well short
/// of the full total, not just that it eventually records a cancellation
/// after silently processing everything anyway.
#[tokio::test]
async fn test_cancel_mid_flight_stops_before_processing_all_messages() {
    let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
    SCAN_QUEUE_PAUSED.store(false, Ordering::Relaxed);

    let pool = migrated_pool("cancel_mid_flight").await;
    let app = mock_app();
    let handles = spawn_queues(
        app.clone(),
        pool.clone(),
        dinero_app_lib::learning::spawn_learning_worker(pool.clone()),
    );
    app.manage(handles);

    let (server, hits) = mock_gmail_server().await;
    let client =
        GmailClient::new_with_base_url("fake_token".to_string(), pool.clone(), server.url(), None);

    const TOTAL: usize = 300;
    let ids: Vec<String> = (0..TOTAL).map(|i| format!("msg_{i}")).collect();
    // `scans_cancel` validates the `gmail_<uuid>` shape (see
    // `validate_account_id`) -- unlike this file's other tests, which
    // construct/read checkpoints directly and never pass their plain
    // "acc_..." id through that validator.
    let account_id = format!("gmail_{}", uuid::Uuid::new_v4());

    let state = ScanCheckpointState {
        start_date: "2026-01-01".into(),
        end_date: "2026-06-30".into(),
        all_message_ids: ids,
        processed_count: 0,
        ..Default::default()
    };

    let scan_pool = pool.clone();
    let scan_app = app.clone();
    let scan_account = account_id.clone();
    let handle = tokio::spawn(async move {
        run_scan_batches(scan_app, scan_pool, scan_account, state, client).await
    });

    // Let a handful of real fetches actually land before requesting
    // cancellation -- proves this exercises genuinely concurrent in-flight
    // requests, not a cancel-before-anything-happens shortcut.
    for _ in 0..2000 {
        if hits.load(Ordering::SeqCst) >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        hits.load(Ordering::SeqCst) >= 2,
        "scan never got moving after 10s ({} hits) -- test setup is broken, not proving cancellation",
        hits.load(Ordering::SeqCst)
    );

    dinero_app_lib::ingestion::historical_scan::scans_cancel(account_id.clone())
        .await
        .expect("scans_cancel failed");

    tokio::time::timeout(Duration::from_secs(30), handle)
        .await
        .expect("scan did not stop within 30s of cancellation -- this is the reported bug")
        .expect("task panicked")
        .expect("run_scan_batches failed");

    let conn = pool.get().await.unwrap();
    let cp = conn
        .interact(move |c| get_checkpoint(c, "historical_scan", &account_id))
        .await
        .unwrap()
        .unwrap();
    assert!(
        cp.is_none(),
        "cancelling wipes progress so the next scan starts from scratch -- the \
         checkpoint must be deleted, not left behind as resumable state"
    );

    // Every well-formed message is fetched twice (metadata, then full) --
    // see test_checkpoint_resume_after_interruption's identical note. If
    // cancellation actually stopped the fetch loop, total hits must land far
    // short of processing the full 300-message corpus.
    let final_hits = hits.load(Ordering::SeqCst);
    assert!(
        final_hits < TOTAL * 2,
        "cancellation did not stop the scan early: {final_hits} fetches happened, \
         which is the full {TOTAL}-message corpus (x2 for metadata+full) -- \
         the scan ran to completion instead of stopping"
    );
}

/// Root-cause regression, second half: the previous test proves cancellation
/// works when in-flight fetches resolve at a normal pace. It does NOT cover
/// the actual production failure mode -- every in-flight fetch stuck at once
/// (a Gmail rate-limit backoff sleep, or an unresponsive local LLM call
/// during Layer 6 classification). Before the fix, `run_scan_batches`' loop
/// only got a chance to check `is_scan_cancelled` when `join_set.join_next()`
/// resolved -- if every in-flight task is stuck, that never happens, and
/// `scans_cancel` has no way to take effect no matter how long the user
/// waits. This makes every in-flight fetch hang indefinitely and asserts the
/// scan still stops within a few seconds (the new 1s cancellation-poll
/// ticker), not "whenever the stuck request happens to resolve, if ever."
///
/// `flavor = "multi_thread"`: the mock server's response handler blocks its
/// worker thread with `std::thread::sleep` to simulate a hung request --
/// needs more than one worker thread so the scan's own polling loop (a
/// separate, non-blocking task) can still make progress.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_cancel_is_not_blocked_by_a_stuck_in_flight_fetch() {
    let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
    SCAN_QUEUE_PAUSED.store(false, Ordering::Relaxed);

    let pool = migrated_pool("cancel_stuck_fetch").await;
    let app = mock_app();
    let handles = spawn_queues(
        app.clone(),
        pool.clone(),
        dinero_app_lib::learning::spawn_learning_worker(pool.clone()),
    );
    app.manage(handles);

    // A hand-rolled hanging mock rather than `mock_gmail_server()`: every
    // request blocks for far longer than this test's assertion window,
    // simulating an in-flight fetch that never resolves on its own.
    const HANG_SECS: u64 = 8;
    let mut server = mockito::Server::new_async().await;
    server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"^/gmail/v1/users/me/messages/msg_\d+$".to_string()),
        )
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body_from_request(move |request| {
            std::thread::sleep(Duration::from_secs(HANG_SECS));
            let path = request.path();
            let idx: usize = path
                .rsplit('_')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            serde_json::to_vec(&synthetic_message_json(idx)).unwrap()
        })
        .expect_at_least(1)
        .create_async()
        .await;

    let client =
        GmailClient::new_with_base_url("fake_token".to_string(), pool.clone(), server.url(), None);

    // Small corpus -- every fetch it dispatches gets stuck in the mock's 8s
    // sleep, so there's no risk of exhausting the test's worker threads.
    const TOTAL: usize = 2;
    let ids: Vec<String> = (0..TOTAL).map(|i| format!("msg_{i}")).collect();
    let account_id = format!("gmail_{}", uuid::Uuid::new_v4());

    let state = ScanCheckpointState {
        start_date: "2026-01-01".into(),
        end_date: "2026-06-30".into(),
        all_message_ids: ids,
        processed_count: 0,
        ..Default::default()
    };

    let scan_pool = pool.clone();
    let scan_app = app.clone();
    let scan_account = account_id.clone();
    let handle = tokio::spawn(async move {
        run_scan_batches(scan_app, scan_pool, scan_account, state, client).await
    });

    // Give the scan a brief moment to actually dispatch its fetches (so
    // they're genuinely in flight and stuck, not cancelled before anything
    // started), well short of the 8s hang.
    tokio::time::sleep(Duration::from_millis(200)).await;

    dinero_app_lib::ingestion::historical_scan::scans_cancel(account_id.clone())
        .await
        .expect("scans_cancel failed");

    // The old code could only notice cancellation once a fetch resolved --
    // with every fetch stuck for HANG_SECS (8s), that means this would not
    // return until ~8s. The fix's 1s poll ticker should catch it well
    // before that; 5s leaves comfortable margin over the ticker's 1s period
    // while still failing well short of the old behavior's ~8s floor.
    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect(
            "scan did not stop within 5s of cancellation while every in-flight fetch was stuck \
             -- this is the reported \"Cancel doesn't stop the process\" bug",
        )
        .expect("task panicked")
        .expect("run_scan_batches failed");

    let conn = pool.get().await.unwrap();
    let cp = conn
        .interact(move |c| get_checkpoint(c, "historical_scan", &account_id))
        .await
        .unwrap()
        .unwrap();
    assert!(
        cp.is_none(),
        "a scan cancelled while every in-flight fetch was stuck must still wipe its \
         checkpoint, same as the normal-pace cancellation path"
    );
}
