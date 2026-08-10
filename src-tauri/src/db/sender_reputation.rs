//! Tracks which sender domains have proven to be genuine financial senders.
//!
//! Prevents both false positives and repeated re-evaluation: a domain that has
//! reliably produced valid transactions is trusted, while one repeatedly
//! rejected stops being reconsidered. Reputation is built from observed
//! behaviour rather than a fixed allowlist, so a bank this app has never seen
//! before can still be learned.
use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq)]
pub struct SenderReputationRow {
    pub domain: String,
    pub first_seen_at: NaiveDateTime,
    pub last_seen_at: NaiveDateTime,
    pub message_count: i64,
    pub verified_pass_count: i64,
    pub last_verification_result: String,
}

/// Records that a message was seen from this sender domain.
///
/// Reputation is accumulated from observed behaviour rather than a fixed
/// allowlist, which is what allows a bank this app has never encountered to be
/// learned rather than permanently rejected.
pub fn record_sighting(conn: &Connection, domain: &str, verification_result: &str) -> Result<()> {
    let is_pass = i64::from(verification_result.starts_with("verified"));
    conn.execute(
        "INSERT INTO sender_reputation (domain, first_seen_at, last_seen_at, message_count, verified_pass_count, last_verification_result)
         VALUES (?1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, 1, ?2, ?3)
         ON CONFLICT(domain) DO UPDATE SET
            last_seen_at = CURRENT_TIMESTAMP,
            message_count = message_count + 1,
            verified_pass_count = verified_pass_count + ?2,
            last_verification_result = ?3",
        params![domain, is_pass, verification_result],
    )?;
    Ok(())
}

/// Whether this domain has been seen before.
///
/// A first sighting warrants more scrutiny than a domain with history, since a
/// phishing domain is by definition new.
pub fn has_prior_sighting(conn: &Connection, domain: &str) -> Result<bool> {
    let count: Option<i64> = conn
        .query_row(
            "SELECT message_count FROM sender_reputation WHERE domain = ?1",
            params![domain],
            |row| row.get(0),
        )
        .optional()?;
    Ok(count.unwrap_or(0) > 0)
}

/// Current reputation record for a domain.
pub fn get_reputation(conn: &Connection, domain: &str) -> Result<Option<SenderReputationRow>> {
    conn.query_row(
        "SELECT domain, first_seen_at, last_seen_at, message_count, verified_pass_count, last_verification_result
         FROM sender_reputation WHERE domain = ?1",
        params![domain],
        |row| {
            Ok(SenderReputationRow {
                domain: row.get(0)?,
                first_seen_at: row.get(1)?,
                last_seen_at: row.get(2)?,
                message_count: row.get(3)?,
                verified_pass_count: row.get(4)?,
                last_verification_result: row.get(5)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingSenderRow {
    pub id: String,
    pub domain: String,
    pub bank_name: String,
    pub classification: String,
    pub status: String,
    pub reject_count: i64,
}

/// Notes that a domain produced content rejected as non-financial.
///
/// Repeated rejections are what stop the pipeline re-evaluating the same
/// marketing sender on every scan.
pub fn record_rejection_candidate(
    conn: &Connection,
    id: &str,
    domain: &str,
    bank_name: &str,
    classification: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO pending_senders (id, domain, bank_name, classification, status, reject_count, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'pending', 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
         ON CONFLICT(domain) DO UPDATE SET
            reject_count = reject_count + 1,
            updated_at = CURRENT_TIMESTAMP
         WHERE pending_senders.status = 'pending'",
        params![id, domain, bank_name, classification],
    )?;
    Ok(())
}

/// Sets a domain's approval status.
pub fn update_status(conn: &Connection, id: &str, new_status: &str) -> Result<()> {
    if !["approved", "denied"].contains(&new_status) {
        return Err(anyhow::anyhow!(
            "Invalid pending_senders status transition: {}",
            new_status
        ));
    }
    conn.execute(
        "UPDATE pending_senders SET status = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![id, new_status],
    )?;
    Ok(())
}

/// Domains trusted to send genuine financial mail.
pub fn select_approved_domains(conn: &Connection) -> Result<Vec<PendingSenderRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, domain, bank_name, classification, status, reject_count
         FROM pending_senders WHERE status = 'approved'",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(PendingSenderRow {
                id: row.get(0)?,
                domain: row.get(1)?,
                bank_name: row.get(2)?,
                classification: row.get(3)?,
                status: row.get(4)?,
                reject_count: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Domains seen but not yet judged either way.
pub fn select_pending(conn: &Connection) -> Result<Vec<PendingSenderRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, domain, bank_name, classification, status, reject_count
         FROM pending_senders WHERE status = 'pending' ORDER BY reject_count DESC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(PendingSenderRow {
                id: row.get(0)?,
                domain: row.get(1)?,
                bank_name: row.get(2)?,
                classification: row.get(3)?,
                status: row.get(4)?,
                reject_count: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
