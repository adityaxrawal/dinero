//! Input validation at the IPC trust boundary.
//!
//! Arguments arrive from the frontend as untyped JSON, so they are checked here
//! before reaching any query. Ids are validated as UUIDs, ranges for ordering,
//! amounts for plausibility -- rejecting bad input at the edge rather than
//! letting it fail deeper where the error is far harder to attribute.
use crate::error::AppError;
use chrono::NaiveDate;

pub trait Validate {
    /// Validates this value, returning an error describing any problem.
    fn validate(&self) -> Result<(), AppError>;
}

/// Rejects an empty or whitespace-only string.
pub fn validate_non_empty(field_name: &str, value: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(AppError::Validation(format!(
            "{field_name} must not be empty"
        )));
    }
    Ok(())
}

/// Rejects a value that is not a well-formed UUID.
///
/// Ids arrive from the frontend as untyped strings, so they are checked here
/// rather than failing deeper in a query.
pub fn validate_uuid(field_name: &str, value: &str) -> Result<(), AppError> {
    uuid::Uuid::parse_str(value).map_err(|_| {
        AppError::Validation(format!("{field_name} must be a valid UUID, got '{value}'"))
    })?;
    Ok(())
}

/// Validates an account identifier.
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

/// Rejects a range whose start is after its end.
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

#[allow(clippy::inconsistent_digit_grouping)]
pub const MAX_REASONABLE_AMOUNT_MINOR: i64 = 100_000_000_00;

/// Rejects an implausible monetary amount.
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

pub const MAX_PAGE_SIZE: u32 = 200;

/// Rejects pagination parameters outside sane bounds.
///
/// An unbounded page size would let one call load the entire ledger into memory.
pub fn validate_pagination(page: u32, per_page: u32) -> Result<(), AppError> {
    if page < 1 {
        return Err(AppError::Validation(format!(
            "page must be >= 1, got {page}"
        )));
    }
    if !(1..=MAX_PAGE_SIZE).contains(&per_page) {
        return Err(AppError::Validation(format!(
            "per_page must be between 1 and {MAX_PAGE_SIZE}, got {per_page}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
