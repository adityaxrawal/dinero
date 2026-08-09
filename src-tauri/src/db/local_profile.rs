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

/// `llm_model` (migration `20260101000019_local_profile_onboarding_fields`)
/// was only ever written, via `onboarding_save_preferences`'s raw SQL — not
/// modeled on `LocalProfileRow`, and nothing anywhere ever read it back.
/// `extraction/ladder.rs`'s `Layer6LlmLayer` needs exactly this: which
/// catalog model id the user actually selected.
pub fn get_llm_model(conn: &Connection) -> Result<Option<String>> {
    let result: rusqlite::Result<Option<String>> = conn.query_row(
        "SELECT llm_model FROM local_profile WHERE id = 1",
        [],
        |row| row.get(0),
    );
    match result {
        Ok(model) => Ok(model),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Settings' model picker writes here directly — a single-column update,
/// deliberately not routed through `onboarding_save_preferences` (which
/// would require resending every other onboarding field just to change
/// one).
pub fn set_llm_model(conn: &Connection, model_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE local_profile SET llm_model = ?1 WHERE id = 1",
        params![model_id],
    )?;
    Ok(())
}

/// Clears the active model back to "unset" — used when the previously
/// active model was deleted and no other downloaded model is available to
/// take its place.
pub fn clear_llm_model(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE local_profile SET llm_model = NULL WHERE id = 1", [])?;
    Ok(())
}
