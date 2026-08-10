//! Budget threshold alerts and ingestion sync metadata.
//!
//! Alert rows record that a spending threshold was crossed, so a given breach is
//! announced once rather than on every recalculation. The sync-metadata helpers
//! share this module because both track "what has already been handled".
use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Alert {
    pub alert_id: String,
    pub alert_type: String,
    pub message: String,
    pub related_cluster_id: Option<String>,
    pub status: String,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncMetadata {
    pub bank_name: String,
    pub last_synced_at: NaiveDateTime,
    pub updated_at: Option<NaiveDateTime>,
}

/// Record that a budget threshold was crossed.
///
/// Persisting the alert is what makes a breach announced once rather than on
/// every recalculation.
pub fn insert_alert(conn: &Connection, alert: &Alert) -> Result<()> {
    conn.execute(
        "INSERT INTO alerts (alert_id, type, message, related_cluster_id, status)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            alert.alert_id,
            alert.alert_type,
            alert.message,
            alert.related_cluster_id,
            alert.status
        ],
    )?;
    Ok(())
}

/// Fetch one alert.
pub fn get_alert(conn: &Connection, alert_id: &str) -> Result<Option<Alert>> {
    let mut stmt = conn.prepare(
        "SELECT alert_id, type, message, related_cluster_id, status, created_at, updated_at
         FROM alerts WHERE alert_id = ?1",
    )?;

    let alert = stmt
        .query_row(params![alert_id], |row| {
            let alert_type: String = row.get(1)?;
            let created_at: Option<String> = row.get(5)?;
            let updated_at: Option<String> = row.get(6)?;

            let created_dt = created_at
                .and_then(|s| NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S").ok());
            let updated_dt = updated_at
                .and_then(|s| NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S").ok());

            Ok(Alert {
                alert_id: row.get(0)?,
                alert_type,
                message: row.get(2)?,
                related_cluster_id: row.get(3)?,
                status: row.get(4)?,
                created_at: created_dt,
                updated_at: updated_dt,
            })
        })
        .optional()?;

    Ok(alert)
}

/// Update an alert's status, such as marking it acknowledged.
pub fn update_alert_status(conn: &Connection, alert_id: &str, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE alerts SET status = ?1 WHERE alert_id = ?2",
        params![status, alert_id],
    )?;
    Ok(())
}

/// Reads an ingestion sync marker.
pub fn get_sync_metadata(conn: &Connection, bank_name: &str) -> Result<Option<SyncMetadata>> {
    let mut stmt = conn.prepare(
        "SELECT bank_name, last_synced_at, updated_at
         FROM sync_metadata WHERE bank_name = ?1",
    )?;

    let meta = stmt
        .query_row(params![bank_name], |row| {
            let last_synced_at: String = row.get(1)?;
            let updated_at: Option<String> = row.get(2)?;

            let last_synced_dt =
                NaiveDateTime::parse_from_str(&last_synced_at, "%Y-%m-%d %H:%M:%S")
                    .unwrap_or_default();
            let updated_dt = updated_at
                .and_then(|s| NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S").ok());

            Ok(SyncMetadata {
                bank_name: row.get(0)?,
                last_synced_at: last_synced_dt,
                updated_at: updated_dt,
            })
        })
        .optional()?;

    Ok(meta)
}

/// Writes an ingestion sync marker, inserting or updating as needed.
pub fn upsert_sync_metadata(
    conn: &Connection,
    bank_name: &str,
    last_synced_at: NaiveDateTime,
) -> Result<()> {
    let dt_str = last_synced_at.format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "INSERT INTO sync_metadata (bank_name, last_synced_at)
         VALUES (?1, ?2)
         ON CONFLICT(bank_name) DO UPDATE SET last_synced_at = excluded.last_synced_at",
        params![bank_name, dt_str],
    )?;
    Ok(())
}
