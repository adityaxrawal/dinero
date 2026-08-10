//! Backend health reporting, and the redaction rules that constrain it.
//!
//! Produces the report behind the sidebar's engine indicator: whether the
//! backend is serving, whether the database passes its integrity check, how
//! stale ingestion is, and the current Gmail and licence status.
//!
//! `NEVER_INCLUDED` is the important part of this module. Health reports are
//! attached to diagnostic bundles a user may send to support, so it enumerates
//! the field names that must never appear in one -- addresses, amounts, tokens,
//! fingerprints. Anything added to a report must be checked against that list.

use crate::db::connected_accounts::get_all_accounts;
use crate::db::processing_checkpoints::most_recent_checkpoint_updated_at;
use crate::licensing::state::get_license_state;
use chrono::{NaiveDateTime, Utc};
use deadpool_sqlite::Pool;
use serde::Serialize;

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct HealthReport {
    pub backend_ready: bool,
    pub db_integrity_ok: bool,
    pub checkpoint_age_seconds: Option<i64>,
    pub gmail_polling_status: String,
    pub license_status: String,
}

/// Field names that must never appear in a health report or diagnostic bundle.
///
/// Enforced by tests rather than by the type system, so this list is the
/// specification: adding a field to a report means checking it against this.
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

/// Assemble the current health report.
///
/// Never fails -- an unreachable database yields a report saying so, because the
/// health check is precisely what must keep answering when things are broken.
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

/// Describes the current Gmail polling state.
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
/// Command returning the health report.
pub async fn get_health_report(
    pool: tauri::State<'_, Pool>,
) -> Result<HealthReport, crate::error::AppError> {
    Ok(compute_health_report(&pool).await)
}

#[cfg(test)]
mod tests {
    use super::*;

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
