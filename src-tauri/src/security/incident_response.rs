//! Escalates security-relevant triggers into a response.
//!
//! Triggers are deduplicated before acting, so a repeating condition -- a failing
//! integrity check on every cycle -- produces one response rather than a storm of
//! them. Response can revoke the local session, which is what makes the guard on
//! write commands meaningful.
use anyhow::Result;
use deadpool_sqlite::Pool;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TriggerKind {
    RepeatedOAuthFailure,
    RepeatedDbDecryptionFailure,
    IntegrityCheckFailure,
    RepeatedLicenseValidateFailure,
    SignatureVerificationFailure,
    BackendStartupFailure,
}

impl TriggerKind {
    /// Stable identifier for the trigger kind.
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

    /// How many occurrences before this trigger escalates.
    ///
    /// Thresholds prevent a repeating condition producing a storm of responses: an
    /// integrity check failing every cycle should escalate once, not hourly.
    fn threshold(&self) -> u32 {
        match self {
            Self::IntegrityCheckFailure
            | Self::SignatureVerificationFailure
            | Self::BackendStartupFailure => 1,
            _ => 3,
        }
    }
}

#[derive(Default)]
pub struct IncidentMonitor(Mutex<std::collections::HashMap<TriggerKind, u32>>);

/// Records a trigger, returning whether it now warrants a response.
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

/// Responds to an incident, up to revoking the local session.
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

/// Emits a health alert to the frontend.
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
        assert!(!record_trigger(&monitor, TriggerKind::RepeatedOAuthFailure));
    }

    #[test]
    fn test_alert_thresholds_trigger_on_critical_failures() {
        let monitor = IncidentMonitor::default();
        assert!(record_trigger(&monitor, TriggerKind::BackendStartupFailure));
        assert!(record_trigger(
            &monitor,
            TriggerKind::SignatureVerificationFailure
        ));

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
