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
    SystemWarning,
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
            Self::SystemWarning => "system_warning",
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
