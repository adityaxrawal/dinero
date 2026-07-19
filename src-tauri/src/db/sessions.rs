use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsRow {
    pub id: String,
    pub device_name: Option<String>,
    pub device_fingerprint: Option<String>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

pub fn insert(conn: &Connection, row: &SessionsRow) -> Result<()> {
    conn.execute(
        "INSERT INTO sessions (id, device_name, device_fingerprint, revoked_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            row.id,
            row.device_name,
            row.device_fingerprint,
            row.revoked_at,
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<SessionsRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, device_name, device_fingerprint, created_at, revoked_at
         FROM sessions WHERE id = ?1",
    )?;
    let row = stmt
        .query_row(params![id], |r| {
            Ok(SessionsRow {
                id: r.get(0)?,
                device_name: r.get(1)?,
                device_fingerprint: r.get(2)?,
                created_at: r.get(3)?,
                revoked_at: r.get(4)?,
            })
        })
        .optional()?;
    Ok(row)
}

pub fn revoke(conn: &Connection, id: &str, revoked_at: DateTime<Utc>) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET revoked_at = ?2 WHERE id = ?1",
        params![id, revoked_at],
    )?;
    Ok(())
}
