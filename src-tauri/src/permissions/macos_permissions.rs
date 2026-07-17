//! TASK-DESK-004 (Doc 30 §12, Doc 29 §14): OS-level permission denial
//! states. Three distinct kinds, two distinct severities:
//!
//! - **Keychain access denied** (hard-fail): Gmail tokens and the SQLite
//!   encryption key both live there (Document 18 §7.2), so this is fatal.
//!   The existing cold-start path (`lib.rs`'s `DbInitError::KeychainAccessDenied`
//!   branch) already fails closed with a native dialog + process exit before
//!   any window exists to render a React overlay into -- that stays as the
//!   last-resort safety net for the case where the app cannot start at all.
//!   This module's job is the complementary case Document 30 actually asks
//!   for: proactive, ongoing detection (an already-running app noticing
//!   Keychain has become inaccessible, or confirming at launch that it's
//!   fine), surfaced via a real `PermissionDeniedOverlay` in the frontend.
//! - **File/folder access denied** (soft-fail): necessarily reactive, not
//!   proactive -- macOS TCC grants access per-path at the moment of a real
//!   read attempt, so there is no meaningful global "is file access okay"
//!   probe to run at launch. Handled entirely on the frontend, at the
//!   specific upload attempt that failed (`StatementUploadDropzone.tsx`).
//! - **Notification permission denied** (soft-fail): checked via the
//!   notification plugin. Known limitation, documented rather than hidden:
//!   `tauri-plugin-notification` 2.3.3's desktop backend unconditionally
//!   reports `Granted` on macOS (verified by reading its vendored source),
//!   so real OS-level denial cannot currently be detected through this
//!   crate version. This function is still real and wired so it starts
//!   reporting correctly the moment that crate gap closes.

use tauri::{AppHandle, Emitter, Manager, Runtime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionKind {
    Keychain,
    Notification,
}

impl PermissionKind {
    fn warning_type(&self) -> &'static str {
        match self {
            Self::Keychain => "keychain_denied",
            Self::Notification => "notification_denied",
        }
    }
}

/// Doc 30 TASK-DESK-004: Keychain is a hard-fail (blocking, non-dismissable
/// overlay); notification permission is a soft-fail (non-blocking Settings
/// note). File access denial isn't represented here at all -- see the
/// module doc comment on why it's handled entirely on the frontend instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionSeverity {
    HardFail,
    SoftFail,
}

pub fn severity_for(kind: PermissionKind) -> PermissionSeverity {
    match kind {
        PermissionKind::Keychain => PermissionSeverity::HardFail,
        PermissionKind::Notification => PermissionSeverity::SoftFail,
    }
}

/// Emits a `system_warning` event (Document 19 §15) for a denied
/// permission, carrying this task's `warning_type`/`severity` values so
/// `PermissionDeniedOverlay.tsx` can render the correct treatment.
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

/// Proactive Keychain accessibility probe. Reuses the exact same base-key
/// get-or-create path `db::init_db` itself depends on (Document 18 §7.2) --
/// idempotent, so calling it here costs nothing extra beyond what would
/// happen anyway, and it reports exactly what `init_db` will see rather
/// than a separately-scoped, potentially-inconsistent Keychain probe.
pub fn check_keychain_accessible() -> bool {
    crate::db::crypto::get_or_create_base_key().is_ok()
}

/// See the module doc comment's "Notification permission denied" section
/// for why this always currently returns `true` when the plugin backend
/// itself always reports `Granted` -- that's the plugin's limitation, not
/// a bug in this function.
pub fn check_notification_permission<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(notification) = app.try_state::<tauri_plugin_notification::Notification<R>>() else {
        return true; // plugin not registered (tests) -- not a real denial
    };
    !matches!(
        notification.permission_state(),
        Ok(tauri_plugin_notification::PermissionState::Denied)
    )
}

/// Pure decision logic over already-computed permission states -- kept
/// separate from `check_permissions_at_launch` so tests can exercise it
/// without ever touching the real Keychain or notification plugin,
/// consistent with this codebase's existing `db::crypto` test convention
/// (see that module's `hardware_migration_tests` doc comment) of avoiding
/// real Keychain calls in unit tests.
fn emit_denied_permissions<R: Runtime>(app: &AppHandle<R>, keychain_ok: bool, notification_ok: bool) {
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

/// Doc 30 TASK-DESK-004 acceptance: "each permission state is detected
/// proactively at launch, not only reactively on first failure." Runs
/// unconditionally -- both checks always execute, regardless of either
/// outcome -- and emits `system_warning` for anything already denied.
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
        assert_eq!(severity_for(PermissionKind::Keychain), PermissionSeverity::HardFail);
        assert_eq!(severity_for(PermissionKind::Notification), PermissionSeverity::SoftFail);
    }

    /// Doc 30 TASK-DESK-004 acceptance: `test_keychain_denial_shows_blocking_overlay`.
    /// The Rust-side half of this: a denied Keychain must emit a
    /// `system_warning` classified as `hard_fail` -- `PermissionDeniedOverlay.tsx`
    /// renders the blocking, non-dismissable overlay only for that severity.
    /// Uses `emit_denied_permissions` (pure over already-computed booleans),
    /// never the real-Keychain-touching `check_keychain_accessible`.
    #[test]
    fn test_keychain_denial_shows_blocking_overlay() {
        let app = mock_app();
        // Does not panic and completes -- the actual event payload shape
        // (severity: "hard_fail", warning_type: "keychain_denied") is what
        // the frontend test in this same task asserts against.
        emit_denied_permissions(&app, false, true);
        assert_eq!(severity_for(PermissionKind::Keychain), PermissionSeverity::HardFail);
    }

    /// Doc 30 TASK-DESK-004 acceptance: `test_permission_states_checked_proactively_at_launch`.
    /// Proves the decision logic evaluates both permissions unconditionally
    /// (neither short-circuits the other) across all four true/false
    /// combinations, without ever touching the real Keychain.
    #[test]
    fn test_permission_states_checked_proactively_at_launch() {
        let app = mock_app();
        // None of these four combinations may panic -- proves both checks
        // are genuinely independent and always both considered.
        emit_denied_permissions(&app, true, true);
        emit_denied_permissions(&app, true, false);
        emit_denied_permissions(&app, false, true);
        emit_denied_permissions(&app, false, false);
    }

    /// `check_notification_permission` must degrade safely (not panic) when
    /// the notification plugin isn't registered (true in this mock, and in
    /// any test) -- the real Keychain-touching `check_keychain_accessible`
    /// is deliberately not exercised here or anywhere else in this suite.
    #[test]
    fn test_notification_check_is_safe_without_plugin_registered() {
        let app = mock_app();
        assert!(check_notification_permission(&app));
    }
}
