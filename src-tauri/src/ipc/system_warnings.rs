//! Emits system warnings to the frontend, honouring dismissals.
//!
//! Dismissals are held in memory and hydrated from the database at startup, so a
//! warning the user has dismissed does not reappear on every launch while the
//! underlying condition persists.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WarningSeverity {
    Critical,
    Degraded,
    Info,
}

impl WarningSeverity {
    /// Numeric severity, so warnings can be ordered by importance.
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
    pub action_hint: Option<String>,
}

static ACTIVE_WARNINGS: Mutex<Option<HashMap<String, SystemWarningPayload>>> = Mutex::new(None);

static DISMISSED: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// Warnings the app re-derives on its own after a restart (watchdog, RAM and
/// clock checks). They are not persisted: a stored copy could outlive its
/// cause, since nothing would clear it.
const REDERIVED: &[&str] = &["gmail_sync_stale", "gmail_token_degraded", "low_ram", "clock_skew"];

fn persisted(warning_type: &str) -> bool {
    !REDERIVED.contains(&warning_type)
}

/// Loads persisted active warnings at startup, so a warning whose cause is
/// still present (e.g. a failed backup) survives a restart.
pub fn load_active(payloads: Vec<SystemWarningPayload>) {
    let mut guard = ACTIVE_WARNINGS.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    for w in payloads.into_iter().filter(|w| persisted(&w.warning_type)) {
        map.entry(w.warning_type.clone()).or_insert(w);
    }
}

/// Mirrors an active-warning change to the database, if a pool is managed.
fn persist<R: tauri::Runtime>(app: &tauri::AppHandle<R>, warning_type: &str, payload: Option<String>) {
    use tauri::Manager;
    if !persisted(warning_type) {
        return;
    }
    let Some(pool) = app.try_state::<deadpool_sqlite::Pool>().map(|p| p.inner().clone()) else {
        return;
    };
    let wt = warning_type.to_string();
    tauri::async_runtime::spawn(async move {
        let Ok(conn) = pool.get().await else { return };
        let result = conn
            .interact(move |c| match payload {
                Some(json) => crate::db::active_warnings::upsert(c, &wt, &json),
                None => crate::db::active_warnings::delete(c, &wt),
            })
            .await;
        if let Ok(Err(e)) = result {
            tracing::warn!("Failed to persist system warning state: {}", e);
        }
    });
}

/// Loads persisted dismissals at startup, before any warning can be raised.
pub fn load_dismissals(map: HashMap<String, String>) {
    *DISMISSED.lock().unwrap() = Some(map);
}

/// Whether this exact warning has been dismissed.
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

/// Records a dismissal in memory and persists it.
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

/// Emits a warning unless the user has dismissed it.
pub fn emit_system_warning<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    warning: SystemWarningPayload,
) {
    {
        let mut guard = ACTIVE_WARNINGS.lock().unwrap();
        guard
            .get_or_insert_with(HashMap::new)
            .insert(warning.warning_type.clone(), warning.clone());
    }
    persist(app, &warning.warning_type, serde_json::to_string(&warning).ok());
    if is_dismissed(&warning) {
        tracing::debug!(
            warning_type = warning.warning_type,
            "system warning suppressed — previously dismissed by the user"
        );
        return;
    }
    let _ =
        crate::ipc::events::emit_event(app, crate::ipc::events::AppEvent::SystemWarning, warning);
}

/// Clears a warning once its underlying condition is resolved.
pub fn clear_system_warning<R: tauri::Runtime>(app: &tauri::AppHandle<R>, warning_type: &str) {
    {
        let mut guard = ACTIVE_WARNINGS.lock().unwrap();
        let removed = guard.as_mut().and_then(|map| map.remove(warning_type));
        // Callers clear on every successful cycle; a warning that is not
        // active needs no event or write.
        if removed.is_none() {
            return;
        }
    }
    persist(app, warning_type, None);
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

/// All currently active warnings.
pub fn active_system_warnings() -> Vec<SystemWarningPayload> {
    ACTIVE_WARNINGS
        .lock()
        .unwrap()
        .as_ref()
        .map(|m| m.values().cloned().collect())
        .unwrap_or_default()
}

/// The most severe active warning, for surfaces showing only one.
pub fn highest_priority_warning(
    warnings: &[SystemWarningPayload],
) -> Option<&SystemWarningPayload> {
    warnings.iter().max_by(|a, b| {
        a.severity
            .rank()
            .cmp(&b.severity.rank())
            .then_with(|| b.warning_type.cmp(&a.warning_type))
    })
}

#[tauri::command]
/// Command returning the active warnings.
pub fn get_active_system_warnings() -> Vec<SystemWarningPayload> {
    active_system_warnings()
}

#[tauri::command]
/// Command dismissing a warning.
pub async fn settings_dismiss_system_warning(
    warning_type: String,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<(), crate::error::AppError> {
    let Some(active) = active_system_warnings()
        .into_iter()
        .find(|w| w.warning_type == warning_type)
    else {
        return Ok(());
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

    /// Follow-up sweep: warnings survive a restart (except re-derived ones),
    /// and clearing removes the stored copy.
    #[tokio::test]
    async fn warnings_are_persisted_reloaded_and_cleared() {
        use tauri::Manager;
        let dir = std::env::temp_dir().join(format!("dinero_warn_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = crate::db::init_db(dir.join("t.db")).await.unwrap();
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        app.manage(pool.clone());
        let warn = |t: &str| SystemWarningPayload {
            warning_type: t.into(),
            message: "m".into(),
            severity: WarningSeverity::Degraded,
            action_hint: None,
        };
        emit_system_warning(app.handle(), warn("persist_test_backup"));
        emit_system_warning(app.handle(), warn("gmail_sync_stale"));
        let stored = |pool: deadpool_sqlite::Pool| async move {
            pool.get()
                .await
                .unwrap()
                .interact(|c| crate::db::active_warnings::load_all(c))
                .await
                .unwrap()
                .unwrap()
        };
        let mut rows = Vec::new();
        for _ in 0..50 {
            rows = stored(pool.clone()).await;
            if !rows.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(rows.len(), 1, "only the non-re-derived warning is stored: {rows:?}");
        assert!(rows[0].contains("persist_test_backup"));

        clear_system_warning(app.handle(), "persist_test_backup");
        for _ in 0..50 {
            if stored(pool.clone()).await.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(stored(pool.clone()).await.is_empty(), "cleared warnings leave the table");
        clear_system_warning(app.handle(), "gmail_sync_stale");
    }

    #[test]
    fn reload_skips_re_derived_warnings() {
        let w = |t: &str| SystemWarningPayload {
            warning_type: t.into(),
            message: "m".into(),
            severity: WarningSeverity::Info,
            action_hint: None,
        };
        load_active(vec![w("reload_test_kept"), w("low_ram")]);
        let active = active_system_warnings();
        assert!(active.iter().any(|a| a.warning_type == "reload_test_kept"));
        assert!(!active.iter().any(|a| a.warning_type == "low_ram" && a.message == "m"));
        ACTIVE_WARNINGS.lock().unwrap().as_mut().unwrap().remove("reload_test_kept");
    }

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
            SystemWarningPayload {
                warning_type: "gmail_quota".to_string(),
                message: "quota".to_string(),
                severity: WarningSeverity::Degraded,
                action_hint: None,
            },
            SystemWarningPayload {
                warning_type: "clock_skew".to_string(),
                message: "skew".to_string(),
                severity: WarningSeverity::Critical,
                action_hint: None,
            },
            SystemWarningPayload {
                warning_type: "low_ram".to_string(),
                message: "ram".to_string(),
                severity: WarningSeverity::Info,
                action_hint: None,
            },
        ];
        let top = highest_priority_warning(&warnings).unwrap();
        assert_eq!(top.warning_type, "clock_skew");
    }

    #[test]
    fn test_warning_auto_clears_when_condition_resolves() {
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        emit_system_warning(
            &handle,
            SystemWarningPayload {
                warning_type: "test_auto_clear_warning".to_string(),
                message: "x".to_string(),
                severity: WarningSeverity::Info,
                action_hint: None,
            },
        );
        assert!(active_system_warnings()
            .iter()
            .any(|w| w.warning_type == "test_auto_clear_warning"));
        clear_system_warning(&handle, "test_auto_clear_warning");
        assert!(!active_system_warnings()
            .iter()
            .any(|w| w.warning_type == "test_auto_clear_warning"));
    }

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

        let low_ram = warn(
            "test_dismiss_low_ram",
            "Low RAM: 9 GB free",
            WarningSeverity::Info,
        );
        assert!(!is_dismissed(&low_ram), "nothing dismissed yet");

        remember_dismissal(&low_ram.warning_type, &low_ram.message);
        assert!(
            is_dismissed(&low_ram),
            "the exact dismissed message stays quiet"
        );

        let worse = warn(
            "test_dismiss_low_ram",
            "Low RAM: 1 GB free",
            WarningSeverity::Info,
        );
        assert!(
            !is_dismissed(&worse),
            "a changed message must not inherit a prior dismissal"
        );

        let critical = warn(
            "test_dismiss_crit",
            "Clock skew — licensing locked",
            WarningSeverity::Critical,
        );
        remember_dismissal(&critical.warning_type, &critical.message);
        assert!(
            !is_dismissed(&critical),
            "a Critical warning reports blocked functionality and must always fire"
        );

        emit_system_warning(&handle, low_ram.clone());
        assert!(
            active_system_warnings()
                .iter()
                .any(|w| w.warning_type == "test_dismiss_low_ram"),
            "suppressing the event must not hide the condition from diagnostics"
        );

        clear_system_warning(&handle, "test_dismiss_low_ram");
        assert!(
            !is_dismissed(&low_ram),
            "a resolved-then-recurring condition is a new event the user has not seen"
        );

        clear_system_warning(&handle, "test_dismiss_crit");
    }
}
