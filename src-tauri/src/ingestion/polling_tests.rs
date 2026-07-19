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
}
