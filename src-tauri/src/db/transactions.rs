use anyhow::Result;
use chrono::{Datelike, NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TransactionsRow {
    pub id: String,
    pub unique_event_id: Option<String>,
    pub instrument_id: Option<String>,
    pub instrument_type: Option<String>,
    pub direction: Option<String>,
    pub amount: Option<f64>,
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub authorization_time: Option<NaiveDateTime>,
    pub best_event_time: Option<NaiveDateTime>,
    pub event_time_confidence: Option<String>,
    pub best_posting_date: Option<NaiveDate>,
    pub posting_date_confidence: Option<String>,
    pub merchant_display_name: Option<String>,
    pub merchant_normalized_name: Option<String>,
    pub merchant_entity_id: Option<String>,
    pub reference_id: Option<String>,
    pub location: Option<String>,
    pub original_amount_minor: Option<i64>,
    pub original_currency: Option<String>,
    pub exchange_rate: Option<f64>,
    pub balance_after_transaction: Option<f64>,
    pub status: Option<String>,
    pub match_confidence: Option<String>,
    pub source_mix: Option<String>,
    pub alert_fired: Option<bool>,
    pub parent_transaction_id: Option<String>,
    pub transaction_subtype: Option<String>,
    pub emi_group_id: Option<String>,
    pub category_id: Option<String>,
    /// Display-only transaction rail/channel (`"upi"`, `"imps"`, ...) --
    /// see `extraction::ladder::detect_channel`. Distinct from
    /// `transaction_subtype` (refund/emi_installment classification):
    /// a refund can itself be UPI or IMPS, so the two are independent tags.
    pub channel: Option<String>,
    pub is_deleted: bool,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
    /// Document 19 §8.3 editable field; added by migration 038 -- Document
    /// 18 §4.3's schema never had this column despite Document 19 already
    /// documenting it as editable (Aditya's decision, 2026-07-16).
    pub notes: Option<String>,
}

pub fn insert_transaction(conn: &Connection, tx: &TransactionsRow) -> Result<()> {
    conn.execute(
        "INSERT INTO transactions (
            id, unique_event_id, instrument_id, instrument_type, direction, amount, amount_minor,
            currency, authorization_time, best_event_time, event_time_confidence, best_posting_date,
            posting_date_confidence, merchant_display_name, merchant_normalized_name, merchant_entity_id,
            reference_id, location, original_amount_minor, original_currency, exchange_rate,
            balance_after_transaction, status, match_confidence, source_mix, alert_fired,
            parent_transaction_id, transaction_subtype, emi_group_id, category_id, channel, is_deleted,
            created_at, updated_at, notes
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
            ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, COALESCE(?33, CURRENT_TIMESTAMP), COALESCE(?34, CURRENT_TIMESTAMP), ?35
         )",
        params![
            tx.id, tx.unique_event_id, tx.instrument_id, tx.instrument_type, tx.direction, tx.amount,
            tx.amount_minor, tx.currency, tx.authorization_time, tx.best_event_time, tx.event_time_confidence,
            tx.best_posting_date, tx.posting_date_confidence, tx.merchant_display_name, tx.merchant_normalized_name,
            tx.merchant_entity_id, tx.reference_id, tx.location, tx.original_amount_minor, tx.original_currency,
            tx.exchange_rate, tx.balance_after_transaction, tx.status, tx.match_confidence, tx.source_mix,
            tx.alert_fired, tx.parent_transaction_id, tx.transaction_subtype, tx.emi_group_id, tx.category_id,
            tx.channel, tx.is_deleted, tx.created_at, tx.updated_at, tx.notes
        ],
    )?;
    Ok(())
}

pub fn update_transaction(conn: &Connection, tx: &TransactionsRow) -> Result<()> {
    let count = conn.execute(
        "UPDATE transactions SET
            unique_event_id = ?2, instrument_id = ?3, instrument_type = ?4, direction = ?5, amount = ?6,
            amount_minor = ?7, currency = ?8, authorization_time = ?9, best_event_time = ?10,
            event_time_confidence = ?11, best_posting_date = ?12, posting_date_confidence = ?13,
            merchant_display_name = ?14, merchant_normalized_name = ?15, merchant_entity_id = ?16,
            reference_id = ?17, location = ?18, original_amount_minor = ?19, original_currency = ?20,
            exchange_rate = ?21, balance_after_transaction = ?22, status = ?23, match_confidence = ?24,
            source_mix = ?25, alert_fired = ?26, parent_transaction_id = ?27, transaction_subtype = ?28,
            emi_group_id = ?29, category_id = ?30, channel = ?31, is_deleted = ?32, notes = ?33
         WHERE id = ?1",
        params![
            tx.id, tx.unique_event_id, tx.instrument_id, tx.instrument_type, tx.direction, tx.amount,
            tx.amount_minor, tx.currency, tx.authorization_time, tx.best_event_time, tx.event_time_confidence,
            tx.best_posting_date, tx.posting_date_confidence, tx.merchant_display_name, tx.merchant_normalized_name,
            tx.merchant_entity_id, tx.reference_id, tx.location, tx.original_amount_minor, tx.original_currency,
            tx.exchange_rate, tx.balance_after_transaction, tx.status, tx.match_confidence, tx.source_mix,
            tx.alert_fired, tx.parent_transaction_id, tx.transaction_subtype, tx.emi_group_id, tx.category_id,
            tx.channel, tx.is_deleted, tx.notes
        ],
    )?;
    if count == 0 {
        return Err(anyhow::anyhow!("Transaction not found"));
    }
    Ok(())
}

pub fn get_transaction(conn: &Connection, id: &str) -> Result<Option<TransactionsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM transactions WHERE id = ?1 AND is_deleted = 0")?;
    let mut rows = stmt.query([id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_transaction(row)?))
    } else {
        Ok(None)
    }
}

pub fn delete_transaction(conn: &Connection, id: &str) -> Result<()> {
    // Soft delete
    let count = conn.execute(
        "UPDATE transactions SET is_deleted = 1 WHERE id = ?1 AND is_deleted = 0",
        params![id],
    )?;
    if count == 0 {
        return Err(anyhow::anyhow!("Transaction not found"));
    }
    Ok(())
}

pub fn get_paginated_transactions(
    conn: &Connection,
    limit: i64,
    offset: i64,
) -> Result<Vec<TransactionsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM transactions WHERE is_deleted = 0 ORDER BY best_event_time DESC LIMIT ?1 OFFSET ?2")?;
    let rows = stmt.query_map(params![limit, offset], row_to_transaction)?;

    let mut transactions = Vec::new();
    for row in rows {
        transactions.push(row?);
    }
    Ok(transactions)
}

pub fn search_transactions(
    conn: &Connection,
    query: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<TransactionsRow>> {
    search_transactions_with_filters(conn, query, None, limit, offset)
}

pub fn search_transactions_with_filters(
    conn: &Connection,
    query: &str,
    filters: Option<&crate::commands::data::TransactionListFilters>,
    limit: i64,
    offset: i64,
) -> Result<Vec<TransactionsRow>> {
    let trimmed = query.trim();
    let like_pattern = format!("%{trimmed}%");

    // Clean numeric extraction (remove currency symbols like ₹, $, commas, etc.)
    let clean_num_str: String = trimmed
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();

    let (has_num, num_val, minor_val) = match clean_num_str.parse::<f64>() {
        Ok(val) if !clean_num_str.is_empty() && clean_num_str != "-" => {
            let abs_val = val.abs();
            let minor = (abs_val * 100.0).round() as i64;
            (1i64, abs_val, minor)
        }
        _ => (0i64, 0.0f64, 0i64),
    };

    // Sanitize FTS5 query string to avoid syntax errors on special characters like &, /, :, ₹, $, etc.
    let clean_fts: String = trimmed
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    let fts_query = if clean_fts.trim().is_empty() {
        String::new()
    } else {
        format!("\"{}\"*", clean_fts.trim())
    };

    let mut sql = String::from(
        "SELECT DISTINCT t.*
         FROM transactions t
         LEFT JOIN instruments i ON t.instrument_id = i.id
         LEFT JOIN categories c ON t.category_id = c.id
         LEFT JOIN transaction_tags tt ON t.id = tt.transaction_id
         LEFT JOIN tags tag ON tt.tag_id = tag.id
         WHERE t.is_deleted = 0
           AND (
             (?1 != '' AND t.id IN (SELECT id FROM transactions_fts WHERE transactions_fts MATCH ?1))
             OR (t.merchant_display_name LIKE ?2)
             OR (t.merchant_normalized_name LIKE ?2)
             OR (t.reference_id LIKE ?2)
             OR (t.location LIKE ?2)
             OR (t.notes LIKE ?2)
             OR (t.status LIKE ?2)
             OR (t.transaction_subtype LIKE ?2)
             OR (t.direction LIKE ?2)
             OR (c.name LIKE ?2)
             OR (c.id LIKE ?2)
             OR (IFNULL(c.name, 'Uncategorized') LIKE ?2)
             OR (i.issuer_name LIKE ?2)
             OR (i.nickname LIKE ?2)
             OR (i.masked_identifier LIKE ?2)
             OR (i.type LIKE ?2)
             OR (i.upi_vpa LIKE ?2)
             OR (tag.name LIKE ?2)
             OR (CAST(t.amount AS TEXT) LIKE ?2)
             OR (?3 = 1 AND (ABS(t.amount) = ?4 OR ABS(t.amount_minor) = ?5 OR CAST(t.amount AS TEXT) LIKE ?6))
           )"
    );

    let num_like_pattern = format!("%{clean_num_str}%");

    let mut query_params: Vec<Box<dyn rusqlite::ToSql>> = vec![
        Box::new(fts_query),
        Box::new(like_pattern),
        Box::new(has_num),
        Box::new(num_val),
        Box::new(minor_val),
        Box::new(num_like_pattern),
    ];

    if let Some(f) = filters {
        if let Some(from) = &f.from_date {
            sql.push_str(" AND t.authorization_time >= ?");
            query_params.push(Box::new(format!("{from} 00:00:00")));
        }
        if let Some(to) = &f.to_date {
            sql.push_str(" AND t.authorization_time <= ?");
            query_params.push(Box::new(format!("{to} 23:59:59")));
        }
        if let Some(instrument_id) = &f.instrument_id {
            sql.push_str(" AND t.instrument_id = ?");
            query_params.push(Box::new(instrument_id.clone()));
        }
        if let Some(direction) = &f.direction {
            sql.push_str(" AND t.direction = ?");
            query_params.push(Box::new(direction.clone()));
        }
        if let Some(category_id) = &f.category_id {
            sql.push_str(" AND t.category_id = ?");
            query_params.push(Box::new(category_id.clone()));
        }
        if let Some(status) = &f.status {
            sql.push_str(" AND t.status = ?");
            query_params.push(Box::new(status.clone()));
        }
    }

    sql.push_str(" ORDER BY t.authorization_time DESC LIMIT ? OFFSET ?");
    query_params.push(Box::new(limit));
    query_params.push(Box::new(offset));

    let mut stmt = conn.prepare(&sql)?;
    let slice_params: Vec<&dyn rusqlite::ToSql> = query_params.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(slice_params.as_slice(), row_to_transaction)?;

    let mut transactions = Vec::new();
    for row in rows {
        transactions.push(row?);
    }

    Ok(transactions)
}

fn row_to_transaction(row: &Row) -> rusqlite::Result<TransactionsRow> {
    Ok(TransactionsRow {
        id: row.get("id")?,
        unique_event_id: row.get("unique_event_id")?,
        instrument_id: row.get("instrument_id")?,
        instrument_type: row.get("instrument_type")?,
        direction: row.get("direction")?,
        amount: row.get("amount")?,
        amount_minor: row.get("amount_minor")?,
        currency: row.get("currency")?,
        authorization_time: row.get("authorization_time")?,
        best_event_time: row.get("best_event_time")?,
        event_time_confidence: row.get("event_time_confidence")?,
        best_posting_date: row.get("best_posting_date")?,
        posting_date_confidence: row.get("posting_date_confidence")?,
        merchant_display_name: row.get("merchant_display_name")?,
        merchant_normalized_name: row.get("merchant_normalized_name")?,
        merchant_entity_id: row.get("merchant_entity_id")?,
        reference_id: row.get("reference_id")?,
        location: row.get("location")?,
        original_amount_minor: row.get("original_amount_minor")?,
        original_currency: row.get("original_currency")?,
        exchange_rate: row.get("exchange_rate")?,
        balance_after_transaction: row.get("balance_after_transaction")?,
        status: row.get("status")?,
        match_confidence: row.get("match_confidence")?,
        source_mix: row.get("source_mix")?,
        alert_fired: row.get("alert_fired")?,
        parent_transaction_id: row.get("parent_transaction_id")?,
        transaction_subtype: row.get("transaction_subtype")?,
        emi_group_id: row.get("emi_group_id")?,
        category_id: row.get("category_id")?,
        channel: row.get("channel")?,
        is_deleted: row.get("is_deleted")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        notes: row.get("notes")?,
    })
}

pub fn find_exact_match(
    conn: &Connection,
    instrument_id: &str,
    amount_minor: i64,
    currency: &str,
    direction: &str,
    reference_id: &str,
) -> Result<Option<TransactionsRow>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM transactions 
         WHERE instrument_id = ?1 AND amount_minor = ?2 AND currency = ?3 
           AND direction = ?4 AND reference_id = ?5 AND is_deleted = 0
         LIMIT 1",
    )?;

    let mut rows = stmt.query(params![
        instrument_id,
        amount_minor,
        currency,
        direction,
        reference_id
    ])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_transaction(row)?))
    } else {
        Ok(None)
    }
}

pub fn find_candidates_within_window(
    conn: &Connection,
    instrument_id: &str,
    amount_minor: i64,
    direction: &str,
    event_time_utc: &NaiveDateTime,
    days_window: i64,
) -> Result<Vec<TransactionsRow>> {
    let window_seconds = days_window * 24 * 60 * 60;

    let mut stmt = conn.prepare(
        "SELECT * FROM transactions 
         WHERE instrument_id = ?1 AND amount_minor = ?2 AND direction = ?3 AND is_deleted = 0
           AND best_event_time >= datetime(?4, '-' || ?5 || ' seconds')
           AND best_event_time <= datetime(?4, '+' || ?5 || ' seconds')",
    )?;

    let event_time_str = event_time_utc.format("%Y-%m-%d %H:%M:%S").to_string();
    let rows = stmt.query_map(
        params![
            instrument_id,
            amount_minor,
            direction,
            event_time_str,
            window_seconds
        ],
        row_to_transaction,
    )?;

    let mut transactions = Vec::new();
    for row in rows {
        transactions.push(row?);
    }
    Ok(transactions)
}

pub fn find_parent_for_refund(
    conn: &Connection,
    instrument_id: &str,
    refund_amount_minor: i64,
    refund_event_time_utc: &NaiveDateTime,
) -> Result<Option<TransactionsRow>> {
    // Search backward 30 days for debits >= refund amount
    let mut stmt = conn.prepare(
        "SELECT * FROM transactions 
         WHERE instrument_id = ?1 AND direction = 'debit' AND amount_minor >= ?2 AND is_deleted = 0
           AND best_event_time <= ?3
           AND best_event_time >= datetime(?3, '-30 days')
         ORDER BY best_event_time DESC
         LIMIT 1",
    )?;

    let event_time_str = refund_event_time_utc
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let mut rows = stmt.query(params![instrument_id, refund_amount_minor, event_time_str])?;

    if let Some(row) = rows.next()? {
        Ok(Some(row_to_transaction(row)?))
    } else {
        Ok(None)
    }
}

/// Doc 30 TASK-TXN-011: prior occurrences for the same instrument + merchant
/// entity, oldest first, for recurring-payment interval detection. Excludes
/// the transaction currently being evaluated (`exclude_transaction_id`) so a
/// canonical-update re-run doesn't count itself.
pub fn find_prior_occurrences_for_merchant(
    conn: &Connection,
    instrument_id: &str,
    merchant_entity_id: &str,
    exclude_transaction_id: &str,
) -> Result<Vec<TransactionsRow>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM transactions
         WHERE instrument_id = ?1 AND merchant_entity_id = ?2 AND direction = 'debit'
           AND is_deleted = 0 AND id != ?3 AND best_event_time IS NOT NULL
         ORDER BY best_event_time ASC",
    )?;
    let rows = stmt.query_map(
        params![instrument_id, merchant_entity_id, exclude_transaction_id],
        row_to_transaction,
    )?;

    let mut transactions = Vec::new();
    for row in rows {
        transactions.push(row?);
    }
    Ok(transactions)
}

pub fn get_trailing_30_day_merchant_average(
    conn: &Connection,
    merchant_entity_id: &str,
    current_date_utc: &NaiveDateTime,
) -> Result<f64> {
    let mut stmt = conn.prepare(
        "SELECT AVG(amount_minor) FROM transactions 
         WHERE merchant_entity_id = ?1 AND direction = 'debit' AND is_deleted = 0
           AND best_event_time < ?2
           AND best_event_time >= datetime(?2, '-30 days')",
    )?;

    let current_time_str = current_date_utc.format("%Y-%m-%d %H:%M:%S").to_string();
    let avg: rusqlite::Result<f64> = stmt
        .query_row(params![merchant_entity_id, current_time_str], |row| {
            row.get(0)
        });

    match avg {
        Ok(val) => Ok(val),
        Err(_) => Ok(0.0), // No past transactions
    }
}

pub fn get_global_spend_current_month(
    conn: &Connection,
    current_date_utc: &NaiveDateTime,
) -> Result<f64> {
    // Scaffold: assume current_date_utc has a year/month, we query from start of month
    let start_of_month = format!(
        "{}-{:02}-01 00:00:00",
        current_date_utc.date().year(),
        current_date_utc.date().month()
    );
    let current_time_str = current_date_utc.format("%Y-%m-%d %H:%M:%S").to_string();

    let mut stmt = conn.prepare(
        "SELECT SUM(amount_minor) FROM transactions 
         WHERE direction = 'debit' AND is_deleted = 0
           AND best_event_time >= ?1
           AND best_event_time <= ?2
           AND id NOT IN (
               SELECT m.canonical_transaction_id FROM reconciliation_cluster_members m
               JOIN reconciliation_clusters c ON c.id = m.cluster_id
               WHERE c.cluster_status = 'open' AND m.canonical_transaction_id IS NOT NULL
           )",
    )?;

    let sum: rusqlite::Result<i64> =
        stmt.query_row(params![start_of_month, current_time_str], |row| row.get(0));

    match sum {
        Ok(val) => Ok(val as f64 / 100.0), // Convert minor to major for threshold check (assuming minor is cents/paise)
        Err(_) => Ok(0.0),
    }
}

/// Doc 30 TASK-API-006 / Document 19 §11.1: mirrors `get_global_spend_current_month`
/// for `direction = 'credit'` -- `dashboard_summary`'s additive `income` field
/// needs the same amount_minor-based, ambiguous-cluster-excluded aggregation
/// as spend, not the ad-hoc float `amount` scan it used before this task.
pub fn get_global_income_current_month(
    conn: &Connection,
    current_date_utc: &NaiveDateTime,
) -> Result<f64> {
    let start_of_month = format!(
        "{}-{:02}-01 00:00:00",
        current_date_utc.date().year(),
        current_date_utc.date().month()
    );
    let current_time_str = current_date_utc.format("%Y-%m-%d %H:%M:%S").to_string();

    let mut stmt = conn.prepare(
        "SELECT SUM(amount_minor) FROM transactions
         WHERE direction = 'credit' AND is_deleted = 0
           AND best_event_time >= ?1
           AND best_event_time <= ?2
           AND id NOT IN (
               SELECT m.canonical_transaction_id FROM reconciliation_cluster_members m
               JOIN reconciliation_clusters c ON c.id = m.cluster_id
               WHERE c.cluster_status = 'open' AND m.canonical_transaction_id IS NOT NULL
           )",
    )?;

    let sum: rusqlite::Result<i64> =
        stmt.query_row(params![start_of_month, current_time_str], |row| row.get(0));

    match sum {
        Ok(val) => Ok(val as f64 / 100.0),
        Err(_) => Ok(0.0),
    }
}

/// Doc 30 TASK-API-006 / Document 19 §11.1's `recent_transactions_count`:
/// same month-scoped, ambiguous-cluster-excluded window as spend/income,
/// but counting rows regardless of direction.
pub fn count_transactions_current_month(
    conn: &Connection,
    current_date_utc: &NaiveDateTime,
) -> Result<i64> {
    let start_of_month = format!(
        "{}-{:02}-01 00:00:00",
        current_date_utc.date().year(),
        current_date_utc.date().month()
    );
    let current_time_str = current_date_utc.format("%Y-%m-%d %H:%M:%S").to_string();

    conn.query_row(
        "SELECT COUNT(*) FROM transactions
         WHERE is_deleted = 0
           AND best_event_time >= ?1
           AND best_event_time <= ?2
           AND id NOT IN (
               SELECT m.canonical_transaction_id FROM reconciliation_cluster_members m
               JOIN reconciliation_clusters c ON c.id = m.cluster_id
               WHERE c.cluster_status = 'open' AND m.canonical_transaction_id IS NOT NULL
           )",
        params![start_of_month, current_time_str],
        |row| row.get(0),
    )
    .map_err(anyhow::Error::from)
}

pub fn get_category_spend_current_month(
    conn: &Connection,
    category_id: &str,
    current_date_utc: &NaiveDateTime,
) -> Result<f64> {
    let start_of_month = format!(
        "{}-{:02}-01 00:00:00",
        current_date_utc.date().year(),
        current_date_utc.date().month()
    );
    let current_time_str = current_date_utc.format("%Y-%m-%d %H:%M:%S").to_string();

    let mut stmt = conn.prepare(
        "SELECT SUM(amount_minor) FROM transactions 
         WHERE category_id = ?1 AND direction = 'debit' AND is_deleted = 0
           AND best_event_time >= ?2
           AND best_event_time <= ?3
           AND id NOT IN (
               SELECT m.canonical_transaction_id FROM reconciliation_cluster_members m
               JOIN reconciliation_clusters c ON c.id = m.cluster_id
               WHERE c.cluster_status = 'open' AND m.canonical_transaction_id IS NOT NULL
           )",
    )?;

    let sum: rusqlite::Result<i64> = stmt.query_row(
        params![category_id, start_of_month, current_time_str],
        |row| row.get(0),
    );

    match sum {
        Ok(val) => Ok(val as f64 / 100.0),
        Err(_) => Ok(0.0),
    }
}

#[cfg(test)]
mod candidate_search_tests {
    //! Doc 30 TASK-DEDUP-003: Implement Candidate Generation (Windowed Search).
    //! Direct DB-level tests for `find_candidates_within_window` — the
    //! bounded-candidate-set query `reconciliation::engine::fetch_candidates`
    //! wraps for the reconciliation engine.
    use super::*;

    fn setup_test_db() -> Connection {
        let conn = crate::db::test_helpers::setup_test_db();
        // Disable foreign keys for unit tests that test query logic in
        // isolation, without needing real `instruments` rows to satisfy
        // `transactions.instrument_id`'s FK — mirrors the same pattern
        // `reconciliation::engine_tests::setup_test_db` already uses.
        conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        conn
    }

    fn seed_transaction(
        conn: &Connection,
        id: &str,
        instrument_id: &str,
        amount_minor: i64,
        direction: &str,
        best_event_time: &str,
    ) {
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, amount_minor, currency, direction, best_event_time, is_deleted) \
             VALUES (?1, ?2, ?3, 'INR', ?4, ?5, 0)",
            params![id, instrument_id, amount_minor, direction, best_event_time],
        )
        .unwrap();
    }

    /// Doc 30 TASK-DEDUP-003 acceptance test: only same-instrument,
    /// same-direction rows are returned — a matching amount/time on a
    /// different instrument or opposite direction must never appear.
    #[test]
    fn test_candidate_search_filters_by_instrument_and_direction() {
        let conn = setup_test_db();
        seed_transaction(
            &conn,
            "match",
            "inst_1",
            1000,
            "debit",
            "2026-06-10 12:00:00",
        );
        seed_transaction(
            &conn,
            "wrong_instrument",
            "inst_2",
            1000,
            "debit",
            "2026-06-10 12:00:00",
        );
        seed_transaction(
            &conn,
            "wrong_direction",
            "inst_1",
            1000,
            "credit",
            "2026-06-10 12:00:00",
        );

        let anchor =
            NaiveDateTime::parse_from_str("2026-06-10 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let results =
            find_candidates_within_window(&conn, "inst_1", 1000, "debit", &anchor, 3).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "match");
    }

    /// Doc 30 TASK-DEDUP-003 acceptance test: the +/-3-day window boundary —
    /// a candidate exactly at the edge is included, one day further out is
    /// excluded.
    #[test]
    fn test_candidate_search_date_window_boundary() {
        let conn = setup_test_db();
        seed_transaction(
            &conn,
            "within_window",
            "inst_1",
            1000,
            "debit",
            "2026-06-13 12:00:00",
        ); // +3 days exactly
        seed_transaction(
            &conn,
            "outside_window",
            "inst_1",
            1000,
            "debit",
            "2026-06-14 12:00:00",
        ); // +4 days

        let anchor =
            NaiveDateTime::parse_from_str("2026-06-10 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let results =
            find_candidates_within_window(&conn, "inst_1", 1000, "debit", &anchor, 3).unwrap();

        let ids: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"within_window"));
        assert!(!ids.contains(&"outside_window"));
    }

    /// Doc 30 TASK-DEDUP-003 acceptance test: a genuinely new transaction
    /// (no prior row shares instrument+amount+direction within the window)
    /// returns an empty candidate set, not an error.
    #[test]
    fn test_candidate_search_returns_empty_for_new_transaction() {
        let conn = setup_test_db();
        seed_transaction(
            &conn,
            "unrelated",
            "inst_1",
            5000,
            "debit",
            "2026-06-10 12:00:00",
        );

        let anchor =
            NaiveDateTime::parse_from_str("2026-06-10 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let results =
            find_candidates_within_window(&conn, "inst_1", 1000, "debit", &anchor, 3).unwrap();

        assert!(results.is_empty());
    }
}
