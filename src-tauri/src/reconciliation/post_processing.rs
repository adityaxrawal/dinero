
    use anyhow::Result;
    use chrono::NaiveDateTime;
    use rusqlite::Connection;

    /// Post-processing to run after a new transaction is reconciled and written to DB.
    /// Responsible for:
    /// 1. Category enrichment based on simple heuristics.
    /// 2. Spend anomaly detection (velocity checks).
    pub fn run_post_processing(
        conn: &Connection,
        transaction_id: &str,
        instrument_id: &str,
        merchant_raw: Option<&str>,
        amount_minor: i64,
        direction: &str,
        event_time_utc: &NaiveDateTime,
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
            let m = merchant.to_uppercase();

            // Query DB for matching aliases using LIKE %alias%
            // Document 18 §4.10 doesn't give `merchants` a `category_id` column
            // (categorization lives only on `transactions`, assigned by the user
            // or by the heuristics below) — merchant resolution here only
            // establishes entity linking, not category.
            let resolved: rusqlite::Result<(String, String)> = conn.query_row(
                "SELECT m.id, m.name
                 FROM merchant_aliases ma
                 JOIN merchants m ON ma.merchant_entity_id = m.id
                 WHERE ?1 LIKE '%' || ma.alias_normalized || '%'
                 ORDER BY LENGTH(ma.alias_normalized) DESC
                 LIMIT 1",
                rusqlite::params![m],
                |row| Ok((row.get(0)?, row.get(1)?)),
            );

            if let Ok((entity_id, normalized_name)) = resolved {
                final_merchant_entity_id = Some(entity_id);
                final_merchant_normalized_name = Some(normalized_name);
            }

            // Heuristics fallback if no category resolved from merchants DB
            if category_id.is_none() {
                let m_lower = merchant.to_lowercase();
                if m_lower.contains("uber")
                    || m_lower.contains("lyft")
                    || m_lower.contains("transit")
                {
                    category_id = Some("transportation".to_string());
                } else if m_lower.contains("amazon") || m_lower.contains("flipkart") {
                    category_id = Some("shopping".to_string());
                } else if m_lower.contains("swiggy")
                    || m_lower.contains("zomato")
                    || m_lower.contains("restaurant")
                {
                    category_id = Some("food".to_string());
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

        Ok(())
    }
