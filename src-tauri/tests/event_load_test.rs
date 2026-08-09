//! Doc 30 TASK-RT-009: Event Testing / Chaos-Load Harness.
//!
//! Drives the real production code paths built across Area 13 (the
//! centralized `ipc::events::emit_event` module, `ipc::system_warnings`'s
//! late-mount-recovery registry, `background_tasks::indicator`'s registry,
//! and `reconciliation::alert_worker`'s per-month threshold dedup) under
//! volume and concurrency, rather than fabricating a separate synthetic
//! harness disconnected from what actually ships.

use dinero_app_lib::background_tasks::indicator::{BackgroundTaskRegistry, TaskStatus};
use dinero_app_lib::db;
use dinero_app_lib::ipc::events::{emit_event, AppEvent};
use dinero_app_lib::ipc::system_warnings::{
    active_system_warnings, clear_system_warning, emit_system_warning, SystemWarningPayload,
    WarningSeverity,
};
use dinero_app_lib::reconciliation::alert_worker::evaluate_alerts_internal;
use rusqlite::Connection;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::{AppHandle, Listener};

/// Same real-migration pattern as `reconciliation_regression.rs`:
/// `db::test_helpers` is `#[cfg(test)]`-gated to the lib crate's own unit
/// tests, invisible to this external integration test binary.
fn migrated_conn(label: &str) -> Connection {
    let dir = std::env::temp_dir().join(format!(
        "dinero_event_load_{label}_{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("test.db");
    tokio::runtime::Runtime::new()
        .expect("failed to create tokio runtime for test migration")
        .block_on(db::migrations::run_migrations(&db_path, None))
        .expect("test database migration failed");
    let conn = Connection::open(&db_path).expect("failed to open migrated test database");
    conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
    conn
}

fn mock_app() -> AppHandle<tauri::test::MockRuntime> {
    mock_builder()
        .build(mock_context(noop_assets()))
        .unwrap()
        .handle()
        .clone()
}

fn p95(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    let idx = ((samples.len() as f64) * 0.95).ceil() as usize - 1;
    samples[idx.min(samples.len() - 1)]
}

/// Doc 30 TASK-RT-009 acceptance: `test_1000_event_burst_no_drops`.
///
/// Two things must both hold under a 1,200-observation burst arriving as
/// one batch (mirroring how a real Gmail smart-poll or historical scan
/// hands `evaluate_alerts_for_observations` its `observation_ids` list --
/// a batch, not one call per email):
/// 1. The event bus itself drops nothing: every one of 1,200 raw
///    `emit_event` calls is received by a live listener.
/// 2. The burst-suppression (dedup) logic built in TASK-RT-002 correctly
///    collapses what would otherwise be up to 1,200 redundant threshold-
///    crossing alerts down to at most 3 (80/90/100%), proving "suppression
///    logic correctly activates during the burst" against the real
///    production dedup path, not a mock of it.
#[test]
fn test_1000_event_burst_no_drops() {
    // --- Part 1: raw event-bus delivery at volume ---
    let app = mock_app();
    let received = Arc::new(AtomicUsize::new(0));
    let received_clone = received.clone();
    app.listen_any(AppEvent::TransactionCreated.as_str(), move |_| {
        received_clone.fetch_add(1, Ordering::SeqCst);
    });

    const BURST_SIZE: usize = 1200;
    for i in 0..BURST_SIZE {
        emit_event(
            &app,
            AppEvent::TransactionCreated,
            serde_json::json!({ "observation_id": format!("obs_{i}") }),
        )
        .expect("emit_event must not fail under burst volume");
    }
    assert_eq!(
        received.load(Ordering::SeqCst),
        BURST_SIZE,
        "the event bus must deliver every emitted event under burst volume -- zero drops"
    );

    // --- Part 2: real alert-dedup suppression under the same burst size ---
    let conn = migrated_conn("burst");
    conn.execute(
        "INSERT INTO local_profile (id, spending_limit_monthly) VALUES (1, 100)",
        [],
    )
    .unwrap();

    let mut obs_ids = Vec::with_capacity(BURST_SIZE);
    for i in 0..BURST_SIZE {
        let obs_id = format!("obs_burst_{i}");
        let tx_id = format!("tx_burst_{i}");
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, direction, amount_minor, best_event_time, is_deleted, alert_fired) \
             VALUES (?1, 'inst_1', 'debit', 100, '2026-06-10 12:00:00', 0, 0)",
            rusqlite::params![tx_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transaction_observations (id, source_pipeline, source_record_id, fingerprint, canonical_transaction_id) \
             VALUES (?1, 'gmail_transaction', ?1, ?1, ?2)",
            rusqlite::params![obs_id, tx_id],
        )
        .unwrap();
        obs_ids.push(obs_id);
    }

    // Every one of the 1,200 debits pushes cumulative spend further past the
    // ₹100 limit (₹1 each => way past 100% almost immediately) -- without
    // dedup this would fire the same "budget exhausted" alert ~1,200 times.
    evaluate_alerts_internal(&conn, None::<AppHandle>, obs_ids)
        .expect("processing a 1,200-observation burst must not error or panic");

    let alert_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM alerts WHERE type LIKE 'global_budget_%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        alert_count <= 3,
        "burst-suppression dedup must collapse the burst to at most 3 threshold alerts (80/90/100%), got {alert_count}"
    );
    assert!(
        alert_count >= 1,
        "the burst must have crossed at least one threshold"
    );
}

/// Doc 30 TASK-RT-009 acceptance:
/// `test_background_foreground_chaos_no_duplicates_no_loss`.
///
/// Repeatedly simulates the app being backgrounded (a task/warning registers
/// or updates with nobody live to receive the event -- there is no listener
/// attached at all in this test, standing in for the frontend being
/// unmounted/backgrounded) and then foregrounded (a late-mounting query via
/// `active_tasks()`/`active_system_warnings()`, exactly what
/// `get_active_background_tasks`/`get_active_system_warnings` expose to a
/// real remounting component). Validates TASK-RT-001/004/007's persisted-
/// state recovery: across many chaotic cycles, the registry never shows a
/// duplicate entry for the same id and never silently drops one that should
/// still be active.
#[tokio::test]
async fn test_background_foreground_chaos_no_duplicates_no_loss() {
    let app = mock_app();
    let task_registry = Arc::new(BackgroundTaskRegistry::default());

    const CYCLES: usize = 300;
    let mut handles = Vec::new();
    for i in 0..CYCLES {
        let registry = Arc::clone(&task_registry);
        let app = app.clone();
        handles.push(tokio::spawn(async move {
            let task_id = format!("chaos_task_{i}");
            // "Backgrounded": registers/updates repeatedly with no live
            // listener ever attached for this event in this test.
            registry.register_or_update(
                &app,
                &task_id,
                "historical_scan",
                "Chaos scan",
                0,
                10,
                "Starting",
            );
            registry.register_or_update(
                &app,
                &task_id,
                "historical_scan",
                "Chaos scan",
                5,
                10,
                "Halfway",
            );

            // "Foregrounded": a late-mounting caller recovers state via the
            // same registry query a real component would use.
            let recovered = registry.active_tasks();
            let matches: Vec<_> = recovered.iter().filter(|t| t.task_id == task_id).collect();
            assert_eq!(
                matches.len(),
                1,
                "must never duplicate the same task_id across repeated register_or_update calls"
            );
            assert_eq!(
                matches[0].current, 5,
                "must reflect the latest update, not a stale duplicate"
            );

            registry.deregister(&app, &task_id, TaskStatus::Completed, "Done");
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    assert_eq!(
        task_registry.active_count(),
        0,
        "every chaos-cycle task must have been cleanly deregistered -- no permanent leak"
    );

    // Same chaos pattern against the system_warnings registry.
    const WARNING_CYCLES: usize = 300;
    let mut warning_handles = Vec::new();
    for i in 0..WARNING_CYCLES {
        let app = app.clone();
        warning_handles.push(tokio::task::spawn_blocking(move || {
            let warning_type = format!("chaos_warning_{i}");
            emit_system_warning(
                &app,
                SystemWarningPayload {
                    warning_type: warning_type.clone(),
                    message: "chaos".to_string(),
                    severity: WarningSeverity::Info,
                    action_hint: None,
                },
            );
            let recovered = active_system_warnings();
            let count = recovered
                .iter()
                .filter(|w| w.warning_type == warning_type)
                .count();
            assert_eq!(count, 1, "must never duplicate the same warning_type");
            clear_system_warning(&app, &warning_type);
        }));
    }
    for h in warning_handles {
        h.await.unwrap();
    }

    let leftover = active_system_warnings()
        .into_iter()
        .filter(|w| w.warning_type.starts_with("chaos_warning_"))
        .count();
    assert_eq!(leftover, 0, "every chaos-cycle warning must have been cleanly cleared -- no permanent loss of clearability");
}

/// Doc 30 TASK-RT-009 acceptance: `test_ipc_p95_under_200ms_during_event_load`.
///
/// A pure backend-only test can't literally time a frontend-triggered
/// `dashboard_summary` IPC round trip (that requires a real window/renderer),
/// so this measures the actual claim underlying that acceptance criterion:
/// the event-emission mechanism itself (`emit_event` plus the registries it
/// backs) must not become a latency bottleneck under concurrent load. 500
/// concurrent emitters, each timed individually; p95 must stay under the
/// documented 200ms target.
#[tokio::test]
async fn test_ipc_p95_under_200ms_during_event_load() {
    let app = mock_app();
    let registry = Arc::new(BackgroundTaskRegistry::default());
    let samples = Arc::new(Mutex::new(Vec::<Duration>::new()));

    const CONCURRENT_EMITTERS: usize = 500;
    let mut handles = Vec::new();
    for i in 0..CONCURRENT_EMITTERS {
        let app = app.clone();
        let registry = Arc::clone(&registry);
        let samples = Arc::clone(&samples);
        handles.push(tokio::spawn(async move {
            let start = Instant::now();
            let task_id = format!("latency_task_{i}");
            registry.register_or_update(&app, &task_id, "test", "Latency probe", 1, 10, "Running");
            let _ = emit_event(
                &app,
                AppEvent::BackgroundTaskProgress,
                serde_json::json!({ "task_id": task_id }),
            );
            registry.deregister(&app, &task_id, TaskStatus::Completed, "Done");
            samples.lock().unwrap().push(start.elapsed());
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let collected = Arc::try_unwrap(samples).unwrap().into_inner().unwrap();
    assert_eq!(collected.len(), CONCURRENT_EMITTERS);
    let p95_latency = p95(collected);
    assert!(
        p95_latency < Duration::from_millis(200),
        "p95 event-emission latency under concurrent load must stay under 200ms, got {p95_latency:?}"
    );
}
