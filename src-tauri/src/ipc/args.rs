//! Argument types for commands taking structured input.
//!
//! Defined as named structs rather than long parameter lists so that field names
//! travel with the values -- a positional signature is easy to get subtly wrong
//! across the language boundary, where nothing checks the caller.
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateTransactionArgs {
    pub instrument_id: i64,
    pub amount_minor: i64,
    pub currency: String,
    pub direction: String,
    pub merchant_name: String,
    pub category_id: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateTransactionArgs {
    pub id: i64,
    pub merchant_name: Option<String>,
    pub category_id: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ResolveClusterArgs {
    pub cluster_id: i64,
    pub decision: String,
    pub target_transaction_id: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SubmitPasswordArgs {
    pub statement_id: i64,
    pub password_plaintext: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SearchTransactionsArgs {
    pub query: String,
    pub limit: u32,
    pub offset: u32,
}
