//! TASK-AUTH-014: Implement Incident Response — Suspicious Activity Detection.
//!
//! Monitors the trigger events documented in Document 22 §19.5/Document 26
//! §"Local Security Incident Response": repeated OAuth failures, repeated DB
//! decryption failures, `PRAGMA integrity_check` failures, audit-log write
//! failure. Response actions are all on-device: revoke the current session
//! (TASK-AUTH-005 logout), disable the affected integration (Gmail disconnect
//! via TASK-AUTH-006, for OAuth-related triggers specifically — a DB
//! integrity failure has nothing to do with Gmail and must not disconnect
//! it), and write an audit trail of the incident itself.
//!
//! Counters are in-memory only (Tauri-managed state), not persisted —
//! surviving only for the current run. A security monitor watching for *this
//! process's* database corruption/decryption failures has no reason to trust
//! a persisted counter it would need to read from the very store it's
//! watching for corruption.
//!
//! **Not wired into every trigger this task names:** "unexpected refresh
//! spikes" and "audit-log write failure" detection are not implemented here
//! — the former has no existing rate-tracking to hook into without a larger
//! addition to `ingestion::polling`, and the latter is closer to
//! unimplementable-by-construction than merely unwired: if `audit_log::insert`
//! itself is failing, recording *that* failure by calling `audit_log::insert`
//! again is circular. Wired: repeated DB decryption failures (invalid-token
//! Keychain reads are the closest analogous "credential looks compromised"
//! signal already surfaced by TASK-AUTH-004's degradation path) and
//! `PRAGMA integrity_check` failures (TASK-DB-019).

use anyhow::Result;
use deadpool_sqlite::Pool;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

/// The trigger conditions Document 22 §19.5 names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TriggerKind {
    RepeatedOAuthFailure,
    RepeatedDbDecryptionFailure,
    IntegrityCheckFailure,
    /// Doc 30 TASK-OPS-003: repeated Licensing Backend `/license/validate`
    /// failures (network/server error, not a signature problem — that's
    /// `SignatureVerificationFailure` below). Alert-only via
    /// `emit_health_alert`, never `respond_to_incident` — the 7-day grace
    /// period already protects access, so forcing a session logout over a
    /// flaky connection to the Licensing Backend would be actively harmful.
    RepeatedLicenseValidateFailure,
    /// A license JWT that failed RSA-256 signature verification against the
    /// embedded public key — covers both a tampered response and a
    /// production key-rotation mismatch ("signature-rotation issues" in
    /// Document 30's TASK-OPS-003 task text). Alert-only, same reasoning as
    /// above.
    SignatureVerificationFailure,
    /// The local backend (SQLite pool) failed to answer a liveness check
    /// after startup succeeded. Alert-only.
    BackendStartupFailure,
}

impl TriggerKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::RepeatedOAuthFailure => "repeated_oauth_failure",
            Self::RepeatedDbDecryptionFailure => "repeated_db_decryption_failure",
            Self::IntegrityCheckFailure => "integrity_check_failure",
            Self::RepeatedLicenseValidateFailure => "repeated_license_validate_failure",
            Self::SignatureVerificationFailure => "signature_verification_failure",
            Self::BackendStartupFailure => "backend_startup_failure",
        }
    }

    /// How many occurrences before this trigger fires a response. Document
    /// 22/26 name the trigger *conditions* but not a specific count for
    /// "repeated" — 3 is an engineering default (same class of judgment call
    /// as Document 18 §4.21b's retention-window default), not sourced from
    /// any document; adjust if a specific count is ever specified.
    fn threshold(&self) -> u32 {
        match self {
            // A single corrupt DB, bad signature, or dead backend is already
            // an incident worth surfacing — no reason to wait for a repeat.
            Self::IntegrityCheckFailure
            | Self::SignatureVerificationFailure
            | Self::BackendStartupFailure => 1,
            _ => 3,
        }
    }
}

/// Tauri-managed in-memory counters, one per `TriggerKind`. Register via
/// `app.manage(...)`.
#[derive(Default)]
pub struct IncidentMonitor(Mutex<std::collections::HashMap<TriggerKind, u32>>);

/// Records one occurrence of `kind`. Returns `true` exactly on the tick that
/// crosses the threshold (the counter resets immediately after firing, so a
/// sustained failure condition triggers a response once per fresh run of
/// failures, not on every single one).
pub fn record_trigger(monitor: &IncidentMonitor, kind: TriggerKind) -> bool {
    let mut counters = monitor.0.lock().unwrap();
    let count = counters.entry(kind).or_insert(0);
    *count += 1;
    if *count >= kind.threshold() {
        *count = 0;
        true
    } else {
        false
    }
}

/// Executes the on-device incident response for `kind`: writes an
/// `audit_log` entry describing the incident, emits a `security_incident`
/// event (for the frontend to prompt re-authentication), and revokes the
/// current session — always, regardless of trigger, since any of these
/// conditions is reason enough to require re-auth. For OAuth-specific
/// triggers, additionally disconnects every connected Gmail account (Document
/// 22 §19.5: "disable the affected integration ... if token theft
/// suspected").
pub async fn respond_to_incident<R: tauri::Runtime>(
    kind: TriggerKind,
    app: &AppHandle<R>,
    pool: &Pool,
    session_state: &crate::auth::session::SessionState,
) -> Result<()> {
    tracing::warn!("Security incident detected: {}", kind.as_str());

    if let Ok(conn) = pool.get().await {
        let kind_str = kind.as_str();
        let _ = conn
            .interact(move |c| {
                crate::db::audit_log::insert(
                    c,
                    &crate::db::audit_log::AuditLogRow {
                        id: uuid::Uuid::new_v4().to_string(),
                        actor_type: Some("system".to_string()),
                        actor_id: None,
                        action: Some("security_incident_detected".to_string()),
                        resource_type: Some("security".to_string()),
                        resource_id: None,
                        before_json: None,
                        after_json: Some(serde_json::json!({ "trigger": kind_str })),
                        created_at: chrono::Utc::now(),
                    },
                )
            })
            .await;
    }

    let _ = app.emit(
        "security_incident",
        serde_json::json!({ "trigger": kind.as_str() }),
    );

    if kind == TriggerKind::RepeatedOAuthFailure {
        crate::ingestion::oauth::revoke_gmail_access(pool).await;
    }

    crate::auth::session::logout(pool, session_state).await?;

    Ok(())
}

/// Doc 30 TASK-OPS-003: the alert-only counterpart to `respond_to_incident`
/// for `RepeatedLicenseValidateFailure` / `SignatureVerificationFailure` /
/// `BackendStartupFailure` — these are operational health conditions, not
/// suspected security compromises, so the response is a `system_warning`
/// the user/support can see, never a forced session logout. Call after
/// `record_trigger` returns `true`. A no-op for the three security-incident
/// trigger kinds, which must go through `respond_to_incident` instead.
pub fn emit_health_alert<R: tauri::Runtime>(kind: TriggerKind, app: &AppHandle<R>) {
    let (severity, message) = match kind {
        TriggerKind::RepeatedLicenseValidateFailure => (
            crate::ipc::system_warnings::WarningSeverity::Degraded,
            "License validation with the Licensing Backend has failed repeatedly. \
            Your access remains protected by the offline grace period while the \
            app keeps retrying."
                .to_string(),
        ),
        TriggerKind::SignatureVerificationFailure => (
            crate::ipc::system_warnings::WarningSeverity::Critical,
            "A license response failed signature verification and was rejected. \
            Please refresh your license from Settings, or contact support if this persists."
                .to_string(),
        ),
        TriggerKind::BackendStartupFailure => (
            crate::ipc::system_warnings::WarningSeverity::Critical,
            "Dinero's local database stopped responding. Please restart the app; \
            contact support if this persists."
                .to_string(),
        ),
        TriggerKind::RepeatedOAuthFailure
        | TriggerKind::RepeatedDbDecryptionFailure
        | TriggerKind::IntegrityCheckFailure => return,
    };

    crate::ipc::system_warnings::emit_system_warning(
        app,
        crate::ipc::system_warnings::SystemWarningPayload {
            warning_type: kind.as_str().to_string(),
            message,
            severity,
            action_hint: None,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_trigger_fires_exactly_at_threshold_then_resets() {
        let monitor = IncidentMonitor::default();
        assert!(!record_trigger(&monitor, TriggerKind::RepeatedOAuthFailure));
        assert!(!record_trigger(&monitor, TriggerKind::RepeatedOAuthFailure));
        assert!(record_trigger(&monitor, TriggerKind::RepeatedOAuthFailure));

        // Counter reset after firing — takes another full run to fire again.
        assert!(!record_trigger(&monitor, TriggerKind::RepeatedOAuthFailure));
    }

    #[test]
    fn integrity_check_failure_fires_on_the_first_occurrence() {
        let monitor = IncidentMonitor::default();
        assert!(record_trigger(&monitor, TriggerKind::IntegrityCheckFailure));
    }

    #[test]
    fn different_trigger_kinds_have_independent_counters() {
        let monitor = IncidentMonitor::default();
        assert!(!record_trigger(&monitor, TriggerKind::RepeatedOAuthFailure));
        assert!(!record_trigger(
            &monitor,
            TriggerKind::RepeatedDbDecryptionFailure
        ));
        assert!(!record_trigger(
            &monitor,
            TriggerKind::RepeatedDbDecryptionFailure
        ));
        // OAuth failure at 1, DB decryption at 2 (threshold 3 for both) —
        // neither has fired yet, and one more OAuth failure alone must not
        // be enough to cross DB decryption's independent counter.
        assert!(!record_trigger(&monitor, TriggerKind::RepeatedOAuthFailure));
    }

    /// Doc 30 TASK-OPS-003 acceptance: `test_alert_thresholds_trigger_on_critical_failures`.
    /// Backend startup failure and signature-verification failure are, like
    /// `IntegrityCheckFailure`, single-occurrence critical conditions —
    /// there's no reason to wait for a repeat before alerting.
    #[test]
    fn test_alert_thresholds_trigger_on_critical_failures() {
        let monitor = IncidentMonitor::default();
        assert!(record_trigger(&monitor, TriggerKind::BackendStartupFailure));
        assert!(record_trigger(
            &monitor,
            TriggerKind::SignatureVerificationFailure
        ));

        // RepeatedLicenseValidateFailure follows the "repeated" (3-strike)
        // convention shared with OAuth/DB-decryption failures, since a single
        // failed validate call is routine on a flaky connection.
        let license_monitor = IncidentMonitor::default();
        assert!(!record_trigger(
            &license_monitor,
            TriggerKind::RepeatedLicenseValidateFailure
        ));
        assert!(!record_trigger(
            &license_monitor,
            TriggerKind::RepeatedLicenseValidateFailure
        ));
        assert!(record_trigger(
            &license_monitor,
            TriggerKind::RepeatedLicenseValidateFailure
        ));
    }
}
