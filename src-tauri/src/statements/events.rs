//! Tauri event name constants for the statement pipeline (Doc 10 §21).
//! All event names match the spec exactly — do not rename without updating the frontend.

/// Emitted when all stored passwords fail — UI must present a password modal.
pub const PASSWORD_REQUIRED: &str = "statement_password_required";

/// Emitted when a PDF statement is successfully parsed
pub const PARSED: &str = "statement_parsed";

/// Emitted when a PDF statement fails to parse
pub const PARSE_FAILED: &str = "statement_parse_failed";

/// Emitted when a PDF statement is skipped due to being a duplicate
pub const DUPLICATE_REJECTED: &str = "statement_duplicate_rejected";

/// Emitted when the Statement Instrument Gate cannot resolve issuer/masked-ID automatically.
/// The UI must surface the instrument-confirmation prompt (Doc 12 §7.2a, FR-033a).
pub const INSTRUMENT_CONFIRMATION_REQUIRED: &str = "statement_instrument_confirmation_required";

/// Emitted when a bill classification indicates an upcoming bill
pub const UPCOMING_BILL_SET: &str = "statement_upcoming_bill_set";

/// Emitted when a PDF statement is parsed but some rows failed to extract
pub const PARTIAL_ROWS: &str = "statement_partial_rows";

/// Emitted when a new transaction is created from a statement row
pub const TRANSACTION_CREATED: &str = "transaction_created";

/// Doc 30 TASK-STMT-009: periodic progress for a manual-upload batch of more
/// than 10 statements — `{ parsed, total, eta_seconds }`. Not in Document
/// 19's event catalog under any name (the generic "background task progress"
/// system Document 19 alludes to is TASK-RT-004's own later scope, Area 13,
/// not reached yet) — a dedicated, purpose-specific event here, following
/// this module's own established per-domain-event convention, rather than
/// inventing that whole generic system early.
pub const BATCH_PROGRESS: &str = "statement_batch_progress";

/// Emitted when a reconciliation cluster is formed or updated
pub const RECONCILIATION_CLUSTER: &str = "reconciliation_cluster";

/// Emitted when `stage_parse_pipeline` finishes staging a `statement_drafts`
/// row (extraction done, nothing committed yet) — payload `{ draft_id, origin }`.
pub const STAGED: &str = "statement_staged";

/// Periodic progress during staging — payload
/// `{ draft_id, instrument_id, stage, percent }`. `stage` is one of
/// "parsing", "metadata", "instrument_check", "duplicate_check",
/// "extracting_rows", "staged", "failed".
pub const PROCESSING_PROGRESS: &str = "statement_processing_progress";

/// Helper to log a Tauri event emission for structured logging.
///
/// In production this is called alongside `app_handle.emit(event, payload)` from
/// the pipeline commands. The app_handle-based emission is handled at the command
/// layer (commands/mod.rs) where app_handle is available.
pub fn emit(event: &str, payload: serde_json::Value) {
    tracing::info!("Tauri event → '{}': {}", event, payload);
}
