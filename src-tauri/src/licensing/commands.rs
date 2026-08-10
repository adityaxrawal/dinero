//! Tauri commands exposing licence status and billing actions.
use crate::db::local_profile::select_by_id;
use crate::error::AppError;
use crate::licensing::client::{ActivateRequest, LicensingClient, ValidateRequest};
use crate::licensing::device::get_device_id;
use crate::licensing::gate::trial_days_remaining;
use crate::licensing::jwt::verify_license_jwt;
use crate::licensing::state::{
    get_license_state, upsert_license_state, LicenseStateRow, LicenseStatus,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::Serialize;
use tauri::State;

const LICENSING_BASE_URL: &str = "https://api.dinero-app.com";

#[derive(Serialize, Clone)]
pub struct LicenseStatusResponse {
    pub state: String,
    pub is_active: bool,
    pub license_key_masked: Option<String>,
    pub plan_id: Option<String>,
    pub billing_interval: Option<String>,
    pub expiry_date: Option<String>,
    pub days_remaining: Option<i64>,
}

#[derive(Serialize)]
pub struct LicenseActivateResponse {
    pub status: String,
    pub state: String,
    pub device_bound: bool,
    pub plan_id: Option<String>,
    pub billing_interval: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Serialize)]
pub struct LicenseDeactivateResponse {
    pub status: String,
    pub state: String,
}

#[derive(Serialize)]
pub struct LicenseRefreshResponse {
    pub status: String,
    pub state: String,
}

/// Projects stored licence state into the status the frontend consumes.
pub(crate) fn compute_license_status(
    c: &rusqlite::Connection,
) -> Result<LicenseStatusResponse, AppError> {
    let state = get_license_state(c).map_err(|e| AppError::Db(e.to_string()))?;

    let Some(state) = state else {
        return trial_status_response(c);
    };

    if state.subscription_status_cached == LicenseStatus::Trial {
        return trial_status_response(c);
    }

    let is_active = matches!(
        state.subscription_status_cached,
        LicenseStatus::Active | LicenseStatus::Grace
    );
    let days_remaining = state
        .current_period_end_cached
        .map(|end| (end - Utc::now()).num_days());

    Ok(LicenseStatusResponse {
        state: state.subscription_status_cached.as_str().to_uppercase(),
        is_active,
        license_key_masked: None,
        plan_id: state.plan_id_cached,
        billing_interval: state.billing_interval_cached,
        expiry_date: state
            .current_period_end_cached
            .or(Some(state.jwt_expires_at))
            .map(|d| d.to_rfc3339()),
        days_remaining,
    })
}

#[tauri::command]
/// Returns the current licence status.
pub async fn license_get_status(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<LicenseStatusResponse, AppError> {
    let conn = pool.get().await.map_err(|e| AppError::Db(e.to_string()))?;
    conn.interact(|c| compute_license_status(&*c))
        .await
        .map_err(|e| AppError::Unknown(e.to_string()))?
}

/// Notifies the frontend that licence state changed.
///
/// Pushed as an event so a purchase or expiry takes effect immediately, without
/// the user restarting the app.
pub(crate) async fn emit_license_state_changed<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    pool: &deadpool_sqlite::Pool,
) {
    let Ok(conn) = pool.get().await else { return };
    let Ok(Ok(status)) = conn.interact(|c| compute_license_status(&*c)).await else {
        return;
    };
    if let Err(e) = crate::ipc::events::emit_event(
        app_handle,
        crate::ipc::events::AppEvent::LicenseStateChanged,
        status,
    ) {
        tracing::error!("Failed to emit license_state_changed: {}", e);
    }
}

/// Builds the status response for a trial.
fn trial_status_response(c: &rusqlite::Connection) -> Result<LicenseStatusResponse, AppError> {
    let remaining = trial_days_remaining(c)?;
    let profile = select_by_id(c, 1)
        .map_err(|e| AppError::Db(e.to_string()))?
        .ok_or_else(|| AppError::LicenseLocked("No local profile found".to_string()))?;
    let expiry_date = profile.created_at.map(|created| {
        (created + ChronoDuration::days(crate::licensing::gate::TRIAL_WINDOW_DAYS))
            .and_utc()
            .to_rfc3339()
    });

    Ok(LicenseStatusResponse {
        state: if remaining >= 0 {
            "TRIAL".to_string()
        } else {
            "LOCKED".to_string()
        },
        is_active: remaining >= 0,
        license_key_masked: None,
        plan_id: None,
        billing_interval: None,
        expiry_date,
        days_remaining: Some(remaining),
    })
}

#[tauri::command]
/// Activates a licence against the licensing service.
pub async fn license_activate(
    email: String,
    razorpay_payment_id: String,
    razorpay_signature: String,
    billing_interval: String,
    pool: State<'_, deadpool_sqlite::Pool>,
    session_state: State<'_, crate::auth::session::SessionState>,
    app_handle: tauri::AppHandle,
) -> Result<LicenseActivateResponse, AppError> {
    crate::ipc::middleware::require_active_session(&session_state)?;

    let device_id = get_device_id().map_err(|e| AppError::Auth(e.to_string()))?;

    let client = LicensingClient::new(LICENSING_BASE_URL.to_string(), pool.inner().clone());
    let response = client
        .activate(ActivateRequest {
            email,
            razorpay_payment_id,
            razorpay_signature,
            device_id: device_id.clone(),
            billing_interval: billing_interval.clone(),
        })
        .await
        .map_err(|e| {
            if e.to_string() == "DEVICE_ALREADY_BOUND" {
                AppError::Auth(
                    "This license is already active on another Mac. Deactivate it there first \
                     (Settings → License → Deactivate), then activate here."
                        .to_string(),
                )
            } else {
                AppError::Network(e.to_string())
            }
        })?;

    let claims = verify_license_jwt(&response.jwt)
        .map_err(|e| AppError::Auth(format!("Activation JWT failed verification: {}", e)))?;

    if claims.device_id != device_id {
        return Err(AppError::Auth(
            "Activation JWT device_id does not match this device".to_string(),
        ));
    }

    let expires_at: DateTime<Utc> = DateTime::from_timestamp(claims.exp, 0)
        .ok_or_else(|| AppError::Auth("Invalid JWT expiry claim".to_string()))?;

    let status = LicenseStatus::parse_status(&response.status).unwrap_or(LicenseStatus::Active);

    let conn = pool.get().await.map_err(|e| AppError::Db(e.to_string()))?;
    let row = LicenseStateRow {
        id: 1,
        license_jwt: response.jwt.clone(),
        subscription_status_cached: status.clone(),
        plan_id_cached: Some(claims.plan.clone()),
        current_period_end_cached: Some(expires_at),
        jwt_expires_at: expires_at,
        last_server_validated_at: Some(Utc::now()),
        last_known_valid_time: Utc::now(),
        device_fingerprint: Some(device_id),
        source: "server_fresh".to_string(),
        billing_interval_cached: Some(claims.billing_interval.clone()),
    };
    conn.interact(move |c| upsert_license_state(c, &row))
        .await
        .map_err(|e| AppError::Unknown(e.to_string()))?
        .map_err(|e| AppError::Db(e.to_string()))?;

    emit_license_state_changed(&app_handle, pool.inner()).await;

    Ok(LicenseActivateResponse {
        status: "activated".to_string(),
        state: status.as_str().to_uppercase(),
        device_bound: true,
        plan_id: Some(claims.plan),
        billing_interval: Some(claims.billing_interval),
        expires_at: Some(expires_at.to_rfc3339()),
    })
}

#[tauri::command]
/// Command deactivating this device's licence.
pub async fn license_deactivate(
    pool: State<'_, deadpool_sqlite::Pool>,
    session_state: State<'_, crate::auth::session::SessionState>,
    app_handle: tauri::AppHandle,
) -> Result<LicenseDeactivateResponse, AppError> {
    crate::ipc::middleware::require_active_session(&session_state)?;
    let response = deactivate_license_internal(pool.inner()).await?;
    emit_license_state_changed(&app_handle, pool.inner()).await;
    Ok(response)
}

/// Performs deactivation and clears local licence state.
pub async fn deactivate_license_internal(
    pool: &deadpool_sqlite::Pool,
) -> Result<LicenseDeactivateResponse, AppError> {
    let device_id = get_device_id().map_err(|e| AppError::Auth(e.to_string()))?;

    let client = LicensingClient::new(LICENSING_BASE_URL.to_string(), pool.clone());
    client
        .deactivate(ValidateRequest { device_id })
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;

    let conn = pool.get().await.map_err(|e| AppError::Db(e.to_string()))?;
    conn.interact(|c| {
        c.execute(
            "UPDATE license_state SET subscription_status_cached = 'anonymous', updated_at = CURRENT_TIMESTAMP WHERE id = 1",
            [],
        )
    })
    .await
    .map_err(|e| AppError::Unknown(e.to_string()))?
    .map_err(|e| AppError::Db(e.to_string()))?;

    Ok(LicenseDeactivateResponse {
        status: "deactivated".to_string(),
        state: "ANONYMOUS_EVAL".to_string(),
    })
}

#[tauri::command]
/// Re-validates the licence with the service.
pub async fn license_refresh(
    pool: State<'_, deadpool_sqlite::Pool>,
    session_state: State<'_, crate::auth::session::SessionState>,
    app_handle: tauri::AppHandle,
) -> Result<LicenseRefreshResponse, AppError> {
    crate::ipc::middleware::require_active_session(&session_state)?;

    let device_id = get_device_id().map_err(|e| AppError::Auth(e.to_string()))?;

    let client = LicensingClient::new(LICENSING_BASE_URL.to_string(), pool.inner().clone());
    let response = client
        .validate(ValidateRequest {
            device_id: device_id.clone(),
        })
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;

    let claims = verify_license_jwt(&response.jwt)
        .map_err(|e| AppError::Auth(format!("Refresh JWT failed verification: {}", e)))?;

    if claims.device_id != device_id {
        return Err(AppError::Auth(
            "Refreshed JWT device_id does not match this device".to_string(),
        ));
    }

    let expires_at: DateTime<Utc> = DateTime::from_timestamp(claims.exp, 0)
        .ok_or_else(|| AppError::Auth("Invalid JWT expiry claim".to_string()))?;
    let status = LicenseStatus::parse_status(&response.status).unwrap_or(LicenseStatus::Active);
    let status_str = status.as_str();

    let conn = pool.get().await.map_err(|e| AppError::Db(e.to_string()))?;
    conn.interact(move |c| {
        c.execute(
            "UPDATE license_state SET license_jwt = ?1, subscription_status_cached = ?2, current_period_end_cached = ?3, jwt_expires_at = ?3, last_server_validated_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE id = 1",
            rusqlite::params![response.jwt, status_str, expires_at],
        )
    })
    .await
    .map_err(|e| AppError::Unknown(e.to_string()))?
    .map_err(|e| AppError::Db(e.to_string()))?;

    emit_license_state_changed(&app_handle, pool.inner()).await;

    Ok(LicenseRefreshResponse {
        status: "refreshed".to_string(),
        state: status_str.to_uppercase(),
    })
}

#[derive(Serialize)]
pub struct CheckoutCompletedResponse {
    pub razorpay_payment_id: String,
    pub razorpay_signature: String,
}

#[tauri::command]
/// Starts the checkout flow for a purchase.
pub async fn billing_start_checkout(
    email: String,
    plan_id: String,
    pool: State<'_, deadpool_sqlite::Pool>,
    session_state: State<'_, crate::auth::session::SessionState>,
) -> Result<CheckoutCompletedResponse, AppError> {
    crate::ipc::middleware::require_active_session(&session_state)?;

    let client = LicensingClient::new(LICENSING_BASE_URL.to_string(), pool.inner().clone());
    let order = client
        .create_order(crate::licensing::client::CreateOrderRequest { email, plan_id })
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;

    let server = tiny_http::Server::http("127.0.0.1:0").map_err(|e| {
        AppError::Unknown(format!("Failed to bind checkout loopback listener: {e}"))
    })?;
    let redirect_port = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| AppError::Unknown("Loopback server has no IP address".to_string()))?
        .port();

    let checkout_url = format!("http://127.0.0.1:{redirect_port}/?checkout");
    if let Err(e) = tauri_plugin_opener::open_url(&checkout_url, None::<&str>) {
        tracing::error!("Failed to open browser for checkout: {}", e);
    }

    let result = tokio::task::spawn_blocking(move || {
        crate::billing::checkout::serve_checkout_and_wait(
            &server,
            &order,
            crate::billing::checkout::CHECKOUT_CALLBACK_TIMEOUT,
        )
    })
    .await
    .map_err(|e| AppError::Unknown(e.to_string()))?
    .map_err(|e| AppError::Auth(e.to_string()))?;

    Ok(CheckoutCompletedResponse {
        razorpay_payment_id: result.razorpay_payment_id,
        razorpay_signature: result.razorpay_signature,
    })
}
