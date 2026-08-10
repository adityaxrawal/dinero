//! Command exposing the outbound network activity log.
//!
//! Backs the privacy screen, letting the user read exactly what left the machine
//! rather than taking the disclosure on trust.
use crate::db::network_activity_log::{self, NetworkActivityLogRow};
use deadpool_sqlite::Pool;
use tauri::State;

#[derive(serde::Serialize)]
pub struct NetworkActivityEntry {
    pub id: String,
    pub channel: String,
    pub destination: String,
    pub bytes_transferred: i64,
    pub occurred_at: Option<chrono::NaiveDateTime>,
}

/// Maps a destination host to its disclosed channel.
///
/// The channel is what ties a logged request back to a row in the privacy
/// disclosure, so the two can be compared.
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

/// Projects a log row into the frontend's shape.
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

#[tauri::command]
/// Returns a page of network activity for the privacy screen.
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

    #[test]
    fn test_to_entry_prefers_stored_channel_over_inference() {
        let row = base_row(Some("gmail_api"));
        let entry = to_entry(row);
        assert_eq!(entry.channel, "gmail_api");
    }

    #[test]
    fn test_to_entry_falls_back_to_inference_for_legacy_rows() {
        let row = base_row(None);
        let entry = to_entry(row);
        assert_eq!(entry.channel, "gmail_api");
    }
}
