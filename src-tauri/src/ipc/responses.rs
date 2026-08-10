//! Response types returned to the frontend.
//!
//! Mirrored by hand in the frontend's IPC module, so a change here needs the
//! matching change there; nothing enforces that across the boundary.
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Payload<T> {
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> Payload<T> {
    /// Wraps a successful result.
    pub fn success(data: T) -> Self {
        Self {
            data: Some(data),
            error: None,
        }
    }

    /// Wraps an error result.
    pub fn error(err: String) -> Self {
        Self {
            data: None,
            error: Some(err),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionResponse {
    pub id: i64,
    pub instrument_id: i64,
    pub amount_minor: i64,
    pub currency: String,
    pub merchant_display_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: u32,
}
