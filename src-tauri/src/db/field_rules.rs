//! Learned per-bank extraction rules and their success statistics.
//!
//! Bank email formats differ and change without notice, so extraction patterns
//! are stored as data rather than compiled in. Each rule tracks its own hit rate,
//! which is what allows a rule that has stopped working after a template change
//! to be identified and retired instead of silently producing wrong values.
use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

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

/// Maps a result row onto a field rule.
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

/// Returns the parent rule for a bank/field pair, creating it if absent.
///
/// Rules are versioned as variants under a parent, so the parent must exist
/// before a variant can be attached.
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

/// Records a change to a rule, for audit and rollback.
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

/// Stores a new rule variant beside the one serving its bank, field and
/// template (review P2).
///
/// - No variant serves that template: the new one is stored as given.
/// - It comes from the user's own correction (`user_edit`): it serves at
///   once and the previous serving variant is set inactive (kept for revert).
/// - Otherwise (drift LLM, merchant cleanup) it becomes the template's
///   pending challenger, evaluated in shadow, and takes over only by beating
///   the serving variant (see `record_success`). A serving variant learned
///   from a user correction is never displaced automatically.
///
/// Each template has at most one challenger; a newer automatic rule replaces
/// an unproven one. Returns the id of the stored (or identical existing)
/// variant.
pub fn upsert_variant(
    conn: &Connection,
    v: &FieldRuleVariant,
    feedback_log_id: Option<&str>,
) -> Result<String> {
    if !["pending", "active", "trusted", "inactive", "flagged"].contains(&v.status.as_str()) {
        return Err(anyhow::anyhow!("Invalid status: {}", v.status));
    }
    let parent_id = get_or_create_parent(conn, &v.bank_name, &v.field_name, &v.source_type)?;

    // An identical live rule is already there: nothing to add.
    let live = live_variants(conn, &parent_id, &v.template_hash)?;
    if let Some(same) = live.iter().find(|l| l.rule_payload_json == v.rule_payload_json) {
        return Ok(same.id.clone());
    }
    let serving = live.iter().find(|l| l.status == "active" || l.status == "trusted");
    let challenger = live.iter().find(|l| l.status == "pending");

    let mut stored = v.clone();
    match serving {
        None => {}
        Some(incumbent)
            if v.learned_from == "user_edit" && (v.status == "active" || v.status == "trusted") =>
        {
            set_inactive(conn, incumbent, "superseded by the user's correction")?;
        }
        Some(incumbent) => {
            stored.status = "pending".to_string();
            stored.success_count = 0;
            stored.failure_count = 0;
            stored.confidence = 0.0;
            tracing::info!(
                incumbent = %incumbent.id,
                challenger = %stored.id,
                "New rule shadows the serving rule instead of replacing it"
            );
        }
    }
    if stored.status == "pending" {
        if let Some(old) = challenger {
            set_inactive(conn, old, "replaced by a newer challenger")?;
        }
    }

    conn.execute(
        "INSERT INTO field_rule_variants
            (id, field_rule_id, template_hash, rule_payload_json, status,
             success_count, failure_count, confidence, authored_by, learned_from)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            stored.id,
            parent_id,
            stored.template_hash,
            serde_json::to_string(&stored.rule_payload_json)?,
            stored.status,
            stored.success_count,
            stored.failure_count,
            stored.confidence,
            stored.authored_by,
            stored.learned_from,
        ],
    )?;
    insert_change_log(
        conn,
        Some(&stored.id),
        "created",
        serving.map(|s| &s.rule_payload_json),
        Some(&stored.rule_payload_json),
        feedback_log_id,
        if stored.status == "pending" && serving.is_some() {
            "challenger to the serving rule, evaluated in shadow"
        } else {
            "synthesized from a validated correction"
        },
    )?;
    Ok(stored.id)
}

/// The live (serving or challenger) variants for one rule and template.
fn live_variants(
    conn: &Connection,
    parent_id: &str,
    template_hash: &str,
) -> Result<Vec<FieldRuleVariant>> {
    let sql = format!(
        "{ROW_SELECT} WHERE v.field_rule_id = ?1 AND v.template_hash = ?2 \
         AND v.status IN {LIVE_STATUSES}"
    );
    let rows = conn
        .prepare(&sql)?
        .query_map(params![parent_id, template_hash], map_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Retires a variant, logging why.
fn set_inactive(conn: &Connection, v: &FieldRuleVariant, reason: &str) -> Result<()> {
    conn.execute(
        "UPDATE field_rule_variants SET status = 'inactive', updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![v.id],
    )?;
    insert_change_log(conn, Some(&v.id), "reverted", Some(&v.rule_payload_json), None, None, reason)
}

/// Promotes a challenger that has beaten the serving variant (review P2).
///
/// A challenger needs the usual three shadow successes and a strictly higher
/// hit rate than the variant serving its template, so a rule that still
/// extracts perfectly is never displaced; a serving rule learned from a user
/// correction is never displaced at all.
fn promote_challenger_if_better(conn: &Connection, id: &str) -> Result<()> {
    let Some(challenger) = select_by_id(conn, id)? else {
        return Ok(());
    };
    if challenger.status != "pending" || challenger.success_count < 3 {
        return Ok(());
    }
    let parent_id: String = conn.query_row(
        "SELECT field_rule_id FROM field_rule_variants WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    let live = live_variants(conn, &parent_id, &challenger.template_hash)?;
    let Some(incumbent) = live.iter().find(|l| l.status == "active" || l.status == "trusted")
    else {
        return Ok(());
    };
    if incumbent.learned_from == "user_edit" || challenger.confidence <= incumbent.confidence {
        return Ok(());
    }
    set_inactive(conn, incumbent, "outperformed by a challenger in shadow evaluation")?;
    conn.execute(
        "UPDATE field_rule_variants SET status = 'active', updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![id],
    )?;
    insert_change_log(
        conn,
        Some(id),
        "activated",
        Some(&incumbent.rule_payload_json),
        Some(&challenger.rule_payload_json),
        None,
        "beat the serving rule in shadow evaluation",
    )
}

/// Active rules for a bank, used during extraction.
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

/// Counts live rules matching a template hash.
///
/// Used to decide whether a bank's template is already covered, so synthesis is
/// not repeated for a layout that already extracts cleanly.
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

/// Scores pending rules against a confirmed extraction (review M10).
///
/// Pending rules (synthesised from LLM drift) are never applied, so they only
/// earn evidence here: each pending rule for this bank, source and email
/// template is run on the body, and agreement with the confirmed value counts
/// as a success (3 promote it to active, 10 to trusted) and disagreement as a
/// failure. Returns how many rules were scored.
pub fn shadow_evaluate_pending(
    conn: &Connection,
    bank_name: &str,
    source_type: &str,
    body: &str,
    confirmed: &crate::extraction::ladder::ExtractionResult,
) -> Result<usize> {
    let template_hash = crate::extraction::ladder::compute_template_hash(body);
    let sql = format!(
        "{ROW_SELECT} WHERE p.bank_name = ?1 AND p.source_type = ?2 \
         AND v.template_hash = ?3 AND v.status = 'pending'"
    );
    let pending = conn
        .prepare(&sql)?
        .query_map(params![bank_name, source_type, template_hash], map_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut scored = 0;
    for rule in pending {
        let Some(captured) =
            crate::extraction::rule_synthesis::apply_payload(&rule.rule_payload_json, body)
        else {
            record_failure(conn, &rule.id)?;
            scored += 1;
            continue;
        };
        match crate::extraction::ladder::rule_capture_agrees(&rule.field_name, &captured, confirmed) {
            Some(true) => record_success(conn, &rule.id)?,
            Some(false) => record_failure(conn, &rule.id)?,
            None => continue,
        }
        scored += 1;
    }
    Ok(scored)
}

/// Fetch one rule.
pub fn select_by_id(conn: &Connection, id: &str) -> Result<Option<FieldRuleVariant>> {
    let sql = format!("{ROW_SELECT} WHERE v.id = ?1");
    conn.query_row(&sql, params![id], map_row)
        .optional()
        .map_err(Into::into)
}

/// All rules, for the settings screen.
pub fn select_all(conn: &Connection) -> Result<Vec<FieldRuleVariant>> {
    let sql = format!("{ROW_SELECT} ORDER BY p.bank_name ASC, p.field_name ASC, v.status ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], map_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Increments a rule's success count.
///
/// The hit rate accumulated here is what distinguishes a rule that still works
/// from one that has quietly stopped matching after a bank changed its template.
pub fn record_success(conn: &Connection, id: &str) -> Result<()> {
    let updated = conn.execute(
        "UPDATE field_rule_variants
         SET success_count = success_count + 1,
             confidence = CAST(success_count + 1 AS REAL) / (success_count + 1 + failure_count),
             status = CASE
                 -- A pending rule beside a serving one is a challenger: it is
                 -- promoted only by promote_challenger_if_better (review P2).
                 WHEN status = 'pending' AND success_count + 1 >= 3
                      AND NOT EXISTS (
                          SELECT 1 FROM field_rule_variants s
                          WHERE s.field_rule_id = field_rule_variants.field_rule_id
                            AND s.template_hash = field_rule_variants.template_hash
                            AND s.status IN ('active', 'trusted'))
                      THEN 'active'
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
    promote_challenger_if_better(conn, id)
}

/// Increments a rule's failure count.
///
/// Sustained failures are the signal to retire a rule rather than let it keep
/// producing wrong values.
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

/// Reverts a rule to its previous variant.
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
    insert_change_log(
        conn,
        Some(id),
        "reverted",
        Some(&parsed),
        None,
        None,
        reason,
    )
}

/// Records that a synthesised rule was rejected before going live.
///
/// Keeps the record of what was tried, so the same bad candidate is not
/// repeatedly re-synthesised.
pub fn log_rejection(
    conn: &Connection,
    feedback_log_id: Option<&str>,
    payload: Option<&serde_json::Value>,
    reason: &str,
) -> Result<()> {
    insert_change_log(
        conn,
        None,
        "rejected",
        None,
        payload,
        feedback_log_id,
        reason,
    )
}

/// Past extractions for a rule, used to regression-check a replacement.
///
/// A new rule is validated against these before it goes live, so it cannot break
/// extractions that already worked.
pub fn historical_samples(
    conn: &Connection,
    bank_name: &str,
    field_name: &str,
    source_type: &str,
    exclude_observation_id: Option<&str>,
    limit: usize,
) -> Result<Vec<(String, Option<String>)>> {
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
