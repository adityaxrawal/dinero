// Doc 30 TASK-RT-007: standardized `system_warning` payload shape and a
// process-wide registry of currently-active warnings, so a component that
// mounts after the emitting check already ran (a real gap: prior sites
// emitted ad-hoc shapes -- a bare string in oauth.rs, no `severity`/
// `action_hint` anywhere) can re-derive correct state instead of only ever
// reacting to a live event it may have missed.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::Emitter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WarningSeverity {
    /// Blocks all functionality (e.g. clock-skew licensing lockout) --
    /// outranks every other simultaneously-active warning.
    Critical,
    /// Degrades a specific feature (Gmail sync paused) but the app remains usable.
    Degraded,
    /// Informational, no functional impact yet.
    Info,
}

impl WarningSeverity {
    fn rank(self) -> u8 {
        match self {
            WarningSeverity::Critical => 3,
            WarningSeverity::Degraded => 2,
            WarningSeverity::Info => 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemWarningPayload {
    pub warning_type: String,
    pub message: String,
    pub severity: WarningSeverity,
    /// Doc 30 TASK-RT-007: "so the frontend renders a specific recovery CTA
    /// rather than a generic error." e.g. "open_system_settings",
    /// "retry_gmail_connection", "wait_for_quota_reset", `None` when no
    /// specific recovery action exists beyond the message itself.
    pub action_hint: Option<String>,
}

static ACTIVE_WARNINGS: Mutex<Option<HashMap<String, SystemWarningPayload>>> = Mutex::new(None);

/// Emits `system_warning` and records it in the process-wide registry so a
/// late-mounting component can query `active_system_warnings()` instead of
/// only ever reacting to the live event.
pub fn emit_system_warning<R: tauri::Runtime>(app: &tauri::AppHandle<R>, warning: SystemWarningPayload) {
    {
        let mut guard = ACTIVE_WARNINGS.lock().unwrap();
        guard.get_or_insert_with(HashMap::new).insert(warning.warning_type.clone(), warning.clone());
    }
    let _ = app.emit("system_warning", &warning);
}

/// Doc 30 TASK-RT-007: "Warnings auto-clear once their condition resolves."
pub fn clear_system_warning<R: tauri::Runtime>(app: &tauri::AppHandle<R>, warning_type: &str) {
    {
        let mut guard = ACTIVE_WARNINGS.lock().unwrap();
        if let Some(map) = guard.as_mut() {
            map.remove(warning_type);
        }
    }
    let _ = app.emit("system_warning_cleared", warning_type);
}

pub fn active_system_warnings() -> Vec<SystemWarningPayload> {
    ACTIVE_WARNINGS
        .lock()
        .unwrap()
        .as_ref()
        .map(|m| m.values().cloned().collect())
        .unwrap_or_default()
}

/// Doc 30 TASK-RT-007: "`ConnectionStatusBanner` prioritizes by severity when
/// multiple warnings are simultaneously active." Ties broken by warning_type
/// for deterministic ordering.
pub fn highest_priority_warning(warnings: &[SystemWarningPayload]) -> Option<&SystemWarningPayload> {
    warnings.iter().max_by(|a, b| {
        a.severity
            .rank()
            .cmp(&b.severity.rank())
            .then_with(|| b.warning_type.cmp(&a.warning_type))
    })
}

#[tauri::command]
pub fn get_active_system_warnings() -> Vec<SystemWarningPayload> {
    active_system_warnings()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_warning_includes_actionable_hint() {
        let w = SystemWarningPayload {
            warning_type: "gmail_degraded".to_string(),
            message: "Gmail sync is paused".to_string(),
            severity: WarningSeverity::Degraded,
            action_hint: Some("retry_gmail_connection".to_string()),
        };
        assert_eq!(w.action_hint.as_deref(), Some("retry_gmail_connection"));
    }

    #[test]
    fn test_banner_prioritizes_by_severity() {
        let warnings = vec![
            SystemWarningPayload { warning_type: "gmail_quota".to_string(), message: "quota".to_string(), severity: WarningSeverity::Degraded, action_hint: None },
            SystemWarningPayload { warning_type: "clock_skew".to_string(), message: "skew".to_string(), severity: WarningSeverity::Critical, action_hint: None },
            SystemWarningPayload { warning_type: "low_ram".to_string(), message: "ram".to_string(), severity: WarningSeverity::Info, action_hint: None },
        ];
        let top = highest_priority_warning(&warnings).unwrap();
        assert_eq!(top.warning_type, "clock_skew");
    }

    #[test]
    fn test_warning_auto_clears_when_condition_resolves() {
        // Isolate from other tests sharing the same process-wide static --
        // use a warning_type unique to this test.
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        emit_system_warning(&handle, SystemWarningPayload {
            warning_type: "test_auto_clear_warning".to_string(),
            message: "x".to_string(),
            severity: WarningSeverity::Info,
            action_hint: None,
        });
        assert!(active_system_warnings().iter().any(|w| w.warning_type == "test_auto_clear_warning"));
        clear_system_warning(&handle, "test_auto_clear_warning");
        assert!(!active_system_warnings().iter().any(|w| w.warning_type == "test_auto_clear_warning"));
    }
}
