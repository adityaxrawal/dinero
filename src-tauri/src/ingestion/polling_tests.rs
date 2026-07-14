#[cfg(test)]
mod tests {
    use crate::db::connected_accounts::{self, ConnectedAccountsRow};
    use crate::db::init_db;
    use crate::db::local_profile::{self, LocalProfileRow};
    use crate::db::processing_checkpoints;
    use crate::ingestion::polling::{next_backoff, save_history_id, start_polling_loop};
    use std::fs;
    use std::time::Duration;
    use tokio::time::sleep;
    use tokio_util::sync::CancellationToken;

    // Tauri test builder
    use tauri::test::{mock_builder, mock_context};

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
}
