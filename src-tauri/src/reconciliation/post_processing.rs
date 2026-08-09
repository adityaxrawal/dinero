use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::Connection;

/// Post-processing to run after a new transaction is reconciled and written to DB.
/// Responsible for:
/// 1. Category enrichment based on simple heuristics.
/// 2. Spend anomaly detection (velocity checks).
#[allow(clippy::too_many_arguments)] // wide-but-flat domain signature; a params struct would add indirection without removing a single field
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
    // 1. Refund and Reversal Linking
    if direction == "credit" {
        // Find a matching debit transaction on the same instrument with the same amount within the last 14 days
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
            // Link them
            conn.execute(
                    "UPDATE transactions SET parent_transaction_id = ?1, transaction_subtype = 'refund', updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
                    rusqlite::params![original_tx_id, transaction_id],
                )?;
            conn.execute(
                    "UPDATE transactions SET status = 'refunded', updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
                    rusqlite::params![original_tx_id],
                )?;
        }

        return Ok(()); // Credits don't trigger spend alerts
    }

    // 2. Merchant Normalization & Category Extraction
    let mut category_id = None;
    let mut final_merchant_entity_id = None;
    let mut final_merchant_normalized_name = None;

    if let Some(merchant) = merchant_raw {
        // Doc 30 TASK-TXN-007: real normalization pipeline (uppercase +
        // noise-token stripping, exact alias match, fuzzy match >= 0.92,
        // else auto-create a new merchant + alias) -- replaces the previous
        // inline LIKE-substring query, which never stripped noise tokens,
        // never fuzzy-matched, and left merchant_entity_id/
        // merchant_normalized_name permanently NULL on no-match instead of
        // auto-creating. Document 18 §4.10 doesn't give `merchants` a
        // `category_id` column (categorization lives only on `transactions`,
        // assigned by the user or by the heuristics below) — merchant
        // resolution here only establishes entity linking, not category.
        if let Ok((entity_id, normalized_name)) =
            crate::extraction::merchant_normalizer::normalize_merchant_sync(conn, merchant)
        {
            if !normalized_name.is_empty() {
                final_merchant_entity_id = Some(entity_id);
                final_merchant_normalized_name = Some(normalized_name);
            }
        }

        // Heuristics fallback if no category resolved from merchants DB.
        // Doc 30 TASK-RT-002 fix: these previously wrote the bare English
        // words "transportation"/"shopping"/"food" -- not any real
        // `categories.id` (the seeded system categories are `cat_transport`/
        // `cat_shopping`/`cat_food`, migration 20260101000002). Nothing
        // enforces a foreign key on `transactions.category_id` so this went
        // unnoticed, but it silently orphaned every heuristically-tagged
        // transaction from its category's real budget row -- a user setting
        // a Transportation budget in Settings could never have it actually
        // fire, since the alert engine looks up the budget by `cat_transport`
        // while these transactions were tagged `transportation`.
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

    // Doc 30 TASK-TXN-011: "After each canonical create/update" -- this is
    // the shared post-processing hook every canonical write already runs
    // through, so recurring detection lives here rather than needing its
    // own separate call site. Re-reads merchant_entity_id fresh (not just
    // `final_merchant_entity_id`) since a transaction resolved on an
    // earlier pass already has it set and this call may not have found a
    // new alias match this time.
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

    // Doc 30 TASK-TXN-012: same "after each canonical create/update" hook,
    // using `merchant_normalized_name` (not `merchant_entity_id` --
    // Document 18 §4.2's `emi_group_id` formula hashes the normalized text,
    // not the entity UUID) re-read fresh for the same reason as above.
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
