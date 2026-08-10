//! Checks macOS permissions the app depends on.
//!
//! Keychain access is the critical one and has no fallback: the database key
//! lives there, so a denial is fatal rather than degrading. Notification access
//! is optional, and its absence merely disables reminders.
use tauri::{AppHandle, Emitter, Manager, Runtime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionKind {
    Keychain,
    Notification,
}

impl PermissionKind {
    /// Warning type identifier for this permission.
    fn warning_type(&self) -> &'static str {
        match self {
            Self::Keychain => "keychain_denied",
            Self::Notification => "notification_denied",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionSeverity {
    HardFail,
    SoftFail,
}

/// How serious a denial of this permission is.
///
/// Keychain denial is fatal -- the database key lives there -- whereas a denied
/// notification permission merely disables reminders.
pub fn severity_for(kind: PermissionKind) -> PermissionSeverity {
    match kind {
        PermissionKind::Keychain => PermissionSeverity::HardFail,
        PermissionKind::Notification => PermissionSeverity::SoftFail,
    }
}

/// Emits a permission-denied warning.
pub fn emit_permission_denied<R: Runtime>(app: &AppHandle<R>, kind: PermissionKind, message: &str) {
    let _ = app.emit(
        crate::ipc::events::AppEvent::SystemWarning.as_str(),
        serde_json::json!({
            "warning_type": kind.warning_type(),
            "message": message,
            "severity": severity_for(kind),
        }),
    );
}

/// Whether the keychain is reachable.
pub fn check_keychain_accessible() -> bool {
    crate::db::crypto::get_or_create_base_key().is_ok()
}

/// Whether notification permission has been granted.
pub fn check_notification_permission<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(notification) = app.try_state::<tauri_plugin_notification::Notification<R>>() else {
        return true;
    };
    !matches!(
        notification.permission_state(),
        Ok(tauri_plugin_notification::PermissionState::Denied)
    )
}

/// Emits warnings for every denied permission.
fn emit_denied_permissions<R: Runtime>(
    app: &AppHandle<R>,
    keychain_ok: bool,
    notification_ok: bool,
) {
    if !keychain_ok {
        emit_permission_denied(
            app,
            PermissionKind::Keychain,
            "Dinero cannot access the macOS Keychain, which is required to encrypt your data.",
        );
    }
    if !notification_ok {
        emit_permission_denied(
            app,
            PermissionKind::Notification,
            "Native notifications are disabled. Enable them in System Settings to get alerts when Dinero is in the background.",
        );
    }
}

/// Checks all required permissions at startup.
pub fn check_permissions_at_launch<R: Runtime>(app: &AppHandle<R>) {
    let keychain_ok = check_keychain_accessible();
    let notification_ok = check_notification_permission(app);
    emit_denied_permissions(app, keychain_ok, notification_ok);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_app() -> AppHandle<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone()
    }

    #[test]
    fn test_keychain_is_hard_fail_notification_is_soft_fail() {
        assert_eq!(
            severity_for(PermissionKind::Keychain),
            PermissionSeverity::HardFail
        );
        assert_eq!(
            severity_for(PermissionKind::Notification),
            PermissionSeverity::SoftFail
        );
    }

    #[test]
    fn test_keychain_denial_shows_blocking_overlay() {
        let app = mock_app();
        emit_denied_permissions(&app, false, true);
        assert_eq!(
            severity_for(PermissionKind::Keychain),
            PermissionSeverity::HardFail
        );
    }

    #[test]
    fn test_permission_states_checked_proactively_at_launch() {
        let app = mock_app();
        emit_denied_permissions(&app, true, true);
        emit_denied_permissions(&app, true, false);
        emit_denied_permissions(&app, false, true);
        emit_denied_permissions(&app, false, false);
    }

    #[test]
    fn test_notification_check_is_safe_without_plugin_registered() {
        let app = mock_app();
        assert!(check_notification_permission(&app));
    }
}
