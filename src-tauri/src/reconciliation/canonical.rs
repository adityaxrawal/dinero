use crate::db::transaction_observations::update_canonical_transaction_id;
use crate::db::transactions::{insert_transaction, TransactionsRow};
use crate::reconciliation::engine::IncomingObservation;
use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection};
use uuid::Uuid;

/// Doc 18 §4.3's `source_mix` enum is `email_only` / `statement_only` /
/// `merged` / `manual` -- distinct from the raw `source_pipeline` values
/// (`gmail_transaction` / `statement_pdf` / `manual`) an observation itself
/// carries. Maps the latter to the former for a brand-new canonical row.
fn source_mix_for_new_canonical(source_pipeline: &str) -> &'static str {
    match source_pipeline {
        "gmail_transaction" => "email_only",
        "statement_pdf" => "statement_only",
        _ => "manual",
    }
}

/// Creates a new canonical transaction from an incoming observation that had no matching
/// candidate. Uses normalized fields from the observation as specified in Doc 11 §8.
///
/// Field precedence when both Gmail and statement arrive later (Doc 11 §5):
///  - reference_id      → Statement preferred
///  - posting_date      → Statement preferred
///  - merchant display  → Statement preferred
///  - settled amount    → Statement preferred
///  - event_time        → Email preferred (authorization-time context)
///  - balance_after     → Email preferred
pub fn create_canonical_transaction(conn: &Connection, obs: &IncomingObservation) -> Result<()> {
    let tx_id = Uuid::new_v4().to_string();

    // Parse event_time if possible
    let fmt = "%Y-%m-%d %H:%M:%S";
    let event_time_dt = NaiveDateTime::parse_from_str(&obs.event_time, fmt)
        .or_else(|_| NaiveDateTime::parse_from_str(&format!("{} 00:00:00", obs.event_time), fmt))
        .ok();

    // Doc 30 TASK-TXN-007: this call site previously stored `merchant_raw`
    // verbatim as `merchant_display_name` and left `merchant_normalized_name`/
    // `merchant_entity_id` permanently NULL -- meaning the plausibility-gated
    // normalization pipeline (`merchant_normalizer::normalize_merchant_sync`)
    // never ran on a brand-new transaction at all, so a mis-extracted
    // boilerplate fragment (e.g. "inform you that") landed straight on the
    // transaction with no check whatsoever. `merchant_display_name` still
    // shows the raw string either way (Doc 11 §8 wants the human-readable
    // original), but `normalized_name`/`entity_id` are now only populated
    // when the string clears the same plausibility gate step 3 of
    // `normalize_merchant_sync` applies to auto-created merchants.
    let (merchant_entity_id, merchant_normalized_name) = match &obs.merchant_raw {
        Some(raw) => match crate::extraction::merchant_normalizer::normalize_merchant_sync(conn, raw) {
            Ok((entity_id, normalized)) if !normalized.is_empty() => {
                (Some(entity_id), Some(normalized))
            }
            _ => (None, None),
        },
        None => (None, None),
    };

    let new_tx = TransactionsRow {
        id: tx_id.clone(),
        unique_event_id: None,
        instrument_id: if obs.instrument_id == "unknown" { None } else { Some(obs.instrument_id.clone()) },
        instrument_type: None,
        direction: Some(obs.direction.clone()),
        // Generated column (audit_05 #4) -- SQLite derives it from
        // `amount_minor`, so anything set here would be ignored.
        amount: None,
        amount_minor: Some(obs.amount_minor),
        currency: Some(obs.currency.clone()),
        authorization_time: event_time_dt,
        best_event_time: event_time_dt,
        // Default "high" for the common case (unambiguous parse); an
        // observation flagged by `apply_date_cross_check`
        // (extraction::ladder) -- "swapped_by_anchor" or
        // "anchor_mismatch_needs_review" -- carries that instead, so the
        // UI (TransactionInspector/TransactionDetail "Time Confidence")
        // reflects the real uncertainty instead of always claiming "high".
        event_time_confidence: obs
            .event_time_confidence
            .clone()
            .or_else(|| Some("high".to_string())),
        best_posting_date: event_time_dt.map(|dt| dt.date()),
        posting_date_confidence: None,
        merchant_display_name: obs.merchant_raw.clone(),
        merchant_normalized_name,
        merchant_entity_id,
        reference_id: obs.reference_id.clone(),
        location: None,
        original_amount_minor: None,
        original_currency: None,
        exchange_rate: None,
        balance_after_transaction: None,
        // Doc 30 TASK-TXN-010: "status = 'confirmed'" -- but Document 18 §4.3
        // (schema-authoritative, Doc 49 §6) has no 'confirmed' value at all;
        // its actual enum is posted/pending/pending_fx/reversed/refunded/
        // declined. 'posted' is the correct value for a normal, successfully
        // processed transaction -- the prior 'canonical' value matched
        // neither document and had no CHECK constraint to catch it.
        status: Some("posted".to_string()),
        match_confidence: None,
        source_mix: Some(source_mix_for_new_canonical(&obs.source_pipeline).to_string()),
        alert_fired: Some(false),
        parent_transaction_id: None,
        transaction_subtype: None,
        emi_group_id: None,
        category_id: None,
        channel: obs.channel.clone(),
        notes: None,
        is_deleted: false,
        created_at: None,
        updated_at: None,
    };

    insert_transaction(conn, &new_tx)?;
    update_canonical_transaction_id(conn, &obs.id, Some(&tx_id))?;

    if let Some(dt) = event_time_dt {
        let _ = crate::reconciliation::post_processing::run_post_processing(
            conn,
            &tx_id,
            &obs.instrument_id,
            obs.merchant_raw.as_deref(),
            obs.amount_minor,
            &obs.direction,
            &dt,
            obs.emi_total_installments,
            obs.emi_original_amount_minor,
        );
    }

    Ok(())
}

/// Called when statement evidence arrives after a canonical transaction already exists from
/// an email observation. Updates the canonical record with statement-preferred fields without
/// overwriting raw source evidence. Logs as 'canonical_field_overwritten_by_statement'
/// with before/after values (Doc 30 TASK-DEDUP-008).
pub fn update_canonical_with_statement(
    conn: &Connection,
    canonical_id: &str,
    reference_id: Option<&str>,
    posting_date: Option<&str>,
    merchant_display: Option<&str>,
    settled_amount_minor: Option<i64>,
) -> Result<()> {
    // We update fields if they are provided, leaving existing ones if None
    // In SQLite, we can dynamically build or just update what's Some.
    // For simplicity here, we'll update reference_id, posting_date, merchant, amount_minor if provided
    //
    // Real bug fixed here: every real call site passes `obs.event_time`,
    // which is a full "YYYY-MM-DD HH:MM:SS" datetime, not a bare date -- the
    // original bare-`%Y-%m-%d`-only parse always failed on real input,
    // silently leaving `posting_date_parsed` `None` forever, so
    // `best_posting_date` (Doc 30 TASK-TXN-010: "postingDate → Statement
    // preferred") never actually updated via this path in production.
    let posting_date_parsed = posting_date.and_then(|pd| {
        chrono::NaiveDateTime::parse_from_str(pd, "%Y-%m-%d %H:%M:%S")
            .map(|dt| dt.date())
            .or_else(|_| chrono::NaiveDate::parse_from_str(pd, "%Y-%m-%d"))
            .ok()
            .map(|d| d.format("%Y-%m-%d").to_string())
    });

    // Doc 30 TASK-DEDUP-008: "every precedence-driven overwrite is logged to
    // audit_log... with before/after values for full auditability" --
    // snapshot the fields this UPDATE can change before it runs.
    let before: Option<(Option<String>, Option<String>, Option<String>, Option<i64>)> = conn
        .query_row(
            "SELECT reference_id, best_posting_date, merchant_display_name, amount_minor FROM transactions WHERE id = ?1",
            params![canonical_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .ok();

    conn.execute(
        "UPDATE transactions SET
            reference_id = COALESCE(?2, reference_id),
            best_posting_date = COALESCE(?3, best_posting_date),
            merchant_display_name = COALESCE(?4, merchant_display_name),
            amount_minor = COALESCE(?5, amount_minor),
            -- audit_05 #4: `amount` used to be hand-synced here as
            -- `COALESCE(CAST(?5 AS REAL)/100.0, amount)`. It is a generated
            -- column as of migration 058, so SQLite derives it from
            -- `amount_minor` and writing it is now an error.
            -- Doc 18 §4.3: 'email_only' becomes 'merged' once statement
            -- evidence arrives too; an already-'statement_only'/'merged' row
            -- stays as-is (this is itself still statement evidence, not a
            -- new source).
            source_mix = CASE WHEN source_mix = 'email_only' THEN 'merged' ELSE source_mix END,
            updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![
            canonical_id,
            reference_id,
            posting_date_parsed,
            merchant_display,
            settled_amount_minor
        ],
    )?;

    // Doc 30 TASK-DEDUP-008: named exactly `canonical_field_overwritten_by_statement`.
    let audit_id = Uuid::new_v4().to_string();
    let (before_ref, before_posting, before_merchant, before_amount) =
        before.unwrap_or((None, None, None, None));
    conn.execute(
        "INSERT INTO audit_log (id, actor_type, actor_id, action, resource_type, resource_id, before_json, after_json, created_at)
         VALUES (?1, 'system', 'reconciliation_engine', 'canonical_field_overwritten_by_statement', 'transaction', ?2, ?3, ?4, CURRENT_TIMESTAMP)",
        params![
            audit_id,
            canonical_id,
            serde_json::json!({
                "reference_id": before_ref,
                "best_posting_date": before_posting,
                "merchant_display_name": before_merchant,
                "amount_minor": before_amount,
            })
            .to_string(),
            serde_json::json!({
                "reference_id": reference_id,
                "best_posting_date": posting_date_parsed,
                "merchant_display_name": merchant_display,
                "amount_minor": settled_amount_minor,
            })
            .to_string(),
        ],
    )?;

    Ok(())
}

/// Doc 30 TASK-TXN-010: "an email observation on an already-statement-sourced
/// canonical row only fills currently-`NULL` fields, never overwrites."
/// The mirror case of [`update_canonical_with_statement`] — only called when
/// the matched canonical's `source_mix` is already `statement_only`/`merged`;
/// callers must check that before calling this (this function itself doesn't
/// re-check, to avoid a redundant read here).
pub fn fill_null_fields_from_email(
    conn: &Connection,
    canonical_id: &str,
    reference_id: Option<&str>,
    merchant_display: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE transactions SET
            reference_id = COALESCE(reference_id, ?2),
            merchant_display_name = COALESCE(merchant_display_name, ?3),
            source_mix = CASE WHEN source_mix = 'statement_only' THEN 'merged' ELSE source_mix END,
            updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![canonical_id, reference_id, merchant_display],
    )?;

    let audit_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO audit_log (id, actor_type, actor_id, action, resource_type, resource_id, created_at)
         VALUES (?1, 'system', 'reconciliation_engine', 'canonical_null_fields_filled_from_email', 'transaction', ?2, CURRENT_TIMESTAMP)",
        params![audit_id, canonical_id],
    )?;

    Ok(())
}

/// Doc 30 TASK-DEDUP-008: "both email (e.g. two connected accounts alerting
/// on the same transaction) -> standard scoring match, first-arriving data
/// generally retained unless the second has materially higher-confidence
/// fields." How much higher `obs.confidence_score` must be than the
/// already-linked email observation's own confidence before the newer
/// arrival is allowed to overwrite it.
const EMAIL_VS_EMAIL_CONFIDENCE_MARGIN: f64 = 0.15;

/// Doc 30 TASK-DEDUP-008 email-vs-email precedence path. Only called when
/// the matched canonical's `source_mix` is `email_only` (both sides are
/// email evidence, no statement involved anywhere) — compares the incoming
/// observation's extraction confidence against the already-linked email
/// observation's; overwrites merchant/reference fields only if the new one
/// is materially more confident.
fn maybe_overwrite_from_higher_confidence_email(
    conn: &Connection,
    canonical_id: &str,
    obs: &IncomingObservation,
) -> Result<()> {
    let Some(new_confidence) = obs.confidence_score else {
        return Ok(()); // No confidence signal on the new observation -- can't compare, keep first-arriving.
    };

    // Doc 30 TASK-DEDUP-008: compare against the observation that actually
    // last *won* the field values, not just the oldest linked email. The
    // oldest-linked row can go stale: if obs1 (confidence 0.5) sets the
    // fields, obs2 (0.9) later materially overwrites them, and obs3 (0.92)
    // arrives after that, comparing against obs1's stale 0.5 wrongly
    // triggers another overwrite (large margin) instead of comparing
    // against obs2's current 0.9 (small margin, correctly not "materially
    // higher"). The latest `auto_matched_exact`/`auto_matched_scored`/
    // `manually_confirmed` `match_decisions` row for this canonical
    // identifies whichever observation most recently won; if none exists
    // yet (no overwrite has ever happened), fall back to the founding
    // (oldest-linked) email observation, which is the only correct
    // reference point at that point in the canonical's history anyway.
    let existing_confidence: Option<f64> = conn
        .query_row(
            "SELECT o.confidence_score
             FROM match_decisions md
             JOIN transaction_observations o ON o.id = md.observation_id
             WHERE md.matched_transaction_id = ?1
               AND md.decision IN ('auto_matched_exact', 'auto_matched_scored', 'manually_confirmed')
               AND o.source_pipeline = 'gmail_transaction' AND o.id != ?2
             ORDER BY md.created_at DESC LIMIT 1",
            params![canonical_id, obs.id],
            |row| row.get(0),
        )
        .ok()
        .flatten()
        .or_else(|| {
            conn.query_row(
                "SELECT confidence_score FROM transaction_observations \
                 WHERE canonical_transaction_id = ?1 AND source_pipeline = 'gmail_transaction' AND id != ?2 \
                 ORDER BY created_at ASC LIMIT 1",
                params![canonical_id, obs.id],
                |row| row.get(0),
            )
            .ok()
            .flatten()
        });

    let Some(existing_confidence) = existing_confidence else {
        return Ok(()); // Nothing to compare against -- keep first-arriving.
    };

    if new_confidence - existing_confidence <= EMAIL_VS_EMAIL_CONFIDENCE_MARGIN {
        return Ok(()); // Not materially higher -- first-arriving data is retained.
    }

    let before: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT reference_id, merchant_display_name FROM transactions WHERE id = ?1",
            params![canonical_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();
    let (before_ref, before_merchant) = before.unwrap_or((None, None));

    conn.execute(
        "UPDATE transactions SET
            reference_id = COALESCE(?2, reference_id),
            merchant_display_name = COALESCE(?3, merchant_display_name),
            updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![canonical_id, obs.reference_id, obs.merchant_raw],
    )?;

    let audit_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO audit_log (id, actor_type, actor_id, action, resource_type, resource_id, before_json, after_json, created_at)
         VALUES (?1, 'system', 'reconciliation_engine', 'canonical_field_overwritten_by_higher_confidence_email', 'transaction', ?2, ?3, ?4, CURRENT_TIMESTAMP)",
        params![
            audit_id,
            canonical_id,
            serde_json::json!({ "reference_id": before_ref, "merchant_display_name": before_merchant }).to_string(),
            serde_json::json!({ "reference_id": obs.reference_id, "merchant_display_name": obs.merchant_raw }).to_string(),
        ],
    )?;

    Ok(())
}

/// Doc 30 TASK-TXN-010: shared entry point for both `AutoMatchedExact` and
/// `AutoMatchedScored` decisions — "update the existing row per the
/// statement-overrides-email precedence rule... [and] populate
/// `transaction_observations.canonical_transaction_id` for full
/// traceability." Applies regardless of which field-update branch (if any)
/// fires, since traceability must hold for every matched decision, not just
/// the ones that also touch a field.
pub fn apply_match_precedence_and_link(
    conn: &Connection,
    obs: &IncomingObservation,
    matched_id: &str,
    matched_source_mix: Option<&str>,
) -> Result<()> {
    if obs.source_pipeline == "statement_pdf" {
        update_canonical_with_statement(
            conn,
            matched_id,
            obs.reference_id.as_deref(),
            Some(&obs.event_time),
            obs.merchant_raw.as_deref(),
            Some(obs.amount_minor),
        )?;
    } else if obs.source_pipeline == "gmail_transaction"
        && matches!(matched_source_mix, Some("statement_only") | Some("merged"))
    {
        fill_null_fields_from_email(
            conn,
            matched_id,
            obs.reference_id.as_deref(),
            obs.merchant_raw.as_deref(),
        )?;
    } else if obs.source_pipeline == "gmail_transaction" && matched_source_mix == Some("email_only")
    {
        maybe_overwrite_from_higher_confidence_email(conn, matched_id, obs)?;
    }
    // obs.source_pipeline == "manual": no document specifies a precedence
    // rule for a manual entry matching an existing canonical, so no field
    // update is made beyond linking below.

    update_canonical_transaction_id(conn, &obs.id, Some(matched_id))?;

    // Doc 30 TASK-TXN-011/012: "after each canonical create/update" --
    // matched decisions are an update, not just a create, so post-processing
    // (which now also runs recurring-payment detection) must run here too,
    // not just from `create_canonical_transaction`.
    let fmt = "%Y-%m-%d %H:%M:%S";
    if let Ok(event_time_dt) = NaiveDateTime::parse_from_str(&obs.event_time, fmt)
        .or_else(|_| NaiveDateTime::parse_from_str(&format!("{} 00:00:00", obs.event_time), fmt))
    {
        let _ = crate::reconciliation::post_processing::run_post_processing(
            conn,
            matched_id,
            &obs.instrument_id,
            obs.merchant_raw.as_deref(),
            obs.amount_minor,
            &obs.direction,
            &event_time_dt,
            obs.emi_total_installments,
            obs.emi_original_amount_minor,
        );
    }

    Ok(())
}
