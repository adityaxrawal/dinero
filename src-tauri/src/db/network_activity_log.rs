//! Append-only record of every outbound network request.
//!
//! The evidence behind the privacy disclosure screen. Metadata only --
//! destination, channel, timing, status -- never request or response bodies.
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
    pub channel: Option<String>,
}

/// Appends one outbound request record.
///
/// Metadata only -- destination, channel, timing, status -- never bodies.
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

/// One page of network activity, for the privacy screen.
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
