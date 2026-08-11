//! Transactions that could not be attributed to any known instrument.
//!
//! Held here rather than guessed into an arbitrary instrument, which would
//! quietly corrupt per-account balances. They surface as a work queue for the
//! user to resolve manually.
use anyhow::{bail, Result};
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

const BODY_SNIPPET_MAX_CHARS: usize = 240;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UnassignedTransactionRow {
    pub id: String,
    pub observation_id: String,
    pub reason: String,
    pub status: String,
    pub created_at: Option<NaiveDateTime>,
}

/// Records a transaction that could not be attributed to an instrument.
///
/// Held separately rather than guessed into an arbitrary account, which would
/// silently corrupt that account's balance.
pub fn insert(conn: &Connection, unassigned: &UnassignedTransactionRow) -> Result<()> {
    conn.execute(
        "INSERT INTO unassigned_transactions (
            id, observation_id, reason, status
        ) VALUES (
            ?1, ?2, ?3, ?4
        )",
        params![
            unassigned.id,
            unassigned.observation_id,
            unassigned.reason,
            unassigned.status,
        ],
    )?;
    Ok(())
}

/// Update status as the user resolves or dismisses the entry.
///
/// The transition is conditional on the entry still being `open`, in one atomic
/// statement. Resolving creates a real transaction first, so two resolvers
/// racing on the same entry -- or a dismiss landing after a resolve, or a
/// replayed Layer 6 job overruling the user -- must not silently overwrite each
/// other: the loser is told, rather than booking a duplicate and reporting
/// success. Re-applying the status an entry already holds stays a no-op so a
/// retried job is harmless.
pub fn update_status(conn: &Connection, id: &str, new_status: &str) -> Result<()> {
    let changed = conn.execute(
        "UPDATE unassigned_transactions SET status = ?1 WHERE id = ?2 AND status = 'open'",
        params![new_status, id],
    )?;
    if changed > 0 {
        return Ok(());
    }
    // Nothing moved: the entry is either gone or already left the queue. The
    // re-read is only for the message -- the UPDATE above is what serialises.
    match current_status(conn, id)? {
        None => bail!("unassigned transaction '{id}' does not exist"),
        Some(current) if current == new_status => Ok(()),
        Some(current) => bail!(
            "unassigned transaction '{id}' is already '{current}', refusing to overwrite with '{new_status}'"
        ),
    }
}

/// Close an entry on behalf of a background job, tolerating one that moved on.
///
/// A Layer 6 job runs for minutes, so by the time it lands the user may have
/// already resolved or dismissed the entry -- and a cancelled scan may have
/// deleted it outright. None of those are failures: the job has nothing left to
/// do. `update_status` is right to shout at the user-facing resolve path, which
/// books money before it writes, but a shout here is never seen and leaves the
/// durable job row undeleted, so the whole LLM run replays at every launch and
/// never converges. Returns whether this call is what moved the entry.
pub fn settle_if_open(conn: &Connection, id: &str, new_status: &str) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE unassigned_transactions SET status = ?1 WHERE id = ?2 AND status = 'open'",
        params![new_status, id],
    )?;
    Ok(changed > 0)
}

/// Whether the entry is still queued.
///
/// Lets a background job check before doing irreversible work -- booking a
/// transaction for an entry the user already dismissed overrules them with
/// money.
pub fn is_open(conn: &Connection, id: &str) -> Result<bool> {
    Ok(current_status(conn, id)?.as_deref() == Some("open"))
}

/// Refine the recorded reason attribution failed.
///
/// Only meaningful while the entry is still queued: once resolved or dismissed
/// the reason is history, so a late refinement is dropped rather than rewriting
/// it. A missing id is still an error -- silently discarding it hides the bug.
pub fn update_reason(conn: &Connection, id: &str, new_reason: &str) -> Result<()> {
    let changed = conn.execute(
        "UPDATE unassigned_transactions SET reason = ?1 WHERE id = ?2 AND status = 'open'",
        params![new_reason, id],
    )?;
    if changed == 0 && current_status(conn, id)?.is_none() {
        bail!("unassigned transaction '{id}' does not exist");
    }
    Ok(())
}

fn current_status(conn: &Connection, id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT status FROM unassigned_transactions WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()?)
}

/// Unresolved entries, for the reconciliation queue.
pub fn select_open(conn: &Connection) -> Result<Vec<UnassignedTransactionRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, observation_id, reason, status, created_at \
         FROM unassigned_transactions WHERE status = 'open' \
         ORDER BY created_at DESC, id DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(UnassignedTransactionRow {
            id: row.get(0)?,
            observation_id: row.get(1)?,
            reason: row.get(2)?,
            status: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UnassignedTransactionDetail {
    pub id: String,
    pub observation_id: String,
    pub reason: String,
    pub status: String,
    pub created_at: Option<NaiveDateTime>,
    pub merchant_raw: Option<String>,
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub direction: Option<String>,
    pub event_time: Option<NaiveDateTime>,
    pub source_message_id: Option<String>,
    pub body_snippet: Option<String>,
    pub raw_payload_json: Option<String>,
    pub extraction_method: Option<String>,
    pub confidence_score: Option<f64>,
}

/// Unresolved entries together with their source evidence.
///
/// Fetching the context alongside lets the resolver show the original email
/// beside the form, so the user can answer from the source rather than memory.
pub fn select_open_with_context(conn: &Connection) -> Result<Vec<UnassignedTransactionDetail>> {
    let mut stmt = conn.prepare(
        "SELECT u.id, u.observation_id, u.reason, u.status, u.created_at, \
                o.merchant_raw, o.amount_minor, o.currency, o.direction, o.event_time, \
                o.source_message_id, o.raw_payload_json, \
                o.extraction_method, o.confidence_score \
         FROM unassigned_transactions u \
         JOIN transaction_observations o ON o.id = u.observation_id \
         WHERE u.status = 'open' \
         ORDER BY u.created_at DESC, u.id DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        let raw_payload_json: Option<String> = row.get(11)?;
        let body_snippet = raw_payload_json.as_deref().and_then(extract_body_snippet);
        Ok(UnassignedTransactionDetail {
            id: row.get(0)?,
            observation_id: row.get(1)?,
            reason: row.get(2)?,
            status: row.get(3)?,
            created_at: row.get(4)?,
            merchant_raw: row.get(5)?,
            amount_minor: row.get(6)?,
            currency: row.get(7)?,
            direction: row.get(8)?,
            event_time: row.get(9)?,
            source_message_id: row.get(10)?,
            body_snippet,
            raw_payload_json,
            extraction_method: row.get(12)?,
            confidence_score: row.get(13)?,
        })
    })?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Pulls a short readable excerpt from a raw message payload.
///
/// Enough context to identify the transaction in a list without loading and
/// rendering the whole message.
fn extract_body_snippet(raw_payload_json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(raw_payload_json).ok()?;
    let body = value.get("body")?.as_str()?;
    // Collapse only as far as the cap. A multi-megabyte HTML mail must not be
    // rebuilt in full, once per queue row, just to keep its first 240 chars --
    // and a body with no whitespace at all is one single "word", so cap that
    // too. Both bounds sit past the cap, so the result is byte-identical to
    // collapsing everything and truncating afterwards.
    let mut collapsed = String::new();
    for word in body.split_whitespace() {
        if !collapsed.is_empty() {
            collapsed.push(' ');
        }
        collapsed.extend(word.chars().take(BODY_SNIPPET_MAX_CHARS + 1));
        if collapsed.len() > BODY_SNIPPET_MAX_CHARS * 4 {
            break; // 4 bytes/char max in UTF-8, so this is certainly past the cap
        }
    }
    if collapsed.is_empty() {
        return None;
    }
    Some(if collapsed.chars().count() > BODY_SNIPPET_MAX_CHARS {
        let truncated: String = collapsed.chars().take(BODY_SNIPPET_MAX_CHARS).collect();
        format!("{truncated}…")
    } else {
        collapsed
    })
}
