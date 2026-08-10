//! Derivations that run once a transaction is settled.
//!
//! Deferred until after reconciliation because they depend on the final merged
//! record: running them per observation would compute against data still subject
//! to being merged away.
use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::Connection;

#[allow(clippy::too_many_arguments)]
/// Runs derivations that depend on the final merged transaction.
///
/// Deferred until after reconciliation, since running them per observation would
/// compute against data still subject to being merged away.
pub fn run_post_processing(
    conn: &Connection,
    transaction_id: &str,
    instrument_id: &str,
    merchant_raw: Option<&str>,
    amount_minor: i64,
    direction: &str,
    event_time_utc: &NaiveDateTime,
    emi_total_installments: Option<i32>,
    emi_original_amount_minor: Option<i64>,
) -> Result<()> {
    if direction == "credit" {
        let matching_debit: rusqlite::Result<String> = conn.query_row(
            "SELECT id FROM transactions 
                 WHERE direction = 'debit' 
                 AND instrument_id = ?3
                 AND amount_minor = ?1 
                 AND is_deleted = 0 
                 AND best_event_time <= ?2 
                 AND best_event_time >= datetime(?2, '-30 days') 
                 AND parent_transaction_id IS NULL 
                 ORDER BY best_event_time DESC LIMIT 1",
            rusqlite::params![
                amount_minor,
                event_time_utc.format("%Y-%m-%d %H:%M:%S").to_string(),
                instrument_id
            ],
            |row| row.get(0),
        );

        if let Ok(original_tx_id) = matching_debit {
            conn.execute(
                    "UPDATE transactions SET parent_transaction_id = ?1, transaction_subtype = 'refund', updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
                    rusqlite::params![original_tx_id, transaction_id],
                )?;
            conn.execute(
                    "UPDATE transactions SET status = 'refunded', updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
                    rusqlite::params![original_tx_id],
                )?;
        }

        return Ok(());
    }

    let mut category_id = None;
    let mut final_merchant_entity_id = None;
    let mut final_merchant_normalized_name = None;

    if let Some(merchant) = merchant_raw {
        if let Ok((entity_id, normalized_name)) =
            crate::extraction::merchant_normalizer::normalize_merchant_sync(conn, merchant)
        {
            if !normalized_name.is_empty() {
                final_merchant_entity_id = Some(entity_id);
                final_merchant_normalized_name = Some(normalized_name);
            }
        }

        if category_id.is_none() {
            let m_lower = merchant.to_lowercase();
            if m_lower.contains("uber") || m_lower.contains("lyft") || m_lower.contains("transit") {
                category_id = Some("cat_transport".to_string());
            } else if m_lower.contains("amazon") || m_lower.contains("flipkart") {
                category_id = Some("cat_shopping".to_string());
            } else if m_lower.contains("swiggy")
                || m_lower.contains("zomato")
                || m_lower.contains("restaurant")
            {
                category_id = Some("cat_food".to_string());
            }
        }
    }

    if category_id.is_some() || final_merchant_entity_id.is_some() {
        conn.execute(
            "UPDATE transactions
                 SET category_id = COALESCE(?2, category_id),
                     merchant_entity_id = COALESCE(?3, merchant_entity_id),
                     merchant_normalized_name = COALESCE(?4, merchant_normalized_name),
                     updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?1",
            rusqlite::params![
                transaction_id,
                category_id,
                final_merchant_entity_id,
                final_merchant_normalized_name
            ],
        )?;
    }

    let current_merchant_entity_id: Option<String> = conn
        .query_row(
            "SELECT merchant_entity_id FROM transactions WHERE id = ?1",
            rusqlite::params![transaction_id],
            |row| row.get(0),
        )
        .unwrap_or(None);
    let _ = crate::extraction::recurring_detector::detect_and_update_recurring(
        conn,
        instrument_id,
        current_merchant_entity_id.as_deref(),
        transaction_id,
        amount_minor,
        direction,
        *event_time_utc,
    );

    let current_merchant_normalized: Option<String> = conn
        .query_row(
            "SELECT merchant_normalized_name FROM transactions WHERE id = ?1",
            rusqlite::params![transaction_id],
            |row| row.get(0),
        )
        .unwrap_or(None);
    let _ = crate::extraction::emi_detector::link_emi_installment(
        conn,
        transaction_id,
        instrument_id,
        current_merchant_normalized.as_deref(),
        emi_total_installments,
        emi_original_amount_minor,
    );

    Ok(())
}
