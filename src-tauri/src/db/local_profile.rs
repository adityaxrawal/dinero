use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocalProfileRow {
    pub id: i64,
    pub primary_email: Option<String>,
    pub display_name: Option<String>,
    pub timezone: Option<String>,
    pub spending_limit_monthly: Option<f64>,
    pub limit_thresholds: Option<serde_json::Value>,
    pub recovery_phrase_enabled: bool,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

pub fn insert(conn: &Connection, profile: &LocalProfileRow) -> Result<()> {
    conn.execute(
        "INSERT INTO local_profile (
            id, primary_email, display_name, timezone, spending_limit_monthly, 
            limit_thresholds, recovery_phrase_enabled
         )
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            profile.primary_email,
            profile.display_name,
            profile.timezone,
            profile.spending_limit_monthly,
            profile.limit_thresholds,
            profile.recovery_phrase_enabled,
        ],
    )?;
    Ok(())
}

pub fn update(conn: &Connection, profile: &LocalProfileRow) -> Result<()> {
    let count = conn.execute(
        "UPDATE local_profile SET
            primary_email = ?1,
            display_name = ?2,
            timezone = ?3,
            spending_limit_monthly = ?4,
            limit_thresholds = ?5,
            recovery_phrase_enabled = ?6
         WHERE id = 1",
        params![
            profile.primary_email,
            profile.display_name,
            profile.timezone,
            profile.spending_limit_monthly,
            profile.limit_thresholds,
            profile.recovery_phrase_enabled,
        ],
    )?;
    if count == 0 {
        return Err(anyhow::anyhow!("Profile not found"));
    }
    Ok(())
}

pub fn select_by_id(conn: &Connection, id: i64) -> Result<Option<LocalProfileRow>> {
    let mut stmt = conn.prepare("SELECT * FROM local_profile WHERE id = ?1")?;
    let mut rows = stmt.query([id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_profile(row)?))
    } else {
        Ok(None)
    }
}

fn row_to_profile(row: &Row) -> rusqlite::Result<LocalProfileRow> {
    Ok(LocalProfileRow {
        id: row.get("id")?,
        primary_email: row.get("primary_email")?,
        display_name: row.get("display_name")?,
        timezone: row.get("timezone")?,
        spending_limit_monthly: row.get("spending_limit_monthly")?,
        limit_thresholds: row.get("limit_thresholds")?,
        recovery_phrase_enabled: row.get("recovery_phrase_enabled")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}
