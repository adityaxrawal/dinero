//! Creates and updates the canonical transaction behind observations.
//!
//! Sources have different strengths -- an alert arrives immediately but sparsely,
//! a statement arrives late but authoritative -- so merging follows a precedence
//! rule rather than last-write-wins, and null fields are backfilled from whatever
//! source can supply them.
use crate::db::transaction_observations::update_canonical_transaction_id;
use crate::db::transactions::{insert_transaction, TransactionsRow};
use crate::reconciliation::engine::IncomingObservation;
use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection};
use uuid::Uuid;

type CanonicalAuditSnapshot = (Option<String>, Option<String>, Option<String>, Option<i64>);

/// The initial source mix for a canonical built from one pipeline.
fn source_mix_for_new_canonical(source_pipeline: &str) -> &'static str {
    match source_pipeline {
        "gmail_transaction" => "email_only",
        "statement_pdf" => "statement_only",
        _ => "manual",
    }
}

/// Creates a canonical transaction from an incoming observation.
pub fn create_canonical_transaction(conn: &Connection, obs: &IncomingObservation) -> Result<()> {
    let tx_id = Uuid::new_v4().to_string();

    let fmt = "%Y-%m-%d %H:%M:%S";
    let event_time_dt = NaiveDateTime::parse_from_str(&obs.event_time, fmt)
        .or_else(|_| NaiveDateTime::parse_from_str(&format!("{} 00:00:00", obs.event_time), fmt))
        .ok();

    let (merchant_entity_id, merchant_normalized_name) = match &obs.merchant_raw {
        Some(raw) => {
            match crate::extraction::merchant_normalizer::normalize_merchant_sync(conn, raw) {
                Ok((entity_id, normalized)) if !normalized.is_empty() => {
                    (Some(entity_id), Some(normalized))
                }
                _ => (None, None),
            }
        }
        None => (None, None),
    };

    let new_tx = TransactionsRow {
        id: tx_id.clone(),
        unique_event_id: None,
        instrument_id: if obs.instrument_id == "unknown" {
            None
        } else {
            Some(obs.instrument_id.clone())
        },
        instrument_type: None,
        direction: Some(obs.direction.clone()),
        amount: None,
        amount_minor: Some(obs.amount_minor),
        currency: Some(obs.currency.clone()),
        authorization_time: event_time_dt,
        best_event_time: event_time_dt,
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

/// Merges statement data into an existing canonical transaction.
///
/// Statements arrive later but are authoritative, so they can correct values an
/// earlier alert only approximated.
pub fn update_canonical_with_statement(
    conn: &Connection,
    canonical_id: &str,
    reference_id: Option<&str>,
    posting_date: Option<&str>,
    merchant_display: Option<&str>,
    settled_amount_minor: Option<i64>,
) -> Result<()> {
    let posting_date_parsed = posting_date.and_then(|pd| {
        chrono::NaiveDateTime::parse_from_str(pd, "%Y-%m-%d %H:%M:%S")
            .map(|dt| dt.date())
            .or_else(|_| chrono::NaiveDate::parse_from_str(pd, "%Y-%m-%d"))
            .ok()
            .map(|d| d.format("%Y-%m-%d").to_string())
    });

    let before: Option<CanonicalAuditSnapshot> = conn
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

/// Backfills null fields from an email observation.
///
/// Only fills gaps -- it never overwrites a value already present, so a later
/// weaker source cannot degrade what a stronger one established.
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

const EMAIL_VS_EMAIL_CONFIDENCE_MARGIN: f64 = 0.15;

/// Overwrites a field when the email source is more confident.
///
/// The exception to fill-only merging, and deliberately narrow: precedence is
/// checked explicitly rather than letting the most recent write win.
fn maybe_overwrite_from_higher_confidence_email(
    conn: &Connection,
    canonical_id: &str,
    obs: &IncomingObservation,
) -> Result<()> {
    let Some(new_confidence) = obs.confidence_score else {
        return Ok(());
    };

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
        return Ok(());
    };

    if new_confidence - existing_confidence <= EMAIL_VS_EMAIL_CONFIDENCE_MARGIN {
        return Ok(());
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

/// Applies match precedence and links the observation to its canonical.
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

    update_canonical_transaction_id(conn, &obs.id, Some(matched_id))?;

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
