//! The error type crossing the IPC boundary to the frontend.
//!
//! Every Tauri command returns `AppError` on failure, and its custom `Serialize`
//! implementation is what produces the `{ code, message }` shape the frontend's
//! error-mapping layer branches on. Because the variant name becomes the code,
//! renaming one silently breaks the frontend's handling of that error -- the two
//! sides are coupled through these identifiers.
//!
//! Variants carry a `String` rather than the underlying error type on purpose:
//! it forces each failure to be converted at the point it is understood, and
//! stops internal detail (SQL text, file paths) from reaching the UI unexamined.

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Db(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Unknown error: {0}")]
    Unknown(String),

    #[error("File access denied: {0}")]
    FileAccessDenied(String),

    #[error("License locked: {0}")]
    LicenseLocked(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("I/O error: {0}")]
    Io(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Rate limited: {0}")]
    RateLimited(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Gmail not connected: {0}")]
    GmailNotConnected(String),

    #[error("Gmail API error: {0}")]
    GmailApiError(String),

    #[error("Scan already running: {0}")]
    ScanAlreadyRunning(String),

    #[error("Scan not found: {0}")]
    ScanNotFound(String),

    #[error("File too large: {0}")]
    FileTooLarge(String),

    #[error("PDF page limit exceeded: {0}")]
    PdfPageLimitExceeded(String),

    #[error("Invalid file type: {0}")]
    InvalidFileType(String),

    #[error("Password incorrect: {0}")]
    PasswordIncorrect(String),

    #[error("Statement not awaiting password: {0}")]
    StatementNotAwaitingPassword(String),

    #[error("Statement not awaiting instrument confirmation: {0}")]
    StatementNotAwaitingInstrumentConfirmation(String),

    #[error("Cluster not found: {0}")]
    ClusterNotFound(String),

    #[error("Invalid resolution action: {0}")]
    InvalidResolutionAction(String),

    #[error("License invalid: {0}")]
    LicenseInvalid(String),

    #[error("Device already bound: {0}")]
    DeviceAlreadyBound(String),

    #[error("Payment verification failed: {0}")]
    PaymentVerificationFailed(String),

    #[error("Keychain access denied: {0}")]
    KeychainAccessDenied(String),
}

impl AppError {
    /// The stable code the frontend branches on.
    ///
    /// Derived from the variant name, so renaming a variant silently changes the
    /// contract the frontend matches against.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Db(_) => "INTERNAL_ERROR",
            Self::Network(_) => "NETWORK_ERROR",
            Self::Auth(_) => "UNAUTHORIZED",
            Self::Unknown(_) => "INTERNAL_ERROR",
            Self::FileAccessDenied(_) => "INTERNAL_ERROR",
            Self::LicenseLocked(_) => "LICENSE_LOCKED",
            Self::Parse(_) => "VALIDATION_ERROR",
            Self::Io(_) => "INTERNAL_ERROR",
            Self::Internal(_) => "INTERNAL_ERROR",
            Self::Validation(_) => "VALIDATION_ERROR",
            Self::Forbidden(_) => "FORBIDDEN",
            Self::NotFound(_) => "NOT_FOUND",
            Self::RateLimited(_) => "RATE_LIMITED",
            Self::Conflict(_) => "CONFLICT",
            Self::GmailNotConnected(_) => "GMAIL_NOT_CONNECTED",
            Self::GmailApiError(_) => "GMAIL_API_ERROR",
            Self::ScanAlreadyRunning(_) => "SCAN_ALREADY_RUNNING",
            Self::ScanNotFound(_) => "SCAN_NOT_FOUND",
            Self::FileTooLarge(_) => "FILE_TOO_LARGE",
            Self::PdfPageLimitExceeded(_) => "PDF_PAGE_LIMIT_EXCEEDED",
            Self::InvalidFileType(_) => "INVALID_FILE_TYPE",
            Self::PasswordIncorrect(_) => "PASSWORD_INCORRECT",
            Self::StatementNotAwaitingPassword(_) => "STATEMENT_NOT_AWAITING_PASSWORD",
            Self::StatementNotAwaitingInstrumentConfirmation(_) => {
                "STATEMENT_NOT_AWAITING_INSTRUMENT_CONFIRMATION"
            }
            Self::ClusterNotFound(_) => "CLUSTER_NOT_FOUND",
            Self::InvalidResolutionAction(_) => "INVALID_RESOLUTION_ACTION",
            Self::LicenseInvalid(_) => "LICENSE_INVALID",
            Self::DeviceAlreadyBound(_) => "DEVICE_ALREADY_BOUND",
            Self::PaymentVerificationFailed(_) => "PAYMENT_VERIFICATION_FAILED",
            Self::KeychainAccessDenied(_) => "KEYCHAIN_ACCESS_DENIED",
        }
    }
}

// Hand-written rather than derived: the frontend contract is a flat
// { code, message } object, where the code is the variant name. A derived
// implementation would emit Rust's externally-tagged enum shape instead.
impl Serialize for AppError {
    /// Serialises to the flat { code, message } shape the frontend expects.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AppError", 2)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

/// Translate a unique-constraint violation into a domain-level conflict.
///
/// Without this, a duplicate insert surfaces as a raw SQL error string, which
/// tells the user nothing actionable about what already exists.
pub fn map_insert_conflict(e: anyhow::Error, conflict_message: &str) -> AppError {
    let is_constraint_violation = e
        .downcast_ref::<rusqlite::Error>()
        .map(|re| {
            matches!(
                re,
                rusqlite::Error::SqliteFailure(err, _)
                    if err.code == rusqlite::ErrorCode::ConstraintViolation
            )
        })
        .unwrap_or(false);
    if is_constraint_violation {
        AppError::Conflict(conflict_message.to_string())
    } else {
        AppError::Db(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn serialized(err: &AppError) -> serde_json::Value {
        serde_json::to_value(err).unwrap()
    }

    #[test]
    fn db_error_serializes_with_code_and_message() {
        let err = AppError::Db("Connection failed".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "INTERNAL_ERROR", "message": "Database error: Connection failed" })
        );
    }

    #[test]
    fn network_error_serializes_with_code_and_message() {
        let err = AppError::Network("Timeout".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "NETWORK_ERROR", "message": "Network error: Timeout" })
        );
    }

    #[test]
    fn auth_error_serializes_with_code_and_message() {
        let err = AppError::Auth("Invalid token".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "UNAUTHORIZED", "message": "Authentication error: Invalid token" })
        );
    }

    #[test]
    fn unknown_error_serializes_with_code_and_message() {
        let err = AppError::Unknown("Something went wrong".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "INTERNAL_ERROR", "message": "Unknown error: Something went wrong" })
        );
    }

    #[test]
    fn file_access_denied_serializes_with_code_and_message() {
        let err = AppError::FileAccessDenied("no permission".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "INTERNAL_ERROR", "message": "File access denied: no permission" })
        );
    }

    #[test]
    fn license_locked_serializes_with_code_and_message() {
        let err = AppError::LicenseLocked("grace expired".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "LICENSE_LOCKED", "message": "License locked: grace expired" })
        );
    }

    #[test]
    fn parse_error_serializes_with_code_and_message() {
        let err = AppError::Parse("bad date format".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "VALIDATION_ERROR", "message": "Parse error: bad date format" })
        );
    }

    #[test]
    fn io_error_serializes_with_code_and_message() {
        let err = AppError::Io("disk full".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "INTERNAL_ERROR", "message": "I/O error: disk full" })
        );
    }

    #[test]
    fn internal_error_serializes_with_code_and_message() {
        let err = AppError::Internal("unexpected panic".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "INTERNAL_ERROR", "message": "Internal error: unexpected panic" })
        );
    }

    #[test]
    fn validation_error_serializes_with_code_and_message() {
        let err = AppError::Validation("field required".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "VALIDATION_ERROR", "message": "Validation error: field required" })
        );
    }

    #[test]
    fn forbidden_error_serializes_with_code_and_message() {
        let err = AppError::Forbidden("not allowed".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "FORBIDDEN", "message": "Forbidden: not allowed" })
        );
    }

    #[test]
    fn not_found_error_serializes_with_code_and_message() {
        let err = AppError::NotFound("missing row".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "NOT_FOUND", "message": "Not found: missing row" })
        );
    }

    #[test]
    fn rate_limited_error_serializes_with_code_and_message() {
        let err = AppError::RateLimited("too many requests".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "RATE_LIMITED", "message": "Rate limited: too many requests" })
        );
    }

    #[test]
    fn conflict_error_serializes_with_code_and_message() {
        let err = AppError::Conflict("duplicate instrument".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "CONFLICT", "message": "Conflict: duplicate instrument" })
        );
    }

    #[test]
    fn gmail_not_connected_error_serializes_with_code_and_message() {
        let err = AppError::GmailNotConnected("no account linked".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "GMAIL_NOT_CONNECTED", "message": "Gmail not connected: no account linked" })
        );
    }

    #[test]
    fn gmail_api_error_serializes_with_code_and_message() {
        let err = AppError::GmailApiError("quota exceeded".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "GMAIL_API_ERROR", "message": "Gmail API error: quota exceeded" })
        );
    }

    #[test]
    fn scan_already_running_error_serializes_with_code_and_message() {
        let err = AppError::ScanAlreadyRunning("account already scanning".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "SCAN_ALREADY_RUNNING", "message": "Scan already running: account already scanning" })
        );
    }

    #[test]
    fn scan_not_found_error_serializes_with_code_and_message() {
        let err = AppError::ScanNotFound("no checkpoint".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "SCAN_NOT_FOUND", "message": "Scan not found: no checkpoint" })
        );
    }

    #[test]
    fn file_too_large_error_serializes_with_code_and_message() {
        let err = AppError::FileTooLarge("exceeds 25MB".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "FILE_TOO_LARGE", "message": "File too large: exceeds 25MB" })
        );
    }

    #[test]
    fn pdf_page_limit_exceeded_error_serializes_with_code_and_message() {
        let err = AppError::PdfPageLimitExceeded("exceeds 200 pages".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "PDF_PAGE_LIMIT_EXCEEDED", "message": "PDF page limit exceeded: exceeds 200 pages" })
        );
    }

    #[test]
    fn invalid_file_type_error_serializes_with_code_and_message() {
        let err = AppError::InvalidFileType("not a PDF".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "INVALID_FILE_TYPE", "message": "Invalid file type: not a PDF" })
        );
    }

    #[test]
    fn password_incorrect_error_serializes_with_code_and_message() {
        let err = AppError::PasswordIncorrect("wrong password".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "PASSWORD_INCORRECT", "message": "Password incorrect: wrong password" })
        );
    }

    #[test]
    fn statement_not_awaiting_password_error_serializes_with_code_and_message() {
        let err = AppError::StatementNotAwaitingPassword("wrong state".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "STATEMENT_NOT_AWAITING_PASSWORD", "message": "Statement not awaiting password: wrong state" })
        );
    }

    #[test]
    fn statement_not_awaiting_instrument_confirmation_error_serializes_with_code_and_message() {
        let err = AppError::StatementNotAwaitingInstrumentConfirmation("wrong state".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "STATEMENT_NOT_AWAITING_INSTRUMENT_CONFIRMATION", "message": "Statement not awaiting instrument confirmation: wrong state" })
        );
    }

    #[test]
    fn cluster_not_found_error_serializes_with_code_and_message() {
        let err = AppError::ClusterNotFound("no such cluster".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "CLUSTER_NOT_FOUND", "message": "Cluster not found: no such cluster" })
        );
    }

    #[test]
    fn invalid_resolution_action_error_serializes_with_code_and_message() {
        let err = AppError::InvalidResolutionAction("unknown action".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "INVALID_RESOLUTION_ACTION", "message": "Invalid resolution action: unknown action" })
        );
    }

    #[test]
    fn license_invalid_error_serializes_with_code_and_message() {
        let err = AppError::LicenseInvalid("signature mismatch".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "LICENSE_INVALID", "message": "License invalid: signature mismatch" })
        );
    }

    #[test]
    fn device_already_bound_error_serializes_with_code_and_message() {
        let err = AppError::DeviceAlreadyBound("seat taken".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "DEVICE_ALREADY_BOUND", "message": "Device already bound: seat taken" })
        );
    }

    #[test]
    fn payment_verification_failed_error_serializes_with_code_and_message() {
        let err = AppError::PaymentVerificationFailed("receipt invalid".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "PAYMENT_VERIFICATION_FAILED", "message": "Payment verification failed: receipt invalid" })
        );
    }

    #[test]
    fn map_insert_conflict_maps_real_unique_violation_to_conflict() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (name TEXT NOT NULL UNIQUE);")
            .unwrap();
        conn.execute("INSERT INTO t (name) VALUES ('a')", [])
            .unwrap();
        let insert_err = conn
            .execute("INSERT INTO t (name) VALUES ('a')", [])
            .unwrap_err();
        let mapped = map_insert_conflict(anyhow::Error::new(insert_err), "duplicate name");
        assert_eq!(
            serialized(&mapped),
            json!({ "code": "CONFLICT", "message": "Conflict: duplicate name" })
        );
    }

    #[test]
    fn map_insert_conflict_leaves_other_errors_as_db() {
        let other_err = anyhow::anyhow!("connection lost");
        let mapped = map_insert_conflict(other_err, "duplicate name");
        assert_eq!(
            serialized(&mapped),
            json!({ "code": "INTERNAL_ERROR", "message": "Database error: connection lost" })
        );
    }

    #[test]
    fn keychain_access_denied_error_serializes_with_code_and_message() {
        let err = AppError::KeychainAccessDenied("user denied prompt".to_string());
        assert_eq!(
            serialized(&err),
            json!({ "code": "KEYCHAIN_ACCESS_DENIED", "message": "Keychain access denied: user denied prompt" })
        );
    }
}
