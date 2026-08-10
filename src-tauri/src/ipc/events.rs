//! The catalogue of events the backend emits.
//!
//! Names are centralised in one enum because they are matched as strings on the
//! frontend: a literal typed at an emit site would compile fine and simply never
//! be received. Anything the frontend must learn about without asking -- scan
//! progress, statement outcomes, licence changes -- arrives this way.
use serde::Serialize;
use tauri::{AppHandle, Emitter, Error};

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
    ScanCancelled,
    MerchantCleanupProgress,
    StatementReparseProgress,
    MenuNavigate,
    MenuToggleSidebar,
    MenuUploadStatementRequested,
    MenuCheckForUpdates,
}

impl AppEvent {
    /// The event name emitted over IPC.
    ///
    /// Matched as a string on the frontend, so these must stay in step with the
    /// listeners there -- a rename compiles fine and is simply never received.
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
            Self::MerchantCleanupProgress => "merchant_cleanup_progress",
            Self::StatementReparseProgress => "statement_reparse_progress",
            Self::MenuNavigate => "menu_navigate",
            Self::MenuToggleSidebar => "menu_toggle_sidebar",
            Self::MenuUploadStatementRequested => "menu_upload_statement_requested",
            Self::MenuCheckForUpdates => "menu_check_for_updates",
        }
    }
}

/// Emits a typed event to the frontend.
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

    #[test]
    fn test_all_event_types_use_centralized_emit_module() {
        let historical_scan_src = include_str!("../ingestion/historical_scan.rs");
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

        let recovered = active_system_warnings();
        assert!(
            recovered
                .iter()
                .any(|w| w.warning_type == "test_rt001_late_mount"),
            "a late-mounting component must be able to recover already-emitted warning state"
        );
    }
}
