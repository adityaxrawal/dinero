//! Doc 30 TASK-API-001: IPC Request Validation Middleware.
//!
//! A shared `Validate` trait every IPC argument struct can implement, plus
//! the individual field-level checks Document 30 names: non-empty-string,
//! UUID format on every `*_id` field, date-range sanity, amount bounds, and
//! pagination bounds. Failures return `AppError::Validation` with a
//! specific, actionable message — never a generic "invalid input."
//!
//! **Retrofit status:** wired into the IPC argument structs for the
//! command surfaces built or touched in this same Area 8 pass
//! (transactions/instruments/statements/reconciliation list+search
//! commands, TASK-API-002 through TASK-API-009) — retrofitting the full
//! ~66-command surface (including commands outside Area 8's own file list,
//! e.g. `licensing/commands.rs`) is a wider change than any single Area 8
//! task's own file scope covers; flagged here rather than silently
//! expanded.

use crate::error::AppError;
use chrono::NaiveDate;

/// Implemented by every IPC argument struct that carries user-suppliable
/// fields needing validation before the command body runs.
pub trait Validate {
    fn validate(&self) -> Result<(), AppError>;
}

/// Non-empty-string check, Document 30's first named rule.
pub fn validate_non_empty(field_name: &str, value: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(AppError::Validation(format!(
            "{field_name} must not be empty"
        )));
    }
    Ok(())
}

/// UUID format check for every `*_id` field. Dinero's IDs are UUIDv4
/// (Document 18, throughout) except `local_profile`/`license_state`
/// (always `id = 1`, never user-suppliable) — this validator is for the
/// UUID-keyed entities (transactions, instruments, statements, clusters,
/// etc.).
pub fn validate_uuid(field_name: &str, value: &str) -> Result<(), AppError> {
    uuid::Uuid::parse_str(value).map_err(|_| {
        AppError::Validation(format!("{field_name} must be a valid UUID, got '{value}'"))
    })?;
    Ok(())
}

/// Format check for a connected-account id: `gmail_<uuidv5>` (see
/// `oauth.rs`'s `account_id` construction — a `gmail_`-prefixed deterministic
/// hash of the account's email, not a bare UUID like other `*_id` fields).
pub fn validate_account_id(field_name: &str, value: &str) -> Result<(), AppError> {
    let uuid_part = value.strip_prefix("gmail_").ok_or_else(|| {
        AppError::Validation(format!(
            "{field_name} must be a valid account id, got '{value}'"
        ))
    })?;
    uuid::Uuid::parse_str(uuid_part).map_err(|_| {
        AppError::Validation(format!(
            "{field_name} must be a valid account id, got '{value}'"
        ))
    })?;
    Ok(())
}

/// Date-range sanity: `start_date <= end_date`, both `YYYY-MM-DD`.
pub fn validate_date_range(start_date: &str, end_date: &str) -> Result<(), AppError> {
    let start = NaiveDate::parse_from_str(start_date, "%Y-%m-%d").map_err(|_| {
        AppError::Validation(format!(
            "start_date '{start_date}' is not a valid YYYY-MM-DD date"
        ))
    })?;
    let end = NaiveDate::parse_from_str(end_date, "%Y-%m-%d").map_err(|_| {
        AppError::Validation(format!(
            "end_date '{end_date}' is not a valid YYYY-MM-DD date"
        ))
    })?;
    if start > end {
        return Err(AppError::Validation(format!(
            "start_date ({start_date}) must not be after end_date ({end_date})"
        )));
    }
    Ok(())
}

/// Doc 30: "reject invalid negative `amount_minor`, reject unreasonably
/// large amounts as likely input errors." `allow_negative` distinguishes
/// fields where a negative value is meaningful (none currently — all
/// monetary fields in this schema are unsigned magnitudes with `direction`
/// carrying sign, Document 18 §4.3) from ones where it never is; kept as a
/// parameter rather than hardcoded so a future signed field doesn't need a
/// second function.
pub const MAX_REASONABLE_AMOUNT_MINOR: i64 = 100_000_000_00; // Doc 30: "unreasonably large... likely input errors" -- INR 1,00,00,000 (1 crore), an engineering default, not a sourced figure.

pub fn validate_amount_minor(
    field_name: &str,
    amount_minor: i64,
    allow_negative: bool,
) -> Result<(), AppError> {
    if !allow_negative && amount_minor < 0 {
        return Err(AppError::Validation(format!(
            "{field_name} must not be negative, got {amount_minor}"
        )));
    }
    if amount_minor.abs() > MAX_REASONABLE_AMOUNT_MINOR {
        return Err(AppError::Validation(format!(
            "{field_name} ({amount_minor}) exceeds the maximum reasonable amount ({MAX_REASONABLE_AMOUNT_MINOR}) -- likely an input error"
        )));
    }
    Ok(())
}

/// Pagination bounds: `page >= 1`, `per_page` in `[1, 200]` (Document 19
/// §3.3's `page_size` cap).
pub const MAX_PAGE_SIZE: u32 = 200;

pub fn validate_pagination(page: u32, per_page: u32) -> Result<(), AppError> {
    if page < 1 {
        return Err(AppError::Validation(format!(
            "page must be >= 1, got {page}"
        )));
    }
    if per_page < 1 || per_page > MAX_PAGE_SIZE {
        return Err(AppError::Validation(format!(
            "per_page must be between 1 and {MAX_PAGE_SIZE}, got {per_page}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Doc 30 TASK-API-001 acceptance test.
    #[test]
    fn test_uuid_validation_rejects_malformed() {
        assert!(validate_uuid("transaction_id", "not-a-uuid").is_err());
        assert!(validate_uuid("transaction_id", "").is_err());
        assert!(validate_uuid("transaction_id", &uuid::Uuid::new_v4().to_string()).is_ok());
    }

    #[test]
    fn test_account_id_validation_requires_gmail_prefix() {
        let uuid = uuid::Uuid::new_v4().to_string();
        assert!(validate_account_id("account_id", &format!("gmail_{uuid}")).is_ok());
        assert!(validate_account_id("account_id", &uuid).is_err());
        assert!(validate_account_id("account_id", "gmail_not-a-uuid").is_err());
        assert!(validate_account_id("account_id", "").is_err());
    }

    /// Doc 30 TASK-API-001 acceptance test.
    #[test]
    fn test_date_range_validation_rejects_inverted() {
        assert!(validate_date_range("2026-06-01", "2026-01-01").is_err());
        assert!(validate_date_range("2026-01-01", "2026-06-01").is_ok());
        assert!(
            validate_date_range("2026-01-01", "2026-01-01").is_ok(),
            "equal dates are a valid single-day range"
        );
        assert!(validate_date_range("not-a-date", "2026-01-01").is_err());
    }

    /// Doc 30 TASK-API-001 acceptance test.
    #[test]
    fn test_pagination_bounds_enforced() {
        assert!(validate_pagination(0, 50).is_err(), "page must be >= 1");
        assert!(validate_pagination(1, 0).is_err(), "per_page must be >= 1");
        assert!(
            validate_pagination(1, 201).is_err(),
            "per_page must be <= 200"
        );
        assert!(validate_pagination(1, 200).is_ok());
        assert!(validate_pagination(1, 50).is_ok());
    }

    /// Doc 30 TASK-API-001 acceptance test.
    #[test]
    fn test_negative_amount_rejected_where_invalid() {
        assert!(validate_amount_minor("amount_minor", -100, false).is_err());
        assert!(validate_amount_minor("amount_minor", 100, false).is_ok());
        assert!(
            validate_amount_minor("amount_minor", -100, true).is_ok(),
            "allow_negative fields must accept a negative value"
        );
        assert!(
            validate_amount_minor("amount_minor", MAX_REASONABLE_AMOUNT_MINOR + 1, false).is_err(),
            "unreasonably large amounts must be rejected"
        );
    }

    #[test]
    fn test_non_empty_rejects_blank_and_whitespace_only() {
        assert!(validate_non_empty("merchant_name", "").is_err());
        assert!(validate_non_empty("merchant_name", "   ").is_err());
        assert!(validate_non_empty("merchant_name", "Uber").is_ok());
    }
}
