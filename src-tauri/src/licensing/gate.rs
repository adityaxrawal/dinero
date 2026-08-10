//! Enforces entitlement on operations that write.
//!
//! Reads stay available when locked by deliberate choice: a lapsed subscription
//! must not hold the user's own financial history hostage. What stops is
//! ingesting new data.
use crate::db::local_profile::select_by_id;
use crate::error::AppError;
use crate::licensing::state::{get_license_state, LicenseStatus};
use chrono::Utc;

pub const TRIAL_WINDOW_DAYS: i64 = 14;

/// Refuses a write when the licence does not permit it.
///
/// Applied to writes only. Reads stay available deliberately -- a lapsed
/// subscription must not hold the user's own financial history hostage.
pub async fn assert_write_allowed(pool: &deadpool_sqlite::Pool) -> Result<(), AppError> {
    let conn = pool.get().await.map_err(|e| AppError::Db(e.to_string()))?;
    conn.interact(|c| {
        let state = get_license_state(c).map_err(|e| AppError::Db(e.to_string()))?;

        let Some(state) = state else {
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

        let claims =
            crate::licensing::jwt::verify_license_jwt(&state.license_jwt).map_err(|_| {
                AppError::LicenseLocked(
                    "License could not be cryptographically verified — treating as locked"
                        .to_string(),
                )
            })?;

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

/// Confirms the trial has not expired.
fn assert_within_trial_window(c: &rusqlite::Connection) -> Result<(), AppError> {
    if trial_days_remaining(c)? < 0 {
        return Err(AppError::LicenseLocked(
            "14-day trial has expired — activate a license to continue".to_string(),
        ));
    }
    Ok(())
}

/// Days left in the trial, for display.
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
