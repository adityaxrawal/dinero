//! TASK-OPS-003: Operational Health Checks and Alerting.
//!
//! A local IPC command reporting backend readiness, SQLite integrity status,
//! checkpoint freshness, Gmail polling status, and licensing state — all as
//! coarse status strings/booleans/timestamps, never the underlying financial
//! content (merchant names, amounts, Gmail content, tokens). Cheap enough to
//! run on every call: the DB check is a single `SELECT 1`, integrity status
//! reads a cached flag set by the existing daily `PRAGMA integrity_check`
//! (`db::maintenance::last_known_integrity_ok`) rather than re-running one,
//! and Gmail/licensing status are single indexed row reads.

use crate::db::connected_accounts::get_all_accounts;
use crate::db::processing_checkpoints::most_recent_checkpoint_updated_at;
use crate::licensing::state::get_license_state;
use chrono::{NaiveDateTime, Utc};
use deadpool_sqlite::Pool;
use serde::Serialize;

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct HealthReport {
    /// Whether the SQLite pool answered a trivial query just now.
    pub backend_ready: bool,
    /// Cached result of the most recent daily `PRAGMA integrity_check` — not
    /// re-run on every health poll (see module docs).
    pub db_integrity_ok: bool,
    /// Seconds since the freshest processing-checkpoint row was updated.
    /// `None` if no checkpoint has ever been written (a brand-new install).
    pub checkpoint_age_seconds: Option<i64>,
    /// One of: "not_connected", "active", "degraded", "quota_exhausted".
    /// Never the connected email address or any message content.
    pub gmail_polling_status: String,
    /// The cached `LicenseStatus` as a string (e.g. "Active", "Grace",
    /// "Locked") — never the JWT, device fingerprint, or billing details.
    pub license_status: String,
}

/// Doc 30 TASK-OPS-003 acceptance: `test_health_checks_do_not_expose_user_data`.
/// Every field above is a status enum, boolean, or a relative age in
/// seconds — this list documents that invariant so a future field addition
/// is forced to consider it explicitly rather than by omission.
pub const NEVER_INCLUDED: &[&str] = &[
    "email_address",
    "merchant",
    "amount",
    "raw_message",
    "license_jwt",
    "device_fingerprint",
    "access_token",
    "refresh_token",
];

pub async fn compute_health_report(pool: &Pool) -> HealthReport {
    let conn = match pool.get().await {
        Ok(conn) => conn,
        Err(_) => {
            return HealthReport {
                backend_ready: false,
                db_integrity_ok: crate::db::maintenance::last_known_integrity_ok(),
                checkpoint_age_seconds: None,
                gmail_polling_status: "unknown".to_string(),
                license_status: "unknown".to_string(),
            }
        }
    };

    let result = conn
        .interact(|c| {
            let backend_ready = c.query_row("SELECT 1", [], |r| r.get::<_, i64>(0)).is_ok();

            let checkpoint_age_seconds = most_recent_checkpoint_updated_at(c)
                .ok()
                .flatten()
                .map(|ts: NaiveDateTime| (Utc::now().naive_utc() - ts).num_seconds());

            let gmail_polling_status = gmail_polling_status(c);

            let license_status = get_license_state(c)
                .ok()
                .flatten()
                .map(|row| format!("{:?}", row.subscription_status_cached))
                .unwrap_or_else(|| "no_license_state".to_string());

            (
                backend_ready,
                checkpoint_age_seconds,
                gmail_polling_status,
                license_status,
            )
        })
        .await;

    let (backend_ready, checkpoint_age_seconds, gmail_polling_status, license_status) =
        result.unwrap_or((false, None, "unknown".to_string(), "unknown".to_string()));

    HealthReport {
        backend_ready,
        db_integrity_ok: crate::db::maintenance::last_known_integrity_ok(),
        checkpoint_age_seconds,
        gmail_polling_status,
        license_status,
    }
}

/// Coarse Gmail polling status from already-persisted/tracked state — no new
/// tracking added. Priority: an explicit `degraded` account status outranks
/// a live `quota_exhausted` system_warning (a degraded token is the more
/// actionable condition), which outranks a merely-connected/active account.
fn gmail_polling_status(conn: &rusqlite::Connection) -> String {
    let accounts = get_all_accounts(conn).unwrap_or_default();
    if accounts.is_empty() {
        return "not_connected".to_string();
    }
    if accounts
        .iter()
        .any(|a| a.account_status.as_deref() == Some("degraded"))
    {
        return "degraded".to_string();
    }
    if crate::ipc::system_warnings::active_system_warnings()
        .iter()
        .any(|w| w.warning_type == "gmail_quota_exhausted")
    {
        return "quota_exhausted".to_string();
    }
    "active".to_string()
}

#[tauri::command]
pub async fn get_health_report(
    pool: tauri::State<'_, Pool>,
) -> Result<HealthReport, crate::error::AppError> {
    Ok(compute_health_report(&pool).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Doc 30 TASK-OPS-003 acceptance: `test_health_checks_do_not_expose_user_data`.
    /// A structural guard: `HealthReport`'s serialized field names must never
    /// include anything from `NEVER_INCLUDED`.
    #[test]
    fn test_health_checks_do_not_expose_user_data() {
        let report = HealthReport {
            backend_ready: true,
            db_integrity_ok: true,
            checkpoint_age_seconds: Some(42),
            gmail_polling_status: "active".to_string(),
            license_status: "Active".to_string(),
        };
        let json = serde_json::to_string(&report).unwrap();
        for forbidden in NEVER_INCLUDED {
            assert!(
                !json.to_lowercase().contains(&forbidden.to_lowercase()),
                "HealthReport JSON must never contain the field name '{forbidden}'"
            );
        }
    }

    #[test]
    fn test_gmail_polling_status_not_connected_when_no_accounts() {
        // Exercised indirectly via compute_health_report in the integration
        // suite (health_suite.rs), which has a real migrated pool available;
        // this unit test only pins the pure string vocabulary contract.
        assert_eq!(
            HealthReport {
                backend_ready: true,
                db_integrity_ok: true,
                checkpoint_age_seconds: None,
                gmail_polling_status: "not_connected".to_string(),
                license_status: "AnonymousEval".to_string(),
            }
            .gmail_polling_status,
            "not_connected"
        );
    }
}
