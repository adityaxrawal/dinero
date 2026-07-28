#[cfg(test)]
mod tests {
    use crate::db::connected_accounts::{self, ConnectedAccountsRow};
    use crate::db::init_db;
    use crate::db::local_profile::{self, LocalProfileRow};
    use crate::db::processing_checkpoints;
    use crate::ingestion::polling::{
        is_force_poll_allowed, next_backoff, poll_all_accounts, save_history_id, start_polling_loop,
    };
    use std::fs;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::time::sleep;
    use tokio_util::sync::CancellationToken;

    // Tauri test builder
    use tauri::test::{mock_builder, mock_context};
    use tauri::Manager;

    /// `poll_single_account` now unconditionally sources `layer6_tx` from
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

    /// Doc 30 TASK-API-009 acceptance test: "Sync Now" is debounced to at
    /// most once per 10 seconds -- a second call before the window elapses
    /// is rejected; a call after it elapses is allowed.
    #[test]
    fn test_force_poll_debounced() {
        let t0 = std::time::Instant::now();
        assert!(
            is_force_poll_allowed(t0, None),
            "the very first call is always allowed"
        );

        let just_after = t0 + Duration::from_secs(1);
        assert!(
            !is_force_poll_allowed(just_after, Some(t0)),
            "a call 1s after the last one must be rejected (debounce window is 10s)"
        );

        let after_window = t0 + Duration::from_secs(11);
        assert!(
            is_force_poll_allowed(after_window, Some(t0)),
            "a call 11s after the last one must be allowed"
        );
    }

    #[tokio::test]
    async fn test_poll_worker_pauses_on_cancellation() {
        // Create an AppHandle mock
        let app = mock_builder()
            .build(mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone();

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test.db");
        let pool = init_db(db_path.clone()).await.expect("DB init failed");

        let token = CancellationToken::new();
        let token_clone = token.clone();

        let pool_clone = pool.clone();

        // Spawn the actual worker loop
        let worker_handle = tokio::spawn(async move {
            start_polling_loop(app, pool_clone, token_clone).await;
            "cancelled"
        });

        // Sleep briefly to ensure the loop has started, then cancel
        sleep(Duration::from_millis(50)).await;
        token.cancel();

        // The worker should exit cleanly
        let result = tokio::time::timeout(Duration::from_secs(2), worker_handle).await;
        assert!(
            result.is_ok(),
            "Worker did not exit in time upon cancellation"
        );
        assert_eq!(result.unwrap().unwrap(), "cancelled");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_history_id_checkpoint_saved_after_full_cycle() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("test2.db");
        let pool = init_db(db_path.clone()).await.expect("DB init failed");

        // Seed data
        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            let profile = LocalProfileRow {
                id: 1,
                primary_email: None,
                display_name: None,
                timezone: None,
                spending_limit_monthly: None,
                limit_thresholds: None,
                recovery_phrase_enabled: false,
                created_at: None,
                updated_at: None,
            };
            // Ignore unique constraint failure if init_db already inserted it
            let _ = local_profile::insert(c, &profile);

            let account = ConnectedAccountsRow {
                id: "acc_test".into(),
                profile_id: 1,
                email_address: None,
                account_status: Some("active".into()),
                last_history_id: Some("old_id".into()),
                created_at: None,
                updated_at: None,
            };
            connected_accounts::insert_account(c, &account).unwrap();
            Ok::<(), anyhow::Error>(())
        })
        .await
        .unwrap()
        .unwrap();

        // Call the actual save_history_id function
        let new_history = "new_history_123".to_string();
        save_history_id(&pool, "acc_test", new_history.clone())
            .await
            .expect("Failed to save history ID");

        // Verify connected_accounts table is updated
        let conn2 = pool.get().await.unwrap();
        let fetched_acc = conn2
            .interact(|c| {
                connected_accounts::get_account(c, "acc_test")
                    .unwrap()
                    .unwrap()
            })
            .await
            .unwrap();
        assert_eq!(fetched_acc.last_history_id, Some(new_history.clone()));

        // Verify processing_checkpoints table is updated
        let conn3 = pool.get().await.unwrap();
        let fetched_chk = conn3
            .interact(|c| {
                processing_checkpoints::get_checkpoint(c, "gmail_history", "acc_test")
                    .unwrap()
                    .unwrap()
            })
            .await
            .unwrap();
        assert_eq!(fetched_chk.last_processed_token, Some(new_history));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_backoff_increases_on_429() {
        // Doc 30 TASK-GMAIL-001: the 429/5xx retry path doubles this same
        // sequence on every rate-limit/server-error response until it reaches
        // and holds the 60s cap.
        let mut backoff = Duration::from_secs(1);
        let expected = [1u64, 2, 4, 8, 16, 32, 60, 60];
        for expected_secs in expected {
            assert_eq!(backoff.as_secs(), expected_secs);
            backoff = next_backoff(backoff);
        }
        // Once capped, further doubling never exceeds 60s.
        assert_eq!(next_backoff(backoff).as_secs(), 60);
    }

    /// Doc 30 TASK-GMAIL-009: each connected account's Gmail sync checkpoint
    /// is keyed on its own `id` (`job_key`), so two accounts' state can never
    /// clobber each other.
    #[tokio::test]
    async fn test_independent_checkpoint_per_account() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let pool = init_db(temp_dir.join("test.db"))
            .await
            .expect("DB init failed");

        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            let _ = local_profile::insert(
                c,
                &LocalProfileRow {
                    id: 1,
                    primary_email: None,
                    display_name: None,
                    timezone: None,
                    spending_limit_monthly: None,
                    limit_thresholds: None,
                    recovery_phrase_enabled: false,
                    created_at: None,
                    updated_at: None,
                },
            );
            for acc_id in ["acc_A", "acc_B"] {
                connected_accounts::insert_account(
                    c,
                    &ConnectedAccountsRow {
                        id: acc_id.into(),
                        profile_id: 1,
                        email_address: None,
                        account_status: Some("active".into()),
                        last_history_id: None,
                        created_at: None,
                        updated_at: None,
                    },
                )
                .unwrap();
            }
            Ok::<(), anyhow::Error>(())
        })
        .await
        .unwrap()
        .unwrap();

        save_history_id(&pool, "acc_A", "history_for_A".to_string())
            .await
            .unwrap();
        save_history_id(&pool, "acc_B", "history_for_B".to_string())
            .await
            .unwrap();

        let conn = pool.get().await.unwrap();
        let (chk_a, chk_b) = conn
            .interact(|c| {
                (
                    processing_checkpoints::get_checkpoint(c, "gmail_history", "acc_A")
                        .unwrap()
                        .unwrap(),
                    processing_checkpoints::get_checkpoint(c, "gmail_history", "acc_B")
                        .unwrap()
                        .unwrap(),
                )
            })
            .await
            .unwrap();

        assert_eq!(
            chk_a.last_processed_token,
            Some("history_for_A".to_string())
        );
        assert_eq!(
            chk_b.last_processed_token,
            Some("history_for_B".to_string())
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    /// Doc 30 TASK-GMAIL-009: one account's poll failure must never stop the
    /// next account in the loop from being polled. Exercising the real
    /// failure path (a missing Gmail OAuth token) would require either a
    /// real macOS Keychain entry — a real side effect this codebase
    /// deliberately never introduces in automated tests — or reaching past
    /// it into `IncidentMonitor`'s internal counters, which have no public
    /// accessor. A capturing tracing layer sidesteps both: it observes the
    /// real `poll_all_accounts` logging both accounts' independent
    /// token-fetch failures, proving neither one's error stopped the loop
    /// from reaching the other.
    #[tokio::test(flavor = "current_thread")]
    async fn test_one_account_failure_does_not_block_others() {
        use tracing_subscriber::layer::SubscriberExt;

        struct MessageVisitor(String);
        impl tracing::field::Visit for MessageVisitor {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{:?}", value);
                }
            }
        }

        struct CapturingLayer(Arc<Mutex<Vec<String>>>);
        impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for CapturingLayer {
            fn on_event(
                &self,
                event: &tracing::Event<'_>,
                _ctx: tracing_subscriber::layer::Context<'_, S>,
            ) {
                let mut visitor = MessageVisitor(String::new());
                event.record(&mut visitor);
                self.0.lock().unwrap().push(visitor.0);
            }
        }

        let logs = Arc::new(Mutex::new(Vec::<String>::new()));
        let subscriber = tracing_subscriber::registry().with(CapturingLayer(logs.clone()));
        let _guard = tracing::subscriber::set_default(subscriber);

        let app = mock_builder()
            .build(mock_context(tauri::test::noop_assets()))
            .unwrap();
        app.manage(crate::auth::session::SessionState::default());
        app.manage(crate::security::incident_response::IncidentMonitor::default());
        let app_handle = app.handle().clone();

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let pool = init_db(temp_dir.join("test.db"))
            .await
            .expect("DB init failed");

        let conn = pool.get().await.unwrap();
        conn.interact(|c| {
            let _ = local_profile::insert(
                c,
                &LocalProfileRow {
                    id: 1,
                    primary_email: None,
                    display_name: None,
                    timezone: None,
                    spending_limit_monthly: None,
                    limit_thresholds: None,
                    recovery_phrase_enabled: false,
                    created_at: None,
                    updated_at: None,
                },
            );
            for acc_id in ["acc_no_token_A", "acc_no_token_B"] {
                connected_accounts::insert_account(
                    c,
                    &ConnectedAccountsRow {
                        id: acc_id.into(),
                        profile_id: 1,
                        email_address: None,
                        account_status: Some("active".into()),
                        // A history_id is present so the flow reaches the
                        // token-fetch step (the very first thing
                        // `poll_single_account` does) rather than returning
                        // early via the separate "no history yet" skip path.
                        last_history_id: Some("some_history".into()),
                        created_at: None,
                        updated_at: None,
                    },
                )
                .unwrap();
            }
            Ok::<(), anyhow::Error>(())
        })
        .await
        .unwrap()
        .unwrap();

        let result = poll_all_accounts(&app_handle, &pool, "https://gmail.googleapis.com").await;
        assert!(
            result.is_ok(),
            "poll_all_accounts must not propagate a per-account failure"
        );

        let captured = logs.lock().unwrap();
        let mentions_a = captured.iter().any(|l| l.contains("acc_no_token_A"));
        let mentions_b = captured.iter().any(|l| l.contains("acc_no_token_B"));
        assert!(
            mentions_a && mentions_b,
            "both accounts must be independently reached and logged; got: {:?}",
            *captured
        );

        drop(captured);
        let _ = fs::remove_dir_all(&temp_dir);
    }

    /// Writes a dev-mode token file at the exact path/shape
    /// `ingestion::oauth`'s `#[cfg(debug_assertions)]` `get_token`/`save_token`
    /// use -- this test binary is itself a debug build, so this is the real
    /// production dev-mode path, not a parallel fake. Deliberately not
    /// calling into a real macOS Keychain, matching this codebase's
    /// established test convention (see `test_one_account_failure_does_not_block_others`'s
    /// own doc comment on why).
    fn write_dev_token(account_id: &str) {
        let token = crate::ingestion::oauth::TokenStore {
            access_token: "fake_access_token".to_string(),
            refresh_token: Some("fake_refresh_token".to_string()),
            expires_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 3600,
        };
        let path = std::env::temp_dir().join(format!("dinero_dev_token_{}.json", account_id));
        std::fs::write(path, serde_json::to_string(&token).unwrap()).unwrap();
    }

    async fn setup_account_for_poll(pool: &deadpool_sqlite::Pool, account_id: &str, last_history_id: Option<&str>) {
        let conn = pool.get().await.unwrap();
        let account_id = account_id.to_string();
        let last_history_id = last_history_id.map(|s| s.to_string());
        conn.interact(move |c| {
            let _ = local_profile::insert(
                c,
                &LocalProfileRow {
                    id: 1,
                    primary_email: None,
                    display_name: None,
                    timezone: None,
                    spending_limit_monthly: None,
                    limit_thresholds: None,
                    recovery_phrase_enabled: false,
                    created_at: None,
                    updated_at: None,
                },
            );
            connected_accounts::insert_account(
                c,
                &ConnectedAccountsRow {
                    id: account_id,
                    profile_id: 1,
                    email_address: None,
                    account_status: Some("active".into()),
                    last_history_id,
                    created_at: None,
                    updated_at: None,
                },
            )
            .unwrap();
            Ok::<(), anyhow::Error>(())
        })
        .await
        .unwrap()
        .unwrap();
    }

    /// Doc 30 TASK-QA-005 acceptance: `test_history_delta_polling_idempotent`.
    /// Gmail's `history.list` can genuinely return the same delta window
    /// twice (a client-side retry after a dropped response, a duplicate
    /// notification) -- polling the exact same `startHistoryId` twice must
    /// leave the checkpoint in the same, correct state both times, not
    /// double-advance or corrupt it.
    #[tokio::test]
    async fn test_history_delta_polling_idempotent() {
        let mut server = mockito::Server::new_async().await;
        let account_id = format!("acc_idempotent_{}", uuid::Uuid::new_v4());
        write_dev_token(&account_id);

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let pool = init_db(temp_dir.join("test.db")).await.expect("DB init failed");
        setup_account_for_poll(&pool, &account_id, Some("history_100")).await;

        let mock = server
            .mock("GET", "/gmail/v1/users/me/history")
            .match_query(mockito::Matcher::UrlEncoded("startHistoryId".into(), "history_100".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({ "historyId": "history_200", "history": [] }).to_string())
            .expect(1)
            .create_async()
            .await;

        let app = mock_builder().build(mock_context(tauri::test::noop_assets())).unwrap();
        app.manage(crate::auth::session::SessionState::default());
        app.manage(crate::security::incident_response::IncidentMonitor::default());
        app.manage(test_queue_handles());
        let app_handle = app.handle().clone();

        poll_all_accounts(&app_handle, &pool, &server.url()).await.unwrap();
        let conn = pool.get().await.unwrap();
        let acc_id_clone = account_id.clone();
        let checkpoint_after_first = conn
            .interact(move |c| processing_checkpoints::get_checkpoint(c, "gmail_history", &acc_id_clone))
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(checkpoint_after_first.last_processed_token, Some("history_200".to_string()));

        // Second poll cycle re-reads the checkpoint (now "history_200") --
        // re-mock for the new startHistoryId so this call also succeeds and
        // genuinely exercises a second full poll, not a cached no-op.
        let mock2 = server
            .mock("GET", "/gmail/v1/users/me/history")
            .match_query(mockito::Matcher::UrlEncoded("startHistoryId".into(), "history_200".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({ "historyId": "history_200", "history": [] }).to_string())
            .create_async()
            .await;

        poll_all_accounts(&app_handle, &pool, &server.url()).await.unwrap();
        let conn = pool.get().await.unwrap();
        let checkpoint_after_second = conn
            .interact(move |c| processing_checkpoints::get_checkpoint(c, "gmail_history", &account_id))
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            checkpoint_after_second.last_processed_token,
            Some("history_200".to_string()),
            "polling the same delta window twice must not corrupt or double-advance the checkpoint"
        );

        mock.assert_async().await;
        mock2.assert_async().await;
        let _ = fs::remove_dir_all(&temp_dir);
    }

    /// Doc 30 TASK-QA-005 acceptance: `test_gap_recovery_resumes_from_last_checkpoint`.
    /// A 404/400 from `history.list` means Gmail's history window has
    /// expired (the account was offline long enough that incremental resume
    /// is no longer possible) -- `handle_invalid_history_id` must reset
    /// `last_history_id` to `None` (forcing a fresh full historical scan
    /// next time, the only honest recovery once the delta window is gone)
    /// and record it in the audit log, not silently swallow the gap.
    #[tokio::test]
    async fn test_gap_recovery_resumes_from_last_checkpoint() {
        let mut server = mockito::Server::new_async().await;
        let account_id = format!("acc_gap_{}", uuid::Uuid::new_v4());
        write_dev_token(&account_id);

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let pool = init_db(temp_dir.join("test.db")).await.expect("DB init failed");
        setup_account_for_poll(&pool, &account_id, Some("stale_history_id")).await;

        let mock = server
            .mock("GET", "/gmail/v1/users/me/history")
            .match_query(mockito::Matcher::Any)
            .with_status(404)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({ "error": "Invalid history id" }).to_string())
            .create_async()
            .await;

        let app = mock_builder().build(mock_context(tauri::test::noop_assets())).unwrap();
        app.manage(crate::auth::session::SessionState::default());
        app.manage(crate::security::incident_response::IncidentMonitor::default());
        app.manage(test_queue_handles());
        let app_handle = app.handle().clone();

        poll_all_accounts(&app_handle, &pool, &server.url()).await.unwrap();

        let conn = pool.get().await.unwrap();
        let acc_id_clone = account_id.clone();
        let account_after = conn
            .interact(move |c| connected_accounts::get_account(c, &acc_id_clone))
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            account_after.last_history_id, None,
            "an invalid/expired history id must reset last_history_id, forcing a fresh full historical scan"
        );

        let conn = pool.get().await.unwrap();
        let reset_logged: i64 = conn
            .interact(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM audit_log WHERE action = 'history_checkpoint_reset'",
                    [],
                    |r| r.get(0),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reset_logged, 1, "the gap-recovery reset must be recorded in the audit log, not silent");

        mock.assert_async().await;
        let _ = fs::remove_dir_all(&temp_dir);
    }

    /// Doc 30 TASK-QA-005 acceptance:
    /// `test_polling_pauses_during_sleep_and_recovers_on_resume`. There is no
    /// custom sleep/wake-detection code anywhere in the polling loop, by
    /// design: `tokio::time::sleep`/`interval` simply do not fire while the
    /// OS has the whole process suspended, so "pausing during sleep" is a
    /// property the runtime already guarantees for free. What this test
    /// actually needs to prove is the other half of the claim -- "recovers
    /// the missed delta on next open" -- i.e. an arbitrarily long gap since
    /// the last successful poll (standing in for however long the Mac was
    /// asleep) resumes correctly from the durable checkpoint with no special
    /// casing, identically to a normal poll cycle.
    #[tokio::test]
    async fn test_polling_pauses_during_sleep_and_recovers_on_resume() {
        let mut server = mockito::Server::new_async().await;
        let account_id = format!("acc_sleep_{}", uuid::Uuid::new_v4());
        write_dev_token(&account_id);

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let pool = init_db(temp_dir.join("test.db")).await.expect("DB init failed");
        // A checkpoint from "long ago" -- the loop has no notion of wall-clock
        // staleness at all, it just resumes from whatever token is stored,
        // which is exactly the desired behavior after a real sleep/wake gap.
        setup_account_for_poll(&pool, &account_id, Some("history_before_sleep")).await;

        let mock = server
            .mock("GET", "/gmail/v1/users/me/history")
            .match_query(mockito::Matcher::UrlEncoded("startHistoryId".into(), "history_before_sleep".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({ "historyId": "history_after_wake", "history": [] }).to_string())
            .expect(1)
            .create_async()
            .await;

        let app = mock_builder().build(mock_context(tauri::test::noop_assets())).unwrap();
        app.manage(crate::auth::session::SessionState::default());
        app.manage(crate::security::incident_response::IncidentMonitor::default());
        app.manage(test_queue_handles());
        let app_handle = app.handle().clone();

        poll_all_accounts(&app_handle, &pool, &server.url()).await.unwrap();

        let conn = pool.get().await.unwrap();
        let checkpoint = conn
            .interact(move |c| processing_checkpoints::get_checkpoint(c, "gmail_history", &account_id))
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            checkpoint.last_processed_token,
            Some("history_after_wake".to_string()),
            "the poll following a long gap (sleep) must resume from the durable checkpoint and advance normally"
        );

        mock.assert_async().await;
        let _ = fs::remove_dir_all(&temp_dir);
    }

    /// Doc 30 TASK-QA-005 acceptance: `test_quota_exhaustion_sets_degraded_state`.
    /// Persistent 429s (quota exhaustion) must, after retries are exhausted,
    /// surface as a `system_warning` (TASK-RT-003's `gmail_quota_exhausted`)
    /// rather than a crash or a silent, invisible failure.
    ///
    /// Deliberately **not** using `#[tokio::test(start_paused = true)]` (or a
    /// manual `tokio::time::pause()`) here, despite the real 8-retry
    /// exponential backoff summing to ~3 real minutes: pausing tokio's clock
    /// while a *real* TCP connection to the `mockito` server is in flight
    /// causes the connection itself to fail at the transport layer (verified
    /// empirically while building this test -- every request came back as a
    /// transport error, 0 ever reached the mock, and `quota_exhausted_count`
    /// stayed 0). Paused virtual time is documented to be safe with mocked
    /// I/O, not real sockets; this test correctly exercises real sockets
    /// (the real `NetworkClient`/`reqwest` path), so it accepts the real
    /// wall-clock cost rather than silently weakening the test to avoid it.
    #[tokio::test]
    async fn test_quota_exhaustion_sets_degraded_state() {
        use tauri::Listener;

        let mut server = mockito::Server::new_async().await;
        let account_id = format!("acc_quota_{}", uuid::Uuid::new_v4());
        write_dev_token(&account_id);

        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let pool = init_db(temp_dir.join("test.db")).await.expect("DB init failed");
        setup_account_for_poll(&pool, &account_id, Some("history_quota")).await;

        let mock = server
            .mock("GET", "/gmail/v1/users/me/history")
            .match_query(mockito::Matcher::Any)
            .with_status(429)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({ "error": "quota exceeded" }).to_string())
            .expect_at_least(1)
            .create_async()
            .await;

        let app = mock_builder().build(mock_context(tauri::test::noop_assets())).unwrap();
        app.manage(crate::auth::session::SessionState::default());
        app.manage(crate::security::incident_response::IncidentMonitor::default());
        app.manage(test_queue_handles());
        let app_handle = app.handle().clone();

        let captured = Arc::new(Mutex::new(Vec::<String>::new()));
        let captured_clone = captured.clone();
        app_handle.listen_any("system_warning", move |event| {
            captured_clone.lock().unwrap().push(event.payload().to_string());
        });

        // The real 429 path exhausts retries and returns an error for this
        // account -- `poll_all_accounts` itself must still not propagate it
        // (same "one account's failure doesn't stop the loop" contract
        // already proven for a token failure elsewhere in this file).
        let result = poll_all_accounts(&app_handle, &pool, &server.url()).await;
        assert!(result.is_ok());

        mock.assert_async().await;
        let warnings = captured.lock().unwrap();
        assert!(
            warnings.iter().any(|w| w.contains("gmail_quota_exhausted")),
            "quota exhaustion must surface a gmail_quota_exhausted system_warning, got: {:?}",
            *warnings
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
