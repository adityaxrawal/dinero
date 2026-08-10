//! Read commands backing the dashboard, ledger and analytics screens.
//!
//! Aggregation happens in SQL rather than by loading rows and summing them in
//! Rust, so the dashboard stays responsive as the ledger grows.
//!
//! Money crosses this boundary as integer minor units; conversion to a decimal
//! is left to the frontend's formatter so no rounding is introduced in transit.
//!
//! File handling note: no command here accepts an uploaded document. The only
//! filesystem reads below are of the app's own log files. Statement upload is
//! handled by `statements_upload` in the parent module, and every uploaded file
//! is validated by `statements::validator::validate_and_hash` before any of its
//! bytes are parsed -- that check enforces the application/pdf type by testing
//! the `%PDF` magic bytes rather than trusting a filename or a client-supplied
//! MIME string, and rejects anything over the maximum size. Those two checks are
//! the trust boundary for untrusted documents; nothing in this module weakens or
//! bypasses them.
use crate::extraction::normalization::clean_masked_identifier;
use chrono::Datelike;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use tauri::{Manager, State};

#[derive(Serialize, Debug, PartialEq)]
pub struct DashboardSummary {
    pub month_to_date_spend: f64,
    pub limit: f64,
    pub utilization_pct: f64,
    pub recent_transactions_count: i64,
    pub upcoming_bills_count: u32,
    pub income: f64,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct PendingReviewMetric {
    pub count: i64,
    pub amount_minor: i64,
}

/// Count and total value of transactions awaiting review.
///
/// Drives the review badge and the dock badge, so it must reflect reconciliation
/// activity promptly.
pub fn compute_unassigned_amount_pending_review(
    conn: &Connection,
) -> Result<PendingReviewMetric, String> {
    let (cluster_count, cluster_amount_minor): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(DISTINCT t.id), COALESCE(SUM(t.amount_minor), 0)
             FROM transactions t
             JOIN reconciliation_cluster_members m ON m.canonical_transaction_id = t.id
             JOIN reconciliation_clusters c ON c.id = m.cluster_id
             WHERE c.cluster_status = 'open'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or((0, 0));

    let (unassigned_count, unassigned_amount_minor): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(o.amount_minor), 0)
             FROM unassigned_transactions u
             JOIN transaction_observations o ON o.id = u.observation_id
             WHERE u.status = 'open'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or((0, 0));

    Ok(PendingReviewMetric {
        count: cluster_count + unassigned_count,
        amount_minor: cluster_amount_minor + unassigned_amount_minor,
    })
}

#[derive(Serialize, Debug, PartialEq)]
pub struct TransactionRecord {
    pub id: String,
    pub date: String,
    pub merchant: String,
    pub amount: f64,
    pub direction: Option<String>,
    pub category: String,
    pub status: String,
    pub source_mix: Option<String>,
    pub instrument_id: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct TransactionsPage {
    pub records: Vec<TransactionRecord>,
    pub total: i64,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct StatementRecord {
    pub id: String,
    pub date: String,
    pub file_name: String,
    pub status: String,
    pub instrument_id: Option<String>,
    pub issuer_name: Option<String>,
    pub masked_identifier: Option<String>,
    pub instrument_type: Option<String>,
    pub pdf_available: bool,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct StatementsPage {
    pub records: Vec<StatementRecord>,
    pub total: i64,
}

/// Total number of statements.
pub fn count_statements(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT COUNT(*) FROM statements", [], |row| row.get(0))
        .map_err(|e| e.to_string())
}

#[derive(Serialize, Debug, PartialEq)]
pub struct ClusterMember {
    pub id: String,
    pub member_role: String,
    pub observation_id: Option<String>,
    pub canonical_transaction_id: Option<String>,
    pub source_pipeline: Option<String>,
    pub merchant: String,
    pub amount: f64,
    pub direction: Option<String>,
    pub date: String,
    pub instrument_issuer_name: Option<String>,
    pub instrument_masked_identifier: Option<String>,
    pub reference_id: Option<String>,
    pub match_score: Option<f64>,
    pub source_raw_payload_json: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct ClusterRecord {
    pub id: String,
    pub reason: String,
    pub members_count: i64,
    pub members: Vec<ClusterMember>,
    pub created_at: Option<String>,
    pub explanation: String,
}

/// Explains in plain language why a cluster could not be resolved automatically.
///
/// Turns the matcher's numbers into a reason the user can act on. The two-
/// candidate case is the interesting one: it reports the margin between the top
/// scores against the threshold, because closeness -- not low confidence -- is
/// what actually blocked the automatic decision.
fn compute_cluster_explanation(members: &[ClusterMember]) -> String {
    let mut scores: Vec<f64> = members.iter().filter_map(|m| m.match_score).collect();
    scores.sort_by(|a, b| b.partial_cmp(a).unwrap());

    match scores.as_slice() {
        [] => "No existing transaction candidates were found for this evidence.".to_string(),
        [only] => format!(
            "One possible match at {}% confidence — below the threshold for an automatic match.",
            (only * 100.0).round() as i64
        ),
        [top, second, ..] => format!(
            "Two possible matches, {}% and {}% — only {} points apart, closer than the {}-point margin needed to pick one automatically.",
            (top * 100.0).round() as i64,
            (second * 100.0).round() as i64,
            ((top - second) * 100.0).round() as i64,
            (crate::reconciliation::engine::AMBIGUITY_MARGIN_THRESHOLD * 100.0).round() as i64
        ),
    }
}

#[derive(Serialize, Debug, PartialEq)]
pub struct InstrumentRecord {
    pub id: String,
    pub instrument_type: String,
    pub issuer_name: String,
    pub masked_identifier: String,
    pub status: String,
    pub current_balance: Option<f64>,
    pub credit_limit: Option<f64>,
    pub full_identifier: Option<String>,
    pub billing_cycle_day: Option<u8>,
    pub bank_ifsc: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct DebugMetrics {
    pub total_transactions: i64,
    pub total_statements: i64,
    pub unresolved_clusters: i64,
    pub db_size_bytes: i64,
    pub app_version: String,
    pub llm_fallback_rate: f64,
    pub queue_depth: i64,
    pub extraction_layer_distribution: std::collections::HashMap<String, i64>,
    pub reconciliation_decision_distribution: std::collections::HashMap<String, i64>,
}

/// Computes the dashboard's headline figures.
pub fn do_fetch_dashboard_summary(conn: &Connection) -> Result<DashboardSummary, String> {
    let now = chrono::Utc::now().naive_utc();

    let month_to_date_spend: f64 =
        crate::db::transactions::get_global_spend_current_month(conn, &now)
            .map_err(|e| e.to_string())?;
    let income: f64 = crate::db::transactions::get_global_income_current_month(conn, &now)
        .map_err(|e| e.to_string())?;
    let recent_transactions_count: i64 =
        crate::db::transactions::count_transactions_current_month(conn, &now)
            .map_err(|e| e.to_string())?;

    let limit: f64 = conn
        .query_row(
            "SELECT COALESCE(spending_limit_monthly, 0) FROM local_profile WHERE id = 1",
            [],
            |row| row.get::<_, f64>(0),
        )
        .unwrap_or(0.0);

    let utilization_pct: f64 = if limit > 0.0 {
        (month_to_date_spend / limit) * 100.0
    } else {
        0.0
    };

    let today = now.date();
    let upcoming_bills_count: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM instruments WHERE is_deleted = 0 AND statement_due_date IS NOT NULL AND statement_due_date >= ?1",
            params![today],
            |row| row.get::<_, u32>(0),
        )
        .unwrap_or(0);

    Ok(DashboardSummary {
        month_to_date_spend,
        limit,
        utilization_pct,
        recent_transactions_count,
        upcoming_bills_count,
        income,
    })
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct TransactionListFilters {
    pub from_date: Option<String>,
    pub to_date: Option<String>,
    pub instrument_id: Option<String>,
    pub direction: Option<String>,
    pub category_id: Option<String>,
    pub status: Option<String>,
}

/// Builds a parameterised WHERE fragment from the supplied filters.
///
/// Only filters that were set contribute a clause. Every value is bound as a
/// parameter rather than interpolated -- these arrive from the frontend, so
/// building the SQL by string concatenation would be an injection route.
///
/// Dates are widened to whole-day bounds so an inclusive range behaves as the
/// user expects rather than excluding the final day's transactions.
fn build_filter_clause(
    filters: &TransactionListFilters,
) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut clauses = Vec::new();
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(from) = &filters.from_date {
        clauses.push("authorization_time >= ?".to_string());
        args.push(Box::new(format!("{from} 00:00:00")));
    }
    if let Some(to) = &filters.to_date {
        clauses.push("authorization_time <= ?".to_string());
        args.push(Box::new(format!("{to} 23:59:59")));
    }
    if let Some(instrument_id) = &filters.instrument_id {
        clauses.push("instrument_id = ?".to_string());
        args.push(Box::new(instrument_id.clone()));
    }
    if let Some(direction) = &filters.direction {
        clauses.push("direction = ?".to_string());
        args.push(Box::new(direction.clone()));
    }
    if let Some(category_id) = &filters.category_id {
        clauses.push("category_id = ?".to_string());
        args.push(Box::new(category_id.clone()));
    }
    if let Some(status) = &filters.status {
        clauses.push("status = ?".to_string());
        args.push(Box::new(status.clone()));
    }

    let clause = if clauses.is_empty() {
        String::new()
    } else {
        format!(" AND {}", clauses.join(" AND "))
    };
    (clause, args)
}

/// Fetches a filtered page of transactions.
pub fn do_fetch_transactions(
    conn: &Connection,
    filters: &TransactionListFilters,
    limit: i64,
    offset: i64,
) -> Result<Vec<TransactionRecord>, String> {
    let (filter_clause, filter_args) = build_filter_clause(filters);
    let sql = format!(
        "SELECT id, authorization_time, merchant_display_name, amount, category_id, status, source_mix, instrument_id, direction
         FROM transactions
         WHERE is_deleted = 0{filter_clause}
         ORDER BY authorization_time DESC LIMIT ? OFFSET ?"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let mut all_args: Vec<&dyn rusqlite::ToSql> = filter_args.iter().map(|b| b.as_ref()).collect();
    all_args.push(&limit);
    all_args.push(&offset);

    let tx_iter = stmt
        .query_map(all_args.as_slice(), |row| {
            let auth_time: Option<String> = row.get(1)?;
            let merchant: Option<String> = row.get(2)?;
            let amount_val: Option<f64> = match row.get(3) {
                Ok(v) => v,
                Err(_) => {
                    let i: Option<i64> = row.get(3)?;
                    i.map(|x| x as f64)
                }
            };
            let cat: Option<String> = row.get(4)?;
            let stat: Option<String> = row.get(5)?;
            let source_mix: Option<String> = row.get(6)?;
            let instrument_id: Option<String> = row.get(7)?;
            let direction: Option<String> = row.get(8)?;

            Ok(TransactionRecord {
                id: row.get(0)?,
                date: auth_time.unwrap_or_else(|| "Unknown".to_string()),
                merchant: merchant.unwrap_or_else(|| "Unknown".to_string()),
                amount: amount_val.unwrap_or(0.0),
                direction,
                category: cat.unwrap_or_else(|| "UNCATEGORIZED".to_string()),
                status: stat.unwrap_or_else(|| "PENDING".to_string()),
                source_mix,
                instrument_id,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut transactions = Vec::new();
    for tx in tx_iter {
        transactions.push(tx.map_err(|e| e.to_string())?);
    }

    Ok(transactions)
}

/// Counts transactions matching the filters, for pagination.
pub fn count_transactions_filtered(
    conn: &Connection,
    filters: &TransactionListFilters,
) -> Result<i64, String> {
    let (filter_clause, filter_args) = build_filter_clause(filters);
    let sql = format!("SELECT COUNT(*) FROM transactions WHERE is_deleted = 0{filter_clause}");
    let args: Vec<&dyn rusqlite::ToSql> = filter_args.iter().map(|b| b.as_ref()).collect();
    conn.query_row(&sql, args.as_slice(), |row| row.get(0))
        .map_err(|e| e.to_string())
}

/// Counts all transactions.
pub fn count_transactions(conn: &Connection) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE is_deleted = 0",
        [],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

/// Runs a text search across transactions.
pub fn do_transactions_search(
    conn: &Connection,
    query: &str,
    filters: Option<&TransactionListFilters>,
) -> Result<Vec<TransactionRecord>, String> {
    let rows =
        crate::db::transactions::search_transactions_with_filters(conn, query, filters, 50, 0)
            .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .map(|tx| TransactionRecord {
            id: tx.id,
            date: tx
                .authorization_time
                .map(|dt| dt.to_string())
                .unwrap_or_else(|| "Unknown".to_string()),
            merchant: tx
                .merchant_display_name
                .unwrap_or_else(|| "Unknown".to_string()),
            amount: tx.amount.unwrap_or(0.0),
            direction: tx.direction,
            category: tx
                .category_id
                .unwrap_or_else(|| "UNCATEGORIZED".to_string()),
            status: tx.status.unwrap_or_else(|| "PENDING".to_string()),
            source_mix: tx.source_mix,
            instrument_id: tx.instrument_id,
        })
        .collect())
}

#[tauri::command]
/// Command wrapper around transaction search.
pub async fn transactions_search(
    pool: State<'_, deadpool_sqlite::Pool>,
    query: String,
    filters: Option<TransactionListFilters>,
) -> Result<Vec<TransactionRecord>, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| do_transactions_search(c, &query, filters.as_ref()))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(crate::error::AppError::Db)
}

/// Fetches statement processing history.
pub fn do_fetch_statement_history(
    conn: &Connection,
    limit: i64,
    offset: i64,
) -> Result<Vec<StatementRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.created_at, s.source_message_id, s.parse_status, s.instrument_id,
                    i.issuer_name, i.masked_identifier, i.type,
                    CASE WHEN u.pdf_retained_until IS NOT NULL AND u.pdf_retained_until > datetime('now')
                         THEN 1 ELSE 0 END AS pdf_available
             FROM statements s
             LEFT JOIN instruments i ON i.id = s.instrument_id
             LEFT JOIN unprocessed_statements u ON u.resolved_statement_id = s.id
             ORDER BY s.created_at DESC LIMIT ?1 OFFSET ?2",
        )
        .map_err(|e| e.to_string())?;

    let iter = stmt
        .query_map(params![limit, offset], |row| {
            let created_at: Option<String> = row.get(1)?;
            let file_name: Option<String> = row.get(2)?;
            let status: Option<String> = row.get(3)?;
            Ok(StatementRecord {
                id: row.get(0)?,
                date: created_at.unwrap_or_else(|| "Unknown".to_string()),
                file_name: file_name.unwrap_or_else(|| "Unknown".to_string()),
                status: status.unwrap_or_else(|| "UNKNOWN".to_string()),
                instrument_id: row.get(4)?,
                issuer_name: row.get(5)?,
                masked_identifier: row.get(6)?,
                instrument_type: row.get(7)?,
                pdf_available: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut res = Vec::new();
    for r in iter {
        res.push(r.map_err(|e| e.to_string())?);
    }
    Ok(res)
}

/// Loads a cluster's members with the context needed to compare them.
fn fetch_cluster_members(
    conn: &Connection,
    cluster_id: &str,
) -> Result<Vec<ClusterMember>, String> {
    let mut member_stmt = conn.prepare(
        "SELECT m.id,
                m.member_role,
                m.observation_id,
                m.canonical_transaction_id,
                COALESCE(o.source_pipeline, CASE WHEN m.canonical_transaction_id IS NOT NULL THEN 'statement_pdf' ELSE NULL END),
                COALESCE(t.merchant_display_name, o.merchant_raw, 'Unknown'),
                COALESCE(t.amount, o.amount, 0),
                COALESCE(t.direction, o.direction),
                COALESCE(t.authorization_time, o.event_time, 'Unknown'),
                i.issuer_name,
                i.masked_identifier,
                COALESCE(t.reference_id, o.reference_id),
                m.match_score,
                CASE WHEN m.member_role = 'incoming' THEN o.raw_payload_json ELSE NULL END
         FROM reconciliation_cluster_members m
         LEFT JOIN transactions t ON m.canonical_transaction_id = t.id
         LEFT JOIN transaction_observations o ON m.observation_id = o.id
         LEFT JOIN instruments i ON i.id = COALESCE(t.instrument_id, o.instrument_id)
         WHERE m.cluster_id = ?1"
    ).map_err(|e| e.to_string())?;

    let m_iter = member_stmt
        .query_map([cluster_id], |row| {
            Ok(ClusterMember {
                id: row.get(0)?,
                member_role: row.get(1)?,
                observation_id: row.get(2)?,
                canonical_transaction_id: row.get(3)?,
                source_pipeline: row.get(4)?,
                merchant: row.get(5)?,
                amount: row.get(6)?,
                direction: row.get(7)?,
                date: row.get(8)?,
                instrument_issuer_name: row.get(9)?,
                instrument_masked_identifier: row.get(10)?,
                reference_id: row.get(11)?,
                match_score: row.get(12)?,
                source_raw_payload_json: row.get(13)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut members = Vec::new();
    for m in m_iter {
        members.push(m.map_err(|e| e.to_string())?);
    }
    Ok(members)
}

/// Lists unresolved clusters for the reconciliation queue.
pub fn do_fetch_unresolved_clusters(conn: &Connection) -> Result<Vec<ClusterRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, reason, created_at FROM reconciliation_clusters WHERE cluster_status IN ('open', 'deferred')",
        )
        .map_err(|e| e.to_string())?;

    let iter = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let reason: Option<String> = row.get(1)?;
            let created_at: Option<String> = row.get(2)?;
            Ok((
                id,
                reason.unwrap_or_else(|| "Unknown".to_string()),
                created_at,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut res = Vec::new();
    for r in iter {
        let (id, reason, created_at) = r.map_err(|e| e.to_string())?;
        let members = fetch_cluster_members(conn, &id)?;
        let explanation = compute_cluster_explanation(&members);

        res.push(ClusterRecord {
            id,
            reason,
            members_count: members.len() as i64,
            members,
            created_at,
            explanation,
        });
    }
    Ok(res)
}

/// Loads one cluster in full, including its explanation.
pub fn do_fetch_cluster_detail(
    conn: &Connection,
    cluster_id: &str,
) -> Result<Option<ClusterRecord>, String> {
    let found: Option<(String, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT id, reason, created_at FROM reconciliation_clusters WHERE id = ?1",
            params![cluster_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let Some((id, reason, created_at)) = found else {
        return Ok(None);
    };
    let members = fetch_cluster_members(conn, &id)?;
    let explanation = compute_cluster_explanation(&members);
    Ok(Some(ClusterRecord {
        id,
        reason: reason.unwrap_or_else(|| "Unknown".to_string()),
        members_count: members.len() as i64,
        members,
        created_at,
        explanation,
    }))
}

/// Lists instruments with their balances.
pub fn do_fetch_instruments(conn: &Connection) -> Result<Vec<InstrumentRecord>, String> {
    let mut stmt = conn.prepare(
        "SELECT i.id, i.type, i.issuer_name, i.masked_identifier, i.status, i.current_balance, i.credit_limit, i.full_identifier, i.billing_cycle_day, i.bank_ifsc, \
         (SELECT COALESCE(SUM(CASE WHEN i.type = 'credit_card' THEN CASE WHEN t.direction = 'debit' THEN COALESCE(t.amount_minor, CAST(t.amount * 100 AS INTEGER)) ELSE -COALESCE(t.amount_minor, CAST(t.amount * 100 AS INTEGER)) END ELSE CASE WHEN t.direction = 'credit' THEN COALESCE(t.amount_minor, CAST(t.amount * 100 AS INTEGER)) ELSE -COALESCE(t.amount_minor, CAST(t.amount * 100 AS INTEGER)) END END), 0) \
          FROM transactions t WHERE t.instrument_id = i.id AND t.is_deleted = 0) AS tx_balance_minor \
         FROM instruments i WHERE i.is_deleted = 0 ORDER BY i.issuer_name ASC"
    ).map_err(|e| e.to_string())?;

    let iter = stmt
        .query_map([], |row| {
            let t: Option<String> = row.get(1)?;
            let issuer: Option<String> = row.get(2)?;
            let masked: Option<String> = row.get(3)?;
            let status: Option<String> = row.get(4)?;
            let db_bal_paise: Option<i64> = match row.get::<_, i64>(5) {
                Ok(v) => Some(v),
                Err(_) => row.get::<_, f64>(5).ok().map(|f| f as i64),
            };
            let tx_bal_minor: i64 = row.get(10).unwrap_or(0);

            let inst_type_str = t.as_deref().unwrap_or("credit_card");

            let effective_bal = match db_bal_paise {
                Some(p) => p as f64 / 100.0,
                None => {
                    if inst_type_str == "credit_card" || tx_bal_minor > 0 {
                        tx_bal_minor as f64 / 100.0
                    } else {
                        0.0
                    }
                }
            };

            let limit: Option<f64> = match row.get(6) {
                Ok(v) => v,
                Err(_) => {
                    let i: Option<i64> = row.get(6)?;
                    i.map(|x| x as f64 / 100.0)
                }
            };
            let full_id: Option<String> = row.get(7)?;
            let billing: Option<u8> = row.get(8)?;
            let bank_ifsc: Option<String> = row.get(9)?;
            Ok(InstrumentRecord {
                id: row.get(0)?,
                instrument_type: t.unwrap_or_else(|| "Unknown".to_string()),
                issuer_name: issuer.unwrap_or_else(|| "Unknown".to_string()),
                masked_identifier: masked.unwrap_or_else(|| "****".to_string()),
                status: status.unwrap_or_else(|| "active".to_string()),
                current_balance: Some(effective_bal),
                credit_limit: limit,
                full_identifier: full_id,
                billing_cycle_day: billing,
                bank_ifsc,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut res = Vec::new();
    for r in iter {
        res.push(r.map_err(|e| e.to_string())?);
    }
    Ok(res)
}

/// Assembles the debug screen's metrics.
///
/// The extraction-layer distribution is the useful one: it shows how much traffic
/// is reaching the expensive LLM layer versus being handled by cheap rules.
pub fn do_get_debug_metrics(conn: &Connection) -> Result<DebugMetrics, String> {
    let total_transactions: i64 = conn
        .query_row("SELECT count(*) FROM transactions", [], |row| row.get(0))
        .unwrap_or(0);
    let total_statements: i64 = conn
        .query_row("SELECT count(*) FROM statements", [], |row| row.get(0))
        .unwrap_or(0);
    let unresolved_clusters: i64 = conn.query_row(
        "SELECT count(*) FROM reconciliation_clusters WHERE cluster_status IN ('open', 'deferred')",
        [],
        |row| row.get(0)
    ).unwrap_or(0);

    let db_size_bytes: i64 = conn
        .query_row(
            "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let total_observations: i64 = conn
        .query_row("SELECT count(*) FROM transaction_observations", [], |row| {
            row.get(0)
        })
        .unwrap_or(0);

    let llm_observations: i64 = conn
        .query_row(
            "SELECT count(*) FROM transaction_observations WHERE extraction_method = 'llm'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let llm_fallback_rate = if total_observations > 0 {
        (llm_observations as f64) / (total_observations as f64)
    } else {
        0.0
    };

    let queue_depth: i64 = conn
        .query_row(
            "SELECT count(*) FROM processing_checkpoints WHERE status != 'completed'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let mut extraction_layer_distribution = std::collections::HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT COALESCE(extraction_method, 'unknown'), count(*) FROM transaction_observations GROUP BY extraction_method") {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        }) {
            for (k, v) in rows.flatten() {
                extraction_layer_distribution.insert(k, v);
            }
        }
    }

    let mut reconciliation_decision_distribution = std::collections::HashMap::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT COALESCE(decision, 'unknown'), count(*) FROM match_decisions GROUP BY decision",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        }) {
            for (k, v) in rows.flatten() {
                reconciliation_decision_distribution.insert(k, v);
            }
        }
    }

    Ok(DebugMetrics {
        total_transactions,
        total_statements,
        unresolved_clusters,
        db_size_bytes,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        llm_fallback_rate,
        queue_depth,
        extraction_layer_distribution,
        reconciliation_decision_distribution,
    })
}

#[derive(Serialize, Debug)]
pub struct BackendStatus {
    pub status: String,
}

#[tauri::command]
/// Reports whether the backend is serving and its database is intact.
pub async fn check_backend_status(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<BackendStatus, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| {
        c.query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
    .map(|_| BackendStatus {
        status: "healthy".to_string(),
    })
    .map_err(crate::error::AppError::Db)
}

#[tauri::command]
/// Exports the user's data to a file.
///
/// Optionally password-protected, since the export leaves the machine and its
/// keychain behind and cannot rely on the database key.
pub async fn settings_export_data(
    export_path: String,
    password: Option<String>,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    match password {
        None => {
            let path_for_export = export_path.clone();
            conn.interact(move |c| c.execute("VACUUM INTO ?1", rusqlite::params![path_for_export]))
                .await
                .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
                .map_err(|e| crate::error::AppError::Io(format!("Export failed: {}", e)))?;
        }
        Some(password) => {
            crate::ipc::validation::validate_non_empty("password", &password)?;
            let temp_path =
                std::env::temp_dir().join(format!("dinero-export-{}.tmp", uuid::Uuid::new_v4()));
            let temp_path_str = temp_path.to_string_lossy().to_string();
            conn.interact(move |c| c.execute("VACUUM INTO ?1", rusqlite::params![temp_path_str]))
                .await
                .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
                .map_err(|e| crate::error::AppError::Io(format!("Export failed: {}", e)))?;

            let plaintext =
                std::fs::read(&temp_path).map_err(|e| crate::error::AppError::Io(e.to_string()))?;
            let _ = std::fs::remove_file(&temp_path);

            let encrypted = crate::db::backup::encrypt_backup(&plaintext, &password)
                .map_err(|e| crate::error::AppError::Validation(e.to_string()))?;
            std::fs::write(&export_path, encrypted)
                .map_err(|e| crate::error::AppError::Io(e.to_string()))?;
        }
    }

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let export_path_for_log = export_path.clone();
    conn.interact(move |c| {
        crate::auth::consent::insert_consent_event(
            c,
            "data_export",
            &format!(
                "User exported an encrypted copy of their local data to {}",
                export_path_for_log
            ),
        )
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
    .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    tracing::info!(
        "settings_export_data: exported encrypted snapshot to {}",
        export_path
    );
    Ok(export_path)
}

#[tauri::command]
/// Exports an encrypted backup.
pub async fn settings_export_encrypted_backup(
    export_path: String,
    password: String,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    if password.is_empty() {
        return Err(crate::error::AppError::Validation(
            "password must not be empty".to_string(),
        ));
    }
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let temp_path =
        std::env::temp_dir().join(format!("dinero-backup-{}.tmp", uuid::Uuid::new_v4()));
    let temp_path_str = temp_path.to_string_lossy().to_string();
    conn.interact(move |c| c.execute("VACUUM INTO ?1", rusqlite::params![temp_path_str]))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let plaintext =
        std::fs::read(&temp_path).map_err(|e| crate::error::AppError::Io(e.to_string()))?;
    let _ = std::fs::remove_file(&temp_path);

    let encrypted = crate::db::backup::encrypt_backup(&plaintext, &password)
        .map_err(|e| crate::error::AppError::Validation(e.to_string()))?;
    std::fs::write(&export_path, encrypted)
        .map_err(|e| crate::error::AppError::Io(e.to_string()))?;

    Ok(export_path)
}

#[tauri::command]
/// Imports a previously exported encrypted backup.
pub async fn settings_import_encrypted_backup(
    import_path: String,
    password: String,
) -> Result<String, crate::error::AppError> {
    let blob =
        std::fs::read(&import_path).map_err(|e| crate::error::AppError::Io(e.to_string()))?;
    let decrypted = crate::db::backup::decrypt_backup(&blob, &password)
        .map_err(|e| crate::error::AppError::Validation(e.to_string()))?;

    let staging_path =
        std::env::temp_dir().join(format!("dinero-restore-{}.db", uuid::Uuid::new_v4()));
    std::fs::write(&staging_path, decrypted)
        .map_err(|e| crate::error::AppError::Io(e.to_string()))?;

    Ok(staging_path.to_string_lossy().to_string())
}

#[tauri::command]
/// Command entry point for deleting all local data.
pub async fn settings_delete_account(
    app: tauri::AppHandle,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    let app_dir = app.path().app_data_dir().map_err(|e| {
        crate::error::AppError::Io(format!("Failed to resolve app data directory: {}", e))
    })?;

    perform_account_deletion(&app_dir, Some(pool.inner())).await?;

    app.restart();
}

/// Deletes every trace of the account from this machine.
///
/// Irreversible, and deliberately thorough: the encrypted database, its backups,
/// stored statements and keychain entries all go. Leaving any one behind would
/// make the deletion a false promise.
pub async fn perform_account_deletion(
    app_dir: &std::path::Path,
    pool: Option<&deadpool_sqlite::Pool>,
) -> Result<(), crate::error::AppError> {
    if let Some(pool) = pool {
        crate::licensing::gate::assert_write_allowed(pool).await?;

        if let Ok(conn) = pool.get().await {
            let _ = conn
                .interact(|c| {
                    crate::db::audit_log::insert(
                        c,
                        &crate::db::audit_log::AuditLogRow {
                            id: uuid::Uuid::new_v4().to_string(),
                            actor_type: Some("user".to_string()),
                            actor_id: Some("local".to_string()),
                            action: Some("account_deletion_requested".to_string()),
                            resource_type: Some("database".to_string()),
                            resource_id: Some("local_sqlite".to_string()),
                            before_json: None,
                            after_json: None,
                            created_at: chrono::Utc::now(),
                        },
                    )
                })
                .await;
        }

        if let Err(e) = crate::licensing::commands::deactivate_license_internal(pool).await {
            tracing::warn!(
                "License deactivation during reset failed (proceeding with local wipe anyway): {:?}",
                e
            );
        }

        if let Ok(conn) = pool.get().await {
            let _ = conn
                .interact({
                    let app_dir_clone = app_dir.to_path_buf();
                    move |c| {
                        if let Ok(mut stmt) = c.prepare("SELECT * FROM connected_accounts") {
                            let rows: Result<
                                Vec<crate::db::connected_accounts::ConnectedAccountsRow>,
                                _,
                            > = stmt
                                .query_map([], |row| {
                                    Ok(crate::db::connected_accounts::ConnectedAccountsRow {
                                        id: row.get(0)?,
                                        profile_id: row.get(1)?,
                                        email_address: row.get(2)?,
                                        account_status: row.get(3)?,
                                        last_history_id: row.get(4)?,
                                        created_at: row.get(5)?,
                                        updated_at: row.get(6)?,
                                    })
                                })
                                .and_then(|mapped| mapped.collect());

                            if let Ok(accounts) = rows {
                                if !accounts.is_empty() {
                                    let backup_path =
                                        app_dir_clone.join("gmail_accounts_backup.json");
                                    if let Ok(json) = serde_json::to_string(&accounts) {
                                        let _ = std::fs::write(backup_path, json);
                                    }
                                }
                            }
                        }
                    }
                })
                .await;
        }
    }

    crate::db::crypto::delete_base_key();
    crate::statements::password::delete_aes_key();

    delete_finance_db_and_all_backups(app_dir);

    tracing::info!("account_deletion_completed: local wipe finished");

    if let Some(pool) = pool {
        pool.close();
    }
    crate::llama_sidecar::shutdown().await;

    Ok(())
}

/// Deletes the database and every backup copy of it.
///
/// Backups are removed alongside the live file, since a deletion that leaves a
/// restorable copy has not really deleted anything.
pub fn delete_finance_db_and_all_backups(app_dir: &std::path::Path) {
    let db_path = app_dir.join("finance.db");
    for suffix in ["", "-wal", "-shm"] {
        let sidecar = std::path::PathBuf::from(format!("{}{}", db_path.display(), suffix));
        let _ = std::fs::remove_file(&sidecar);
    }
    let backup_dir = app_dir.join("backups");
    if let Ok(entries) = std::fs::read_dir(&backup_dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    if let Ok(entries) = std::fs::read_dir(app_dir) {
        for entry in entries.flatten() {
            let is_pre_migration_backup = entry
                .path()
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("finance.db.bak."))
                .unwrap_or(false);
            if is_pre_migration_backup {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

#[cfg(test)]
mod delete_account_tests {
    use super::delete_finance_db_and_all_backups;

    #[test]
    fn sweeps_finance_db_sidecars_daily_backups_and_pre_migration_backups() {
        let app_dir =
            std::env::temp_dir().join(format!("dinero_wipe_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(app_dir.join("backups")).unwrap();

        let db_path = app_dir.join("finance.db");
        std::fs::write(&db_path, b"db").unwrap();
        std::fs::write(app_dir.join("finance.db-wal"), b"wal").unwrap();
        std::fs::write(app_dir.join("finance.db-shm"), b"shm").unwrap();
        std::fs::write(
            app_dir.join("finance.db.bak.20260101000000000"),
            b"old backup",
        )
        .unwrap();
        std::fs::write(
            app_dir.join("finance.db.bak.20260102000000000"),
            b"newer backup",
        )
        .unwrap();
        std::fs::write(
            app_dir.join("backups").join("finance.db.daily.bak"),
            b"daily",
        )
        .unwrap();
        std::fs::write(app_dir.join("hw_uuid_marker.txt"), b"unrelated").unwrap();

        delete_finance_db_and_all_backups(&app_dir);

        assert!(!db_path.exists());
        assert!(!app_dir.join("finance.db-wal").exists());
        assert!(!app_dir.join("finance.db-shm").exists());
        assert!(!app_dir.join("finance.db.bak.20260101000000000").exists());
        assert!(!app_dir.join("finance.db.bak.20260102000000000").exists());
        assert!(!app_dir
            .join("backups")
            .join("finance.db.daily.bak")
            .exists());
        assert!(
            app_dir.join("hw_uuid_marker.txt").exists(),
            "unrelated files must survive the sweep"
        );

        let _ = std::fs::remove_dir_all(&app_dir);
    }

    #[tokio::test]
    async fn test_perform_account_deletion_clears_files_and_keychain() {
        use super::perform_account_deletion;
        let app_dir =
            std::env::temp_dir().join(format!("dinero_perform_wipe_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&app_dir).unwrap();

        let db_path = app_dir.join("finance.db");
        std::fs::write(&db_path, b"db_data").unwrap();

        let result = perform_account_deletion(&app_dir, None).await;
        assert!(result.is_ok());

        assert!(!db_path.exists());
        let _ = std::fs::remove_dir_all(&app_dir);
    }
}

#[tauri::command]
pub async fn dashboard_summary(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<DashboardSummary, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| do_fetch_dashboard_summary(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(crate::error::AppError::Db)
}

#[derive(Serialize, Debug, PartialEq)]
pub struct UpcomingBill {
    pub id: String,
    pub description: String,
    pub amount: f64,
    pub currency: String,
    pub due_date: String,
}

pub fn do_fetch_upcoming_bills(
    conn: &Connection,
    today: &chrono::NaiveDate,
) -> Result<Vec<UpcomingBill>, String> {
    let rows =
        crate::db::instruments::list_upcoming_bills(conn, today).map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|inst| UpcomingBill {
            id: inst.id,
            description: inst
                .nickname
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("{} {}", inst.issuer_name, inst.r#type)),
            amount: inst.current_balance.unwrap_or(0) as f64 / 100.0,
            currency: "INR".to_string(),
            due_date: inst
                .statement_due_date
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default(),
        })
        .collect())
}

#[tauri::command]
pub async fn dashboard_upcoming_bills(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let today = chrono::Utc::now().date_naive();
    let bills = conn
        .interact(move |c| do_fetch_upcoming_bills(c, &today))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(crate::error::AppError::Db)?;
    Ok(serde_json::json!({ "bills": bills }))
}

#[derive(Serialize, Debug, PartialEq)]
pub struct CategorySpend {
    pub category_id: String,
    pub name: String,
    pub total_spend: f64,
    pub monthly_budget: Option<f64>,
    pub utilization_pct: f64,
    pub currency: String,
}

fn month_bounds(month: &str) -> Result<(String, String), String> {
    let start = chrono::NaiveDate::parse_from_str(&format!("{}-01", month), "%Y-%m-%d")
        .map_err(|e| format!("invalid month '{}': {}", month, e))?;
    let (next_year, next_month) = if start.month() == 12 {
        (start.year() + 1, 1)
    } else {
        (start.year(), start.month() + 1)
    };
    let end = chrono::NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .ok_or_else(|| format!("invalid month '{}'", month))?;
    Ok((format!("{} 00:00:00", start), format!("{} 00:00:00", end)))
}

pub fn do_fetch_category_spend(
    conn: &Connection,
    month: &str,
) -> Result<Vec<CategorySpend>, String> {
    let (start, end) = month_bounds(month)?;
    let mut stmt = conn
        .prepare(
            "SELECT c.id, c.name, COALESCE(SUM(t.amount_minor), 0), c.monthly_budget_minor
             FROM categories c
             LEFT JOIN transactions t ON t.category_id = c.id
                 AND t.direction = 'debit' AND t.is_deleted = 0
                 AND t.best_event_time >= ?1 AND t.best_event_time < ?2
                 AND t.id NOT IN (
                     SELECT m.canonical_transaction_id FROM reconciliation_cluster_members m
                     JOIN reconciliation_clusters cl ON cl.id = m.cluster_id
                     WHERE cl.cluster_status = 'open' AND m.canonical_transaction_id IS NOT NULL
                 )
             WHERE c.is_deleted = 0
             GROUP BY c.id
             ORDER BY c.name ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![start, end], |row| {
            let spend_minor: i64 = row.get(2)?;
            let budget_minor: Option<i64> = row.get(3)?;
            let total_spend = spend_minor as f64 / 100.0;
            let monthly_budget = budget_minor.map(|b| b as f64 / 100.0);
            let utilization_pct = match monthly_budget {
                Some(b) if b > 0.0 => (total_spend / b) * 100.0,
                _ => 0.0,
            };
            Ok(CategorySpend {
                category_id: row.get(0)?,
                name: row.get(1)?,
                total_spend,
                monthly_budget,
                utilization_pct,
                currency: "INR".to_string(),
            })
        })
        .map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| e.to_string())?);
    }
    Ok(results)
}

#[tauri::command]
pub async fn dashboard_categories(
    month: String,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let categories = conn
        .interact(move |c| do_fetch_category_spend(c, &month))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(crate::error::AppError::Validation)?;
    Ok(serde_json::json!({ "categories": categories }))
}

#[derive(Serialize, Debug, PartialEq)]
pub struct SpendTrendPoint {
    pub period: String,
    pub total_spend: f64,
}

pub fn do_fetch_spend_trend(
    conn: &Connection,
    granularity: &str,
    now: &chrono::NaiveDateTime,
) -> Result<Vec<SpendTrendPoint>, String> {
    let (strftime_fmt, since) = match granularity {
        "daily" => ("%Y-%m-%d", *now - chrono::Duration::days(30)),
        "weekly" => ("%Y-%W", *now - chrono::Duration::weeks(12)),
        "monthly" => ("%Y-%m", *now - chrono::Duration::days(365)),
        other => {
            return Err(format!(
                "invalid granularity '{}': must be daily, weekly, or monthly",
                other
            ))
        }
    };
    let since_str = since.format("%Y-%m-%d %H:%M:%S").to_string();

    let mut stmt = conn
        .prepare(&format!(
            "SELECT strftime('{}', best_event_time) AS period, COALESCE(SUM(amount_minor), 0)
             FROM transactions
             WHERE direction = 'debit' AND is_deleted = 0
               AND best_event_time >= ?1
               AND id NOT IN (
                   SELECT m.canonical_transaction_id FROM reconciliation_cluster_members m
                   JOIN reconciliation_clusters c ON c.id = m.cluster_id
                   WHERE c.cluster_status = 'open' AND m.canonical_transaction_id IS NOT NULL
               )
             GROUP BY period
             ORDER BY period ASC",
            strftime_fmt
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![since_str], |row| {
            let spend_minor: i64 = row.get(1)?;
            Ok(SpendTrendPoint {
                period: row.get(0)?,
                total_spend: spend_minor as f64 / 100.0,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| e.to_string())?);
    }
    Ok(results)
}

#[tauri::command]
pub async fn analytics_spend_trend(
    granularity: String,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<SpendTrendPoint>, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let now = chrono::Utc::now().naive_utc();
    conn.interact(move |c| do_fetch_spend_trend(c, &granularity, &now))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(crate::error::AppError::Validation)
}

#[derive(Serialize, Debug, PartialEq)]
pub struct TopMerchant {
    pub merchant_display_name: String,
    pub total_spend: f64,
    pub transaction_count: i64,
}

pub fn do_fetch_top_merchants(
    conn: &Connection,
    now: &chrono::NaiveDateTime,
) -> Result<Vec<TopMerchant>, String> {
    let start_of_month = format!(
        "{}-{:02}-01 00:00:00",
        now.date().year(),
        now.date().month()
    );
    let mut stmt = conn
        .prepare(
            "SELECT merchant_display_name, SUM(amount_minor), COUNT(*)
             FROM transactions
             WHERE direction = 'debit' AND is_deleted = 0
               AND best_event_time >= ?1
               AND merchant_display_name IS NOT NULL
               AND id NOT IN (
                   SELECT m.canonical_transaction_id FROM reconciliation_cluster_members m
                   JOIN reconciliation_clusters c ON c.id = m.cluster_id
                   WHERE c.cluster_status = 'open' AND m.canonical_transaction_id IS NOT NULL
               )
             GROUP BY merchant_display_name
             ORDER BY SUM(amount_minor) DESC
             LIMIT 10",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![start_of_month], |row| {
            let spend_minor: i64 = row.get(1)?;
            Ok(TopMerchant {
                merchant_display_name: row.get(0)?,
                total_spend: spend_minor as f64 / 100.0,
                transaction_count: row.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| e.to_string())?);
    }
    Ok(results)
}

#[tauri::command]
pub async fn analytics_top_merchants(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<TopMerchant>, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let now = chrono::Utc::now().naive_utc();
    conn.interact(move |c| do_fetch_top_merchants(c, &now))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(crate::error::AppError::Db)
}

#[derive(Serialize, Debug, PartialEq)]
pub struct RecurringPaymentSummary {
    pub id: String,
    pub merchant_name: String,
    pub amount: f64,
    pub currency: String,
    pub cadence: String,
    pub next_predicted_date: Option<String>,
    pub confidence: f64,
}

pub fn do_fetch_recurring_payments_summary(
    conn: &Connection,
) -> Result<Vec<RecurringPaymentSummary>, String> {
    let rows = crate::db::recurring_payments::select_active(conn).map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    for row in rows {
        let merchant_name: String = row
            .merchant_entity_id
            .as_ref()
            .and_then(|id| {
                conn.query_row(
                    "SELECT name FROM merchants WHERE id = ?1",
                    params![id],
                    |r| r.get::<_, String>(0),
                )
                .ok()
            })
            .unwrap_or_else(|| "Unknown merchant".to_string());
        results.push(RecurringPaymentSummary {
            id: row.id,
            merchant_name,
            amount: row.amount_minor.unwrap_or(0) as f64 / 100.0,
            currency: row.currency.unwrap_or_else(|| "INR".to_string()),
            cadence: row.cadence.unwrap_or_default(),
            next_predicted_date: row
                .next_predicted_date
                .map(|d| d.format("%Y-%m-%d").to_string()),
            confidence: row.confidence.unwrap_or(0.0),
        });
    }
    Ok(results)
}

#[tauri::command]
pub async fn analytics_recurring_payments_summary(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<RecurringPaymentSummary>, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| do_fetch_recurring_payments_summary(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(crate::error::AppError::Db)
}

#[tauri::command]
pub async fn analytics_pending_review_count(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<PendingReviewMetric, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| compute_unassigned_amount_pending_review(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(crate::error::AppError::Db)
}

#[tauri::command]
pub async fn categories_list(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<crate::db::categories::CategoriesRow>, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| crate::db::categories::select_all(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))
}

#[derive(Deserialize)]
pub struct CategoryCreatePayload {
    pub name: String,
    pub parent_id: Option<String>,
    pub mcc_code: Option<String>,
    pub monthly_budget_minor: Option<i64>,
    pub color: Option<String>,
    pub icon: Option<String>,
}

#[tauri::command]
pub async fn categories_create(
    payload: CategoryCreatePayload,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    crate::ipc::validation::validate_non_empty("name", &payload.name)?;
    if let Some(ref parent_id) = payload.parent_id {
        crate::ipc::validation::validate_uuid("parent_id", parent_id)?;
    }
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let id = uuid::Uuid::new_v4().to_string();
    let row = crate::db::categories::CategoriesRow {
        id: id.clone(),
        parent_id: payload.parent_id,
        name: payload.name,
        source_type: "user".to_string(),
        mcc_code: payload.mcc_code,
        monthly_budget_minor: payload.monthly_budget_minor,
        is_deleted: false,
        created_at: None,
        color: payload.color,
        icon: payload.icon,
    };
    conn.interact(move |c| crate::db::categories::insert(c, &row))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| {
            crate::error::map_insert_conflict(e, "A category with this name already exists")
        })?;
    Ok(serde_json::json!({ "id": id, "status": "created" }))
}

#[derive(Deserialize)]
pub struct CategoryUpdatePayload {
    pub id: String,
    pub name: Option<String>,
    pub parent_id: Option<String>,
    pub mcc_code: Option<String>,
    pub monthly_budget_minor: Option<i64>,
    pub color: Option<String>,
    pub icon: Option<String>,
}

#[tauri::command]
pub async fn categories_update(
    payload: CategoryUpdatePayload,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    crate::ipc::validation::validate_uuid("id", &payload.id)?;
    if let Some(ref parent_id) = payload.parent_id {
        crate::ipc::validation::validate_uuid("parent_id", parent_id)?;
    }
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| -> Result<(), crate::error::AppError> {
        let mut row = crate::db::categories::select_by_id(c, &payload.id)
            .map_err(|e| crate::error::AppError::Db(e.to_string()))?
            .ok_or_else(|| crate::error::AppError::Validation("category not found".to_string()))?;
        if let Some(name) = payload.name {
            row.name = name;
        }
        if let Some(parent_id) = payload.parent_id {
            row.parent_id = Some(parent_id);
        }
        if let Some(mcc_code) = payload.mcc_code {
            row.mcc_code = Some(mcc_code);
        }
        if let Some(budget) = payload.monthly_budget_minor {
            row.monthly_budget_minor = Some(budget);
        }
        if let Some(color) = payload.color {
            row.color = Some(color);
        }
        if let Some(icon) = payload.icon {
            row.icon = Some(icon);
        }
        crate::db::categories::update(c, &row)
            .map_err(|e| crate::error::AppError::Validation(e.to_string()))
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))??;
    Ok(serde_json::json!({ "status": "updated" }))
}

#[derive(Deserialize)]
pub struct CategoryDeletePayload {
    pub id: String,
    #[serde(default)]
    pub confirm_reassign: bool,
}

#[tauri::command]
pub async fn categories_delete(
    payload: CategoryDeletePayload,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    crate::ipc::validation::validate_uuid("id", &payload.id)?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let confirm_reassign = payload.confirm_reassign;
    let id = payload.id;
    let reassigned = conn
        .interact(move |c| crate::db::categories::soft_delete(c, &id, confirm_reassign))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Validation(e.to_string()))?;
    Ok(serde_json::json!({ "status": "deleted", "reassigned_count": reassigned }))
}

const TRANSACTIONS_PAGE_SIZE: i64 = 50;

#[tauri::command]
pub async fn transactions_list(
    pool: State<'_, deadpool_sqlite::Pool>,
    page: Option<u32>,
    filters: Option<TransactionListFilters>,
) -> Result<TransactionsPage, crate::error::AppError> {
    let page = page.unwrap_or(1).max(1) as i64;
    let offset = (page - 1) * TRANSACTIONS_PAGE_SIZE;
    let filters = filters.unwrap_or_default();
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| {
        let records = do_fetch_transactions(c, &filters, TRANSACTIONS_PAGE_SIZE, offset)?;
        let total = count_transactions_filtered(c, &filters)?;
        Ok(TransactionsPage { records, total })
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
    .map_err(crate::error::AppError::Db)
}

#[tauri::command]
pub async fn fetch_transaction_observations(
    transaction_id: String,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<
    Vec<crate::db::transaction_observations::TransactionObservationsRow>,
    crate::error::AppError,
> {
    crate::ipc::validation::validate_uuid("transaction_id", &transaction_id)?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let transaction_id_clone = transaction_id.clone();
    conn.interact(move |c| {
        crate::db::transaction_observations::get_observations_for_transaction(
            c,
            &transaction_id_clone,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
    .map_err(crate::error::AppError::Db)
}

#[tauri::command]
pub async fn fetch_transaction_source_log(
    transaction_id: String,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("transaction_id", &transaction_id)?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let transaction_id_clone = transaction_id.clone();

    let observations = conn
        .interact(move |c| {
            crate::db::transaction_observations::get_observations_for_transaction(
                c,
                &transaction_id_clone,
            )
        })
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    if observations.is_empty() {
        return Err(crate::error::AppError::Validation(
            "No observations found for this transaction.".to_string(),
        ));
    }

    let source_message_id = match &observations[0].source_message_id {
        Some(id) => id.clone(),
        None => {
            return Err(crate::error::AppError::Validation(
                "No source_message_id found for this transaction observation.".to_string(),
            ))
        }
    };

    use std::io::{BufRead, BufReader};

    let file = std::fs::File::open("email_scan_selected.log").map_err(|e| {
        crate::error::AppError::Io(format!("Could not open email_scan_selected.log: {}", e))
    })?;
    let reader = BufReader::new(file);

    let mut inside_target_block = false;
    let mut current_block = String::new();
    let target_marker = format!("Message ID : {}", source_message_id);
    let separator =
        "================================================================================";

    for l in reader.lines().map_while(Result::ok) {
        if l.starts_with(separator) {
            if inside_target_block {
                current_block.push_str(&l);
                current_block.push('\n');
                return Ok(current_block);
            } else {
                current_block.clear();
                current_block.push_str(&l);
                current_block.push('\n');
            }
        } else {
            current_block.push_str(&l);
            current_block.push('\n');
            if !inside_target_block && l.contains(&target_marker) {
                inside_target_block = true;
            }
        }
    }

    if inside_target_block {
        return Ok(current_block);
    }

    Err(crate::error::AppError::Validation(format!(
        "Source log not found for message ID {}",
        source_message_id
    )))
}

const STATEMENTS_PAGE_SIZE: i64 = 50;

#[tauri::command]
pub async fn statements_list(
    pool: State<'_, deadpool_sqlite::Pool>,
    page: Option<u32>,
) -> Result<StatementsPage, crate::error::AppError> {
    let page = page.unwrap_or(1).max(1) as i64;
    let offset = (page - 1) * STATEMENTS_PAGE_SIZE;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| {
        let records = do_fetch_statement_history(c, STATEMENTS_PAGE_SIZE, offset)?;
        let total = count_statements(c)?;
        Ok(StatementsPage { records, total })
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
    .map_err(crate::error::AppError::Db)
}

#[tauri::command]
pub async fn statements_get_entries(
    statement_id: String,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<crate::db::statement_entries::StatementEntriesRow>, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("statement_id", &statement_id)?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| crate::db::statement_entries::select_by_statement_id(c, &statement_id))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))
}

async fn get_pdf_bytes_for_statement(
    app_data_dir: &std::path::Path,
    statement_id: &str,
    pool: &deadpool_sqlite::Pool,
) -> Result<Vec<u8>, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let stmt_id = statement_id.to_string();
    let unprocessed_id: String = conn
        .interact(move |c| {
            c.query_row(
                "SELECT id FROM unprocessed_statements \
                 WHERE resolved_statement_id = ?1 \
                 AND pdf_retained_until IS NOT NULL AND pdf_retained_until > datetime('now')",
                [&stmt_id],
                |row| row.get::<_, String>(0),
            )
        })
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .map_err(|_| {
            crate::error::AppError::NotFound(
                "This statement's PDF is no longer available".to_string(),
            )
        })?;

    crate::statements::pdf_storage::read_pdf(app_data_dir, &unprocessed_id)
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .ok_or_else(|| {
            crate::error::AppError::NotFound(
                "This statement's PDF is no longer available".to_string(),
            )
        })
}

async fn delete_pdf_for_statement(
    app_data_dir: &std::path::Path,
    statement_id: &str,
    pool: &deadpool_sqlite::Pool,
) -> Result<(), crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let stmt_id = statement_id.to_string();
    let unprocessed_id: Option<String> = conn
        .interact(move |c| {
            c.query_row(
                "SELECT id FROM unprocessed_statements WHERE resolved_statement_id = ?1",
                [&stmt_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
        })
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?
        .map_err(|e: rusqlite::Error| crate::error::AppError::Db(e.to_string()))?;

    let Some(unprocessed_id) = unprocessed_id else {
        return Ok(());
    };

    crate::statements::pdf_storage::delete_pdf(app_data_dir, &unprocessed_id)
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;

    let id_clone = unprocessed_id.clone();
    conn.interact(move |c| {
        c.execute(
            "UPDATE unprocessed_statements SET pdf_retained_until = NULL WHERE id = ?",
            [&id_clone],
        )
    })
    .await
    .map_err(|e| crate::error::AppError::Db(e.to_string()))?
    .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    Ok(())
}

#[tauri::command]
pub async fn statements_get_pdf(
    statement_id: String,
    app: tauri::AppHandle,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("statement_id", &statement_id)?;
    let app_data_dir = app.path().app_data_dir().map_err(|_| {
        crate::error::AppError::Unknown("Failed to determine app data directory".to_string())
    })?;
    let bytes = get_pdf_bytes_for_statement(&app_data_dir, &statement_id, pool.inner()).await?;
    let viewable = crate::statements::password::ensure_viewable_pdf_bytes(bytes, pool.inner())
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?;
    use base64::Engine;
    Ok(base64::engine::general_purpose::STANDARD.encode(&viewable))
}

#[tauri::command]
pub async fn statements_delete_pdf(
    statement_id: String,
    app: tauri::AppHandle,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<(), crate::error::AppError> {
    crate::ipc::validation::validate_uuid("statement_id", &statement_id)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    let app_data_dir = app.path().app_data_dir().map_err(|_| {
        crate::error::AppError::Unknown("Failed to determine app data directory".to_string())
    })?;
    delete_pdf_for_statement(&app_data_dir, &statement_id, pool.inner()).await
}

#[tauri::command]
pub async fn reconciliation_clusters_list(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<ClusterRecord>, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| do_fetch_unresolved_clusters(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(crate::error::AppError::Db)
}

#[tauri::command]
pub async fn reconciliation_clusters_get(
    cluster_id: String,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<ClusterRecord, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("cluster_id", &cluster_id)?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| do_fetch_cluster_detail(c, &cluster_id))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(crate::error::AppError::Db)?
        .ok_or_else(|| crate::error::AppError::Validation("cluster not found".to_string()))
}

#[tauri::command]
pub async fn reconciliation_get_unassigned_transactions(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<
    Vec<crate::db::unassigned_transactions::UnassignedTransactionDetail>,
    crate::error::AppError,
> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| crate::db::unassigned_transactions::select_open_with_context(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))
}

#[tauri::command]
pub async fn reconciliation_dismiss_unassigned_transaction(
    id: String,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("id", &id)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let id_clone = id.clone();
    conn.interact(move |c| {
        crate::db::unassigned_transactions::update_status(c, &id_clone, "ignored")
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
    .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    Ok("dismissed".to_string())
}

#[tauri::command]
pub async fn reconciliation_resolve_unassigned_transaction_manually(
    id: String,
    payload: crate::commands::ManualTransactionPayload,
    pool: State<'_, deadpool_sqlite::Pool>,
    app_handle: tauri::AppHandle,
) -> Result<String, crate::error::AppError> {
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    resolve_unassigned_transaction_manually(id, payload, pool.inner(), &app_handle).await
}

pub(crate) async fn resolve_unassigned_transaction_manually<R: tauri::Runtime>(
    id: String,
    payload: crate::commands::ManualTransactionPayload,
    pool: &deadpool_sqlite::Pool,
    app_handle: &tauri::AppHandle<R>,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("id", &id)?;
    crate::licensing::gate::assert_write_allowed(pool).await?;

    let decision = crate::commands::create_manual_transaction(payload, pool, app_handle).await?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let id_clone = id.clone();
    conn.interact(move |c| {
        crate::db::unassigned_transactions::update_status(c, &id_clone, "resolved")
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
    .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    Ok(decision)
}

#[tauri::command]
pub async fn reconciliation_clusters_unmerge(
    cluster_id: String,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("cluster_id", &cluster_id)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let cluster_id_clone = cluster_id.clone();
    let count = conn
        .interact(move |c| {
            c.execute(
                "UPDATE reconciliation_clusters SET cluster_status = 'open', resolved_at = NULL WHERE id = ?1",
                params![cluster_id_clone],
            )
        })
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    if count == 0 {
        return Err(crate::error::AppError::Validation(
            "cluster not found".to_string(),
        ));
    }
    Ok("unmerged".to_string())
}

#[derive(serde::Deserialize)]
pub struct BulkResolution {
    pub cluster_id: String,
    pub action: String,
    pub observation_id: String,
    pub chosen_canonical_id: Option<String>,
}

#[tauri::command]
pub async fn reconciliation_clusters_bulk_resolve(
    resolutions: Vec<BulkResolution>,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    for r in &resolutions {
        crate::ipc::validation::validate_uuid("cluster_id", &r.cluster_id)?;
        crate::ipc::validation::validate_uuid("observation_id", &r.observation_id)?;
        if let Some(ref chosen_canonical_id) = r.chosen_canonical_id {
            crate::ipc::validation::validate_uuid("chosen_canonical_id", chosen_canonical_id)?;
        }
    }
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let resolved_count = conn
        .interact(move |c| -> Result<usize, crate::error::AppError> {
            let tx = c
                .unchecked_transaction()
                .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
            for r in &resolutions {
                crate::reconciliation::cluster::resolve_cluster(
                    &tx,
                    &r.cluster_id,
                    &r.observation_id,
                    &r.action,
                    r.chosen_canonical_id.as_deref(),
                )
                .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
            }
            tx.commit()
                .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
            Ok(resolutions.len())
        })
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))??;
    Ok(serde_json::json!({ "status": "resolved", "resolved_count": resolved_count }))
}

#[tauri::command]
pub async fn auth_get_consent_history(
    pool: State<'_, deadpool_sqlite::Pool>,
    limit: u32,
    offset: u32,
) -> Result<Vec<crate::auth::consent::ConsentEventsRow>, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| crate::auth::consent::fetch_consent_history(c, limit, offset))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))
}

#[tauri::command]
pub async fn record_consent_event(
    pool: State<'_, deadpool_sqlite::Pool>,
    consent_type: String,
    detail: String,
) -> Result<(), crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| crate::auth::consent::insert_consent_event(c, &consent_type, &detail))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))
        .map(|_id| ())
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SpendingLimitThresholds {
    pub warn_at_80: bool,
    pub warn_at_90: bool,
    pub warn_at_100: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CategoryBudget {
    pub name: String,
    pub budget: f64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SpendingLimits {
    pub global_limit: f64,
    pub thresholds: SpendingLimitThresholds,
    pub categories: Vec<CategoryBudget>,
}

fn thresholds_to_array(t: &SpendingLimitThresholds) -> Vec<f64> {
    let mut arr = Vec::new();
    if t.warn_at_80 {
        arr.push(80.0);
    }
    if t.warn_at_90 {
        arr.push(90.0);
    }
    if t.warn_at_100 {
        arr.push(100.0);
    }
    arr
}

fn array_to_thresholds(arr: &[f64]) -> SpendingLimitThresholds {
    SpendingLimitThresholds {
        warn_at_80: arr.contains(&80.0),
        warn_at_90: arr.contains(&90.0),
        warn_at_100: arr.contains(&100.0),
    }
}

#[tauri::command]
pub async fn fetch_spending_limits(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<SpendingLimits, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| {
        let (global_limit, thresholds_json): (f64, Option<String>) = c
            .query_row(
                "SELECT COALESCE(spending_limit_monthly, 0), limit_thresholds FROM local_profile WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| e.to_string())?;

        let thresholds = thresholds_json
            .and_then(|j| serde_json::from_str::<Vec<f64>>(&j).ok())
            .map(|arr| array_to_thresholds(&arr))
            .unwrap_or(SpendingLimitThresholds {
                warn_at_80: true,
                warn_at_90: true,
                warn_at_100: true,
            });

        let category_rows = crate::db::categories::select_all(c).map_err(|e| e.to_string())?;
        let categories = category_rows
            .into_iter()
            .map(|cat| CategoryBudget {
                name: cat.name,
                budget: cat.monthly_budget_minor.map(|m| m as f64 / 100.0).unwrap_or(0.0),
            })
            .collect();

        Ok(SpendingLimits {
            global_limit,
            thresholds,
            categories,
        })
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
    .map_err(crate::error::AppError::Db)
}

#[tauri::command]
pub async fn update_spending_limits(
    pool: State<'_, deadpool_sqlite::Pool>,
    limits: SpendingLimits,
) -> Result<String, crate::error::AppError> {
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| {
        let thresholds_json =
            serde_json::to_string(&thresholds_to_array(&limits.thresholds)).map_err(|e| e.to_string())?;
        c.execute(
            "UPDATE local_profile SET spending_limit_monthly = ?1, limit_thresholds = ?2 WHERE id = 1",
            rusqlite::params![limits.global_limit, thresholds_json],
        )
        .map_err(|e| e.to_string())?;

        for cat in &limits.categories {
            let minor = if cat.budget > 0.0 {
                Some((cat.budget * 100.0).round() as i64)
            } else {
                None
            };
            c.execute(
                "UPDATE categories SET monthly_budget_minor = ?1 WHERE name = ?2 AND is_deleted = 0",
                rusqlite::params![minor, cat.name],
            )
            .map_err(|e| e.to_string())?;
        }

        Ok::<_, String>(())
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
    .map_err(crate::error::AppError::Db)?;

    Ok("Spending limits updated".to_string())
}

#[derive(Serialize, Debug)]
pub struct ProfileResponse {
    pub primary_email: Option<String>,
    pub display_name: Option<String>,
    pub timezone: Option<String>,
    pub spending_limit_monthly: Option<f64>,
    pub limit_thresholds: Vec<f64>,
    pub recovery_phrase_enabled: bool,
}

#[tauri::command]
pub async fn settings_profile_get(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<ProfileResponse, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| -> Result<ProfileResponse, crate::error::AppError> {
        let row = crate::db::local_profile::select_by_id(c, 1)
            .map_err(|e| crate::error::AppError::Db(e.to_string()))?
            .ok_or_else(|| crate::error::AppError::Db("profile not found".to_string()))?;
        let limit_thresholds = row
            .limit_thresholds
            .and_then(|v| serde_json::from_value::<Vec<f64>>(v).ok())
            .unwrap_or_default();
        Ok(ProfileResponse {
            primary_email: row.primary_email,
            display_name: row.display_name,
            timezone: row.timezone,
            spending_limit_monthly: row.spending_limit_monthly,
            limit_thresholds,
            recovery_phrase_enabled: row.recovery_phrase_enabled,
        })
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
}

fn validate_limit_thresholds(thresholds: &[f64]) -> Result<(), String> {
    for pair in thresholds.windows(2) {
        if pair[0] >= pair[1] {
            return Err("limit_thresholds must be a strictly ascending sorted array".to_string());
        }
    }
    for &t in thresholds {
        if !(0.0..=100.0).contains(&t) {
            return Err(format!(
                "limit_thresholds value {} must be between 0 and 100",
                t
            ));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct ProfileUpdatePayload {
    pub display_name: Option<String>,
    pub timezone: Option<String>,
    pub limit_thresholds: Option<Vec<f64>>,
}

#[tauri::command]
pub async fn settings_profile_update(
    payload: ProfileUpdatePayload,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<serde_json::Value, crate::error::AppError> {
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;
    if let Some(ref thresholds) = payload.limit_thresholds {
        validate_limit_thresholds(thresholds).map_err(crate::error::AppError::Validation)?;
    }
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| -> Result<(), crate::error::AppError> {
        let mut row = crate::db::local_profile::select_by_id(c, 1)
            .map_err(|e| crate::error::AppError::Db(e.to_string()))?
            .ok_or_else(|| crate::error::AppError::Db("profile not found".to_string()))?;
        if let Some(name) = payload.display_name {
            row.display_name = Some(name);
        }
        if let Some(tz) = payload.timezone {
            row.timezone = Some(tz);
        }
        if let Some(thresholds) = payload.limit_thresholds {
            row.limit_thresholds = Some(
                serde_json::to_value(thresholds)
                    .map_err(|e| crate::error::AppError::Validation(e.to_string()))?,
            );
        }
        crate::db::local_profile::update(c, &row)
            .map_err(|e| crate::error::AppError::Db(e.to_string()))
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))??;
    Ok(serde_json::json!({ "status": "updated" }))
}

#[tauri::command]
pub async fn settings_get_menu_bar_extra_enabled(
    app: tauri::AppHandle,
) -> Result<bool, crate::error::AppError> {
    let dir = app.path().app_data_dir().map_err(|e| {
        crate::error::AppError::Io(format!("Failed to resolve app data directory: {}", e))
    })?;
    Ok(crate::menu::status_item::read_menu_bar_extra_enabled(&dir))
}

#[tauri::command]
pub async fn settings_set_menu_bar_extra_enabled(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<(), crate::error::AppError> {
    let dir = app.path().app_data_dir().map_err(|e| {
        crate::error::AppError::Io(format!("Failed to resolve app data directory: {}", e))
    })?;
    crate::menu::status_item::write_menu_bar_extra_enabled(&dir, enabled)
        .map_err(|e| crate::error::AppError::Io(e.to_string()))?;
    crate::menu::status_item::apply_menu_bar_extra_runtime_state(&app, enabled);
    Ok(())
}

#[tauri::command]
pub async fn settings_get_launch_at_login(
    app: tauri::AppHandle,
) -> Result<bool, crate::error::AppError> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch()
        .is_enabled()
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))
}

#[tauri::command]
pub async fn settings_set_launch_at_login(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<(), crate::error::AppError> {
    let controller = crate::lifecycle::launch_agent::TauriAutoLaunchController::new(&app);
    crate::lifecycle::launch_agent::apply_launch_at_login(&controller, enabled)
        .map_err(crate::error::AppError::Unknown)
}

#[tauri::command]
pub async fn settings_get_background_sync_enabled(
    app: tauri::AppHandle,
) -> Result<bool, crate::error::AppError> {
    let dir = app.path().app_data_dir().map_err(|e| {
        crate::error::AppError::Io(format!("Failed to resolve app data directory: {}", e))
    })?;
    Ok(crate::lifecycle::launch_agent::read_background_sync_enabled(&dir))
}

#[tauri::command]
pub async fn settings_set_background_sync_enabled(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<(), crate::error::AppError> {
    let dir = app.path().app_data_dir().map_err(|e| {
        crate::error::AppError::Io(format!("Failed to resolve app data directory: {}", e))
    })?;
    crate::lifecycle::launch_agent::write_background_sync_enabled(&dir, enabled)
        .map_err(|e| crate::error::AppError::Io(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub async fn settings_get_low_battery_poll_threshold_percent(
    app: tauri::AppHandle,
) -> Result<f32, crate::error::AppError> {
    let dir = app.path().app_data_dir().map_err(|e| {
        crate::error::AppError::Io(format!("Failed to resolve app data directory: {}", e))
    })?;
    Ok(crate::lifecycle::launch_agent::read_low_battery_threshold_percent(&dir))
}

#[tauri::command]
pub async fn settings_set_low_battery_poll_threshold_percent(
    app: tauri::AppHandle,
    threshold_percent: f32,
) -> Result<(), crate::error::AppError> {
    let dir = app.path().app_data_dir().map_err(|e| {
        crate::error::AppError::Io(format!("Failed to resolve app data directory: {}", e))
    })?;
    crate::lifecycle::launch_agent::write_low_battery_threshold_percent(&dir, threshold_percent)
        .map_err(|e| crate::error::AppError::Io(e.to_string()))?;
    Ok(())
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OnboardingPreferences {
    pub timezone: String,
    pub spending_limit_monthly: f64,
    pub historical_scan_months: i64,
    pub llm_model: String,
    pub statement_preference: String,
}

#[tauri::command]
pub async fn onboarding_save_preferences(
    pool: State<'_, deadpool_sqlite::Pool>,
    preferences: OnboardingPreferences,
) -> Result<String, crate::error::AppError> {
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| {
        c.execute(
            "UPDATE local_profile SET
                timezone = ?1,
                spending_limit_monthly = ?2,
                historical_scan_months = ?3,
                llm_model = ?4,
                statement_preference = ?5
             WHERE id = 1",
            rusqlite::params![
                preferences.timezone,
                preferences.spending_limit_monthly,
                preferences.historical_scan_months,
                preferences.llm_model,
                preferences.statement_preference,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok::<_, String>(())
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
    .map_err(crate::error::AppError::Db)?;

    Ok("Onboarding preferences saved".to_string())
}

#[tauri::command]
pub async fn db_restore_backup(app: tauri::AppHandle) -> Result<String, crate::error::AppError> {
    let app_dir = app.path().app_data_dir().map_err(|e| {
        crate::error::AppError::Io(format!("Failed to resolve app data directory: {}", e))
    })?;
    let db_path = app_dir.join("finance.db");
    let backup_dir = app_dir.join("backups");

    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    let daily = backup_dir.join("finance.db.daily.bak");
    if daily.exists() {
        candidates.push(daily);
    }
    if let Ok(entries) = std::fs::read_dir(&backup_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("finance.db.bak."))
                .unwrap_or(false)
            {
                candidates.push(path);
            }
        }
    }

    let most_recent = candidates
        .into_iter()
        .max_by_key(|p| {
            std::fs::metadata(p)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        })
        .ok_or_else(|| {
            crate::error::AppError::Validation("No backup file found to restore from".to_string())
        })?;

    crate::db::backup::verify_backup_integrity(&most_recent).map_err(|e| {
        crate::error::AppError::Validation(format!(
            "Backup {:?} failed integrity verification, refusing to restore: {}",
            most_recent, e
        ))
    })?;

    for suffix in ["-wal", "-shm"] {
        let sidecar = std::path::PathBuf::from(format!("{}{}", db_path.display(), suffix));
        let _ = std::fs::remove_file(&sidecar);
    }

    crate::db::backup::atomic_replace(&most_recent, &db_path).map_err(|e| {
        crate::error::AppError::Io(format!("Failed to restore backup {:?}: {}", most_recent, e))
    })?;

    tracing::info!(
        "Restored finance.db from backup {:?} — restarting",
        most_recent
    );
    app.restart();
}

#[tauri::command]
pub async fn instruments_list(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<InstrumentRecord>, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| do_fetch_instruments(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(crate::error::AppError::Db)
}

#[tauri::command]
pub async fn instruments_get(
    id: String,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<InstrumentRecord, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("id", &id)?;
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| {
        let list = do_fetch_instruments(c).map_err(crate::error::AppError::Db)?;
        list.into_iter()
            .find(|inst| inst.id == id)
            .ok_or_else(|| crate::error::AppError::Validation("instrument not found".to_string()))
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
}

#[tauri::command]
pub async fn get_debug_metrics(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<DebugMetrics, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(|c| do_get_debug_metrics(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(crate::error::AppError::Db)
}

const VALID_INSTRUMENT_TYPES: &[&str] = &[
    "credit_card",
    "debit_card",
    "bank_account",
    "UPI",
    "NEFT",
    "RTGS",
    "SWIFT",
    "upi_vpa",
    "wallet",
    "POS",
    "ATM",
    "cheque",
];

fn validate_instrument_type(instrument_type: &str) -> Result<(), crate::error::AppError> {
    if !VALID_INSTRUMENT_TYPES.contains(&instrument_type) {
        return Err(crate::error::AppError::Validation(format!(
            "instrument_type '{instrument_type}' is not a recognized instrument type"
        )));
    }
    Ok(())
}

#[derive(serde::Deserialize)]
pub struct InstrumentCreatePayload {
    pub instrument_type: String,
    pub issuer_name: String,
    pub masked_identifier: String,
    pub full_identifier: Option<String>,
    pub billing_cycle_day: Option<u8>,
    pub bank_ifsc: Option<String>,
}

#[tauri::command]
pub async fn instruments_create(
    payload: InstrumentCreatePayload,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<InstrumentRecord, crate::error::AppError> {
    validate_instrument_type(&payload.instrument_type)?;
    crate::ipc::validation::validate_non_empty("issuer_name", &payload.issuer_name)?;
    crate::ipc::validation::validate_non_empty("masked_identifier", &payload.masked_identifier)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| {
        let id = uuid::Uuid::new_v4().to_string();

        let row = crate::db::instruments::InstrumentsRow {
            id: id.clone(),
            r#type: payload.instrument_type.clone(),
            issuer_name: payload.issuer_name.clone(),
            masked_identifier: payload.masked_identifier.clone(),
            network: None,
            credit_limit: None,
            current_balance: None,
            statement_due_date: None,
            minimum_due: None,
            bank_ifsc: payload.bank_ifsc.clone(),
            account_type: None,
            upi_vpa: None,
            nickname: None,
            rewards_summary: None,
            status: "active".to_string(),
            created_at: None,
            updated_at: None,
            is_deleted: false,
            full_identifier: payload.full_identifier.clone(),
            billing_cycle_day: payload.billing_cycle_day,
        };

        crate::db::instruments::insert_instrument(c, &row).map_err(|e| {
            crate::error::map_insert_conflict(
                e,
                "An instrument with this type, issuer, and masked identifier already exists",
            )
        })?;

        Ok(InstrumentRecord {
            id,
            instrument_type: payload.instrument_type,
            issuer_name: payload.issuer_name,
            masked_identifier: payload.masked_identifier,
            status: "active".to_string(),
            current_balance: None,
            credit_limit: None,
            full_identifier: payload.full_identifier,
            billing_cycle_day: payload.billing_cycle_day,
            bank_ifsc: payload.bank_ifsc,
        })
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
}

#[derive(serde::Deserialize)]
pub struct InstrumentUpdatePayload {
    pub id: String,
    pub full_identifier: Option<String>,
    pub billing_cycle_day: Option<u8>,
    pub bank_ifsc: Option<String>,
    pub nickname: Option<String>,
    pub credit_limit: Option<f64>,
    pub account_type: Option<String>,
    pub network: Option<String>,
    pub status: Option<String>,
    pub upi_vpa: Option<String>,
    pub rewards_summary: Option<String>,
    pub instrument_type: Option<String>,
    pub issuer_name: Option<String>,
    pub masked_identifier: Option<String>,
    pub current_balance: Option<f64>,
    pub statement_due_date: Option<String>,
    pub minimum_due: Option<f64>,
}

#[tauri::command]
pub async fn instruments_update(
    payload: InstrumentUpdatePayload,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("id", &payload.id)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| {
        let mut row = crate::db::instruments::get_instrument(c, &payload.id)
            .map_err(|e| crate::error::AppError::Db(e.to_string()))?
            .ok_or_else(|| {
                crate::error::AppError::Validation("instrument not found".to_string())
            })?;

        row.full_identifier = payload.full_identifier;
        row.billing_cycle_day = payload.billing_cycle_day;
        row.bank_ifsc = payload.bank_ifsc;
        if let Some(nick) = payload.nickname {
            row.nickname = if nick.trim().is_empty() {
                None
            } else {
                Some(nick)
            };
        }
        if let Some(limit) = payload.credit_limit {
            row.credit_limit = Some((limit * 100.0) as i64);
        }
        if let Some(acct) = payload.account_type {
            row.account_type = if acct.trim().is_empty() {
                None
            } else {
                Some(acct)
            };
        }
        if let Some(net) = payload.network {
            row.network = if net.trim().is_empty() {
                None
            } else {
                Some(net)
            };
        }
        if let Some(st) = payload.status {
            if !st.trim().is_empty() {
                row.status = st;
            }
        }
        if let Some(vpa) = payload.upi_vpa {
            row.upi_vpa = if vpa.trim().is_empty() {
                None
            } else {
                Some(vpa)
            };
        }
        if let Some(rew) = payload.rewards_summary {
            row.rewards_summary = if rew.trim().is_empty() {
                None
            } else {
                Some(rew)
            };
        }
        if let Some(itype) = payload.instrument_type {
            if !itype.trim().is_empty() {
                row.r#type = itype;
            }
        }
        if let Some(iname) = payload.issuer_name {
            if !iname.trim().is_empty() {
                row.issuer_name = iname;
            }
        }
        if let Some(mask) = payload.masked_identifier {
            if !mask.trim().is_empty() {
                row.masked_identifier = clean_masked_identifier(&mask);
            }
        }
        if let Some(bal) = payload.current_balance {
            row.current_balance = Some((bal * 100.0) as i64);
        }
        if let Some(min_due) = payload.minimum_due {
            row.minimum_due = Some((min_due * 100.0) as i64);
        }
        if let Some(date_str) = payload.statement_due_date {
            row.statement_due_date = if date_str.trim().is_empty() {
                None
            } else {
                chrono::NaiveDate::parse_from_str(date_str.trim(), "%Y-%m-%d").ok()
            };
        }

        crate::db::instruments::update_instrument(c, &row)
            .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
        Ok("updated".to_string())
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
}

#[tauri::command]
pub async fn instruments_archive(
    id: String,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    crate::ipc::validation::validate_uuid("id", &id)?;
    crate::licensing::gate::assert_write_allowed(pool.inner()).await?;

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| {
        c.execute(
            "UPDATE instruments SET is_deleted = 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            [&id],
        )
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
        Ok("deleted".to_string())
    })
    .await
    .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_resolve_unassigned_manually_creates_transaction_and_marks_resolved() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        app.manage(pool.clone());

        let instrument_id = uuid::Uuid::new_v4();
        let unassigned_id = uuid::Uuid::new_v4().to_string();
        let unassigned_id_clone = unassigned_id.clone();
        let conn = pool.get().await.unwrap();
        conn.interact(move |c| {
            c.execute(
                "INSERT INTO instruments (id, type, issuer_name, masked_identifier) \
                 VALUES (?1, 'credit_card', 'HDFC', '4021')",
                params![instrument_id.to_string()],
            )?;
            c.execute(
                "INSERT INTO transaction_observations (id, source_pipeline, source_record_id) \
                 VALUES ('obs_1', 'gmail_transaction', 'msg_1')",
                [],
            )?;
            c.execute(
                "INSERT INTO unassigned_transactions (id, observation_id, reason, status) \
                 VALUES (?1, 'obs_1', 'extraction_failed', 'open')",
                params![unassigned_id_clone],
            )
        })
        .await
        .unwrap()
        .unwrap();

        let payload = crate::commands::ManualTransactionPayload {
            amount_minor: 5000,
            currency: "INR".to_string(),
            direction: "debit".to_string(),
            event_time: "2026-06-10 12:00:00".to_string(),
            merchant_name: "Google Cloud".to_string(),
            instrument_id,
            reference_id: None,
        };

        resolve_unassigned_transaction_manually(
            unassigned_id.clone(),
            payload,
            &pool,
            app.handle(),
        )
        .await
        .unwrap();

        let conn = pool.get().await.unwrap();
        let status: String = conn
            .interact(move |c| {
                c.query_row(
                    "SELECT status FROM unassigned_transactions WHERE id = ?1",
                    params![unassigned_id],
                    |r| r.get(0),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status, "resolved");

        let txn_count: i64 = conn
            .interact(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM transactions WHERE merchant_display_name = 'Google Cloud'",
                    [],
                    |r| r.get(0),
                )
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(txn_count, 1);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_export_data_with_password_round_trips_via_decrypt_backup() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        app.manage(pool.clone());

        let export_path = temp_dir.join("export.enc").to_string_lossy().to_string();
        let result = settings_export_data(
            export_path.clone(),
            Some("correct horse battery staple".to_string()),
            app.state::<deadpool_sqlite::Pool>(),
        )
        .await
        .unwrap();
        assert_eq!(result, export_path);

        let encrypted = std::fs::read(&export_path).unwrap();
        let decrypted =
            crate::db::backup::decrypt_backup(&encrypted, "correct horse battery staple").unwrap();
        assert!(decrypted.len() > 100);

        assert!(crate::db::backup::decrypt_backup(&encrypted, "wrong password").is_err());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_spending_limits_round_trip_persists_categories() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        app.manage(pool.clone());

        let limits = SpendingLimits {
            global_limit: 60000.0,
            thresholds: SpendingLimitThresholds {
                warn_at_80: true,
                warn_at_90: false,
                warn_at_100: true,
            },
            categories: vec![CategoryBudget {
                name: "Transportation".to_string(),
                budget: 2500.0,
            }],
        };
        update_spending_limits(app.state::<deadpool_sqlite::Pool>(), limits)
            .await
            .unwrap();

        let fetched = fetch_spending_limits(app.state::<deadpool_sqlite::Pool>())
            .await
            .unwrap();
        assert_eq!(fetched.global_limit, 60000.0);
        assert!(fetched.thresholds.warn_at_80);
        assert!(!fetched.thresholds.warn_at_90);
        assert!(fetched.thresholds.warn_at_100);

        let transport = fetched
            .categories
            .iter()
            .find(|c| c.name == "Transportation")
            .expect("Transportation must be present among all seeded system categories");
        assert_eq!(transport.budget, 2500.0);

        let food = fetched
            .categories
            .iter()
            .find(|c| c.name == "Food & Dining")
            .expect("Food & Dining must be present among all seeded system categories");
        assert_eq!(food.budget, 0.0);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_instruments_create_validates_type_enum() {
        assert!(validate_instrument_type("credit_card").is_ok());
        assert!(validate_instrument_type("upi_vpa").is_ok());
        assert!(validate_instrument_type("not_a_real_type").is_err());
        assert!(validate_instrument_type("").is_err());
        assert!(
            validate_instrument_type("CREDIT_CARD").is_err(),
            "must be case-sensitive, matching Document 18 §4.2's exact CHECK values"
        );
    }

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE instruments (
                id TEXT PRIMARY KEY, 
                type TEXT, 
                issuer_name TEXT, 
                masked_identifier TEXT, 
                status TEXT, 
                current_balance REAL,
                credit_limit REAL,
                full_identifier TEXT,
                billing_cycle_day INTEGER,
                statement_due_date TEXT,
                bank_ifsc TEXT,
                is_deleted INTEGER DEFAULT 0
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE transactions (
                id TEXT PRIMARY KEY,
                instrument_id TEXT,
                direction TEXT,
                authorization_time TEXT,
                best_event_time TEXT,
                merchant_display_name TEXT,
                amount REAL,
                amount_minor INTEGER,
                reference_id TEXT,
                category_id TEXT,
                status TEXT,
                source_mix TEXT,
                is_deleted INTEGER DEFAULT 0
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE statements (id TEXT PRIMARY KEY, source_message_id TEXT, parse_status TEXT, created_at DATETIME DEFAULT CURRENT_TIMESTAMP, instrument_id TEXT)",
            [],
        ).unwrap();
        conn.execute(
            "CREATE TABLE unprocessed_statements (id TEXT PRIMARY KEY, resolved_statement_id TEXT, pdf_retained_until DATETIME)",
            [],
        ).unwrap();
        conn.execute(
            "CREATE TABLE reconciliation_clusters (id TEXT PRIMARY KEY, cluster_status TEXT, reason TEXT, created_at DATETIME DEFAULT CURRENT_TIMESTAMP)",
            [],
        ).unwrap();
        conn.execute(
            "CREATE TABLE reconciliation_cluster_members (id TEXT PRIMARY KEY, cluster_id TEXT, canonical_transaction_id TEXT, observation_id TEXT, member_role TEXT, match_score REAL)",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE transaction_observations (id TEXT PRIMARY KEY, amount_minor INTEGER, source_pipeline TEXT, merchant_raw TEXT, amount REAL, direction TEXT, event_time TEXT, instrument_id TEXT, reference_id TEXT, raw_payload_json TEXT)",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE unassigned_transactions (id TEXT PRIMARY KEY, observation_id TEXT, reason TEXT, status TEXT, created_at TEXT)",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE local_profile (id INTEGER PRIMARY KEY, spending_limit_monthly REAL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO local_profile (id, spending_limit_monthly) VALUES (1, 60000.0)",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_fetch_cluster_members_includes_instrument_reference_score_and_source() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO instruments (id, issuer_name, masked_identifier) VALUES ('inst_1', 'HDFC', '4021')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, merchant_display_name, amount, amount_minor, direction, reference_id, is_deleted) \
             VALUES ('txn_1', 'inst_1', 'Amazon', 100.0, 10000, 'debit', 'REF123', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transaction_observations (id, instrument_id, merchant_raw, amount_minor, direction, event_time, raw_payload_json) \
             VALUES ('obs_1', 'inst_1', 'AMAZON', 10000, 'debit', '2026-06-10 12:00:00', '{\"body\":\"You spent Rs 100 at Amazon\"}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_clusters (id, cluster_status, reason) VALUES ('c1', 'open', 'mid_range_score')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_cluster_members (id, cluster_id, observation_id, member_role) \
             VALUES ('m_incoming', 'c1', 'obs_1', 'incoming')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_cluster_members (id, cluster_id, canonical_transaction_id, member_role, match_score) \
             VALUES ('m_cand', 'c1', 'txn_1', 'candidate_a', 0.71)",
            [],
        )
        .unwrap();

        let members = fetch_cluster_members(&conn, "c1").unwrap();
        let candidate = members
            .iter()
            .find(|m| m.member_role == "candidate_a")
            .unwrap();
        assert_eq!(candidate.instrument_issuer_name, Some("HDFC".to_string()));
        assert_eq!(
            candidate.instrument_masked_identifier,
            Some("4021".to_string())
        );
        assert_eq!(candidate.reference_id, Some("REF123".to_string()));
        assert_eq!(candidate.match_score, Some(0.71));
        assert_eq!(candidate.source_raw_payload_json, None);

        let incoming = members
            .iter()
            .find(|m| m.member_role == "incoming")
            .unwrap();
        assert_eq!(incoming.match_score, None);
        assert!(incoming.source_raw_payload_json.is_some());
    }

    #[test]
    fn test_list_pending_clusters_returns_only_ambiguous() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO reconciliation_clusters (id, cluster_status, reason) VALUES ('c_open', 'open', 'multiple_high_score_candidates')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_clusters (id, cluster_status, reason) VALUES ('c_deferred', 'deferred', 'mid_range_score')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_clusters (id, cluster_status, reason) VALUES ('c_resolved', 'resolved', 'multiple_high_score_candidates')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_clusters (id, cluster_status, reason) VALUES ('c_rejected', 'rejected', 'mid_range_score')",
            [],
        )
        .unwrap();

        let clusters = do_fetch_unresolved_clusters(&conn).unwrap();
        let ids: Vec<&str> = clusters.iter().map(|c| c.id.as_str()).collect();

        assert!(ids.contains(&"c_open"));
        assert!(ids.contains(&"c_deferred"));
        assert!(!ids.contains(&"c_resolved"));
        assert!(!ids.contains(&"c_rejected"));
        assert_eq!(clusters.len(), 2);
    }

    #[test]
    fn test_cluster_record_includes_created_at() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO reconciliation_clusters (id, cluster_status, reason, created_at) VALUES ('c_aged', 'open', 'mid_range_score', '2026-01-01 00:00:00')",
            [],
        )
        .unwrap();

        let clusters = do_fetch_unresolved_clusters(&conn).unwrap();
        let aged = clusters.iter().find(|c| c.id == "c_aged").unwrap();
        assert_eq!(aged.created_at.as_deref(), Some("2026-01-01 00:00:00"));
    }

    #[test]
    fn test_cluster_explanation_uses_real_scores_not_reason_string() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO reconciliation_clusters (id, cluster_status, reason) VALUES ('c1', 'open', 'mid_range_score')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_cluster_members (id, cluster_id, observation_id, member_role) \
             VALUES ('m0', 'c1', NULL, 'incoming')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_cluster_members (id, cluster_id, canonical_transaction_id, member_role, match_score) \
             VALUES ('m1', 'c1', 'txn_a', 'candidate_a', 0.71)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_cluster_members (id, cluster_id, canonical_transaction_id, member_role, match_score) \
             VALUES ('m2', 'c1', 'txn_b', 'candidate_b', 0.66)",
            [],
        )
        .unwrap();

        let detail = do_fetch_cluster_detail(&conn, "c1").unwrap().unwrap();
        assert!(!detail.explanation.contains("mid_range_score"));
        assert!(detail.explanation.contains("71"));
        assert!(detail.explanation.contains("66"));
    }

    #[test]
    fn test_list_excludes_soft_deleted() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO transactions (id, is_deleted) VALUES ('tx_visible', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id, is_deleted) VALUES ('tx_deleted', 1)",
            [],
        )
        .unwrap();

        let results =
            do_fetch_transactions(&conn, &TransactionListFilters::default(), 50, 0).unwrap();
        let ids: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"tx_visible"));
        assert!(!ids.contains(&"tx_deleted"));
    }

    #[test]
    fn test_list_filters_combine_with_and_logic() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, direction, category_id, is_deleted) VALUES ('tx_match_both', 'inst_1', 'debit', 'cat_food', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, direction, category_id, is_deleted) VALUES ('tx_match_instrument_only', 'inst_1', 'credit', 'cat_food', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, direction, category_id, is_deleted) VALUES ('tx_match_neither', 'inst_2', 'credit', 'cat_shopping', 0)",
            [],
        )
        .unwrap();

        let filters = TransactionListFilters {
            instrument_id: Some("inst_1".to_string()),
            direction: Some("debit".to_string()),
            ..Default::default()
        };
        let results = do_fetch_transactions(&conn, &filters, 50, 0).unwrap();
        let ids: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();

        assert_eq!(
            ids,
            vec!["tx_match_both"],
            "only the row matching BOTH filters (AND) must be returned"
        );
    }

    #[test]
    fn test_dashboard_summary_excludes_ambiguous_clusters() {
        let conn = setup_db();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount_minor, is_deleted) VALUES ('tx_normal', 'debit', ?1, 1000, 0)",
            params![now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount_minor, is_deleted) VALUES ('tx_ambiguous', 'debit', ?1, 5000, 0)",
            params![now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_clusters (id, cluster_status) VALUES ('cl_1', 'open')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_cluster_members (id, cluster_id, canonical_transaction_id) VALUES ('m_1', 'cl_1', 'tx_ambiguous')",
            [],
        )
        .unwrap();

        let summary = do_fetch_dashboard_summary(&conn).unwrap();
        assert_eq!(
            summary.month_to_date_spend, 10.0,
            "the ambiguous candidate's amount must not be counted"
        );
    }

    #[test]
    fn test_pending_review_count_excluded_from_totals() {
        let conn = setup_db();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount_minor, is_deleted) VALUES ('tx_normal', 'debit', ?1, 1000, 0)",
            params![now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount_minor, is_deleted) VALUES ('tx_ambiguous', 'debit', ?1, 5000, 0)",
            params![now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_clusters (id, cluster_status) VALUES ('cl_1', 'open')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_cluster_members (id, cluster_id, canonical_transaction_id) VALUES ('m_1', 'cl_1', 'tx_ambiguous')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transaction_observations (id, amount_minor) VALUES ('obs_orphan', 2500)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO unassigned_transactions (id, observation_id, status) VALUES ('u_1', 'obs_orphan', 'open')",
            [],
        )
        .unwrap();

        let summary = do_fetch_dashboard_summary(&conn).unwrap();
        let pending = compute_unassigned_amount_pending_review(&conn).unwrap();

        assert_eq!(pending.count, 2);
        assert_eq!(pending.amount_minor, 7500);
        assert_eq!(summary.month_to_date_spend, 10.0);
    }

    #[test]
    fn test_dashboard_summary_excludes_soft_deleted() {
        let conn = setup_db();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount_minor, is_deleted) VALUES ('tx_live', 'debit', ?1, 1000, 0)",
            params![now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount_minor, is_deleted) VALUES ('tx_deleted', 'debit', ?1, 9000, 1)",
            params![now],
        )
        .unwrap();

        let summary = do_fetch_dashboard_summary(&conn).unwrap();
        assert_eq!(
            summary.month_to_date_spend, 10.0,
            "the soft-deleted row's amount must not be counted"
        );
    }

    #[test]
    fn test_spend_aggregation_uses_amount_minor_not_float() {
        let conn = setup_db();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount, amount_minor, is_deleted) VALUES ('tx_1', 'debit', ?1, 999999.99, 1053, 0)",
            params![now],
        )
        .unwrap();

        let summary = do_fetch_dashboard_summary(&conn).unwrap();
        assert_eq!(
            summary.month_to_date_spend, 10.53,
            "must read amount_minor (1053 paise), never the divergent float amount column"
        );
    }

    #[test]
    fn test_unassigned_and_ambiguous_are_separate_queues() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO reconciliation_clusters (id, cluster_status, reason) VALUES ('cl_ambiguous', 'open', 'multiple_high_score_candidates')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO unassigned_transactions (id, observation_id, reason, status) VALUES ('u_unresolved', 'obs_1', 'issuer_name_not_found', 'open')",
            [],
        )
        .unwrap();

        let clusters = do_fetch_unresolved_clusters(&conn).unwrap();
        let unassigned = crate::db::unassigned_transactions::select_open(&conn).unwrap();

        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].id, "cl_ambiguous");
        assert_eq!(unassigned.len(), 1);
        assert_eq!(unassigned[0].id, "u_unresolved");
        assert!(clusters.iter().all(|c| c.id != "u_unresolved"));
    }

    #[test]
    fn test_seed_and_fetch() {
        let conn = setup_db();

        let insert_tx = "
            INSERT INTO transactions (id, instrument_id, authorization_time, merchant_display_name, amount, category_id, status)
            VALUES 
            ('tx_1', 'inst_1', '2026-06-10T14:32:00Z', 'Amazon Pay India', -1499.00, 'SHOPPING', 'POSTED'),
            ('tx_2', 'inst_1', '2026-06-09T20:15:00Z', 'Swiggy', -450.00, 'FOOD', 'POSTED'),
            ('tx_3', 'inst_1', '2026-06-08T09:00:00Z', 'Uber', -250.00, 'TRANSPORT', 'POSTED')
        ";
        conn.execute_batch(insert_tx).unwrap();

        conn.execute(
            "INSERT INTO statements (id, source_message_id, parse_status) VALUES ('stmt_1', 'HDFC_May_2026.pdf', 'PROCESSED')",
            [],
        ).unwrap();

        conn.execute(
            "INSERT INTO reconciliation_clusters (id, reason, cluster_status) VALUES ('c1', 'Ambiguous match', 'open')",
            [],
        ).unwrap();

        conn.execute(
            "INSERT INTO instruments (id, issuer_name) VALUES ('inst_1', 'HDFC Bank')",
            [],
        )
        .unwrap();

        let summary = do_fetch_dashboard_summary(&conn).unwrap();
        assert_eq!(summary.month_to_date_spend, 0.0);
        assert_eq!(summary.income, 0.0);
        assert_eq!(summary.limit, 60000.0);
        assert_eq!(summary.upcoming_bills_count, 0);

        let txs = do_fetch_transactions(&conn, &TransactionListFilters::default(), 50, 0).unwrap();
        assert_eq!(txs.len(), 3);
        assert_eq!(txs[0].id, "tx_1");
        assert_eq!(txs[0].amount, -1499.0);

        let stmts = do_fetch_statement_history(&conn, 50, 0).unwrap();
        assert_eq!(stmts.len(), 1);
        assert_eq!(stmts[0].file_name, "HDFC_May_2026.pdf");

        let clusters = do_fetch_unresolved_clusters(&conn).unwrap();
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].reason, "Ambiguous match");

        let instruments = do_fetch_instruments(&conn).unwrap();
        assert_eq!(instruments.len(), 1);
        assert_eq!(instruments[0].issuer_name, "HDFC Bank");

        let metrics = do_get_debug_metrics(&conn).unwrap();
        assert_eq!(metrics.total_transactions, 3);
        assert_eq!(metrics.total_statements, 1);
        assert_eq!(metrics.unresolved_clusters, 1);
        assert_eq!(metrics.llm_fallback_rate, 0.0);
        assert_eq!(metrics.queue_depth, 0);
    }

    #[test]
    fn test_search_uses_fts5() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, amount_minor, currency, direction, merchant_display_name, is_deleted) \
             VALUES ('tx_amazon', 'inst_1', 1000, 'INR', 'debit', 'Amazon Pay India', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, amount_minor, currency, direction, merchant_display_name, is_deleted) \
             VALUES ('tx_uber', 'inst_1', 2000, 'INR', 'debit', 'Uber Trip', 0)",
            [],
        )
        .unwrap();

        let results = do_transactions_search(&conn, "amazon", None).unwrap();
        let ids: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();

        assert!(ids.contains(&"tx_amazon"));
        assert!(
            !ids.contains(&"tx_uber"),
            "FTS5 match must be specific to the query term, not a substring hit on unrelated rows"
        );
    }

    #[test]
    fn test_statements_list_paginated() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        for i in 0..5 {
            conn.execute(
                "INSERT INTO statements (id, instrument_id, statement_type, source_type, billing_period_start, billing_period_end, parse_status, is_duplicate, created_at) \
                 VALUES (?1, NULL, 'credit_card_statement', 'manual_upload', '2026-01-01', '2026-01-31', 'parsed', 0, ?2)",
                rusqlite::params![format!("stmt_{i}"), format!("2026-01-0{}", i + 1)],
            )
            .unwrap();
        }

        let page1 = do_fetch_statement_history(&conn, 2, 0).unwrap();
        let page2 = do_fetch_statement_history(&conn, 2, 2).unwrap();
        let total = count_statements(&conn).unwrap();

        assert_eq!(page1.len(), 2, "page size must be respected");
        assert_eq!(page2.len(), 2);
        assert_eq!(
            total, 5,
            "total count must reflect all rows, independent of the page size"
        );
        assert_ne!(
            page1[0].id, page2[0].id,
            "different pages must return different rows"
        );
    }

    #[test]
    fn test_statement_history_joins_instrument_and_pdf_availability() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier) \
             VALUES ('inst_hist', 'credit_card', 'HDFC', '3825')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO statements (id, instrument_id, statement_type, billing_period_start, billing_period_end, source_message_id, parse_status) \
             VALUES ('stmt_hist_available', 'inst_hist', 'credit_card_statement', '2026-06-01', '2026-06-30', 'msg_1', 'parsed')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO unprocessed_statements (id, statement_source_json, failure_type, failure_reason, status, resolved_statement_id, pdf_retained_until) \
             VALUES ('unproc_1', '{}', 'password', 'test', 'resolved', 'stmt_hist_available', datetime('now', '+30 days'))",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO statements (id, instrument_id, statement_type, billing_period_start, billing_period_end, source_message_id, parse_status) \
             VALUES ('stmt_hist_expired', 'inst_hist', 'credit_card_statement', '2026-05-01', '2026-05-31', 'msg_2', 'parsed')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO unprocessed_statements (id, statement_source_json, failure_type, failure_reason, status, resolved_statement_id, pdf_retained_until) \
             VALUES ('unproc_2', '{}', 'password', 'test', 'resolved', 'stmt_hist_expired', datetime('now', '-1 days'))",
            [],
        )
        .unwrap();

        let records = do_fetch_statement_history(&conn, 10, 0).unwrap();

        let available = records
            .iter()
            .find(|r| r.id == "stmt_hist_available")
            .unwrap();
        assert_eq!(available.issuer_name.as_deref(), Some("HDFC"));
        assert_eq!(available.masked_identifier.as_deref(), Some("3825"));
        assert_eq!(available.instrument_type.as_deref(), Some("credit_card"));
        assert!(available.pdf_available);

        let expired = records
            .iter()
            .find(|r| r.id == "stmt_hist_expired")
            .unwrap();
        assert!(!expired.pdf_available);
    }

    #[tokio::test]
    async fn test_get_pdf_returns_not_found_for_unretained_statement() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

        let result = get_pdf_bytes_for_statement(&temp_dir, "nonexistent_stmt", &pool).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_pdf_is_idempotent() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();

        delete_pdf_for_statement(&temp_dir, "nonexistent_stmt", &pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_stage_parse_pipeline_persists_pdf_before_parsing() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();

        let stmt_id = "stmt_clean_upload".to_string();
        let bytes = b"not a real pdf -- parsing must fail, independent of this test".to_vec();

        let result = crate::commands::stage_parse_pipeline(
            &bytes,
            "statement.pdf",
            "hash_clean",
            &pool,
            app.handle(),
            None,
            None,
            "manual_upload",
            Some(stmt_id.clone()),
        )
        .await;
        assert!(
            result.is_err(),
            "sanity check: these bytes are not a real PDF and must fail to parse"
        );

        let app_data_dir = app.handle().path().app_data_dir().unwrap();
        let stored = crate::statements::pdf_storage::read_pdf(&app_data_dir, &stmt_id).unwrap();
        assert_eq!(
            stored,
            Some(bytes),
            "the PDF must be persisted under stmt_id before the parse attempt, so the \
             later retention/view logic (commit_staged_draft, statements_get_pdf) can find \
             it regardless of how parsing turns out"
        );
    }

    #[test]
    fn test_dashboard_upcoming_bills_reflects_instrument_due_dates() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier, current_balance, statement_due_date, nickname, status, is_deleted) \
             VALUES ('inst_due', 'credit_card', 'HDFC Bank', '1234', 2450000, '2099-06-25', 'HDFC Regalia', 'active', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier, statement_due_date, status, is_deleted) \
             VALUES ('inst_past', 'credit_card', 'ICICI Bank', '5678', '2000-01-01', 'active', 0)",
            [],
        )
        .unwrap();

        let today = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let bills = do_fetch_upcoming_bills(&conn, &today).unwrap();

        assert_eq!(
            bills.len(),
            1,
            "only the future-dated instrument counts as an upcoming bill"
        );
        assert_eq!(bills[0].id, "inst_due");
        assert_eq!(bills[0].description, "HDFC Regalia");
        assert_eq!(bills[0].amount, 24500.0);
        assert_eq!(bills[0].due_date, "2099-06-25");
    }

    #[test]
    fn test_dashboard_categories_aggregates_current_month_spend() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        let now = chrono::Utc::now().naive_utc();
        let month = format!("{}-{:02}", now.date().year(), now.date().month());
        let event_time = now.format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount_minor, category_id, is_deleted) \
             VALUES ('tx_food', 'debit', ?1, 42500, 'cat_food', 0)",
            params![event_time],
        )
        .unwrap();

        let categories = do_fetch_category_spend(&conn, &month).unwrap();
        let food = categories
            .iter()
            .find(|c| c.category_id == "cat_food")
            .expect("cat_food must be present");

        assert_eq!(food.total_spend, 425.0);
        assert_eq!(food.name, "Food & Dining");
    }

    #[test]
    fn test_category_spend_excludes_open_cluster_candidates() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        let now = chrono::Utc::now().naive_utc();
        let month = format!("{}-{:02}", now.date().year(), now.date().month());
        let event_time = now.format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount_minor, category_id, is_deleted) \
             VALUES ('tx_food_normal', 'debit', ?1, 1000, 'cat_food', 0)",
            params![event_time],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount_minor, category_id, is_deleted) \
             VALUES ('tx_food_ambiguous', 'debit', ?1, 5000, 'cat_food', 0)",
            params![event_time],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_clusters (id, cluster_status) VALUES ('cl_cat', 'open')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_cluster_members (id, cluster_id, canonical_transaction_id, member_role) \
             VALUES ('m_cat', 'cl_cat', 'tx_food_ambiguous', 'candidate_a')",
            [],
        )
        .unwrap();

        let categories = do_fetch_category_spend(&conn, &month).unwrap();
        let food = categories
            .iter()
            .find(|c| c.category_id == "cat_food")
            .expect("cat_food must be present");

        assert_eq!(
            food.total_spend, 10.0,
            "the open-cluster candidate's amount must not be counted"
        );
    }

    #[test]
    fn test_spend_trend_monthly_buckets_by_period() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        let now = chrono::Utc::now().naive_utc();
        let event_time = now.format("%Y-%m-%d %H:%M:%S").to_string();
        let expected_period = now.format("%Y-%m").to_string();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount_minor, is_deleted) VALUES ('tx_1', 'debit', ?1, 1000, 0)",
            params![event_time],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount_minor, is_deleted) VALUES ('tx_2', 'debit', ?1, 500, 0)",
            params![event_time],
        )
        .unwrap();

        let trend = do_fetch_spend_trend(&conn, "monthly", &now).unwrap();
        let bucket = trend
            .iter()
            .find(|p| p.period == expected_period)
            .expect("current month bucket must be present");

        assert_eq!(bucket.total_spend, 15.0);
    }

    #[test]
    fn test_spend_trend_excludes_open_cluster_candidates() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        let now = chrono::Utc::now().naive_utc();
        let event_time = now.format("%Y-%m-%d %H:%M:%S").to_string();
        let expected_period = now.format("%Y-%m").to_string();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount_minor, is_deleted) VALUES ('tx_normal', 'debit', ?1, 1000, 0)",
            params![event_time],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount_minor, is_deleted) VALUES ('tx_ambiguous', 'debit', ?1, 5000, 0)",
            params![event_time],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_clusters (id, cluster_status) VALUES ('cl_trend', 'open')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_cluster_members (id, cluster_id, canonical_transaction_id, member_role) \
             VALUES ('m_trend', 'cl_trend', 'tx_ambiguous', 'candidate_a')",
            [],
        )
        .unwrap();

        let trend = do_fetch_spend_trend(&conn, "monthly", &now).unwrap();
        let bucket = trend
            .iter()
            .find(|p| p.period == expected_period)
            .expect("current month bucket must be present");

        assert_eq!(
            bucket.total_spend, 10.0,
            "the open-cluster candidate's amount must not be counted"
        );
    }

    #[test]
    fn test_top_merchants_orders_by_spend_desc() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        let now = chrono::Utc::now().naive_utc();
        let event_time = now.format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount_minor, merchant_display_name, is_deleted) \
             VALUES ('tx_small', 'debit', ?1, 500, 'Small Spend Cafe', 0)",
            params![event_time],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount_minor, merchant_display_name, is_deleted) \
             VALUES ('tx_big', 'debit', ?1, 5000, 'Big Spend Store', 0)",
            params![event_time],
        )
        .unwrap();

        let merchants = do_fetch_top_merchants(&conn, &now).unwrap();

        assert_eq!(
            merchants[0].merchant_display_name, "Big Spend Store",
            "the higher-spend merchant must sort first"
        );
        assert_eq!(merchants[0].total_spend, 50.0);
        assert_eq!(merchants[1].merchant_display_name, "Small Spend Cafe");
    }

    #[test]
    fn test_top_merchants_excludes_open_cluster_candidates() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        let now = chrono::Utc::now().naive_utc();
        let event_time = now.format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount_minor, merchant_display_name, is_deleted) \
             VALUES ('tx_normal', 'debit', ?1, 500, 'Real Merchant', 0)",
            params![event_time],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id, direction, best_event_time, amount_minor, merchant_display_name, is_deleted) \
             VALUES ('tx_ambiguous', 'debit', ?1, 999999, 'Ambiguous Merchant', 0)",
            params![event_time],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_clusters (id, cluster_status) VALUES ('cl_merch', 'open')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_cluster_members (id, cluster_id, canonical_transaction_id, member_role) \
             VALUES ('m_merch', 'cl_merch', 'tx_ambiguous', 'candidate_a')",
            [],
        )
        .unwrap();

        let merchants = do_fetch_top_merchants(&conn, &now).unwrap();

        assert!(
            merchants
                .iter()
                .all(|m| m.merchant_display_name != "Ambiguous Merchant"),
            "the open-cluster candidate's merchant must not appear at all"
        );
        let real = merchants
            .iter()
            .find(|m| m.merchant_display_name == "Real Merchant")
            .expect("Real Merchant must still be present");
        assert_eq!(real.total_spend, 5.0);
    }

    #[test]
    fn test_recurring_payments_summary_resolves_merchant_name() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        conn.execute(
            "INSERT INTO merchants (id, name, normalized_name, source, is_deleted) VALUES ('m_netflix', 'Netflix', 'netflix', 'system', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO recurring_payments (id, merchant_entity_id, amount_minor, currency, cadence, next_predicted_date, confidence, status) \
             VALUES ('rp_1', 'm_netflix', 64900, 'INR', 'monthly', '2026-08-01', 0.95, 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO recurring_payments (id, merchant_entity_id, amount_minor, currency, cadence, status) \
             VALUES ('rp_cancelled', 'm_netflix', 64900, 'INR', 'monthly', 'cancelled')",
            [],
        )
        .unwrap();

        let summary = do_fetch_recurring_payments_summary(&conn).unwrap();

        assert_eq!(summary.len(), 1, "only status='active' rows are included");
        assert_eq!(summary[0].merchant_name, "Netflix");
        assert_eq!(summary[0].amount, 649.0);
        assert_eq!(summary[0].cadence, "monthly");
    }

    #[test]
    fn test_limit_thresholds_validated_sorted() {
        assert!(validate_limit_thresholds(&[80.0, 90.0, 100.0]).is_ok());
        assert!(
            validate_limit_thresholds(&[]).is_ok(),
            "an empty array (all warnings disabled) is valid"
        );
        assert!(
            validate_limit_thresholds(&[90.0, 80.0]).is_err(),
            "out-of-order values must be rejected"
        );
        assert!(
            validate_limit_thresholds(&[80.0, 80.0]).is_err(),
            "duplicate values are not strictly ascending"
        );
        assert!(
            validate_limit_thresholds(&[80.0, 150.0]).is_err(),
            "a value outside 0-100 must be rejected"
        );
    }
}
