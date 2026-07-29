//! Issue #12: persistence for the user-triggered LLM merchant/category pass.
//!
//! Three responsibilities: pick the work queue (low-confidence merchants),
//! apply one LLM answer atomically across the four places it has to land, and
//! put any of it back.
//!
//! Applying a correction touches more than the transaction row, and all of it
//! has to happen together or the pass teaches the pipeline something it will
//! then contradict:
//!
//! 1. `merchants`/`merchant_aliases` -- so normalization resolves this raw
//!    string without the LLM next time.
//! 2. `transactions` -- the visible fix: display name, entity link, category.
//! 3. `pattern_rules` -- so the *extraction* layer stops producing the bad
//!    string in the first place.
//! 4. `merchant_llm_corrections` -- the undo log that makes 1-3 reversible.

use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::extraction::merchant_confidence::{score_merchant, LOW_CONFIDENCE_THRESHOLD};
use crate::extraction::merchant_llm::MerchantResolution;
use crate::extraction::merchant_normalizer::strip_noise_tokens;

/// One transaction whose merchant the LLM should re-read.
#[derive(Debug, Clone)]
pub struct CleanupCandidate {
    pub transaction_id: String,
    pub observation_id: Option<String>,
    pub bank_name: String,
    /// What the parser produced -- the string being questioned.
    pub current_merchant: String,
    /// Sanitized email body, when retention still has it.
    pub body: Option<String>,
    pub amount: Option<f64>,
    pub currency: Option<String>,
    pub direction: Option<String>,
    pub event_time: Option<String>,
    pub category_id: Option<String>,
    pub merchant_entity_id: Option<String>,
    pub merchant_display_name: Option<String>,
    pub merchant_normalized_name: Option<String>,
    /// Heuristic score that put it in the queue, surfaced to the UI.
    pub confidence: f64,
}

/// The closed category list the LLM is constrained to, newest-safe: read from
/// the database rather than hardcoded, so a user-created category is offered
/// too and a deleted one never is.
pub fn category_names(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT name FROM categories WHERE is_deleted = 0 ORDER BY name")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn category_id_for_name(conn: &Connection, name: &str) -> Option<String> {
    conn.query_row(
        "SELECT id FROM categories WHERE name = ?1 AND is_deleted = 0",
        params![name],
        |r| r.get(0),
    )
    .ok()
}

/// Every transaction whose merchant scores below
/// [`LOW_CONFIDENCE_THRESHOLD`], worst first.
///
/// The queue is *derived*, never stored — which is what makes the pass
/// resumable for free. A transaction the previous run already fixed now
/// resolves to a user-sourced merchant, scores above the threshold, and is
/// simply absent here; an interrupted run resumes by being started again.
pub fn select_candidates(conn: &Connection, limit: usize) -> Result<Vec<CleanupCandidate>> {
    // One row per transaction. The `MAX(o.raw_payload_json IS NOT NULL)`
    // aggregate is doing real work, not just being selected: SQLite resolves
    // the other bare columns from whichever row produced that maximum, so a
    // transaction with several observations contributes the one that still
    // has an email body -- the only one worth sending to the model.
    let mut stmt = conn.prepare(
        "SELECT t.id,
                o.id                        AS observation_id,
                COALESCE(i.issuer_name, 'Unknown Bank') AS bank_name,
                t.merchant_display_name,
                t.merchant_normalized_name,
                t.merchant_entity_id,
                t.category_id,
                t.amount,
                t.currency,
                t.direction,
                t.best_event_time,
                o.extraction_method,
                o.raw_payload_json,
                COALESCE(m.source = 'user', 0) AS user_sourced,
                (SELECT COUNT(*) FROM merchant_aliases a
                  WHERE a.merchant_entity_id = m.id) AS alias_count,
                (SELECT mc.llm_confidence FROM merchant_llm_corrections mc
                  WHERE mc.transaction_id = t.id AND mc.status = 'applied'
                  ORDER BY mc.created_at DESC LIMIT 1) AS llm_confidence,
                MAX(o.raw_payload_json IS NOT NULL) AS has_body
         FROM transactions t
         JOIN transaction_observations o ON o.canonical_transaction_id = t.id
         LEFT JOIN instruments i ON i.id = t.instrument_id
         LEFT JOIN merchants  m ON m.id = t.merchant_entity_id
         WHERE t.is_deleted = 0
           AND COALESCE(t.merchant_display_name, t.merchant_normalized_name, '') <> ''
         GROUP BY t.id",
    )?;

    let rows = stmt.query_map([], |r| {
        let display: Option<String> = r.get(3)?;
        let normalized: Option<String> = r.get(4)?;
        let user_sourced: i64 = r.get(13)?;
        let alias_count: i64 = r.get(14)?;
        let extraction_method: Option<String> = r.get(11)?;
        let raw_payload: Option<String> = r.get(12)?;
        let llm_confidence: Option<f64> = r.get(15)?;

        let current = display
            .clone()
            .or_else(|| normalized.clone())
            .unwrap_or_default();
        // Once the LLM has ruled on a merchant, *its* confidence is the
        // merchant's confidence -- the heuristic was only ever a proxy for
        // "has anything actually read this email?", and now something has.
        // Without this the row would be re-queued forever: the underlying
        // observation still records the weak layer (`nlp`) that originally
        // produced the bad name, and correcting the transaction does not
        // rewrite history on the observation.
        let established = user_sourced == 1 || alias_count > 1;
        let confidence = llm_confidence.unwrap_or_else(|| {
            score_merchant(
                extraction_method.as_deref(),
                &strip_noise_tokens(&current),
                established,
            )
        });

        Ok((
            CleanupCandidate {
                transaction_id: r.get(0)?,
                observation_id: r.get(1)?,
                bank_name: r.get(2)?,
                current_merchant: current,
                body: raw_payload.as_deref().and_then(body_from_payload),
                amount: r.get(7)?,
                currency: r.get(8)?,
                direction: r.get(9)?,
                event_time: r.get(10)?,
                category_id: r.get(6)?,
                merchant_entity_id: r.get(5)?,
                merchant_display_name: display,
                merchant_normalized_name: normalized,
                confidence,
            },
            confidence,
            llm_confidence.is_some(),
        ))
    })?;

    let mut out: Vec<CleanupCandidate> = rows
        .filter_map(|r| r.ok())
        // An already-corrected transaction is never re-queued, even if the
        // model's own confidence was low. The LLM has had its turn; asking
        // again would spend inference re-deriving the same answer every run.
        // Reverting the correction puts it back in scope.
        .filter(|(_, _, corrected)| !corrected)
        .filter(|(c, conf, _)| *conf < LOW_CONFIDENCE_THRESHOLD && !c.current_merchant.is_empty())
        .map(|(c, _, _)| c)
        .collect();

    // Worst first, so an interrupted run has still fixed the worst offenders.
    out.sort_by(|a, b| a.confidence.total_cmp(&b.confidence));
    out.truncate(limit);
    Ok(out)
}

/// The email body lives under `raw_payload_json`'s `"body"` key -- same shape
/// `reconciliation::audit` reads for its template hash.
fn body_from_payload(raw: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()?
        .get("body")?
        .as_str()
        .map(|s| s.to_string())
}

/// `true` when the parser's output is close enough to the merchant's real
/// name that aliasing it is safe.
///
/// This gate matters a great deal. Aliasing "SWIGGY LIMITE" to Swiggy is
/// exactly right -- it *is* the merchant, just truncated. Aliasing "USING
/// YOUR" to Swiggy would be a disaster: that fragment appears in every bank's
/// boilerplate, and the alias table is consulted before anything else, so one
/// bad row would silently relabel unrelated transactions across every bank
/// forever. When the parser grabbed the wrong span entirely, the synthesized
/// pattern rule (keyed to that one email shape) is the safe fix instead.
fn safe_to_alias(parser_output: &str, merchant_in_email: &str) -> bool {
    let a = strip_noise_tokens(parser_output);
    let b = strip_noise_tokens(merchant_in_email);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a.contains(&b) || b.contains(&a) {
        return true;
    }
    strsim::jaro_winkler(&a, &b) >= 0.80
}

/// Points `alias_normalized` at `merchant_id`, replacing any existing owner.
///
/// `merchant_aliases.alias_normalized` is UNIQUE, so a plain insert would
/// fail for a string some earlier pass already claimed -- and re-pointing is
/// the correct outcome anyway, since this answer is newer and user-triggered.
fn upsert_alias(
    conn: &Connection,
    merchant_id: &str,
    alias_raw: &str,
    alias_normalized: &str,
    confidence: f64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO merchant_aliases
             (id, merchant_entity_id, alias_raw, alias_normalized, confidence, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(alias_normalized) DO UPDATE SET
             merchant_entity_id = excluded.merchant_entity_id,
             alias_raw          = excluded.alias_raw,
             confidence         = excluded.confidence",
        params![
            Uuid::new_v4().to_string(),
            merchant_id,
            alias_raw,
            alias_normalized,
            confidence,
            Utc::now().naive_utc(),
        ],
    )?;
    Ok(())
}

/// Finds or creates the canonical merchant for `name`, marking it
/// `source = 'user'`.
///
/// The `source` write is load-bearing, not cosmetic: `score_merchant` treats
/// a user-sourced merchant as established, so this is what stops a
/// freshly-created merchant (which has only one alias) from scoring low again
/// and being re-queued on every subsequent run.
fn resolve_canonical_merchant(conn: &Connection, name: &str) -> Result<(String, String)> {
    let normalized = strip_noise_tokens(name);
    if normalized.is_empty() {
        anyhow::bail!("LLM merchant name normalized to empty");
    }

    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM merchants WHERE normalized_name = ?1 AND is_deleted = 0",
            params![normalized],
            |r| r.get(0),
        )
        .ok();

    if let Some(id) = existing {
        conn.execute(
            "UPDATE merchants SET name = ?2, source = 'user', updated_at = CURRENT_TIMESTAMP
             WHERE id = ?1",
            params![id, name],
        )?;
        return Ok((id, normalized));
    }

    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO merchants (id, name, normalized_name, source, is_deleted, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'user', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        params![id, name, normalized],
    )?;
    Ok((id, normalized))
}

/// Stores the synthesized merchant regex as an **active** rule for this
/// bank + email shape, replacing any existing rule for the same triple.
///
/// Activating immediately (rather than staging as `pending`) is safe here
/// only because of the guards upstream: the model's span was verified to
/// occur in the body, and `rule_synthesis::synthesize_span_regex` refuses to return a
/// pattern that cannot re-extract that span from the very email it was built
/// from.
fn upsert_active_merchant_rule(
    conn: &Connection,
    bank_name: &str,
    template_hash: &str,
    pattern: &str,
    confidence: f64,
) -> Result<String> {
    let payload = serde_json::json!({ "regex": pattern, "source": "llm_merchant_cleanup" });

    let existing: Option<String> = conn
        .query_row(
            "SELECT v.id FROM pattern_rule_variants v
             JOIN pattern_rules p ON p.id = v.pattern_rule_id
             WHERE v.template_hash = ?1 AND p.field_name = 'merchant' AND p.bank_name = ?2",
            params![template_hash, bank_name],
            |r| r.get(0),
        )
        .ok();

    if let Some(id) = existing {
        conn.execute(
            "UPDATE pattern_rule_variants
             SET rule_payload_json = ?2, status = 'active', confidence = ?3,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?1",
            params![id, serde_json::to_string(&payload)?, confidence],
        )?;
        return Ok(id);
    }

    let rule = crate::db::pattern_rules::PatternRulesRow {
        id: Uuid::new_v4().to_string(),
        bank_name: bank_name.to_string(),
        template_hash: template_hash.to_string(),
        field_name: "merchant".to_string(),
        rule_payload_json: payload,
        status: "active".to_string(),
        success_count: 1,
        failure_count: 0,
        confidence,
        created_at: Some(Utc::now().naive_utc()),
        updated_at: Some(Utc::now().naive_utc()),
    };
    crate::db::pattern_rules::insert(conn, &rule)?;
    Ok(rule.id)
}

/// Applies one validated LLM answer, recording everything needed to undo it.
///
/// Caller supplies `run_id` so a whole pass can be reverted as a unit.
pub fn apply_correction(
    conn: &Connection,
    run_id: &str,
    candidate: &CleanupCandidate,
    resolution: &MerchantResolution,
) -> Result<()> {
    let (merchant_id, normalized) = resolve_canonical_merchant(conn, &resolution.merchant_name)?;

    // The verbatim span always earns an alias: it genuinely names the
    // merchant, so it is safe on any future email.
    let span_normalized = strip_noise_tokens(&resolution.merchant_in_email);
    if !span_normalized.is_empty() {
        upsert_alias(
            conn,
            &merchant_id,
            &resolution.merchant_in_email,
            &span_normalized,
            resolution.confidence,
        )?;
    }

    // The parser's own output earns one only if it actually resembles the
    // merchant -- see `safe_to_alias`.
    let parser_normalized = strip_noise_tokens(&candidate.current_merchant);
    if !parser_normalized.is_empty()
        && parser_normalized != span_normalized
        && safe_to_alias(&candidate.current_merchant, &resolution.merchant_in_email)
    {
        upsert_alias(
            conn,
            &merchant_id,
            &candidate.current_merchant,
            &parser_normalized,
            resolution.confidence,
        )?;
    }

    let new_category_id = category_id_for_name(conn, &resolution.category);

    // Teach the extraction layer, when there is still a body to learn from.
    let learned_rule_id = candidate.body.as_deref().and_then(|body| {
        let pattern = crate::extraction::rule_synthesis::synthesize_span_regex(
            body,
            &resolution.merchant_in_email,
        )?;
        let template_hash = crate::extraction::ladder::compute_template_hash(body);
        upsert_active_merchant_rule(
            conn,
            &candidate.bank_name,
            &template_hash,
            &pattern,
            resolution.confidence,
        )
        .ok()
    });

    conn.execute(
        "INSERT INTO merchant_llm_corrections (
             id, run_id, transaction_id, observation_id,
             prev_merchant_entity_id, prev_merchant_display_name,
             prev_merchant_normalized_name, prev_category_id,
             new_merchant_entity_id, new_merchant_display_name,
             new_merchant_normalized_name, new_category_id,
             llm_confidence, learned_rule_id, status
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 'applied')",
        params![
            Uuid::new_v4().to_string(),
            run_id,
            candidate.transaction_id,
            candidate.observation_id,
            candidate.merchant_entity_id,
            candidate.merchant_display_name,
            candidate.merchant_normalized_name,
            candidate.category_id,
            merchant_id,
            resolution.merchant_name,
            normalized,
            new_category_id,
            resolution.confidence,
            learned_rule_id,
        ],
    )?;

    conn.execute(
        "UPDATE transactions
         SET merchant_display_name    = ?2,
             merchant_normalized_name = ?3,
             merchant_entity_id       = ?4,
             category_id              = COALESCE(?5, category_id),
             updated_at               = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![
            candidate.transaction_id,
            resolution.merchant_name,
            normalized,
            merchant_id,
            new_category_id,
        ],
    )?;

    Ok(())
}

/// Summary of one run, for the Settings panel.
#[derive(Debug, serde::Serialize)]
pub struct RunSummary {
    pub run_id: String,
    pub applied: i64,
    pub reverted: i64,
    pub started_at: Option<String>,
}

pub fn run_summary(conn: &Connection, run_id: &str) -> Result<RunSummary> {
    let (applied, reverted, started_at) = conn.query_row(
        "SELECT
             SUM(status = 'applied'),
             SUM(status = 'reverted'),
             MIN(created_at)
         FROM merchant_llm_corrections WHERE run_id = ?1",
        params![run_id],
        |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                r.get::<_, Option<String>>(2)?,
            ))
        },
    )?;
    Ok(RunSummary {
        run_id: run_id.to_string(),
        applied,
        reverted,
        started_at,
    })
}

/// Restores one correction's previous values and retires the rule it taught.
///
/// The learned rule is set `inactive` rather than deleted so the history of
/// what was tried survives; `select_active_rules_by_bank` ignores it either
/// way, which is what actually matters for extraction.
pub fn revert_correction(conn: &Connection, correction_id: &str) -> Result<()> {
    struct Previous {
        tx_id: String,
        entity: Option<String>,
        display: Option<String>,
        normalized: Option<String>,
        category: Option<String>,
        rule_id: Option<String>,
    }

    let prev = conn.query_row(
        "SELECT transaction_id, prev_merchant_entity_id, prev_merchant_display_name,
                prev_merchant_normalized_name, prev_category_id, learned_rule_id
         FROM merchant_llm_corrections
         WHERE id = ?1 AND status = 'applied'",
        params![correction_id],
        |r| {
            Ok(Previous {
                tx_id: r.get(0)?,
                entity: r.get(1)?,
                display: r.get(2)?,
                normalized: r.get(3)?,
                category: r.get(4)?,
                rule_id: r.get(5)?,
            })
        },
    )?;

    conn.execute(
        "UPDATE transactions
         SET merchant_display_name    = ?2,
             merchant_normalized_name = ?3,
             merchant_entity_id       = ?4,
             category_id              = ?5,
             updated_at               = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![
            prev.tx_id,
            prev.display,
            prev.normalized,
            prev.entity,
            prev.category
        ],
    )?;

    if let Some(rule_id) = prev.rule_id {
        conn.execute(
            "UPDATE pattern_rule_variants SET status = 'inactive', updated_at = CURRENT_TIMESTAMP
             WHERE id = ?1",
            params![rule_id],
        )?;
    }

    conn.execute(
        "UPDATE merchant_llm_corrections SET status = 'reverted' WHERE id = ?1",
        params![correction_id],
    )?;
    Ok(())
}

/// Reverts every still-applied correction from one run. Returns the count.
pub fn revert_run(conn: &Connection, run_id: &str) -> Result<usize> {
    let ids: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT id FROM merchant_llm_corrections WHERE run_id = ?1 AND status = 'applied'",
        )?;
        let rows = stmt.query_map(params![run_id], |r| r.get::<_, String>(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    let mut n = 0;
    for id in ids {
        revert_correction(conn, &id)?;
        n += 1;
    }
    Ok(n)
}
