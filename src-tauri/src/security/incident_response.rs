//! Escalates security-relevant triggers into a response.
//!
//! Triggers are deduplicated before acting, so a repeating condition -- a failing
//! integrity check on every cycle -- produces one response rather than a storm of
//! them.
//!
//! What a response stops (review P5): an integrity failure pauses all
//! ingestion (Gmail polling, scans, statement imports and the queues behind
//! them) until the user restores a backup from the recovery screen; reads and
//! edits stay available. A licence signature failure locks the licence (see
//! the licensing worker). The other triggers only alert.
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

/// Whether ingestion is paused because the database failed its integrity
/// check. Cleared only by a restart (normally after restoring a backup).
pub fn ingestion_paused() -> bool {
    crate::db::maintenance::integrity_failed_this_run()
}

/// Refuses a user-started ingestion action while ingestion is paused.
pub fn assert_ingestion_allowed() -> Result<(), crate::error::AppError> {
    if ingestion_paused() {
        return Err(crate::error::AppError::Forbidden(
            "Importing is paused because the database failed its integrity check. \
             Restore a backup from the recovery screen to resume."
                .to_string(),
        ));
    }
    Ok(())
}

/// Blocks while ingestion is paused; for background workers, which then
/// resume where they were instead of failing their jobs.
pub async fn wait_while_ingestion_paused() {
    while ingestion_paused() {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

/// Records and announces an incident.
pub async fn respond_to_incident<R: tauri::Runtime>(
    kind: TriggerKind,
    app: &AppHandle<R>,
    pool: &Pool,
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
    emit_health_alert(kind, app);

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
        TriggerKind::IntegrityCheckFailure => (
            crate::ipc::system_warnings::WarningSeverity::Critical,
            "Dinero found damage in its database and has paused importing new \
            transactions. Your data stays readable and editable. Restore a backup \
            from the recovery screen to resume importing."
                .to_string(),
        ),
        TriggerKind::RepeatedOAuthFailure | TriggerKind::RepeatedDbDecryptionFailure => return,
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

    /// Serialises the tests that flip the process-wide integrity flags.
    static INTEGRITY_FLAG: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Review P5: an integrity failure pauses ingestion until restart. The
    /// flag is process-wide, so it is flipped for as short a window as possible.
    #[test]
    fn integrity_failure_pauses_ingestion() {
        let _guard = INTEGRITY_FLAG.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        crate::db::maintenance::set_last_integrity_ok_for_test(false);
        let paused = ingestion_paused();
        let refused = assert_ingestion_allowed();
        crate::db::maintenance::set_last_integrity_ok_for_test(true);
        assert!(paused);
        assert!(matches!(refused, Err(crate::error::AppError::Forbidden(_))));
        assert!(!ingestion_paused());
        assert!(assert_ingestion_allowed().is_ok());
    }

    /// A later passing check (the daily re-check, or after an in-app restore)
    /// does not lift the pause: only a restart does.
    #[test]
    fn a_later_passing_check_keeps_ingestion_paused() {
        let _guard = INTEGRITY_FLAG.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        crate::db::maintenance::record_integrity_result_for_test(false);
        crate::db::maintenance::record_integrity_result_for_test(true);
        let still_paused = ingestion_paused();
        let health_ok = crate::db::maintenance::last_known_integrity_ok();
        crate::db::maintenance::set_last_integrity_ok_for_test(true);
        assert!(still_paused);
        assert!(health_ok, "health still reports the latest result");
    }

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
