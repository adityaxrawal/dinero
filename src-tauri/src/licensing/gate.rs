use crate::db::local_profile::select_by_id;
use crate::error::AppError;
use crate::licensing::state::{get_license_state, LicenseStatus};
use chrono::Utc;

/// Trial window length (Doc 22 §11.5: "TRIAL: Full read/write, 14-day window").
pub const TRIAL_WINDOW_DAYS: i64 = 14;

/// Enforces the LOCKED state (and the pre-activation trial window) across every
/// write-path IPC command (Doc 22 §10.3, §11.5). Must be the first thing a
/// mutating command does — never trust `subscription_status_cached` alone: a
/// non-trial status is only honored if the stored JWT it's supposedly backed by
/// still verifies cryptographically, so a hand-edited status column with a
/// stale/invalid JWT is refused regardless of what the column claims
/// (fail closed, Doc 15 §2 principle 12).
pub async fn assert_write_allowed(pool: &deadpool_sqlite::Pool) -> Result<(), AppError> {
    let conn = pool.get().await.map_err(|e| AppError::Db(e.to_string()))?;
    conn.interact(|c| {
        let state = get_license_state(c).map_err(|e| AppError::Db(e.to_string()))?;

        let Some(state) = state else {
            // No license_state row at all — nothing has ever activated on this
            // install. Doc 22 §11.5 defaults a new install to TRIAL (full
            // read/write) for 14 days from local_profile.created_at, not LOCKED —
            // trial requires no server contact since it isn't a paid state.
            return assert_within_trial_window(c);
        };

        if state.subscription_status_cached == LicenseStatus::Locked {
            return Err(AppError::LicenseLocked(
                "License is locked — all writes are blocked until it is refreshed".to_string(),
            ));
        }
        if state.subscription_status_cached == LicenseStatus::AnonymousEval {
            return Err(AppError::LicenseLocked(
                "Anonymous evaluation mode is read-only".to_string(),
            ));
        }
        if state.subscription_status_cached == LicenseStatus::Trial {
            return assert_within_trial_window(c);
        }

        // Active and Grace both permit writes per Doc 22 §11.5 — but only if the
        // stored JWT they're supposedly backed by still verifies.
        let claims =
            crate::licensing::jwt::verify_license_jwt(&state.license_jwt).map_err(|_| {
                AppError::LicenseLocked(
                    "License could not be cryptographically verified — treating as locked"
                        .to_string(),
                )
            })?;

        // Doc 22 §11.2: the JWT's device_id claim must match this machine's
        // derived fingerprint on every gate check — this is what makes a copied
        // JWT useless on a second machine.
        let device_id = crate::licensing::device::get_device_id()
            .map_err(|e| AppError::LicenseLocked(format!("Failed to derive device_id: {}", e)))?;
        if claims.device_id != device_id {
            return Err(AppError::LicenseLocked(
                "License is bound to a different device".to_string(),
            ));
        }

        Ok(())
    })
    .await
    .map_err(|e| AppError::Unknown(e.to_string()))?
}

fn assert_within_trial_window(c: &rusqlite::Connection) -> Result<(), AppError> {
    if trial_days_remaining(c)? < 0 {
        return Err(AppError::LicenseLocked(
            "14-day trial has expired — activate a license to continue".to_string(),
        ));
    }
    Ok(())
}

/// Days left in the 14-day trial window computed from `local_profile.created_at`
/// (Doc 22 §11.5) — negative once expired. Shared by the write-gate above and by
/// `license_get_status` (Doc 19 §14.1) so both report the identical window.
pub fn trial_days_remaining(c: &rusqlite::Connection) -> Result<i64, AppError> {
    let profile = select_by_id(c, 1)
        .map_err(|e| AppError::Db(e.to_string()))?
        .ok_or_else(|| AppError::LicenseLocked("No local profile found".to_string()))?;

    let created_at = profile
        .created_at
        .ok_or_else(|| AppError::LicenseLocked("Profile has no creation timestamp".to_string()))?;

    let elapsed_days = (Utc::now().naive_utc() - created_at).num_days();
    Ok(TRIAL_WINDOW_DAYS - elapsed_days)
}
