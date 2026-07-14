use serde::Serialize;
use thiserror::Error;

/// TASK-SETUP-012. The doc-specified variant set is `Db, Network, Auth,
/// LicenseLocked, Parse, Io, Internal, Validation`; the pre-existing
/// `Unknown`/`FileAccessDenied` variants (each with dozens of call sites
/// across `commands/`, `licensing/`, `ingestion/`) and `LicenseLocked`
/// carrying a message (spec: unit variant) are left as-is here — fully
/// reconciling the enum's shape (and updating every call site) against
/// Document 19 §4's full ~25-code catalog is TASK-API-010's explicit scope
/// ("Standardized Error Response Contract Across All Commands"), not this
/// setup task's. `Parse`/`Io`/`Internal`/`Validation` are added additively.
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
}

impl AppError {
    /// Maps each variant to a Document 19 §4 Error Catalog code. Only codes
    /// that already exist in that catalog and apply generically are used
    /// here (`NETWORK_ERROR`, `UNAUTHORIZED`, `LICENSE_LOCKED`,
    /// `VALIDATION_ERROR`, `INTERNAL_ERROR`) — the catalog's many
    /// domain-specific codes (`SCAN_NOT_FOUND`, `CLUSTER_NOT_FOUND`, etc.)
    /// are assigned per-command by whichever IPC handler raises them
    /// (Area 8), not derivable from this generic error category alone.
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
        }
    }
}

/// Document 19 §3.4's structured error contract: `{ code, message, details? }`.
/// `src/lib/ipc.ts`'s `invokeCommand()` wrapper already expects exactly this
/// shape (checks for `'code' in error && 'message' in error`) — the previous
/// bare-string `Serialize` impl never matched it, so every Rust command
/// error was silently falling through to the frontend's `UNKNOWN_ERROR`
/// catch-all instead of surfacing its real code/message. `details` is
/// omitted (no variant currently carries structured detail data).
impl Serialize for AppError {
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

// Note: no explicit `impl From<AppError> for tauri::ipc::InvokeError` is
// written here, despite Document 30 TASK-SETUP-012 naming one. Tauri 2.11's
// own `impl<T: Serialize> From<T> for InvokeError` (src-tauri's tauri
// dependency, ipc/mod.rs) is a blanket impl already covering every
// `Serialize` type, AppError included — writing a second, overlapping impl
// here would be a coherence violation (E0119, conflicting implementations)
// and fail to compile. The doc's requirement is satisfied automatically by
// AppError already implementing `Serialize`.

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
}
