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

/// Upserts one sighting of `domain` with Gate 1's verdict for this message.
/// `verification_result` is a short classification tag (e.g.
/// `"verified_transaction_candidate"`, `"spoof_reject"`,
/// `"unverified_reject"`) -- any tag starting with `"verified"` counts
/// towards `verified_pass_count`. The first sighting of a never-seen domain
/// creates its row with `message_count = 1`; every later sighting increments
/// the counters and bumps `last_seen_at`.
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

/// Whether `domain` has been recorded at least once *before* this call --
/// used to gate Gate 1's subject-based "Unknown Bank" rescue fallback. A
/// domain's very first-ever message has no history yet to weigh against a
/// spoofed subject line, so it must not qualify for that rescue purely off
/// subject-line wording. Call this BEFORE `record_sighting` for the current
/// message, otherwise the current message's own just-recorded row would make
/// every domain look "previously seen" on its very first sighting.
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

/// A domain a user has manually confirmed as a legitimate sender despite
/// repeatedly failing Gate 1's string-based verification -- the runtime
/// learning-loop counterpart to the compiled-in `verified_senders_registry.json`,
/// mirroring how `field_rules` layers on top of the compiled-in
/// `bank_templates` for the extraction ladder.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingSenderRow {
    pub id: String,
    pub domain: String,
    pub bank_name: String,
    pub classification: String,
    pub status: String,
    pub reject_count: i64,
}

/// Records a rejected domain as a promotion candidate. Idempotent per
/// domain: a repeat rejection of the same still-`pending` domain just bumps
/// `reject_count` rather than inserting a duplicate row; a domain already
/// `approved` (or `denied`) is left untouched -- re-rejecting an
/// already-approved domain must not silently reset its status.
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

/// Promotes (or denies) a pending sender. `new_status` must be `"approved"`
/// or `"denied"` -- `SenderValidator::verify_sender` only consults
/// `approved` rows (see `select_approved_domains`).
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

/// All user-approved domains, consulted by `SenderValidator` as a
/// runtime-updatable second registry layer.
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
