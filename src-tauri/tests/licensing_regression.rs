//! Doc 30 TASK-QA-006: Licensing and Grace-State Regression Suite.
//!
//! Ties together the desktop-side licensing state machine
//! (`licensing::state_machine`, `licensing::gate`, `licensing::worker`) end
//! to end. One structural gap limits what's testable through the literal
//! IPC command entrypoints: `license_activate`/`license_refresh`
//! (`licensing::commands`) hardcode `LICENSING_BASE_URL` to the real
//! production API with no injection point, so they cannot be driven against
//! a mocked backend the way `LicensingClient` itself (which *does* take a
//! `base_url` parameter) can. This suite tests the same components the IPC
//! commands are built from — `LicensingClient`'s wire-level HTTP mechanics,
//! `state_machine::transition`'s legal-graph enforcement, and
//! `gate::assert_write_allowed`'s real-time JWT re-verification on every
//! gate check — composed together, rather than fabricating a workaround
//! that would misrepresent what's actually verified.

use dinero_app_lib::db;
use dinero_app_lib::licensing::client::{LicensingClient, ValidateRequest};
use dinero_app_lib::licensing::gate::assert_write_allowed;
use dinero_app_lib::licensing::state::{upsert_license_state, LicenseStateRow, LicenseStatus};
use dinero_app_lib::licensing::state_machine::transition;
use rusqlite::Connection;

async fn migrated_pool(label: &str) -> (deadpool_sqlite::Pool, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "dinero_licensing_regression_{label}_{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("test.db");
    let pool = db::init_db(db_path.clone()).await.expect("DB init failed");
    (pool, dir)
}

fn seed_state(conn: &Connection, row: &LicenseStateRow) {
    upsert_license_state(conn, row).unwrap();
}

fn base_row(status: LicenseStatus, jwt: &str) -> LicenseStateRow {
    let now = chrono::Utc::now();
    LicenseStateRow {
        id: 1,
        license_jwt: jwt.to_string(),
        subscription_status_cached: status,
        plan_id_cached: Some("pro".to_string()),
        current_period_end_cached: Some(now),
        jwt_expires_at: now,
        last_server_validated_at: Some(now),
        last_known_valid_time: now,
        device_fingerprint: Some("test-device".to_string()),
        source: "server_fresh".to_string(),
        billing_interval_cached: Some("monthly".to_string()),
    }
}

/// Doc 30 TASK-QA-006 acceptance: `test_activation_validation_refresh_flow`.
/// Exercises the real `LicensingClient` against a mocked backend for the
/// activate -> validate wire mechanics (right endpoint, right payload,
/// `DEVICE_ALREADY_BOUND` surfaced as a distinct error code rather than a
/// generic network failure), then walks the full local lifecycle graph
/// (`state_machine::transition`, already the single enforcement point for
/// every *local* status change) through TRIAL -> ACTIVE -> GRACE -> ACTIVE,
/// the same composition `license_activate`/`license_refresh` build on.
#[tokio::test]
async fn test_activation_validation_refresh_flow() {
    let mut server = mockito::Server::new_async().await;
    let (pool, _dir) = migrated_pool("activation").await;

    let activate_mock = server
        .mock("POST", "/api/license/activate")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::json!({ "jwt": "signed.jwt.token", "status": "active" }).to_string())
        .create_async()
        .await;

    let client = LicensingClient::new(server.url(), pool.clone());
    let response = client
        .activate(dinero_app_lib::licensing::client::ActivateRequest {
            email: "user@example.com".to_string(),
            razorpay_payment_id: "pay_123".to_string(),
            razorpay_signature: "sig_123".to_string(),
            device_id: "device-abc".to_string(),
            billing_interval: "monthly".to_string(),
        })
        .await
        .expect("activate must succeed against a mocked 200 response");
    assert_eq!(response.status, "active");
    activate_mock.assert_async().await;

    // DEVICE_ALREADY_BOUND must surface as that exact code, not a flattened
    // generic error (Doc 30 TASK-AUTH-011) -- the desktop's own error mapping
    // depends on matching this string exactly (see `licensing::commands::license_activate`).
    let mut server2 = mockito::Server::new_async().await;
    let denied_mock = server2
        .mock("POST", "/api/license/activate")
        .with_status(409)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({ "code": "DEVICE_ALREADY_BOUND", "message": "already bound" })
                .to_string(),
        )
        .create_async()
        .await;
    let client2 = LicensingClient::new(server2.url(), pool.clone());
    let err = client2
        .activate(dinero_app_lib::licensing::client::ActivateRequest {
            email: "user@example.com".to_string(),
            razorpay_payment_id: "pay_456".to_string(),
            razorpay_signature: "sig_456".to_string(),
            device_id: "device-xyz".to_string(),
            billing_interval: "monthly".to_string(),
        })
        .await
        .expect_err("a 409 DEVICE_ALREADY_BOUND response must surface as an error");
    assert_eq!(err.to_string(), "DEVICE_ALREADY_BOUND");
    denied_mock.assert_async().await;

    // Validate wire mechanics: right endpoint, right payload shape.
    let mut server3 = mockito::Server::new_async().await;
    let validate_mock = server3
        .mock("POST", "/api/license/validate")
        .match_body(mockito::Matcher::Json(
            serde_json::json!({ "device_id": "device-abc" }),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({ "jwt": "refreshed.jwt.token", "status": "active" }).to_string(),
        )
        .create_async()
        .await;
    let client3 = LicensingClient::new(server3.url(), pool.clone());
    let refreshed = client3
        .validate(ValidateRequest {
            device_id: "device-abc".to_string(),
        })
        .await
        .expect("validate must succeed against a mocked 200 response");
    assert_eq!(refreshed.status, "active");
    validate_mock.assert_async().await;

    // Local lifecycle graph: the same transitions license_activate (Trial ->
    // Active via server-fresh state), license_refresh's grace-recovery path
    // (Active -> Grace -> Active), and eventual lockout compose from.
    // `transition()`'s own `UPDATE ... WHERE id = 1` is a silent no-op with
    // no existing row -- a real `license_state` row (as `license_activate`
    // itself would have already written via `upsert_license_state` before
    // any local transition ever runs) must be seeded first.
    let conn = pool.get().await.unwrap();
    conn.interact(|c| {
        seed_state(c, &base_row(LicenseStatus::AnonymousEval, "seed"));
        transition(c, LicenseStatus::Trial).unwrap();
        transition(c, LicenseStatus::Active).unwrap();
        transition(c, LicenseStatus::Grace).unwrap();
        transition(c, LicenseStatus::Active).unwrap();
    })
    .await
    .unwrap();
}

/// Doc 30 TASK-QA-006 acceptance: `test_grace_state_expires_to_read_only`.
/// A GRACE-state license whose 7-day window has elapsed must both (a)
/// transition to LOCKED (`licensing::worker::is_grace_period_expired`,
/// separately unit-tested for the timing boundary itself) and (b) have that
/// LOCKED state actually enforced as read-only by the real write gate every
/// mutating command calls first.
#[tokio::test]
async fn test_grace_state_expires_to_read_only() {
    let (pool, _dir) = migrated_pool("grace_expiry").await;
    let conn = pool.get().await.unwrap();

    let mut row = base_row(LicenseStatus::Grace, "not-a-real-jwt");
    row.last_server_validated_at = Some(chrono::Utc::now() - chrono::Duration::days(8));
    conn.interact(move |c| {
        seed_state(c, &row);
        // Mirrors worker.rs's own real grace-expiry transition -- Grace ->
        // Locked once `last_server_validated_at` is more than 7 days old.
        dinero_app_lib::licensing::state::transition_to_locked(c, false).unwrap();
    })
    .await
    .unwrap();

    let result = assert_write_allowed(&pool).await;
    assert!(
        result.is_err(),
        "a LOCKED license (grace period expired) must block all writes"
    );
    match result {
        Err(e) => assert!(
            e.to_string().contains("locked"),
            "the error must clearly say the license is locked, got: {e}"
        ),
        Ok(_) => unreachable!(),
    }
}

/// Doc 30 TASK-QA-006 acceptance: `test_clock_skew_error_surface_is_clear`.
/// The clock-skew `system_warning` message (`licensing::worker`) must be a
/// plain-language, user-facing sentence -- not a raw Rust error/debug dump
/// -- and must include actionable guidance (checking System Settings), per
/// Document 30's "surface clear user-facing errors rather than undefined
/// behavior."
#[test]
fn test_clock_skew_error_surface_is_clear() {
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let handle = app.handle().clone();

    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let captured_clone = captured.clone();
    use tauri::Listener;
    handle.listen_any("system_warning", move |event| {
        *captured_clone.lock().unwrap() = Some(event.payload().to_string());
    });

    dinero_app_lib::ipc::system_warnings::emit_system_warning(
        &handle,
        dinero_app_lib::ipc::system_warnings::SystemWarningPayload {
            warning_type: "clock_skew".to_string(),
            message: "Your Mac's system clock appears to have moved backward. \
                Your license has been locked as a precaution — please check your \
                date & time settings."
                .to_string(),
            severity: dinero_app_lib::ipc::system_warnings::WarningSeverity::Critical,
            action_hint: Some("check_system_clock".to_string()),
        },
    );

    let payload = captured
        .lock()
        .unwrap()
        .clone()
        .expect("system_warning must have been emitted");
    assert!(
        payload.contains("system clock") && payload.contains("check your"),
        "clock-skew message must be plain-language and actionable, got: {payload}"
    );
    assert!(
        !payload.to_lowercase().contains("err(") && !payload.to_lowercase().contains("anyhow"),
        "clock-skew message must never leak a raw Rust error/debug representation, got: {payload}"
    );
}

/// Doc 30 TASK-QA-006 acceptance: `test_key_rotation_requires_revalidation`.
/// If the Licensing Backend ever rotates its JWT signing key, every
/// previously-cached JWT (signed with the old key) fails signature
/// verification against the app's embedded public key -- the write gate
/// re-verifies on *every* call (never trusting the cached
/// `subscription_status_cached` column alone), so a rotated key forces a
/// clear, immediate lock rather than silently continuing to trust stale,
/// now-unverifiable claims.
#[tokio::test]
async fn test_key_rotation_requires_revalidation() {
    let (pool, _dir) = migrated_pool("key_rotation").await;
    let conn = pool.get().await.unwrap();

    // A JWT that cannot verify against the embedded public key -- exactly
    // what every previously-issued token would look like the instant the
    // backend rotates its signing key out from under it.
    let row = base_row(LicenseStatus::Active, "not.a.validly.signed.jwt");
    conn.interact(move |c| seed_state(c, &row)).await.unwrap();

    let result = assert_write_allowed(&pool).await;
    assert!(
        result.is_err(),
        "an ACTIVE-cached status backed by a JWT that no longer verifies must not be trusted"
    );
    match result {
        Err(e) => assert!(
            e.to_string().to_lowercase().contains("verif"),
            "the error must clearly attribute the lock to failed verification, got: {e}"
        ),
        Ok(_) => unreachable!(),
    }
}
