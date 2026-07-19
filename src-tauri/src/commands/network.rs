use crate::db::network_activity_log::{self, NetworkActivityLogRow};
use deadpool_sqlite::Pool;
use tauri::State;

/// Document 19 §13.10's exact 5 named fields.
#[derive(serde::Serialize)]
pub struct NetworkActivityEntry {
    pub id: String,
    pub channel: String,
    pub destination: String,
    pub bytes_transferred: i64,
    pub occurred_at: Option<chrono::NaiveDateTime>,
}

/// Doc 30 TASK-API-006: `network_activity_log.channel` is now written
/// directly by `NetworkClient::execute`'s caller (gmail_client.rs,
/// oauth.rs, polling.rs, licensing/client.rs each pass their real channel
/// through). This hostname-based inference is now only a fallback for rows
/// written before that column existed (`channel IS NULL`) -- kept rather
/// than deleted so old rows still resolve to a real channel instead of
/// "unknown" in the UI.
fn infer_channel(destination: &str) -> String {
    if destination.contains("gmail.googleapis.com") {
        "gmail_api".to_string()
    } else if destination.contains("accounts.google.com")
        || destination.contains("oauth2.googleapis.com")
    {
        "google_oauth".to_string()
    } else if destination.contains("dinero-app.com") {
        "licensing_backend".to_string()
    } else if destination.contains("github.com") {
        "github_releases".to_string()
    } else if destination.contains("huggingface.co") {
        "huggingface".to_string()
    } else {
        "unknown".to_string()
    }
}

fn to_entry(row: NetworkActivityLogRow) -> NetworkActivityEntry {
    let channel = row
        .channel
        .clone()
        .unwrap_or_else(|| infer_channel(&row.domain));
    NetworkActivityEntry {
        id: row.id,
        channel,
        destination: row.domain,
        bytes_transferred: row.bytes_sent.unwrap_or(0) + row.bytes_received.unwrap_or(0),
        occurred_at: row.timestamp,
    }
}

/// G20/H10/J8 fix: renamed from `settings_network_activity_list` to match
/// Doc 19 §13.10's documented `settings_get_network_activity` naming and
/// `{ "entries": [...] }` response shape (was a bare array).
///
/// Paginated (`page` 1-based, `page_size` capped at
/// `ipc::validation::MAX_PAGE_SIZE`) -- this table has no row-count cap
/// (only Document 18 §4.21b's 30-day time retention window), and a single
/// historical scan can write hundreds of rows in seconds, so fetching every
/// row unconditionally doesn't scale. Response shape matches Document 19
/// §10.1's already-established `{ items/entries, meta: { page, page_size,
/// total } }` pagination convention.
#[tauri::command]
pub async fn settings_get_network_activity(
    page: u32,
    page_size: u32,
    db: State<'_, Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::ipc::validation::validate_pagination(page, page_size)?;

    let conn = db
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let (rows, total) = conn
        .interact(move |conn| network_activity_log::list_paginated(conn, page, page_size))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let entries: Vec<NetworkActivityEntry> = rows.into_iter().map(to_entry).collect();
    Ok(serde_json::json!({
        "entries": entries,
        "meta": { "page": page, "page_size": page_size, "total": total }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_channel_matches_known_hosts() {
        assert_eq!(infer_channel("gmail.googleapis.com"), "gmail_api");
        assert_eq!(infer_channel("oauth2.googleapis.com"), "google_oauth");
        assert_eq!(infer_channel("accounts.google.com"), "google_oauth");
        assert_eq!(infer_channel("api.dinero-app.com"), "licensing_backend");
        assert_eq!(infer_channel("some-unrelated-host.example.com"), "unknown");
    }

    fn base_row(channel: Option<&str>) -> NetworkActivityLogRow {
        NetworkActivityLogRow {
            id: "row_1".to_string(),
            timestamp: None,
            method: "GET".to_string(),
            domain: "gmail.googleapis.com".to_string(),
            url_redacted: "https://gmail.googleapis.com/redacted".to_string(),
            bytes_sent: Some(10),
            bytes_received: Some(20),
            status_code: Some(200),
            secret_fields_masked: None,
            channel: channel.map(|c| c.to_string()),
        }
    }

    /// Doc 30 TASK-API-006: a row with a real stored `channel` (written by
    /// `NetworkClient::execute`'s caller) must use that value directly, not
    /// the hostname-inference fallback.
    #[test]
    fn test_to_entry_prefers_stored_channel_over_inference() {
        let row = base_row(Some("gmail_api"));
        let entry = to_entry(row);
        assert_eq!(entry.channel, "gmail_api");
    }

    /// A legacy row written before the `channel` column existed (`NULL`)
    /// still falls back to hostname inference instead of surfacing as
    /// "unknown" for a recognized host.
    #[test]
    fn test_to_entry_falls_back_to_inference_for_legacy_rows() {
        let row = base_row(None);
        let entry = to_entry(row);
        assert_eq!(entry.channel, "gmail_api");
    }
}
