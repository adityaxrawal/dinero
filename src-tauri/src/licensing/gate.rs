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

/// The token's signed `state` decides entitlement when it has one (audit
/// E-1): the cached status column is locally editable, so a genuine but
/// LOCKED-state token must not pass because the column says ACTIVE. Tokens
/// minted before the claim existed fall back to the cached-row check above.
fn signed_state_entitles(claims: &crate::licensing::jwt::LicenseClaims) -> Result<(), AppError> {
    match claims.state.as_deref().map(LicenseStatus::from_server_state) {
        Some(status) if !status.is_entitled() => Err(AppError::LicenseLocked(format!(
            "License is {} — writes are blocked until it is renewed or refreshed",
            status.as_str()
        ))),
        _ => Ok(()),
    }
}

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

        if state.subscription_status_cached == LicenseStatus::AnonymousEval {
            return Err(AppError::LicenseLocked(
                "Anonymous evaluation mode is read-only".to_string(),
            ));
        }
        // A trial the server never registered (offline first launch) falls
        // back to the local window; a registered one is a signed licence like
        // any other, and the server signs it LOCKED once it ends (M15).
        if state.subscription_status_cached == LicenseStatus::Trial && state.license_jwt.is_empty() {
            return assert_within_trial_window(c);
        }
        // Only entitling states pass; Locked, Canceled and Expired used to be
        // let through by a valid signature alone (review H2).
        if !state.subscription_status_cached.is_entitled() {
            return Err(AppError::LicenseLocked(format!(
                "License is {} — writes are blocked until it is renewed or refreshed",
                state.subscription_status_cached.as_str()
            )));
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

        signed_state_entitles(&claims)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
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

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(state: Option<&str>) -> crate::licensing::jwt::LicenseClaims {
        crate::licensing::jwt::LicenseClaims {
            sub: "user@example.com".into(),
            device_id: "device".into(),
            plan: "full".into(),
            billing_interval: "monthly".into(),
            exp: Utc::now().timestamp() + 3600,
            state: state.map(str::to_string),
        }
    }

    /// Audit E-1: a verified token whose signed state is not entitling blocks
    /// writes whatever the cached column says; no claim defers to the column.
    #[test]
    fn signed_state_decides_entitlement() {
        assert!(signed_state_entitles(&claims(Some("ACTIVE"))).is_ok());
        assert!(signed_state_entitles(&claims(Some("PAST_DUE"))).is_ok());
        assert!(matches!(
            signed_state_entitles(&claims(Some("LOCKED"))),
            Err(AppError::LicenseLocked(_))
        ));
        assert!(matches!(
            signed_state_entitles(&claims(Some("CANCELED"))),
            Err(AppError::LicenseLocked(_))
        ));
        assert!(signed_state_entitles(&claims(None)).is_ok());
    }
    use crate::licensing::state::{upsert_license_state, LicenseStateRow};

    async fn pool_with_trial(jwt: &str) -> deadpool_sqlite::Pool {
        let dir = std::env::temp_dir().join(format!("dinero_gate_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = crate::db::init_db(dir.join("t.db")).await.unwrap();
        let jwt = jwt.to_string();
        pool.get()
            .await
            .unwrap()
            .interact(move |c| {
                c.execute("INSERT OR IGNORE INTO local_profile (id) VALUES (1)", []).unwrap();
                let now = Utc::now();
                upsert_license_state(
                    c,
                    &LicenseStateRow {
                        id: 1,
                        license_jwt: jwt,
                        subscription_status_cached: LicenseStatus::Trial,
                        plan_id_cached: None,
                        current_period_end_cached: None,
                        jwt_expires_at: now,
                        last_server_validated_at: None,
                        last_known_valid_time: now,
                        device_fingerprint: None,
                        source: "server_fresh".into(),
                        billing_interval_cached: None,
                    },
                )
                .unwrap();
            })
            .await
            .unwrap();
        pool
    }

    /// Review M15: an unregistered trial uses the local window; a registered
    /// one must carry a valid signed token (the server signs it LOCKED once
    /// the trial ends), so a forged or broken token does not pass.
    #[tokio::test]
    async fn trial_gate_uses_local_window_only_when_unregistered() {
        assert!(assert_write_allowed(&pool_with_trial("").await).await.is_ok());
        assert!(matches!(
            assert_write_allowed(&pool_with_trial("not-a-real-token").await).await,
            Err(AppError::LicenseLocked(_))
        ));
    }
}
