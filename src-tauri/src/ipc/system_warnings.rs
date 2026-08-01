// Doc 30 TASK-RT-007: standardized `system_warning` payload shape and a
// process-wide registry of currently-active warnings, so a component that
// mounts after the emitting check already ran (a real gap: prior sites
// emitted ad-hoc shapes -- a bare string in oauth.rs, no `severity`/
// `action_hint` anywhere) can re-derive correct state instead of only ever
// reacting to a live event it may have missed.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

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

/// audit_07 #10: `warning_type -> message_hash` for warnings the user has
/// dismissed, mirroring `dismissed_system_warnings`. Cached in the process so
/// `emit_system_warning` can stay synchronous — it is called from deep inside
/// sync check code with no pool access, and a DB round-trip per emit would
/// mean threading async through all of it for a table that changes only when
/// a human clicks something.
///
/// Populated once at startup by [`load_dismissals`]; kept in step by
/// [`record_dismissal`] and [`clear_system_warning`].
static DISMISSED: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// Seeds [`DISMISSED`] from the database. Called once during startup, before
/// the first condition check can emit anything.
pub fn load_dismissals(map: HashMap<String, String>) {
    *DISMISSED.lock().unwrap() = Some(map);
}

/// Whether this exact warning has already been waved away by the user.
///
/// `Critical` is never suppressible: it means functionality is blocked, so
/// silencing it would hide a lockout rather than reduce noise. The check is
/// here rather than at the dismiss command so it holds for every emit path,
/// including any future one that forgets.
fn is_dismissed(warning: &SystemWarningPayload) -> bool {
    if warning.severity == WarningSeverity::Critical {
        return false;
    }
    DISMISSED
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(&warning.warning_type))
        .is_some_and(|hash| *hash == crate::db::dismissed_warnings::message_hash(&warning.message))
}

/// Records a dismissal in the in-process cache. The durable half is written by
/// the `settings_dismiss_system_warning` command; this keeps the two in step
/// without making every emit hit the database.
pub fn remember_dismissal(warning_type: &str, message: &str) {
    DISMISSED
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(
            warning_type.to_string(),
            crate::db::dismissed_warnings::message_hash(message),
        );
}

/// Emits `system_warning` and records it in the process-wide registry so a
/// late-mounting component can query `active_system_warnings()` instead of
/// only ever reacting to the live event.
///
/// A dismissed warning is still recorded as active — the condition is real and
/// `active_system_warnings()` must keep reporting it, e.g. for the diagnostic
/// bundle. Only the user-facing *event* is suppressed.
pub fn emit_system_warning<R: tauri::Runtime>(app: &tauri::AppHandle<R>, warning: SystemWarningPayload) {
    {
        let mut guard = ACTIVE_WARNINGS.lock().unwrap();
        guard.get_or_insert_with(HashMap::new).insert(warning.warning_type.clone(), warning.clone());
    }
    if is_dismissed(&warning) {
        tracing::debug!(
            warning_type = warning.warning_type,
            "system warning suppressed — previously dismissed by the user"
        );
        return;
    }
    let _ = crate::ipc::events::emit_event(app, crate::ipc::events::AppEvent::SystemWarning, warning);
}

/// Doc 30 TASK-RT-007: "Warnings auto-clear once their condition resolves."
///
/// Also drops any dismissal (audit_07 #10). A resolved condition that later
/// recurs is a new event the user has not seen the resolution of, so it must
/// be shown again — a dismissal silences the message they read, not the
/// underlying class of problem forever.
pub fn clear_system_warning<R: tauri::Runtime>(app: &tauri::AppHandle<R>, warning_type: &str) {
    {
        let mut guard = ACTIVE_WARNINGS.lock().unwrap();
        if let Some(map) = guard.as_mut() {
            map.remove(warning_type);
        }
    }
    {
        let mut guard = DISMISSED.lock().unwrap();
        if let Some(map) = guard.as_mut() {
            map.remove(warning_type);
        }
    }
    let _ = crate::ipc::events::emit_event(
        app,
        crate::ipc::events::AppEvent::SystemWarningCleared,
        warning_type,
    );
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

/// audit_07 #10: records that the user has seen and dismissed this exact
/// warning, so it stops re-firing on every launch for a condition that is
/// structural (a machine permanently under the RAM threshold) rather than
/// transient.
///
/// Not gated on `assert_write_allowed`: silencing a notification is not a
/// financial-data write, and a LOCKED user staring at an undismissable banner
/// they cannot act on is worse than the alternative.
#[tauri::command]
pub async fn settings_dismiss_system_warning(
    warning_type: String,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<(), crate::error::AppError> {
    // Dismiss the message that is actually active, so the stored hash matches
    // what the user read — not whatever a caller might pass in.
    let Some(active) = active_system_warnings()
        .into_iter()
        .find(|w| w.warning_type == warning_type)
    else {
        return Ok(()); // Already cleared; nothing to remember.
    };
    if active.severity == WarningSeverity::Critical {
        return Err(crate::error::AppError::Validation(
            "Critical system warnings cannot be dismissed — they report blocked functionality"
                .to_string(),
        ));
    }

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let (wt, msg) = (warning_type.clone(), active.message.clone());
    conn.interact(move |c| crate::db::dismissed_warnings::record_dismissal(c, &wt, &msg))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    remember_dismissal(&warning_type, &active.message);
    Ok(())
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

    /// audit_07 #10: a dismissed warning must stop firing, a *changed* one
    /// must still fire, and a Critical one must never be silenceable. Each
    /// assertion is a separate way the naive version of this feature causes
    /// harm rather than reducing noise.
    #[test]
    fn dismissal_suppresses_only_the_exact_non_critical_warning() {
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();

        let warn = |wt: &str, msg: &str, sev: WarningSeverity| SystemWarningPayload {
            warning_type: wt.to_string(),
            message: msg.to_string(),
            severity: sev,
            action_hint: None,
        };

        let low_ram = warn("test_dismiss_low_ram", "Low RAM: 9 GB free", WarningSeverity::Info);
        assert!(!is_dismissed(&low_ram), "nothing dismissed yet");

        remember_dismissal(&low_ram.warning_type, &low_ram.message);
        assert!(is_dismissed(&low_ram), "the exact dismissed message stays quiet");

        // Same type, materially different message -- the user has not seen
        // this one, and it is the more urgent of the two.
        let worse = warn("test_dismiss_low_ram", "Low RAM: 1 GB free", WarningSeverity::Info);
        assert!(
            !is_dismissed(&worse),
            "a changed message must not inherit a prior dismissal"
        );

        // Critical is never suppressible, even if something recorded one.
        let critical = warn("test_dismiss_crit", "Clock skew — licensing locked", WarningSeverity::Critical);
        remember_dismissal(&critical.warning_type, &critical.message);
        assert!(
            !is_dismissed(&critical),
            "a Critical warning reports blocked functionality and must always fire"
        );

        // Emitting a dismissed warning still records it as active: the
        // condition is real, and the diagnostic bundle reads this registry.
        emit_system_warning(&handle, low_ram.clone());
        assert!(
            active_system_warnings()
                .iter()
                .any(|w| w.warning_type == "test_dismiss_low_ram"),
            "suppressing the event must not hide the condition from diagnostics"
        );

        // Condition resolves -> dismissal is dropped, so a recurrence shows.
        clear_system_warning(&handle, "test_dismiss_low_ram");
        assert!(
            !is_dismissed(&low_ram),
            "a resolved-then-recurring condition is a new event the user has not seen"
        );

        clear_system_warning(&handle, "test_dismiss_crit");
    }
}
