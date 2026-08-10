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

                        let req = ValidateRequest { device_id };

                        match client.validate(req).await {
                            Ok(response) => {
                                match crate::licensing::jwt::verify_license_jwt(&response.jwt) {
                                    Ok(_claims) => {
                                        tracing::info!("License validation successful (JWT signature verified)");
                                        let jwt_clone = response.jwt.clone();
                                        let status_clone = response.status.clone();
                                        let _ = conn.interact(move |c| {
                                            c.execute(
                                                "UPDATE license_state SET license_jwt = ?1, subscription_status_cached = ?2, last_server_validated_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE id = 1",
                                                rusqlite::params![jwt_clone, status_clone],
                                            )?;
                                            Ok::<_, anyhow::Error>(())
                                        }).await;
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
                Ok(Ok(PollOutcome::NoState)) | Ok(Ok(PollOutcome::ClockSkewLocked)) => {}
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
