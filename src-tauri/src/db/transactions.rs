//! Canonical transactions -- the ledger the user actually sees.
//!
//! One row per real-world payment, derived from one or more observations.
//! Amounts are integer minor units so arithmetic stays exact, and deletion is
//! soft: removed rows must not reappear when the same source is ingested again,
//! and the user may want them back.
use anyhow::Result;
use chrono::{Datelike, NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
/// One canonical ledger row.
///
/// Almost every field is optional because extraction is best-effort: a
/// transaction recovered from a terse SMS-style alert may carry little beyond an
/// amount, and forcing defaults would fabricate data the bank never sent.
///
/// `amount_minor` is authoritative; the `amount` float exists for legacy reads
/// and must not be used for arithmetic.
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
    pub channel: Option<String>,
    pub is_deleted: bool,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
    pub notes: Option<String>,
}

/// Insert a new canonical transaction.
pub fn insert_transaction(conn: &Connection, tx: &TransactionsRow) -> Result<()> {
    conn.execute(
        "INSERT INTO transactions (
            id, unique_event_id, instrument_id, instrument_type, direction, amount_minor,
            currency, authorization_time, best_event_time, event_time_confidence, best_posting_date,
            posting_date_confidence, merchant_display_name, merchant_normalized_name, merchant_entity_id,
            reference_id, location, original_amount_minor, original_currency, exchange_rate,
            balance_after_transaction, status, match_confidence, source_mix, alert_fired,
            parent_transaction_id, transaction_subtype, emi_group_id, category_id, channel, is_deleted,
            created_at, updated_at, notes
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
            ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, COALESCE(?32, CURRENT_TIMESTAMP), COALESCE(?33, CURRENT_TIMESTAMP), ?34
         )",
        params![
            tx.id, tx.unique_event_id, tx.instrument_id, tx.instrument_type, tx.direction,
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

/// Overwrite an existing transaction by id.
///
/// Writes the whole row, so callers must pass a fully populated record rather
/// than a partial one -- a partial would blank the fields it omitted.
pub fn update_transaction(conn: &Connection, tx: &TransactionsRow) -> Result<()> {
    let count = conn.execute(
        "UPDATE transactions SET
            unique_event_id = ?2, instrument_id = ?3, instrument_type = ?4, direction = ?5,
            amount_minor = ?6, currency = ?7, authorization_time = ?8, best_event_time = ?9,
            event_time_confidence = ?10, best_posting_date = ?11, posting_date_confidence = ?12,
            merchant_display_name = ?13, merchant_normalized_name = ?14, merchant_entity_id = ?15,
            reference_id = ?16, location = ?17, original_amount_minor = ?18, original_currency = ?19,
            exchange_rate = ?20, balance_after_transaction = ?21, status = ?22, match_confidence = ?23,
            source_mix = ?24, alert_fired = ?25, parent_transaction_id = ?26, transaction_subtype = ?27,
            emi_group_id = ?28, category_id = ?29, channel = ?30, is_deleted = ?31, notes = ?32
         WHERE id = ?1",
        params![
            tx.id, tx.unique_event_id, tx.instrument_id, tx.instrument_type, tx.direction,
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

/// Fetch one transaction by id, including soft-deleted rows.
pub fn get_transaction(conn: &Connection, id: &str) -> Result<Option<TransactionsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM transactions WHERE id = ?1 AND is_deleted = 0")?;
    let mut rows = stmt.query([id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_transaction(row)?))
    } else {
        Ok(None)
    }
}

/// Soft-delete a transaction by setting its deleted flag.
///
/// Never a hard delete: ingestion is idempotent against the fingerprint of a
/// live row, so removing the row entirely would let the next scan re-create the
/// transaction the user just deleted.
pub fn delete_transaction(conn: &Connection, id: &str) -> Result<()> {
    let count = conn.execute(
        "UPDATE transactions SET is_deleted = 1 WHERE id = ?1 AND is_deleted = 0",
        params![id],
    )?;
    if count == 0 {
        return Err(anyhow::anyhow!("Transaction not found"));
    }
    Ok(())
}

/// One page of the ledger, newest first, excluding deleted rows.
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

/// Free-text search across merchant and reference fields.
pub fn search_transactions(
    conn: &Connection,
    query: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<TransactionsRow>> {
    search_transactions_with_filters(conn, query, None, limit, offset)
}

/// Search combined with the ledger's structured filters.
///
/// The WHERE clause is assembled from whichever filters were supplied, with every
/// value bound as a parameter rather than interpolated -- the search term is user
/// input reaching SQL, so string-building the query would be an injection route.
pub fn search_transactions_with_filters(
    conn: &Connection,
    query: &str,
    filters: Option<&crate::commands::data::TransactionListFilters>,
    limit: i64,
    offset: i64,
) -> Result<Vec<TransactionsRow>> {
    let trimmed = query.trim();
    let like_pattern = format!("%{trimmed}%");

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

/// Maps a result row onto TransactionsRow.
///
/// Column order here must track the table definition; these are positional reads,
/// so a schema change that reorders columns silently mis-assigns fields.
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

/// Finds a transaction matching on every distinguishing attribute at once.
///
/// The reconciliation fast path. Agreement on instrument, amount, currency,
/// direction *and* reference id leaves no realistic ambiguity, so a hit here can
/// be merged without scoring.
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

/// Finds plausible matches for an observation within a time window.
///
/// Used when no exact match exists. Amount, currency, instrument and direction
/// must agree exactly; only timing is allowed to differ, because the same payment
/// is timestamped differently by an authorisation alert and a statement posting.
///
/// The window is applied symmetrically in SQL via `datetime` arithmetic, which
/// keeps the comparison in the database rather than over-fetching and filtering
/// in Rust.
pub fn find_candidates_within_window(
    conn: &Connection,
    instrument_id: &str,
    amount_minor: i64,
    currency: &str,
    direction: &str,
    event_time_utc: &NaiveDateTime,
    days_window: i64,
) -> Result<Vec<TransactionsRow>> {
    let window_seconds = days_window * 24 * 60 * 60;

    let mut stmt = conn.prepare(
        "SELECT * FROM transactions
         WHERE instrument_id = ?1 AND amount_minor = ?2 AND currency = ?3
           AND direction = ?4 AND is_deleted = 0
           AND best_event_time >= datetime(?5, '-' || ?6 || ' seconds')
           AND best_event_time <= datetime(?5, '+' || ?6 || ' seconds')",
    )?;

    let event_time_str = event_time_utc.format("%Y-%m-%d %H:%M:%S").to_string();
    let rows = stmt.query_map(
        params![
            instrument_id,
            amount_minor,
            currency,
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

/// Finds the original charge a refund most likely reverses.
///
/// Constrained deliberately: same instrument, a debit, at least as large as the
/// refund (partial refunds are common, refunds exceeding the charge are not), and
/// within the preceding 30 days. Ordered most-recent-first, so a repeated charge
/// to the same merchant links to the one actually being reversed.
pub fn find_parent_for_refund(
    conn: &Connection,
    instrument_id: &str,
    refund_amount_minor: i64,
    refund_event_time_utc: &NaiveDateTime,
) -> Result<Option<TransactionsRow>> {
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

/// Prior debits to the same merchant on the same instrument.
///
/// Feeds recurring-payment detection and anomaly comparison. Excludes the
/// transaction under consideration so it cannot match itself, and orders oldest
/// first so intervals can be measured across the series.
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

/// Average spend with a merchant over the trailing 30 days.
///
/// The baseline an unusually large charge is judged against.
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
        Err(_) => Ok(0.0),
    }
}

const EXCLUDE_OPEN_CLUSTER_MEMBERS: &str = "AND id NOT IN (
               SELECT m.canonical_transaction_id FROM reconciliation_cluster_members m
               JOIN reconciliation_clusters c ON c.id = m.cluster_id
               WHERE c.cluster_status = 'open' AND m.canonical_transaction_id IS NOT NULL
           )";

/// Total debits so far this calendar month, for budget utilisation.
pub fn get_global_spend_current_month(
    conn: &Connection,
    current_date_utc: &NaiveDateTime,
) -> Result<f64> {
    let start_of_month = format!(
        "{}-{:02}-01 00:00:00",
        current_date_utc.date().year(),
        current_date_utc.date().month()
    );
    let current_time_str = current_date_utc.format("%Y-%m-%d %H:%M:%S").to_string();

    let mut stmt = conn.prepare(&format!(
        "SELECT SUM(amount_minor) FROM transactions
         WHERE direction = 'debit' AND is_deleted = 0
           AND best_event_time >= ?1
           AND best_event_time <= ?2
           {EXCLUDE_OPEN_CLUSTER_MEMBERS}"
    ))?;

    let sum: rusqlite::Result<i64> =
        stmt.query_row(params![start_of_month, current_time_str], |row| row.get(0));

    match sum {
        Ok(val) => Ok(val as f64 / 100.0),
        Err(_) => Ok(0.0),
    }
}

/// Total credits so far this calendar month.
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

    let mut stmt = conn.prepare(&format!(
        "SELECT SUM(amount_minor) FROM transactions
         WHERE direction = 'credit' AND is_deleted = 0
           AND best_event_time >= ?1
           AND best_event_time <= ?2
           {EXCLUDE_OPEN_CLUSTER_MEMBERS}"
    ))?;

    let sum: rusqlite::Result<i64> =
        stmt.query_row(params![start_of_month, current_time_str], |row| row.get(0));

    match sum {
        Ok(val) => Ok(val as f64 / 100.0),
        Err(_) => Ok(0.0),
    }
}

/// Number of transactions this calendar month.
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
        &format!(
            "SELECT COUNT(*) FROM transactions
             WHERE is_deleted = 0
               AND best_event_time >= ?1
               AND best_event_time <= ?2
               {EXCLUDE_OPEN_CLUSTER_MEMBERS}"
        ),
        params![start_of_month, current_time_str],
        |row| row.get(0),
    )
    .map_err(anyhow::Error::from)
}

/// Month-to-date spend for one category, for per-category budgets.
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

    let mut stmt = conn.prepare(&format!(
        "SELECT SUM(amount_minor) FROM transactions
         WHERE category_id = ?1 AND direction = 'debit' AND is_deleted = 0
           AND best_event_time >= ?2
           AND best_event_time <= ?3
           {EXCLUDE_OPEN_CLUSTER_MEMBERS}"
    ))?;

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
    use super::*;

    fn setup_test_db() -> Connection {
        let conn = crate::db::test_helpers::setup_test_db();
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
            find_candidates_within_window(&conn, "inst_1", 1000, "INR", "debit", &anchor, 3)
                .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "match");
    }

    #[test]
    fn test_candidate_search_filters_by_currency() {
        let conn = setup_test_db();
        seed_transaction(
            &conn,
            "inr_match",
            "inst_1",
            50000,
            "debit",
            "2026-06-10 12:00:00",
        );
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, amount_minor, currency, direction, best_event_time, is_deleted) \
             VALUES ('usd_decoy', 'inst_1', 50000, 'USD', 'debit', '2026-06-10 12:00:00', 0)",
            [],
        )
        .unwrap();

        let anchor =
            NaiveDateTime::parse_from_str("2026-06-10 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap();

        let inr = find_candidates_within_window(&conn, "inst_1", 50000, "INR", "debit", &anchor, 3)
            .unwrap();
        assert_eq!(inr.len(), 1, "a ₹500 debit must not match a $500 debit");
        assert_eq!(inr[0].id, "inr_match");

        let usd = find_candidates_within_window(&conn, "inst_1", 50000, "USD", "debit", &anchor, 3)
            .unwrap();
        assert_eq!(usd.len(), 1);
        assert_eq!(usd[0].id, "usd_decoy");

        let eur = find_candidates_within_window(&conn, "inst_1", 50000, "EUR", "debit", &anchor, 3)
            .unwrap();
        assert!(eur.is_empty());
    }

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
        );
        seed_transaction(
            &conn,
            "outside_window",
            "inst_1",
            1000,
            "debit",
            "2026-06-14 12:00:00",
        );

        let anchor =
            NaiveDateTime::parse_from_str("2026-06-10 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let results =
            find_candidates_within_window(&conn, "inst_1", 1000, "INR", "debit", &anchor, 3)
                .unwrap();

        let ids: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"within_window"));
        assert!(!ids.contains(&"outside_window"));
    }

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
            find_candidates_within_window(&conn, "inst_1", 1000, "INR", "debit", &anchor, 3)
                .unwrap();

        assert!(results.is_empty());
    }
}

#[cfg(test)]
mod merchant_index_tests {

    #[test]
    fn merchant_scoped_post_processing_queries_use_the_index() {
        let conn = crate::db::test_helpers::setup_test_db();

        let plan_for = |sql: &str| -> String {
            let mut stmt = conn
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .expect("query must parse");
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(3))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect::<Vec<_>>();
            rows.join(" | ")
        };

        let avg_plan = plan_for(
            "SELECT AVG(amount_minor) FROM transactions
             WHERE merchant_entity_id = 'm1' AND direction = 'debit' AND is_deleted = 0
               AND best_event_time < '2026-06-01 00:00:00'
               AND best_event_time >= datetime('2026-06-01 00:00:00', '-30 days')",
        );
        assert!(
            avg_plan.contains("idx_transactions_merchant_entity_event"),
            "trailing-30-day average must not scan transactions; plan was: {avg_plan}"
        );

        let priors_plan = plan_for(
            "SELECT * FROM transactions
             WHERE instrument_id = 'i1' AND merchant_entity_id = 'm1' AND direction = 'debit'
               AND is_deleted = 0 AND id != 'x' AND best_event_time IS NOT NULL
             ORDER BY best_event_time ASC",
        );
        assert!(
            priors_plan.contains("idx_transactions_merchant_entity_event")
                || priors_plan.contains("idx_transactions_instrument_event"),
            "prior-occurrence lookup must use an index; plan was: {priors_plan}"
        );
        assert!(
            !priors_plan.contains("SCAN transactions"),
            "prior-occurrence lookup must not full-scan; plan was: {priors_plan}"
        );
    }
}
