use crate::licensing::client::{LicensingClient, ValidateRequest};
use crate::licensing::state::{get_license_state, record_known_valid_time, transition_to_locked, LicenseStatus};
use chrono::{Utc, Duration as ChronoDuration};
use deadpool_sqlite::Pool;
use std::time::Duration;
use tokio::time;

/// Doc 12 §7.4/§7.5, Doc 33 §4: fixed 7-day offline grace period after
/// payment/validation failure — a business invariant (Doc 03), never configurable.
const GRACE_PERIOD: ChronoDuration = ChronoDuration::days(7);

/// Doc 22 §10.5 (I7 fix): legitimate NTP/DST corrections can move the clock
/// backward by a few minutes — only a rollback larger than this grace window
/// counts as suspicious tampering. Previously *any* backward movement, even
/// one second, triggered an immediate lock.
const CLOCK_CORRECTION_GRACE: ChronoDuration = ChronoDuration::hours(1);

pub async fn start_background_validation(pool: Pool, base_url: String) {
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
                        return Ok::<_, anyhow::Error>(None);
                    }
                    
                    record_known_valid_time(c, now)?;
                    
                    Ok(Some(state))
                } else {
                    Ok(None)
                }
            }).await;

            match res {
                Ok(Ok(Some(state))) => {
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
                                        let _ = conn.interact(move |c| transition_to_locked(c, false)).await;
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
                                    } else if state.subscription_status_cached == LicenseStatus::Grace {
                                        // Doc 12 §7.4/§7.5, Doc 33 §4: 7-day offline grace period is a
                                        // fixed business invariant (Doc 03 pricing/packaging), not a tunable.
                                        if let Some(last_validated) = state.last_server_validated_at {
                                            if now.signed_duration_since(last_validated) > GRACE_PERIOD {
                                                tracing::warn!("Grace period expired. Locking license.");
                                                transition_to_locked(c, false)?;
                                            }
                                        } else {
                                            tracing::warn!("Grace period expired (no last validation). Locking license.");
                                            transition_to_locked(c, false)?;
                                        }
                                    }
                                    Ok::<_, anyhow::Error>(())
                                }).await;
                            }
                        }
                    }
                },
                Ok(Ok(None)) => {},
                Ok(Err(e)) => tracing::error!("Error reading license state: {}", e),
                Err(e) => tracing::error!("Pool interact error: {}", e),
            }
        }
    }
}
