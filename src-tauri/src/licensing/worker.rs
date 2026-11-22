//! Background licence revalidation.
//!
//! Periodically re-checks entitlement so a cancellation or expiry takes effect
//! without a restart. Failures are tolerated rather than escalated -- a network
//! outage must not lock out a paying customer.
use crate::licensing::client::{LicensingClient, ValidateRequest};
use crate::licensing::state::{
    get_license_state, record_known_valid_time, transition_to_locked, LicenseStatus,
};
use chrono::{Duration as ChronoDuration, Utc};
use deadpool_sqlite::Pool;
use std::time::Duration;
use tauri::Manager;
use tokio::time;

const GRACE_PERIOD: ChronoDuration = ChronoDuration::days(7);

/// Whether the grace period has elapsed.
///
/// Grace exists so a failed card does not instantly destroy access for a paying
/// customer.
fn is_grace_period_expired(
    last_validated: Option<chrono::DateTime<Utc>>,
    now: chrono::DateTime<Utc>,
) -> bool {
    match last_validated {
        Some(last_validated) => now.signed_duration_since(last_validated) > GRACE_PERIOD,
        None => true,
    }
}

const CLOCK_CORRECTION_GRACE: ChronoDuration = ChronoDuration::hours(1);

enum PollOutcome {
    NoState,
    ClockSkewLocked,
    Active(crate::licensing::state::LicenseStateRow),
}

/// Registers this device's trial with the server and stores the signed result.
async fn register_server_trial(client: &LicensingClient, pool: &Pool) -> anyhow::Result<()> {
    let device_id = crate::licensing::device::get_device_id()?;
    let response = client
        .start_trial(crate::licensing::client::StartTrialRequest { device_id: device_id.clone() })
        .await?;
    let claims = crate::licensing::jwt::verify_license_jwt(&response.jwt)?;
    anyhow::ensure!(claims.device_id == device_id, "trial token is for a different device");
    let status = crate::licensing::state_machine::server_status(&claims, response.state.as_deref());
    let expires_at = chrono::DateTime::from_timestamp(claims.exp, 0).unwrap_or_else(Utc::now);
    let trial_ends_at = response
        .trial_ends_at
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc));
    let row = crate::licensing::state::LicenseStateRow {
        id: 1,
        license_jwt: response.jwt,
        subscription_status_cached: status,
        plan_id_cached: Some(claims.plan),
        current_period_end_cached: trial_ends_at.or(Some(expires_at)),
        jwt_expires_at: expires_at,
        last_server_validated_at: Some(Utc::now()),
        last_known_valid_time: Utc::now(),
        device_fingerprint: Some(device_id),
        source: "server_fresh".to_string(),
        billing_interval_cached: Some(claims.billing_interval),
    };
    pool.get()
        .await?
        .interact(move |c| crate::licensing::state::upsert_license_state(c, &row))
        .await
        .map_err(|e| anyhow::anyhow!("interact error: {}", e))??;
    Ok(())
}

/// Periodically revalidates the licence in the background.
pub async fn start_background_validation<R: tauri::Runtime>(
    pool: Pool,
    base_url: String,
    app_handle: tauri::AppHandle<R>,
) {
    let client = LicensingClient::new(base_url, pool.clone());
    let mut interval = time::interval(Duration::from_secs(6 * 60 * 60));

    loop {
        interval.tick().await;

        if let Ok(conn) = pool.get().await {
            let res = conn.interact(move |c| {
                let state_opt = get_license_state(c)?;

                if let Some(state) = state_opt {
                    let now = Utc::now();

                    if now < state.last_known_valid_time - CLOCK_CORRECTION_GRACE {
                        tracing::warn!("ClockSkewDetected: System time moved backward beyond the correction grace. Locking license.");
                        transition_to_locked(c, true)?;
                        return Ok::<_, anyhow::Error>(PollOutcome::ClockSkewLocked);
                    }

                    record_known_valid_time(c, now)?;

                    Ok(PollOutcome::Active(state))
                } else {
                    Ok(PollOutcome::NoState)
                }
            }).await;

            if matches!(res, Ok(Ok(PollOutcome::ClockSkewLocked))) {
                crate::ipc::system_warnings::emit_system_warning(
                    &app_handle,
                    crate::ipc::system_warnings::SystemWarningPayload {
                        warning_type: "clock_skew".to_string(),
                        message: "Your Mac's system clock appears to have moved backward. \
                        Your license has been locked as a precaution — please check your \
                        date & time settings."
                            .to_string(),
                        severity: crate::ipc::system_warnings::WarningSeverity::Critical,
                        action_hint: Some("check_system_clock".to_string()),
                    },
                );
            }

            match res {
                Ok(Ok(PollOutcome::Active(state))) => {
                    if state.subscription_status_cached != LicenseStatus::AnonymousEval {
                        let device_id = match crate::licensing::device::get_device_id() {
                            Ok(id) => id,
                            Err(e) => {
                                tracing::error!("Failed to derive device_id: {}", e);
                                continue;
                            }
                        };

                        let req = ValidateRequest {
                            device_id,
                            jwt: state.license_jwt.clone(),
                        };

                        match client.validate(req).await {
                            Ok(response) => {
                                match crate::licensing::jwt::verify_license_jwt(&response.jwt) {
                                    Ok(claims) => {
                                        tracing::info!("License validation successful (JWT signature verified)");
                                        let status = crate::licensing::state_machine::server_status(
                                            &claims,
                                            response.state.as_deref(),
                                        );
                                        let jwt = response.jwt.clone();
                                        let expires_at = chrono::DateTime::from_timestamp(claims.exp, 0)
                                            .unwrap_or_else(Utc::now);
                                        let write = conn
                                            .interact(move |c| {
                                                crate::licensing::state_machine::apply_server_validation(
                                                    c, &status, &jwt, expires_at,
                                                )
                                            })
                                            .await;
                                        match write {
                                            Ok(Ok(())) => {}
                                            Ok(Err(e)) => tracing::error!("Failed to store license validation: {:#}", e),
                                            Err(e) => tracing::error!("Failed to store license validation: {}", e),
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            "License JWT failed signature verification — refusing to trust it: {}",
                                            e
                                        );
                                        let _ = conn
                                            .interact(move |c| transition_to_locked(c, false))
                                            .await;

                                        let monitor = app_handle.state::<crate::security::incident_response::IncidentMonitor>();
                                        if crate::security::incident_response::record_trigger(
                                            &monitor,
                                            crate::security::incident_response::TriggerKind::SignatureVerificationFailure,
                                        ) {
                                            crate::security::incident_response::emit_health_alert(
                                                crate::security::incident_response::TriggerKind::SignatureVerificationFailure,
                                                &app_handle,
                                            );
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("License validation failed: {:?}", e);

                                let monitor = app_handle
                                    .state::<crate::security::incident_response::IncidentMonitor>(
                                );
                                if crate::security::incident_response::record_trigger(
                                    &monitor,
                                    crate::security::incident_response::TriggerKind::RepeatedLicenseValidateFailure,
                                ) {
                                    crate::security::incident_response::emit_health_alert(
                                        crate::security::incident_response::TriggerKind::RepeatedLicenseValidateFailure,
                                        &app_handle,
                                    );
                                }

                                let now = Utc::now();

                                let _ = conn
                                    .interact(move |c| {
                                        if state.subscription_status_cached == LicenseStatus::Active
                                        {
                                            tracing::warn!(
                                                "Transitioning license to GRACE period."
                                            );
                                            crate::licensing::state_machine::transition(
                                                c,
                                                LicenseStatus::Grace,
                                            )?;
                                        } else if state.subscription_status_cached
                                            == LicenseStatus::Grace
                                            && is_grace_period_expired(
                                                state.last_server_validated_at,
                                                now,
                                            )
                                        {
                                            tracing::warn!(
                                                "Grace period expired. Locking license."
                                            );
                                            transition_to_locked(c, false)?;
                                        }
                                        Ok::<_, anyhow::Error>(())
                                    })
                                    .await;
                            }
                        }
                    }
                }
                Ok(Ok(PollOutcome::NoState)) => {
                    // First launch (or after a data reset): anchor the trial on
                    // the server, so resetting data or the clock cannot restart
                    // it. Offline, the local 14-day window applies until the
                    // next attempt (review M15).
                    if let Err(e) = register_server_trial(&client, &pool).await {
                        tracing::warn!("Could not register trial with the server yet: {:#}", e);
                    }
                }
                Ok(Ok(PollOutcome::ClockSkewLocked)) => {}
                Ok(Err(e)) => tracing::error!("Error reading license state: {}", e),
                Err(e) => tracing::error!("Pool interact error: {}", e),
            }

            crate::licensing::commands::emit_license_state_changed(&app_handle, &pool).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_grace_period_expired() {
        let now = Utc::now();

        assert!(
            !is_grace_period_expired(Some(now - ChronoDuration::days(6)), now),
            "6 days into the 7-day grace window must not be expired yet"
        );
        assert!(
            !is_grace_period_expired(Some(now - ChronoDuration::days(7)), now),
            "exactly 7 days must not yet be expired (the check is strictly greater-than)"
        );
        assert!(
            is_grace_period_expired(Some(now - ChronoDuration::days(8)), now),
            "8 days past the last successful validation must be expired"
        );
        assert!(
            is_grace_period_expired(None, now),
            "a GRACE state with no recorded last validation at all must be treated as expired"
        );
    }
}
