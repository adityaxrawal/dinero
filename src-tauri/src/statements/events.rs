//! Emits statement-processing events to the frontend.
//!
//! Statement parsing is long-running and can pause for user input, so progress,
//! completion, failure and the password/instrument prompts are all surfaced as
//! events rather than left to a single blocking response.
pub const PASSWORD_REQUIRED: &str = "statement_password_required";

pub const PARSED: &str = "statement_parsed";

pub const PARSE_FAILED: &str = "statement_parse_failed";

pub const DUPLICATE_REJECTED: &str = "statement_duplicate_rejected";

pub const INSTRUMENT_CONFIRMATION_REQUIRED: &str = "statement_instrument_confirmation_required";

pub const UPCOMING_BILL_SET: &str = "statement_upcoming_bill_set";

pub const PARTIAL_ROWS: &str = "statement_partial_rows";

pub const TRANSACTION_CREATED: &str = "transaction_created";

pub const BATCH_PROGRESS: &str = "statement_batch_progress";

pub const RECONCILIATION_CLUSTER: &str = "reconciliation_cluster";

pub const STAGED: &str = "statement_staged";

pub const PROCESSING_PROGRESS: &str = "statement_processing_progress";

/// Emits a statement event to the frontend.
///
/// Statement processing is long-running and can pause for user input, so progress
/// and prompts are pushed as events rather than returned from one blocking call.
pub fn emit(event: &str, payload: serde_json::Value) {
    tracing::info!("Tauri event → '{}': {}", event, payload);
}
