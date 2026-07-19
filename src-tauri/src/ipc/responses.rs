use serde::{Deserialize, Serialize};

/// Generic payload wrapper enforcing the { data: T, error: null } vs { data: null, error: E } pattern.
#[derive(Debug, Serialize, Deserialize)]
pub struct Payload<T> {
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> Payload<T> {
    /// Constructs a successful payload containing data.
    pub fn success(data: T) -> Self {
        Self {
            data: Some(data),
            error: None,
        }
    }

    /// Constructs an error payload with an error message string.
    pub fn error(err: String) -> Self {
        Self {
            data: None,
            error: Some(err),
        }
    }
}

/// Base response for a single transaction.
#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionResponse {
    pub id: i64,
    pub instrument_id: i64,
    pub amount_minor: i64,
    pub currency: String,
    pub merchant_display_name: String,
}

/// Standardized response wrapper for paginated collections.
#[derive(Debug, Serialize, Deserialize)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: u32,
}
