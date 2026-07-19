use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NetworkActivityLogRow {
    pub id: String,
    pub timestamp: Option<NaiveDateTime>,
    pub method: String,
    pub domain: String,
    pub url_redacted: String,
    pub bytes_sent: Option<i64>,
    pub bytes_received: Option<i64>,
    pub status_code: Option<i64>,
    pub secret_fields_masked: Option<String>,
    /// Doc 30 TASK-API-006 / Document 18 §4.21b: which of the 5 disclosed
    /// network channels made this call (`gmail_api`/`licensing_backend`/
    /// `google_oauth`/`github_releases`/`huggingface`), written directly by
    /// `NetworkClient::execute`'s caller. `None` only for rows written
    /// before this column existed.
    pub channel: Option<String>,
}

pub fn insert(conn: &Connection, log: &NetworkActivityLogRow) -> Result<()> {
    conn.execute(
        "INSERT INTO network_activity_log (
            id, method, domain, url_redacted, bytes_sent, bytes_received, status_code, secret_fields_masked, channel
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            log.id, log.method, log.domain, log.url_redacted, log.bytes_sent, log.bytes_received, log.status_code, log.secret_fields_masked, log.channel
        ],
    )?;
    Ok(())
}

/// This table has no row-count cap (only Document 18 §4.21b's 30-day time
/// retention window), and a single historical scan can write hundreds of
/// rows in seconds -- fetching every row unconditionally doesn't scale.
/// `page` is 1-based (matching `ipc::validation::validate_pagination`,
/// which callers must run before this). Returns `(rows, total_row_count)`
/// so the caller can build a `{ entries, meta: { page, page_size, total } }`
/// response, the same shape Document 19 §10.1 already established for
/// `reconciliation_clusters_list`.
pub fn list_paginated(
    conn: &Connection,
    page: u32,
    page_size: u32,
) -> Result<(Vec<NetworkActivityLogRow>, i64)> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM network_activity_log", [], |row| {
        row.get(0)
    })?;

    let offset = (page.saturating_sub(1)) as i64 * page_size as i64;
    let mut stmt = conn
        .prepare("SELECT * FROM network_activity_log ORDER BY timestamp DESC LIMIT ?1 OFFSET ?2")?;
    let rows = stmt.query_map(params![page_size, offset], |row| {
        Ok(NetworkActivityLogRow {
            id: row.get("id")?,
            timestamp: row.get("timestamp")?,
            method: row.get("method")?,
            domain: row.get("domain")?,
            url_redacted: row.get("url_redacted")?,
            bytes_sent: row.get("bytes_sent")?,
            bytes_received: row.get("bytes_received")?,
            status_code: row.get("status_code")?,
            secret_fields_masked: row.get("secret_fields_masked")?,
            channel: row.get("channel")?,
        })
    })?;

    let mut logs = Vec::new();
    for row in rows {
        logs.push(row?);
    }
    Ok((logs, total))
}
