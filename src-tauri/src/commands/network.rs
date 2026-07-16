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

/// Document 18 §4.21b's authoritative schema for `network_activity_log` is
/// `id`/`channel`/`destination`/`bytes_transferred`/`occurred_at` -- the
/// real implemented table deviates significantly (`method`/`domain`/
/// `url_redacted`/`bytes_sent`+`bytes_received`/`status_code`/
/// `secret_fields_masked`, HTTP-request-log shaped rather than
/// privacy-transparency shaped) and is **missing `channel` entirely**,
/// the field that is the whole point of "which of the 5 disclosed
/// channels made this call" (Document 01 §10.4). A full fix means
/// `network_client.rs::NetworkClient::execute` accepting an explicit
/// channel identifier from every one of its callers (gmail_client.rs,
/// licensing/, oauth.rs) -- out of TASK-API-008's stated scope (Settings
/// IPC surface, not ingestion/licensing internals). This infers `channel`
/// at read time from the destination hostname instead, covering the
/// channels that actually go through `NetworkClient` today (gmail_api,
/// google_oauth, licensing_backend); flagged in fix-log.md as a real gap
/// for a dedicated follow-up, not silently accepted as done.
fn infer_channel(destination: &str) -> String {
    if destination.contains("gmail.googleapis.com") {
        "gmail_api".to_string()
    } else if destination.contains("accounts.google.com") || destination.contains("oauth2.googleapis.com") {
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
    NetworkActivityEntry {
        id: row.id,
        channel: infer_channel(&row.domain),
        destination: row.domain,
        bytes_transferred: row.bytes_sent.unwrap_or(0) + row.bytes_received.unwrap_or(0),
        occurred_at: row.timestamp,
    }
}

/// G20/H10/J8 fix: renamed from `settings_network_activity_list` to match
/// Doc 19 §13.10's documented `settings_get_network_activity` naming and
/// `{ "entries": [...] }` response shape (was a bare array).
#[tauri::command]
pub async fn settings_get_network_activity(
    db: State<'_, Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    let conn = db
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let rows = conn
        .interact(move |conn| network_activity_log::list_all(conn))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let entries: Vec<NetworkActivityEntry> = rows.into_iter().map(to_entry).collect();
    Ok(serde_json::json!({ "entries": entries }))
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
}
