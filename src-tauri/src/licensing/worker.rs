use crate::licensing::client::{LicensingClient, ValidateRequest};
use crate::licensing::state::{
    get_license_state, record_known_valid_time, transition_to_locked, LicenseStatus,
};
use chrono::{Duration as ChronoDuration, Utc};
use deadpool_sqlite::Pool;
use std::time::Duration;
use tokio::time;

/// Doc 12 §7.4/§7.5, Doc 33 §4: fixed 7-day offline grace period after
/// payment/validation failure — a business invariant (Doc 03), never configurable.
const GRACE_PERIOD: ChronoDuration = ChronoDuration::days(7);

/// Doc 30 TASK-QA-006: whether the 7-day offline grace period has elapsed
/// since the last successful server validation -- extracted as a pure
/// function so the grace-expiry-to-LOCKED transition is independently
/// testable without needing the full 6-hour-interval background validation
/// loop (`start_background_validation`) to actually tick 7 days forward.
/// No `last_server_validated_at` at all (a state row somehow reaching GRACE
/// without ever having validated) is treated as expired -- there is no
/// evidence the grace window's clock has ever started, so it cannot be
/// trusted to still be running.
fn is_grace_period_expired(last_validated: Option<chrono::DateTime<Utc>>, now: chrono::DateTime<Utc>) -> bool {
    match last_validated {
        Some(last_validated) => now.signed_duration_since(last_validated) > GRACE_PERIOD,
        None => true,
    }
}

/// Doc 22 §10.5 (I7 fix): legitimate NTP/DST corrections can move the clock
/// backward by a few minutes — only a rollback larger than this grace window
/// counts as suspicious tampering. Previously *any* backward movement, even
/// one second, triggered an immediate lock.
const CLOCK_CORRECTION_GRACE: ChronoDuration = ChronoDuration::hours(1);

/// Distinguishes "no license state row yet" from "clock skew just locked the
/// license" — both used to collapse to the same `None`, which made it
/// impossible to tell the two apart at the call site in order to also emit
/// a `system_warning` for the clock-skew case specifically.
enum PollOutcome {
    NoState,
    ClockSkewLocked,
    Active(crate::licensing::state::LicenseStateRow),
}

pub async fn start_background_validation<R: tauri::Runtime>(
    pool: Pool,
    base_url: String,
    app_handle: tauri::AppHandle<R>,
) {
    let client = LicensingClient::new(base_url, pool.clone());
    // Doc 40 §7: "roughly every 6 hours" — this is also the cadence that must
    // keep running while LOCKED, so a healed payment auto-unlocks the app
    // without the user needing to click "Refresh License" (C11).
    let mut interval = time::interval(Duration::from_secs(6 * 60 * 60));

    loop {
        interval.tick().await;

        if let Ok(conn) = pool.get().await {
            let res = conn.interact(move |c| {
                let state_opt = get_license_state(c)?;

                if let Some(state) = state_opt {
                    let now = Utc::now();

                    // I7 fix: only a rollback beyond the correction grace counts as
                    // tampering — a small backward NTP/DST correction should not lock.
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
                // Doc 19 §15.1: alongside the existing `license_clock_skew` event
                // (kept as-is so any existing consumers don't break), also surface
                // this through the centralized system_warning channel so the
                // banner UI (TASK-RT-007) picks it up like every other
                // degraded-state condition.
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
                    // Doc 40 §7 (C11): LOCKED must keep validating too — this is what
                    // lets a healed payment auto-unlock the app without user action.
                    // Only AnonymousEval (no license ever entered, nothing to validate
                    // against) is skipped.
                    if state.subscription_status_cached != LicenseStatus::AnonymousEval {
                        // Doc 22 §11.1: always derive device_id fresh from the hardware UUID
                        // rather than trusting a possibly-stale/unset stored fingerprint.
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
                                // Doc 22 §10.3: the raw HTTP success is not sufficient — the JWT's
                                // RSA-256 signature must verify locally against the embedded public
                                // key before the response is trusted for anything.
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
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("License validation failed: {:?}", e);

                                let now = Utc::now();

                                // Apply Grace period logic (7 days — Doc 12 §7.4/§7.5, Doc 33 §4)
                                let _ = conn.interact(move |c| {
                                    if state.subscription_status_cached == LicenseStatus::Active {
                                        tracing::warn!("Transitioning license to GRACE period.");
                                        crate::licensing::state_machine::transition(c, LicenseStatus::Grace)?;
                                    } else if state.subscription_status_cached == LicenseStatus::Grace
                                        && is_grace_period_expired(state.last_server_validated_at, now)
                                    {
                                        tracing::warn!("Grace period expired. Locking license.");
                                        transition_to_locked(c, false)?;
                                    }
                                    Ok::<_, anyhow::Error>(())
                                }).await;
                            }
                        }
                    }
                }
                Ok(Ok(PollOutcome::NoState)) | Ok(Ok(PollOutcome::ClockSkewLocked)) => {}
                Ok(Err(e)) => tracing::error!("Error reading license state: {}", e),
                Err(e) => tracing::error!("Pool interact error: {}", e),
            }

            // TASK-FE-002/016: broadcast the (possibly just-changed) status once
            // per tick so `useLicenseStore` stays in sync without polling. Cheap
            // and idempotent on the frontend if nothing actually changed; simpler
            // and more robust than threading a "did this tick mutate?" flag out
            // of every branch above.
            crate::licensing::commands::emit_license_state_changed(&app_handle, &pool).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Doc 30 TASK-QA-006 acceptance: `test_grace_state_expires_to_read_only`
    /// (this half: the pure timing decision itself; the write-gate
    /// enforcement half is covered by `licensing::gate`'s own tests against
    /// the resulting `LOCKED` state).
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
