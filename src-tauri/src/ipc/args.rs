use serde::{Deserialize, Serialize};

/// Arguments for creating a manual transaction via IPC.
#[derive(Debug, Deserialize, Serialize)]
pub struct CreateTransactionArgs {
    pub instrument_id: i64,
    pub amount_minor: i64,
    pub currency: String,
    pub direction: String,
    pub merchant_name: String,
    pub category_id: Option<i64>,
}

/// Arguments for updating a transaction via IPC.
#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateTransactionArgs {
    pub id: i64,
    pub merchant_name: Option<String>,
    pub category_id: Option<i64>,
}

/// Arguments for resolving an ambiguous reconciliation cluster.
#[derive(Debug, Deserialize, Serialize)]
pub struct ResolveClusterArgs {
    pub cluster_id: i64,
    pub decision: String,
    pub target_transaction_id: Option<i64>,
}

/// Arguments for submitting a password for a locked PDF statement.
#[derive(Debug, Deserialize, Serialize)]
pub struct SubmitPasswordArgs {
    pub statement_id: i64,
    pub password_plaintext: String,
}

/// Arguments for the full-text search against transactions.
#[derive(Debug, Deserialize, Serialize)]
pub struct SearchTransactionsArgs {
    pub query: String,
    pub limit: u32,
    pub offset: u32,
}
