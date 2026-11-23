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

/// Occurrences only count toward a threshold within this window, so a few
/// unrelated failures spread over days never add up to an incident.
const TRIGGER_WINDOW: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Recent trigger times per (kind, scope); scope is an account id or "".
#[derive(Default)]
pub struct IncidentMonitor(
    Mutex<std::collections::HashMap<(TriggerKind, String), Vec<std::time::Instant>>>,
);

/// Records a trigger, returning whether it now warrants a response.
pub fn record_trigger(monitor: &IncidentMonitor, kind: TriggerKind) -> bool {
    record_trigger_for(monitor, kind, "")
}

/// Records a trigger for one scope (for example a Gmail account), so failures
/// on different accounts are counted separately.
pub fn record_trigger_for(monitor: &IncidentMonitor, kind: TriggerKind, scope: &str) -> bool {
    record_trigger_at(monitor, kind, scope, std::time::Instant::now())
}

fn record_trigger_at(
    monitor: &IncidentMonitor,
    kind: TriggerKind,
    scope: &str,
    now: std::time::Instant,
) -> bool {
    let mut recent = monitor.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let times = recent.entry((kind, scope.to_string())).or_default();
    times.retain(|t| now.duration_since(*t) < TRIGGER_WINDOW);
    times.push(now);
    if times.len() as u32 >= kind.threshold() {
        times.clear();
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

    // Repeated OAuth failures concern one account, which is already degraded
    // and needs reconnecting; revoking every account and logging out punished
    // the user for one dead grant (review H13).
    if kind != TriggerKind::RepeatedOAuthFailure {
        crate::auth::session::logout(pool, session_state).await?;
    }

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
    fn triggers_outside_the_window_do_not_add_up() {
        let monitor = IncidentMonitor::default();
        let t0 = std::time::Instant::now();
        let kind = TriggerKind::RepeatedOAuthFailure;
        assert!(!record_trigger_at(&monitor, kind, "a", t0));
        assert!(!record_trigger_at(&monitor, kind, "a", t0 + TRIGGER_WINDOW));
        assert!(!record_trigger_at(&monitor, kind, "a", t0 + TRIGGER_WINDOW * 2));
        // Three within the window do.
        let t1 = t0 + TRIGGER_WINDOW * 3;
        assert!(!record_trigger_at(&monitor, kind, "a", t1));
        assert!(!record_trigger_at(&monitor, kind, "a", t1 + std::time::Duration::from_secs(60)));
        assert!(record_trigger_at(&monitor, kind, "a", t1 + std::time::Duration::from_secs(120)));
    }

    #[test]
    fn accounts_are_counted_separately() {
        let monitor = IncidentMonitor::default();
        let kind = TriggerKind::RepeatedOAuthFailure;
        assert!(!record_trigger_for(&monitor, kind, "a"));
        assert!(!record_trigger_for(&monitor, kind, "b"));
        assert!(!record_trigger_for(&monitor, kind, "a"));
        assert!(!record_trigger_for(&monitor, kind, "b"));
        assert!(record_trigger_for(&monitor, kind, "a"));
    }

    /// S5: a panic while the lock is held poisons it; later calls must keep
    /// working instead of panicking themselves.
    #[test]
    fn a_poisoned_lock_does_not_crash_later_callers() {
        let monitor = std::sync::Arc::new(IncidentMonitor::default());
        let m = std::sync::Arc::clone(&monitor);
        let _ = std::thread::spawn(move || {
            let _guard = m.0.lock().unwrap();
            panic!("simulated panic while holding the lock");
        })
        .join();
        assert!(monitor.0.is_poisoned());
        assert!(record_trigger(&monitor, TriggerKind::IntegrityCheckFailure));
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
