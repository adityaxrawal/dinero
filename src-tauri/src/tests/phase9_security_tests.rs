use crate::licensing::client::{ActivateRequest, LicensingClient};
use crate::licensing::state::{
    get_license_state, transition_to_locked, upsert_license_state, LicenseStateRow, LicenseStatus,
};
use chrono::Utc;
use mockito;
use rusqlite::Connection;

/// Superseded as the acceptance-criteria test by
/// `licensing::state_machine::tests::test_license_state_machine_transitions`
/// (TASK-AUTH-009), which also asserts illegal transitions are rejected —
/// this predates that and only exercised the direct SQL/legacy
/// `transition_to_locked` path. Kept as a basic CRUD-level smoke test.
#[test]
fn test_active_grace_locked_basic_sql_transitions() {
    let conn = crate::db::test_helpers::setup_test_db();

    let now = Utc::now();
    let state = LicenseStateRow {
        id: 1,
        license_jwt: "jwt1".to_string(),
        subscription_status_cached: LicenseStatus::Active,
        plan_id_cached: Some("pro".to_string()),
        current_period_end_cached: Some(now),
        jwt_expires_at: now,
        last_server_validated_at: Some(now - chrono::Duration::hours(73)),
        last_known_valid_time: now,
        device_fingerprint: Some("dev1".to_string()),
        source: "server_fresh".to_string(),
        billing_interval_cached: Some("monthly".to_string()),
    };

    upsert_license_state(&conn, &state).unwrap();

    let fetched = get_license_state(&conn).unwrap().unwrap();
    assert_eq!(fetched.subscription_status_cached, LicenseStatus::Active);

    // In worker.rs, if active and fails, it goes to Grace
    conn.execute("UPDATE license_state SET subscription_status_cached = 'grace', updated_at = CURRENT_TIMESTAMP WHERE id = 1", []).unwrap();
    let fetched_grace = get_license_state(&conn).unwrap().unwrap();
    assert_eq!(
        fetched_grace.subscription_status_cached,
        LicenseStatus::Grace
    );

    // Transition to locked
    transition_to_locked(&conn, false).unwrap();

    let fetched_locked = get_license_state(&conn).unwrap().unwrap();
    assert_eq!(
        fetched_locked.subscription_status_cached,
        LicenseStatus::Locked
    );
}

#[tokio::test]
async fn test_licensing_backend_receives_no_financial_data() {
    let mut server = mockito::Server::new_async().await;

    // We verify the request payload
    let mock = server
        .mock("POST", "/api/license/activate")
        .match_body(mockito::Matcher::Json(serde_json::json!({
            "email": "test@example.com",
            "razorpay_payment_id": "pay_29QQoUBi66xm2f",
            "razorpay_signature": "some-signature",
            "device_id": "some-device",
            "billing_interval": "monthly"
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jwt":"mock_jwt","status":"active"}"#)
        .create_async()
        .await;

    let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

    let client = LicensingClient::new(server.url(), pool);
    let req = ActivateRequest {
        email: "test@example.com".to_string(),
        razorpay_payment_id: "pay_29QQoUBi66xm2f".to_string(),
        razorpay_signature: "some-signature".to_string(),
        device_id: "some-device".to_string(),
        billing_interval: "monthly".to_string(),
    };

    let res = client.activate(req).await.unwrap();
    assert_eq!(res.status, "active");
    mock.assert_async().await;
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_sqlite_file_unreadable_without_keychain_key() {
    // In a real environment, we'd use SQLCipher to open the DB with a key.
    // We will verify that trying to open the encrypted DB with a wrong key (or no key) fails.

    let temp_dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&temp_dir).unwrap();
    let db_path = temp_dir.join("test.db");

    // Initialize with a mock keyring by passing the path
    let pool = crate::db::init_db(db_path.clone()).await;

    assert!(pool.is_ok());

    // Try to open it with raw rusqlite without the pragma key
    let raw_conn = Connection::open(&db_path).unwrap();

    // Attempting to read a table should fail because it's encrypted
    let res = raw_conn.execute("SELECT count(*) FROM license_state", []);

    // rusqlite returns DatabaseError if it cannot read the file (file is not a database)
    assert!(res.is_err());
    let err_str = res.unwrap_err().to_string();
    assert!(err_str.contains("file is not a database") || err_str.contains("encrypted"));

    let _ = std::fs::remove_dir_all(temp_dir);
}
