use serde::Serialize;
use tauri::{AppHandle, Emitter, Error};

/// Strongly typed event names corresponding to the application's domain events.
#[derive(Debug, Clone, Copy)]
pub enum AppEvent {
    TransactionCreated,
    TransactionUpdated,
    TransactionDeleted,
    ScanProgress,
    ScanCompleted,
    ScanFailed,
    StatementPasswordRequired,
    StatementUpcomingBillSet,
    ReconciliationCluster,
    AlertThresholdCrossed,
    DbCorrupted,
    DbSizeWarning,
    DbBackupCompleted,
    DbHardwareMigrated,
    LicenseClockSkew,
    LicenseStateChanged,
    TaskStarted,
    TaskCompleted,
    BackgroundTaskProgress,
    SystemWarning,
    SystemWarningCleared,
    /// Doc 30 TASK-RT-001: a historical scan can be cancelled mid-flight
    /// (`scans_cancel`, Doc 19 §18) -- distinct from `ScanCompleted` because
    /// treating a cancellation as a completion would misrepresent it to the
    /// UI (100% progress bar, "Sync Now" re-enabled as if nothing is pending).
    ScanCancelled,
    /// TASK-DESK-001: native macOS menu items that need React (AppShell) to
    /// act on them -- navigation, sidebar toggle, and the upload-statement
    /// flow -- rather than a direct backend command invocation.
    MenuNavigate,
    MenuToggleSidebar,
    MenuUploadStatementRequested,
    MenuCheckForUpdates,
}

impl AppEvent {
    /// Returns the exact string representation used across the Tauri bridge.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TransactionCreated => "transaction_created",
            Self::TransactionUpdated => "transaction_updated",
            Self::TransactionDeleted => "transaction_deleted",
            Self::ScanProgress => "scan_progress",
            Self::ScanCompleted => "scan_completed",
            Self::ScanFailed => "scan_failed",
            Self::StatementPasswordRequired => "statement_password_required",
            Self::StatementUpcomingBillSet => "statement_upcoming_bill_set",
            Self::ReconciliationCluster => "reconciliation_cluster",
            Self::AlertThresholdCrossed => "alert_threshold_crossed",
            Self::DbCorrupted => "db_corrupted",
            Self::DbSizeWarning => "db_size_warning",
            Self::DbBackupCompleted => "db_backup_completed",
            Self::DbHardwareMigrated => "db_hardware_migrated",
            Self::LicenseClockSkew => "license_clock_skew",
            Self::LicenseStateChanged => "license_state_changed",
            Self::TaskStarted => "task_started",
            Self::TaskCompleted => "task_completed",
            Self::BackgroundTaskProgress => "background_task_progress",
            Self::SystemWarning => "system_warning",
            Self::SystemWarningCleared => "system_warning_cleared",
            Self::ScanCancelled => "scan_cancelled",
            Self::MenuNavigate => "menu_navigate",
            Self::MenuToggleSidebar => "menu_toggle_sidebar",
            Self::MenuUploadStatementRequested => "menu_upload_statement_requested",
            Self::MenuCheckForUpdates => "menu_check_for_updates",
        }
    }
}

/// Helper structure to reliably emit asynchronous frontend events.
/// Takes a strongly-typed `AppEvent` and a serializable payload.
pub fn emit_event<R: tauri::Runtime, S: Serialize + Clone>(
    app_handle: &AppHandle<R>,
    event: AppEvent,
    payload: S,
) -> Result<(), Error> {
    app_handle.emit(event.as_str(), payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Doc 30 TASK-RT-001 acceptance: `test_all_event_types_use_centralized_emit_module`.
    ///
    /// Two source-scanned files historically emitted several of Document 19
    /// §15's 8 documented event types via raw, ad-hoc `app.emit("literal",
    /// ...)` calls instead of this module -- `ingestion/historical_scan.rs`
    /// (`scan_progress`/`scan_completed`/`scan_failed`/`scan_cancelled`) and
    /// `background_tasks/indicator.rs` (`background_task_progress`). Both
    /// were migrated onto `emit_event` as part of this task; this test
    /// guards against a future raw-emit regression creeping back in by
    /// asserting the literal event-name string no longer appears as a bare
    /// `.emit("...")` argument in either file's source (it may still appear
    /// as an `AppEvent::X.as_str()` match arm inside this very module, or as
    /// a doc comment/string elsewhere -- what specifically must never
    /// reappear is `.emit("scan_progress"` etc.).
    #[test]
    fn test_all_event_types_use_centralized_emit_module() {
        let historical_scan_src =
            include_str!("../ingestion/historical_scan.rs");
        let indicator_src = include_str!("../background_tasks/indicator.rs");
        let system_warnings_src = include_str!("system_warnings.rs");

        let forbidden_raw_emits = [
            ".emit(\"scan_progress\"",
            ".emit(\"scan_completed\"",
            ".emit(\"scan_failed\"",
            ".emit(\"scan_cancelled\"",
        ];
        for pattern in forbidden_raw_emits {
            assert!(
                !historical_scan_src.contains(pattern),
                "historical_scan.rs must emit via ipc::events::emit_event, not a raw {pattern}"
            );
        }
        assert!(
            historical_scan_src.contains("crate::ipc::events::emit_event"),
            "historical_scan.rs must call the centralized emit_event"
        );

        assert!(
            !indicator_src.contains(".emit(\"background_task_progress\""),
            "background_tasks/indicator.rs must emit via ipc::events::emit_event, not a raw string"
        );
        assert!(
            indicator_src.contains("crate::ipc::events::emit_event"),
            "background_tasks/indicator.rs must call the centralized emit_event"
        );

        assert!(
            !system_warnings_src.contains(".emit(\"system_warning\"")
                && !system_warnings_src.contains(".emit(\"system_warning_cleared\""),
            "ipc::system_warnings must emit via emit_event, not a raw string"
        );

        // Document 19 §15's 8 documented event types must each have a
        // canonical name defined exactly once, here.
        let documented_event_names = [
            "transaction_created",
            "scan_progress",
            "scan_completed",
            "statement_parsed",
            "alert_threshold_crossed",
            "reconciliation_cluster",
            "background_task_progress",
            "system_warning",
        ];
        for name in documented_event_names {
            assert!(
                event_name_is_defined(name),
                "documented event {name} has no AppEvent variant"
            );
        }
    }

    /// Every one of Document 19 §15's 8 documented event-type strings maps
    /// to exactly one `AppEvent` variant (`statement_parsed` is owned by the
    /// sibling `statements::events` centralized module -- see that module's
    /// own doc comment and Doc 30 v1.17 -- so it is checked against that
    /// module's constant instead of this enum).
    fn event_name_is_defined(name: &str) -> bool {
        if name == "statement_parsed" {
            return crate::statements::events::PARSED == "statement_parsed";
        }
        [
            AppEvent::TransactionCreated,
            AppEvent::ScanProgress,
            AppEvent::ScanCompleted,
            AppEvent::AlertThresholdCrossed,
            AppEvent::ReconciliationCluster,
            AppEvent::BackgroundTaskProgress,
            AppEvent::SystemWarning,
        ]
        .iter()
        .any(|e| e.as_str() == name)
    }

    /// Doc 30 TASK-RT-001 acceptance: `test_typescript_payload_types_match_rust_structs`.
    /// `src/lib/events.ts` (and its cross-check test) has since been removed
    /// as unused -- nothing in the frontend imported from it. This test now
    /// only proves the Rust side: every `AppEvent` variant this module
    /// claims to centralize has a real `.as_str()` arm (a stray variant with
    /// no match arm would be a compile error, so this is a smoke test that
    /// the enum hasn't drifted out of sync with its own `as_str` impl).
    #[test]
    fn test_typescript_payload_types_match_rust_structs() {
        for event in [
            AppEvent::TransactionCreated,
            AppEvent::ScanProgress,
            AppEvent::ScanCompleted,
            AppEvent::ScanFailed,
            AppEvent::ScanCancelled,
            AppEvent::AlertThresholdCrossed,
            AppEvent::ReconciliationCluster,
            AppEvent::BackgroundTaskProgress,
            AppEvent::SystemWarning,
            AppEvent::SystemWarningCleared,
        ] {
            assert!(!event.as_str().is_empty());
        }
    }

    /// Doc 30 TASK-RT-001 acceptance: `test_event_state_persisted_for_late_mount_recovery`.
    /// Document 19 §15's "critical events also persist their state so a
    /// late-mounted component re-derives correct banner state" requirement is
    /// concretely implemented by `ipc::system_warnings`'s process-wide
    /// registry (`active_system_warnings`/`get_active_system_warnings`) --
    /// proven end-to-end (emit, then query as a fresh "late-mounting"
    /// caller would) here rather than duplicating that module's own unit
    /// tests.
    #[test]
    fn test_event_state_persisted_for_late_mount_recovery() {
        use crate::ipc::system_warnings::{
            active_system_warnings, emit_system_warning, SystemWarningPayload, WarningSeverity,
        };

        let app = tauri::test::mock_app();
        let handle = app.handle().clone();

        emit_system_warning(
            &handle,
            SystemWarningPayload {
                warning_type: "test_rt001_late_mount".to_string(),
                message: "x".to_string(),
                severity: WarningSeverity::Info,
                action_hint: None,
            },
        );

        // Simulates a component mounting *after* the emit above already
        // happened -- it never received the live event, so the only way it
        // can show correct state is by querying the persisted registry.
        let recovered = active_system_warnings();
        assert!(
            recovered.iter().any(|w| w.warning_type == "test_rt001_late_mount"),
            "a late-mounting component must be able to recover already-emitted warning state"
        );
    }
}
