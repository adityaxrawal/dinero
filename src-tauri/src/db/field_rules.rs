//! Learned per-field extraction rules (design 2026-07-29).
//!
//! Replaces `pattern_rules`. The structural difference from what it replaces is
//! not the schema but the guarantee: nothing reaches `active` here without
//! having proved, mechanically, that it reproduces the correction it was built
//! from. `rule_change_log` is the receipt for that — every write, every
//! replacement, every rejection, every revert leaves a row, which is what makes
//! removing the human approval step accountable rather than merely convenient.

use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Statuses a rule is read at by extraction.
const LIVE_STATUSES: &str = "('pending', 'active', 'trusted')";

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FieldRuleVariant {
    pub id: String,
    pub bank_name: String,
    pub field_name: String,
    pub source_type: String,
    pub template_hash: String,
    pub rule_payload_json: serde_json::Value,
    pub status: String,
    pub success_count: i64,
    pub failure_count: i64,
    pub confidence: f64,
    pub authored_by: String,
    pub learned_from: String,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

const ROW_SELECT: &str = "SELECT v.id, p.bank_name, p.field_name, p.source_type, v.template_hash, \
     v.rule_payload_json, v.status, v.success_count, v.failure_count, v.confidence, \
     v.authored_by, v.learned_from, v.created_at, v.updated_at \
     FROM field_rule_variants v JOIN field_rules p ON p.id = v.field_rule_id";

fn map_row(row: &rusqlite::Row) -> rusqlite::Result<FieldRuleVariant> {
    let payload_str: String = row.get(5)?;
    let payload = serde_json::from_str(&payload_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(FieldRuleVariant {
        id: row.get(0)?,
        bank_name: row.get(1)?,
        field_name: row.get(2)?,
        source_type: row.get(3)?,
        template_hash: row.get(4)?,
        rule_payload_json: payload,
        status: row.get(6)?,
        success_count: row.get(7)?,
        failure_count: row.get(8)?,
        confidence: row.get(9)?,
        authored_by: row.get(10)?,
        learned_from: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn get_or_create_parent(
    conn: &Connection,
    bank_name: &str,
    field_name: &str,
    source_type: &str,
) -> Result<String> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM field_rules
             WHERE bank_name = ?1 AND field_name = ?2 AND source_type = ?3",
            params![bank_name, field_name, source_type],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO field_rules (id, bank_name, field_name, source_type)
         VALUES (?1, ?2, ?3, ?4)",
        params![id, bank_name, field_name, source_type],
    )?;
    Ok(id)
}

fn insert_change_log(
    conn: &Connection,
    variant_id: Option<&str>,
    action: &str,
    old_payload: Option<&serde_json::Value>,
    new_payload: Option<&serde_json::Value>,
    feedback_log_id: Option<&str>,
    reason: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO rule_change_log
            (id, field_rule_variant_id, action, old_payload_json, new_payload_json,
             triggering_feedback_log_id, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            uuid::Uuid::new_v4().to_string(),
            variant_id,
            action,
            old_payload.map(|p| p.to_string()),
            new_payload.map(|p| p.to_string()),
            feedback_log_id,
            reason,
        ],
    )?;
    Ok(())
}

/// Writes a validated rule, replacing whatever live variant already covers the
/// same `(bank, field, source, template_hash)`.
///
/// Replacement is in-place on the existing row rather than insert-then-retire:
/// the partial unique index makes two live variants for one template
/// impossible, and re-pointing the row means an id already recorded on a
/// `merchant_llm_corrections.learned_rule_id` still resolves. The displaced
/// payload lands in `rule_change_log.old_payload_json`, which is what makes
/// the replacement revertible.
///
/// Returns the id of the row that is now live — the caller's `v.id` on insert,
/// the pre-existing row's id on replacement.
pub fn upsert_variant(
    conn: &Connection,
    v: &FieldRuleVariant,
    feedback_log_id: Option<&str>,
) -> Result<String> {
    if !["pending", "active", "trusted", "inactive", "flagged"].contains(&v.status.as_str()) {
        return Err(anyhow::anyhow!("Invalid status: {}", v.status));
    }
    let parent_id = get_or_create_parent(conn, &v.bank_name, &v.field_name, &v.source_type)?;
    let payload_str = serde_json::to_string(&v.rule_payload_json)?;

    let existing: Option<(String, String)> = conn
        .query_row(
            &format!(
                "SELECT id, rule_payload_json FROM field_rule_variants
                 WHERE field_rule_id = ?1 AND template_hash = ?2 AND status IN {LIVE_STATUSES}"
            ),
            params![parent_id, v.template_hash],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    if let Some((existing_id, old_payload_str)) = existing {
        conn.execute(
            "UPDATE field_rule_variants
             SET rule_payload_json = ?2, status = ?3, success_count = ?4, failure_count = ?5,
                 confidence = ?6, authored_by = ?7, learned_from = ?8,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?1",
            params![
                existing_id,
                payload_str,
                v.status,
                v.success_count,
                v.failure_count,
                v.confidence,
                v.authored_by,
                v.learned_from,
            ],
        )?;
        let old_payload: serde_json::Value =
            serde_json::from_str(&old_payload_str).unwrap_or(serde_json::Value::Null);
        insert_change_log(
            conn,
            Some(&existing_id),
            "created",
            Some(&old_payload),
            Some(&v.rule_payload_json),
            feedback_log_id,
            "replaced by a newer validated rule",
        )?;
        return Ok(existing_id);
    }

    conn.execute(
        "INSERT INTO field_rule_variants
            (id, field_rule_id, template_hash, rule_payload_json, status,
             success_count, failure_count, confidence, authored_by, learned_from)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            v.id,
            parent_id,
            v.template_hash,
            payload_str,
            v.status,
            v.success_count,
            v.failure_count,
            v.confidence,
            v.authored_by,
            v.learned_from,
        ],
    )?;
    insert_change_log(
        conn,
        Some(&v.id),
        "created",
        None,
        Some(&v.rule_payload_json),
        feedback_log_id,
        "synthesized from a validated correction",
    )?;
    Ok(v.id.clone())
}

/// Every rule extraction should currently apply for this bank and source.
///
/// `pending` is excluded: a pending candidate is an unproven guess from drift
/// detection, and reading it would give it the influence it has not yet earned.
pub fn select_live_by_bank(
    conn: &Connection,
    bank_name: &str,
    source_type: &str,
) -> Result<Vec<FieldRuleVariant>> {
    let sql = format!(
        "{ROW_SELECT} WHERE p.bank_name = ?1 AND p.source_type = ?2 \
         AND v.status IN ('active', 'trusted')"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![bank_name, source_type], map_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Whether this bank's template is *known* — the drift detector's "rules exist
/// but extraction failed" test.
pub fn count_live_by_bank_and_hash(
    conn: &Connection,
    bank_name: &str,
    template_hash: &str,
    source_type: &str,
) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM field_rule_variants v
         JOIN field_rules p ON p.id = v.field_rule_id
         WHERE p.bank_name = ?1 AND p.source_type = ?3 AND v.template_hash = ?2
           AND v.status IN ('active', 'trusted')",
        params![bank_name, template_hash, source_type],
        |row| row.get(0),
    )?;
    Ok(count)
}

pub fn select_by_id(conn: &Connection, id: &str) -> Result<Option<FieldRuleVariant>> {
    let sql = format!("{ROW_SELECT} WHERE v.id = ?1");
    conn.query_row(&sql, params![id], map_row)
        .optional()
        .map_err(Into::into)
}

/// Every rule, for the read-only Settings view.
pub fn select_all(conn: &Connection) -> Result<Vec<FieldRuleVariant>> {
    let sql = format!("{ROW_SELECT} ORDER BY p.bank_name ASC, p.field_name ASC, v.status ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], map_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// One atomic `UPDATE`: SQLite evaluates every column expression against the
/// pre-update row, so count, confidence and status all derive from one
/// consistent snapshot. Carried over verbatim from `pattern_rules` — a
/// SELECT-mutate-UPDATE round trip here lost updates when two Transaction
/// Queue workers touched the same rule, and that bug is not worth rediscovering.
pub fn record_success(conn: &Connection, id: &str) -> Result<()> {
    let updated = conn.execute(
        "UPDATE field_rule_variants
         SET success_count = success_count + 1,
             confidence = CAST(success_count + 1 AS REAL) / (success_count + 1 + failure_count),
             status = CASE
                 WHEN status = 'pending' AND success_count + 1 >= 3 THEN 'active'
                 WHEN status = 'active' AND success_count + 1 >= 10 THEN 'trusted'
                 ELSE status
             END,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![id],
    )?;
    if updated == 0 {
        return Err(anyhow::anyhow!("Rule not found"));
    }
    Ok(())
}

/// See [`record_success`] — same atomic-`UPDATE` reasoning on the decay path.
pub fn record_failure(conn: &Connection, id: &str) -> Result<()> {
    let updated = conn.execute(
        "UPDATE field_rule_variants
         SET failure_count = failure_count + 1,
             confidence = CAST(success_count AS REAL) / (success_count + failure_count + 1),
             status = CASE
                 WHEN failure_count + 1 >= 3 THEN 'inactive'
                 WHEN CAST(success_count AS REAL) / (success_count + failure_count + 1) < 0.70
                     THEN 'inactive'
                 ELSE status
             END,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![id],
    )?;
    if updated == 0 {
        return Err(anyhow::anyhow!("Rule not found"));
    }
    Ok(())
}

/// Retires a rule without deleting it. `inactive` is invisible to extraction
/// but keeps the row (and therefore the change-log chain) intact, so "what did
/// this system do to my data" stays answerable after the fact.
pub fn revert(conn: &Connection, id: &str, reason: &str) -> Result<()> {
    let payload: Option<String> = conn
        .query_row(
            "SELECT rule_payload_json FROM field_rule_variants WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?;
    let payload = payload.ok_or_else(|| anyhow::anyhow!("Rule not found"))?;
    conn.execute(
        "UPDATE field_rule_variants SET status = 'inactive', updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![id],
    )?;
    let parsed: serde_json::Value =
        serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null);
    insert_change_log(conn, Some(id), "reverted", Some(&parsed), None, None, reason)
}

/// Records a candidate that failed the validation gate. Deliberately writes no
/// variant: a rejected rule must leave extraction exactly as it was, and the
/// user's already-saved correction completely untouched.
pub fn log_rejection(
    conn: &Connection,
    feedback_log_id: Option<&str>,
    payload: Option<&serde_json::Value>,
    reason: &str,
) -> Result<()> {
    insert_change_log(conn, None, "rejected", None, payload, feedback_log_id, reason)
}

/// Retained source bodies for a bank, paired with the value currently accepted
/// for `field_name` — the regression check's corpus.
///
/// Only reconciled observations with a surviving `raw_payload_json` qualify:
/// an unmatched observation has no settled answer to regress against, and a
/// swept payload has no text to test. Both shrink the corpus rather than
/// weaken the check, which is why "no samples" degrades to accept-on-self-check
/// rather than to reject.
pub fn historical_samples(
    conn: &Connection,
    bank_name: &str,
    field_name: &str,
    source_type: &str,
    exclude_observation_id: Option<&str>,
    limit: usize,
) -> Result<Vec<(String, Option<String>)>> {
    // Which observation column holds the currently-accepted answer for this
    // field. `last4` lives on the instrument, not the observation, so it has no
    // regression corpus and returns empty rather than a wrong column.
    let value_column = match field_name {
        "merchant" => "o.merchant_raw",
        "amount" => "CAST(o.amount_minor AS TEXT)",
        "event_time" => "CAST(o.event_time AS TEXT)",
        "reference_id" => "o.reference_id",
        "balance" => "CAST(o.balance_after_transaction AS TEXT)",
        "direction" => "o.direction",
        "currency" => "o.currency",
        _ => return Ok(Vec::new()),
    };
    let pipeline = if source_type == "statement_pdf" {
        "statement_pdf"
    } else {
        "gmail_transaction"
    };
    // A statement-sourced rule learns from the entry's own row text, not from
    // an email body; an email rule learns from raw_payload_json's "body".
    let body_expr = if source_type == "statement_pdf" {
        "e.description_raw"
    } else {
        "json_extract(o.raw_payload_json, '$.body')"
    };
    let sql = format!(
        "SELECT {body_expr}, {value_column}
         FROM transaction_observations o
         JOIN instruments i ON i.id = o.instrument_id
         LEFT JOIN statement_entries e ON e.id = o.statement_entry_id
         WHERE i.issuer_name = ?1
           AND o.source_pipeline = ?2
           AND o.canonical_transaction_id IS NOT NULL
           AND {body_expr} IS NOT NULL
           AND (?3 IS NULL OR o.id != ?3)
         ORDER BY o.created_at DESC
         LIMIT ?4"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        params![bank_name, pipeline, exclude_observation_id, limit as i64],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
    )?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}
