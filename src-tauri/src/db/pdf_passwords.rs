//! Metadata for saved statement-PDF passwords.
//!
//! The passwords themselves are held in the OS keychain; these rows record which
//! instrument each belongs to and how often it has worked, so the most reliable
//! candidate is tried first when unlocking a new statement.
use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PdfPasswordsRow {
    pub id: String,
    pub instrument_id: String,
    pub password_ciphertext: String,
    pub success_count: i64,
    pub last_used_at: Option<NaiveDateTime>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Records that a password exists for an instrument.
///
/// The secret itself lives in the OS keychain; this row is only the metadata
/// needed to find and rank it.
pub fn insert(conn: &Connection, password: &PdfPasswordsRow) -> Result<()> {
    conn.execute(
        "INSERT INTO pdf_passwords (
            id, instrument_id, password_ciphertext, success_count, last_used_at, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            password.id,
            password.instrument_id,
            password.password_ciphertext,
            password.success_count,
            password.last_used_at,
            password.created_at,
            password.updated_at,
        ],
    )?;
    Ok(())
}

/// Password entries for one instrument.
pub fn select_by_instrument(
    conn: &Connection,
    instrument_id: &str,
) -> Result<Vec<PdfPasswordsRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, instrument_id, password_ciphertext, success_count, last_used_at, created_at, updated_at
         FROM pdf_passwords
         WHERE instrument_id = ?1
         ORDER BY success_count DESC"
    )?;

    let rows = stmt.query_map([instrument_id], |row| {
        Ok(PdfPasswordsRow {
            id: row.get(0)?,
            instrument_id: row.get(1)?,
            password_ciphertext: row.get(2)?,
            success_count: row.get(3)?,
            last_used_at: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
        })
    })?;

    let mut passwords = Vec::new();
    for r in rows {
        passwords.push(r?);
    }

    Ok(passwords)
}

#[derive(Debug, Serialize, Clone)]
pub struct PdfPasswordSummary {
    pub id: String,
    pub instrument_id: String,
    pub issuer_name: String,
    pub masked_identifier: String,
    pub success_count: i64,
    pub last_used_at: Option<NaiveDateTime>,
}

/// All entries joined to their instruments, for the settings list.
pub fn select_all_with_instrument(conn: &Connection) -> Result<Vec<PdfPasswordSummary>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.instrument_id, i.issuer_name, i.masked_identifier, p.success_count, p.last_used_at
         FROM pdf_passwords p
         JOIN instruments i ON i.id = p.instrument_id
         ORDER BY p.success_count DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(PdfPasswordSummary {
            id: row.get(0)?,
            instrument_id: row.get(1)?,
            issuer_name: row.get(2)?,
            masked_identifier: row.get(3)?,
            success_count: row.get(4)?,
            last_used_at: row.get(5)?,
        })
    })?;

    let mut summaries = Vec::new();
    for r in rows {
        summaries.push(r?);
    }
    Ok(summaries)
}

/// Counts a successful unlock.
///
/// The success count orders which password is tried first, so a recurring
/// statement unlocks on the first attempt rather than after several.
pub fn increment_success(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE pdf_passwords SET success_count = success_count + 1, last_used_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// Forgets a saved password.
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM pdf_passwords WHERE id = ?1", params![id])?;
    Ok(())
}
