use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection};
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

pub fn update_status(conn: &Connection, id: &str, new_status: &str) -> Result<()> {
    conn.execute(
        "UPDATE unassigned_transactions SET status = ?1 WHERE id = ?2",
        params![new_status, id],
    )?;
    Ok(())
}

/// Keeps the audit-trail `reason` in sync with reality when Layer 6
/// enriches an observation without clearing enough of Gate 3 to promote it
/// (see `apply_layer6_success`). Without this, `reason` is frozen at
/// whatever the *original*, pre-enrichment gate3 evaluation found -- e.g.
/// still reading `missing_counterparty` after Layer 6 has since filled in
/// `merchant_raw`, hiding the item's actual current blocker from anyone
/// triaging the review queue by reason.
pub fn update_reason(conn: &Connection, id: &str, new_reason: &str) -> Result<()> {
    conn.execute(
        "UPDATE unassigned_transactions SET reason = ?1 WHERE id = ?2",
        params![new_reason, id],
    )?;
    Ok(())
}

/// Doc 30 TASK-API-005: `reconciliation_get_unassigned_transactions` -- "a
/// distinct queue from ambiguous clusters: extraction failures vs. matching
/// ambiguity are surfaced separately in the UI." Did not exist at all
/// before this task (only `insert`/`update_status` existed).
pub fn select_open(conn: &Connection) -> Result<Vec<UnassignedTransactionRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, observation_id, reason, status, created_at \
         FROM unassigned_transactions WHERE status = 'open' ORDER BY created_at DESC",
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

/// The plain `select_open` result gives the UI nothing to show the user
/// beyond a raw reason code and an opaque UUID -- joins the linked
/// `transaction_observations` row (a required FK, so an inner join never
/// silently drops a row) for whatever partial signal extraction *did*
/// recover (merchant/amount/currency), plus a short snippet of the original
/// email body (stored verbatim as `{"body": "..."}` in `raw_payload_json`
/// for the `extraction_failed` case -- see
/// `message_processor.rs::record_unassigned_transaction`), so the user has
/// enough context to actually understand what failed without digging
/// through logs.
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
    /// "debit" | "credit" -- often already known even when the instrument
    /// lookup that made this item unassigned (`issuer_name_not_found`)
    /// failed, so the "Save as Transaction" form can prefill it.
    pub direction: Option<String>,
    pub event_time: Option<NaiveDateTime>,
    pub source_message_id: Option<String>,
    pub body_snippet: Option<String>,
    pub raw_payload_json: Option<String>,
    pub extraction_method: Option<String>,
    pub confidence_score: Option<f64>,
}

pub fn select_open_with_context(conn: &Connection) -> Result<Vec<UnassignedTransactionDetail>> {
    let mut stmt = conn.prepare(
        "SELECT u.id, u.observation_id, u.reason, u.status, u.created_at, \
                o.merchant_raw, o.amount_minor, o.currency, o.direction, o.event_time, \
                o.source_message_id, o.raw_payload_json, \
                o.extraction_method, o.confidence_score \
         FROM unassigned_transactions u \
         JOIN transaction_observations o ON o.id = u.observation_id \
         WHERE u.status = 'open' \
         ORDER BY u.created_at DESC",
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

/// Collapses whitespace (the stored body is the raw, unwrapped email text --
/// newlines and repeated spaces make for a poor one-line preview) and caps
/// to [`BODY_SNIPPET_MAX_CHARS`] on a char boundary (not a byte slice --
/// email bodies routinely contain multi-byte characters like `₹`).
fn extract_body_snippet(raw_payload_json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(raw_payload_json).ok()?;
    let body = value.get("body")?.as_str()?;
    let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
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
