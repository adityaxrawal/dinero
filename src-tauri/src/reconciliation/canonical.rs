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

    let new_tx = TransactionsRow {
        id: tx_id.clone(),
        unique_event_id: None,
        instrument_id: Some(obs.instrument_id.clone()),
        instrument_type: None,
        direction: Some(obs.direction.clone()),
        amount: Some((obs.amount_minor as f64) / 100.0),
        amount_minor: Some(obs.amount_minor),
        currency: Some(obs.currency.clone()),
        authorization_time: event_time_dt,
        best_event_time: event_time_dt,
        event_time_confidence: Some("high".to_string()),
        best_posting_date: event_time_dt.map(|dt| dt.date()),
        posting_date_confidence: None,
        merchant_display_name: obs.merchant_raw.clone(),
        merchant_normalized_name: None,
        merchant_entity_id: None,
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
        );
    }

    Ok(())
}

/// Called when statement evidence arrives after a canonical transaction already exists from
/// an email observation. Updates the canonical record with statement-preferred fields without
/// overwriting raw source evidence. Logs as 'canonical_updated_with_statement' (Doc 11 §5).
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

    conn.execute(
        "UPDATE transactions SET
            reference_id = COALESCE(?2, reference_id),
            best_posting_date = COALESCE(?3, best_posting_date),
            merchant_display_name = COALESCE(?4, merchant_display_name),
            amount_minor = COALESCE(?5, amount_minor),
            amount = COALESCE(CAST(?5 AS REAL)/100.0, amount),
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

    // Append audit log
    let audit_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO audit_log (id, actor_type, actor_id, action, resource_type, resource_id, created_at)
         VALUES (?1, 'system', 'reconciliation_engine', 'canonical_updated_with_statement', 'transaction', ?2, CURRENT_TIMESTAMP)",
        params![audit_id, canonical_id],
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
    }
    // obs.source_pipeline == "manual", or an email matching an email-sourced
    // canonical: no document specifies a precedence rule for either case, so
    // no field update is made beyond linking below (Doc 30's rule is
    // specifically statement-vs-email).

    update_canonical_transaction_id(conn, &obs.id, Some(matched_id))?;
    Ok(())
}
