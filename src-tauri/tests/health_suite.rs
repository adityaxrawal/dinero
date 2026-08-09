//! Doc 30 TASK-OPS-003: Operational Health Checks and Alerting.
//!
//! Exercises `health::compute_health_report` against a real migrated pool
//! (same helper pattern as `licensing_regression.rs`) with seeded
//! connected-account/license-state/checkpoint rows, so the report reflects
//! genuine DB reads rather than fabricated fixture data.

use dinero_app_lib::db;
use dinero_app_lib::db::connected_accounts::{insert_account, ConnectedAccountsRow};
use dinero_app_lib::db::processing_checkpoints::{upsert_checkpoint, ProcessingCheckpointRow};
use dinero_app_lib::health::compute_health_report;
use dinero_app_lib::licensing::state::{upsert_license_state, LicenseStateRow, LicenseStatus};

async fn migrated_pool(label: &str) -> deadpool_sqlite::Pool {
    let dir = std::env::temp_dir().join(format!(
        "dinero_health_suite_{label}_{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("test.db");
    db::init_db(db_path.clone()).await.expect("DB init failed")
}

/// Doc 30 TASK-OPS-003 acceptance: `test_local_health_reports_core_status`.
#[tokio::test]
async fn test_local_health_reports_core_status() {
    let pool = migrated_pool("core_status").await;
    let conn = pool.get().await.unwrap();

    conn.interact(|c| {
        insert_account(
            c,
            &ConnectedAccountsRow {
                id: "acct_1".to_string(),
                profile_id: 1,
                email_address: Some("user@example.com".to_string()),
                account_status: Some("ACTIVE".to_string()),
                last_history_id: Some("12345".to_string()),
                created_at: None,
                updated_at: None,
            },
        )
        .unwrap();

        upsert_checkpoint(
            c,
            &ProcessingCheckpointRow {
                id: "chk_1".to_string(),
                job_type: "gmail_poll".to_string(),
                job_key: "acct_1".to_string(),
                checkpoint_state_json: "{}".to_string(),
                last_processed_token: None,
                status: "completed".to_string(),
                updated_at: None,
            },
        )
        .unwrap();

        let now = chrono::Utc::now();
        upsert_license_state(
            c,
            &LicenseStateRow {
                id: 1,
                license_jwt: "test.jwt.value".to_string(),
                subscription_status_cached: LicenseStatus::Active,
                plan_id_cached: Some("pro".to_string()),
                current_period_end_cached: Some(now),
                jwt_expires_at: now,
                last_server_validated_at: Some(now),
                last_known_valid_time: now,
                device_fingerprint: Some("test-device".to_string()),
                source: "server_fresh".to_string(),
                billing_interval_cached: Some("monthly".to_string()),
            },
        )
        .unwrap();
    })
    .await
    .unwrap();
    drop(conn);

    let report = compute_health_report(&pool).await;

    assert!(
        report.backend_ready,
        "a healthy pool must report backend_ready"
    );
    assert!(
        report.db_integrity_ok,
        "no integrity check has failed yet — must default to ok"
    );
    assert!(
        report.checkpoint_age_seconds.is_some(),
        "a seeded checkpoint must produce a checkpoint age"
    );
    assert_eq!(report.gmail_polling_status, "active");
    assert_eq!(report.license_status, "Active");
}

/// Doc 30 TASK-OPS-003 acceptance: `test_health_checks_do_not_expose_user_data`.
/// Seeds a real email address and JWT, then asserts neither ever appears in
/// the serialized health report — the report must only ever surface coarse
/// status strings, never the underlying identity/financial content.
#[tokio::test]
async fn test_health_checks_do_not_expose_user_data() {
    let pool = migrated_pool("no_user_data").await;
    let conn = pool.get().await.unwrap();

    conn.interact(|c| {
        insert_account(
            c,
            &ConnectedAccountsRow {
                id: "acct_1".to_string(),
                profile_id: 1,
                email_address: Some("very-secret-user@example.com".to_string()),
                account_status: Some("ACTIVE".to_string()),
                last_history_id: Some("12345".to_string()),
                created_at: None,
                updated_at: None,
            },
        )
        .unwrap();

        let now = chrono::Utc::now();
        upsert_license_state(
            c,
            &LicenseStateRow {
                id: 1,
                license_jwt: "super.secret.jwt".to_string(),
                subscription_status_cached: LicenseStatus::Active,
                plan_id_cached: Some("pro".to_string()),
                current_period_end_cached: Some(now),
                jwt_expires_at: now,
                last_server_validated_at: Some(now),
                last_known_valid_time: now,
                device_fingerprint: Some("secret-device-fingerprint".to_string()),
                source: "server_fresh".to_string(),
                billing_interval_cached: Some("monthly".to_string()),
            },
        )
        .unwrap();
    })
    .await
    .unwrap();
    drop(conn);

    let report = compute_health_report(&pool).await;
    let json = serde_json::to_string(&report).unwrap();

    assert!(!json.contains("very-secret-user@example.com"));
    assert!(!json.contains("super.secret.jwt"));
    assert!(!json.contains("secret-device-fingerprint"));
}

/// Doc 30 TASK-OPS-003 acceptance: covers the "no accounts connected yet"
/// and "no license state row yet" branches of `compute_health_report`,
/// complementing `test_local_health_reports_core_status`'s populated case.
#[tokio::test]
async fn test_local_health_reports_defaults_on_fresh_install() {
    let pool = migrated_pool("fresh_install").await;
    let report = compute_health_report(&pool).await;

    assert!(report.backend_ready);
    assert_eq!(report.gmail_polling_status, "not_connected");
    assert_eq!(report.license_status, "no_license_state");
    assert!(report.checkpoint_age_seconds.is_none());
}
