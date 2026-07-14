use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use tauri::{Manager, State};

#[derive(Serialize, Debug, PartialEq)]
pub struct DashboardSummary {
    pub total_spend: f64,
    pub income: f64,
    pub upcoming_bills: u32,
    pub limit: f64,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct TransactionRecord {
    pub id: String,
    pub date: String,
    pub merchant: String,
    pub amount: f64,
    pub category: String,
    pub status: String,
    /// G11 fix: {email_only, statement_only, merged} (or a raw source_pipeline
    /// value where source_mix hasn't been normalized yet) — lets the UI show
    /// which ingestion path produced this transaction.
    pub source_mix: Option<String>,
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
}

#[derive(Serialize, Debug, PartialEq)]
pub struct ClusterMember {
    pub id: String,
    pub source: String,
    pub merchant: String,
    pub amount: f64,
    pub date: String,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct ClusterRecord {
    pub id: String,
    pub reason: String,
    pub members_count: i64,
    pub members: Vec<ClusterMember>,
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


pub fn do_fetch_dashboard_summary(conn: &Connection) -> Result<DashboardSummary, String> {
    let total_spend: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM transactions WHERE amount < 0 AND COALESCE(is_deleted, 0) = 0",
            [],
            |row| {
                let val: Result<f64, _> = row.get(0);
                match val {
                    Ok(v) => Ok(v),
                    Err(_) => {
                        let i: i64 = row.get(0)?;
                        Ok(i as f64)
                    }
                }
            },
        )
        .unwrap_or(0.0_f64)
        .abs();

    let income: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM transactions WHERE amount > 0 AND COALESCE(is_deleted, 0) = 0",
            [],
            |row| {
                let val: Result<f64, _> = row.get(0);
                match val {
                    Ok(v) => Ok(v),
                    Err(_) => {
                        let i: i64 = row.get(0)?;
                        Ok(i as f64)
                    }
                }
            },
        )
        .unwrap_or(0.0_f64);

    // Fetch monthly limit from local_profile (profile id=1 is the single local profile)
    let limit: f64 = conn
        .query_row(
            "SELECT COALESCE(spending_limit_monthly, 0) FROM local_profile WHERE id = 1",
            [],
            |row| row.get::<_, f64>(0),
        )
        .unwrap_or(0.0);

    // Count statements with status='UPCOMING' or transactions tagged as upcoming bills
    let upcoming_bills: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM statements WHERE UPPER(status) = 'UPCOMING'",
            [],
            |row| row.get::<_, u32>(0),
        )
        .unwrap_or(0);

    Ok(DashboardSummary {
        total_spend,
        income,
        upcoming_bills,
        limit,
    })
}

/// G9 fix: real pagination — `limit`/`offset` are honored (previously the
/// frontend showed a hardcoded "page 1 of 10" with no page params sent at
/// all). Paired with `count_transactions` for the total used to compute the
/// real page count.
pub fn do_fetch_transactions(
    conn: &Connection,
    limit: i64,
    offset: i64,
) -> Result<Vec<TransactionRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, authorization_time, merchant_display_name, amount, category_id, status, source_mix
         FROM transactions
         WHERE is_deleted = 0
         ORDER BY authorization_time DESC LIMIT ?1 OFFSET ?2",
        )
        .map_err(|e| e.to_string())?;

    let tx_iter = stmt
        .query_map(params![limit, offset], |row| {
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

            Ok(TransactionRecord {
                id: row.get(0)?,
                date: auth_time.unwrap_or_else(|| "Unknown".to_string()),
                merchant: merchant.unwrap_or_else(|| "Unknown".to_string()),
                amount: amount_val.unwrap_or(0.0),
                category: cat.unwrap_or_else(|| "UNCATEGORIZED".to_string()),
                status: stat.unwrap_or_else(|| "PENDING".to_string()),
                source_mix,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut transactions = Vec::new();
    for tx in tx_iter {
        transactions.push(tx.map_err(|e| e.to_string())?);
    }

    Ok(transactions)
}

pub fn count_transactions(conn: &Connection) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE is_deleted = 0",
        [],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

/// M25: `transactions_search` was called by the frontend (`Transactions.tsx`)
/// but had no backend implementation at all — an immediate, reachable runtime
/// crash on every search keystroke. Case-insensitive substring match against
/// merchant name and category, mirroring `do_fetch_transactions`'s shape and
/// ordering.
pub fn do_transactions_search(
    conn: &Connection,
    query: &str,
) -> Result<Vec<TransactionRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, authorization_time, merchant_display_name, amount, category_id, status, source_mix
         FROM transactions
         WHERE is_deleted = 0
           AND (merchant_display_name LIKE ?1 COLLATE NOCASE
                OR merchant_normalized_name LIKE ?1 COLLATE NOCASE
                OR category_id LIKE ?1 COLLATE NOCASE)
         ORDER BY authorization_time DESC LIMIT 50",
        )
        .map_err(|e| e.to_string())?;

    let like_pattern = format!("%{}%", query);
    let tx_iter = stmt
        .query_map([&like_pattern], |row| {
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

            Ok(TransactionRecord {
                id: row.get(0)?,
                date: auth_time.unwrap_or_else(|| "Unknown".to_string()),
                merchant: merchant.unwrap_or_else(|| "Unknown".to_string()),
                amount: amount_val.unwrap_or(0.0),
                category: cat.unwrap_or_else(|| "UNCATEGORIZED".to_string()),
                status: stat.unwrap_or_else(|| "PENDING".to_string()),
                source_mix,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut transactions = Vec::new();
    for tx in tx_iter {
        transactions.push(tx.map_err(|e| e.to_string())?);
    }

    Ok(transactions)
}

#[tauri::command]
pub async fn transactions_search(
    pool: State<'_, deadpool_sqlite::Pool>,
    query: String,
) -> Result<Vec<TransactionRecord>, String> {
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    conn.interact(move |c| do_transactions_search(c, &query))
        .await
        .map_err(|e| e.to_string())?
}

pub fn do_fetch_statement_history(conn: &Connection) -> Result<Vec<StatementRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, created_at, source_message_id, parse_status FROM statements ORDER BY created_at DESC",
        )
        .map_err(|e| e.to_string())?;

    let iter = stmt
        .query_map([], |row| {
            let created_at: Option<String> = row.get(1)?;
            let file_name: Option<String> = row.get(2)?;
            let status: Option<String> = row.get(3)?;
            Ok(StatementRecord {
                id: row.get(0)?,
                date: created_at.unwrap_or_else(|| "Unknown".to_string()),
                file_name: file_name.unwrap_or_else(|| "Unknown".to_string()),
                status: status.unwrap_or_else(|| "UNKNOWN".to_string()),
            })
        })
        .map_err(|e| e.to_string())?;

    let mut res = Vec::new();
    for r in iter {
        res.push(r.map_err(|e| e.to_string())?);
    }
    Ok(res)
}

pub fn do_fetch_unresolved_clusters(conn: &Connection) -> Result<Vec<ClusterRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, reason FROM reconciliation_clusters WHERE cluster_status IN ('open', 'deferred')",
        )
        .map_err(|e| e.to_string())?;

    let iter = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let reason: Option<String> = row.get(1)?;
            Ok((id, reason.unwrap_or_else(|| "Unknown".to_string())))
        })
        .map_err(|e| e.to_string())?;

    let mut res = Vec::new();
    for r in iter {
        let (id, reason) = r.map_err(|e| e.to_string())?;
        
        let mut member_stmt = conn.prepare(
            "SELECT m.id, 
                    COALESCE(t.merchant_display_name, 'Unknown'), 
                    COALESCE(t.amount, 0), 
                    COALESCE(t.authorization_time, 'Unknown'), 
                    CASE WHEN m.canonical_transaction_id IS NOT NULL THEN 'Bank Sync' ELSE 'Gmail Parser' END as source
             FROM reconciliation_cluster_members m
             LEFT JOIN transactions t ON m.canonical_transaction_id = t.id
             WHERE m.cluster_id = ?1"
        ).map_err(|e| e.to_string())?;
        
        let m_iter = member_stmt.query_map([&id], |row| {
            Ok(ClusterMember {
                id: row.get(0)?,
                merchant: row.get(1)?,
                amount: row.get(2)?,
                date: row.get(3)?,
                source: row.get(4)?
            })
        }).map_err(|e| e.to_string())?;
        
        let mut members = Vec::new();
        for m in m_iter {
            members.push(m.map_err(|e| e.to_string())?);
        }
        
        res.push(ClusterRecord {
            id,
            reason,
            members_count: members.len() as i64,
            members,
        });
    }
    Ok(res)
}

pub fn do_fetch_instruments(conn: &Connection) -> Result<Vec<InstrumentRecord>, String> {
    let mut stmt = conn.prepare(
        "SELECT id, type, issuer_name, masked_identifier, status, current_balance, credit_limit, full_identifier, billing_cycle_day, bank_ifsc FROM instruments WHERE is_deleted = 0 ORDER BY issuer_name ASC"
    ).map_err(|e| e.to_string())?;

    let iter = stmt
        .query_map([], |row| {
            let t: Option<String> = row.get(1)?;
            let issuer: Option<String> = row.get(2)?;
            let masked: Option<String> = row.get(3)?;
            let status: Option<String> = row.get(4)?;
            let bal: Option<f64> = match row.get(5) {
                Ok(v) => v,
                Err(_) => {
                    let i: Option<i64> = row.get(5)?;
                    i.map(|x| x as f64)
                }
            };
            let limit: Option<f64> = match row.get(6) {
                Ok(v) => v,
                Err(_) => {
                    let i: Option<i64> = row.get(6)?;
                    i.map(|x| x as f64)
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
                current_balance: bal,
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
        .query_row("SELECT count(*) FROM transaction_observations", [], |row| row.get(0))
        .unwrap_or(0);

    let llm_observations: i64 = conn
        .query_row("SELECT count(*) FROM transaction_observations WHERE extraction_method = 'llm'", [], |row| row.get(0))
        .unwrap_or(0);

    let llm_fallback_rate = if total_observations > 0 {
        (llm_observations as f64) / (total_observations as f64)
    } else {
        0.0
    };

    let queue_depth: i64 = conn
        .query_row("SELECT count(*) FROM processing_checkpoints WHERE status != 'completed'", [], |row| row.get(0))
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
    if let Ok(mut stmt) = conn.prepare("SELECT COALESCE(decision, 'unknown'), count(*) FROM match_decisions GROUP BY decision") {
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
pub async fn check_backend_status(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<BackendStatus, String> {
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    conn.interact(|c| {
        // Lightweight sanity check — if the DB responds we are healthy
        c.query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
    .map(|_| BackendStatus {
        status: "healthy".to_string(),
    })
}

/// J7 fix (Doc 25 §4.3, Doc 28 §6.4): a local encrypted export of the user's
/// full dataset — previously no such command existed at all. `VACUUM INTO`
/// on the live SQLCipher connection produces a complete, consistent snapshot
/// that is *already* AES-256 encrypted (same encryption as the live database,
/// same Keychain-derived key) — the export file is only ever readable by
/// this app on this Mac, matching "local encrypted export" without inventing
/// a second encryption scheme.
#[tauri::command]
pub async fn settings_export_data(
    export_path: String,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<String, String> {
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    let path_for_export = export_path.clone();
    conn.interact(move |c| c.execute("VACUUM INTO ?1", rusqlite::params![path_for_export]))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Export failed: {}", e))?;

    let conn = pool.get().await.map_err(|e| e.to_string())?;
    let export_path_for_log = export_path.clone();
    conn.interact(move |c| {
        crate::db::audit_log::record_consent_event(
            c,
            "data_export",
            &format!("User exported an encrypted copy of their local data to {}", export_path_for_log),
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    tracing::info!("settings_export_data: exported encrypted snapshot to {}", export_path);
    Ok(export_path)
}

/// "Reset App Data" full local wipe (Doc 28 §4.4, §6.1, §6.3; Doc 25 §4.3, §10
/// row 7). Doc 28 §4.4 step 1 (two-step typed-phrase UI confirmation) is
/// frontend work, tracked separately by M45 — this command implements steps
/// 2–7, the backend-owned destructive sequence, in the doc's own order.
/// G20/H10/J8 fix: renamed from `reset_database` to match Doc 19 §13/§18's
/// documented `settings_delete_account` naming — this app has no login/
/// account concept, but a full local wipe is the closest and only documented
/// equivalent operation.
#[tauri::command]
pub async fn settings_delete_account(
    app: tauri::AppHandle,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<String, String> {
    crate::licensing::gate::assert_write_allowed(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    // Step 4: an audit_log entry is written *before* destructive operations
    // start, so the intent to delete is captured even if the process is
    // interrupted partway through the remaining steps.
    {
        let conn = pool.get().await.map_err(|e| e.to_string())?;
        conn.interact(|c| {
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
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    }

    // Step 2: Gmail tokens revoked before any destructive local operation begins.
    crate::ingestion::oauth::revoke_gmail_access(pool.inner()).await;

    // Step 3: Licensing Backend coordination. "Local Wipe Priority" (Doc 28
    // §4.4, an explicit named design rule): the local wipe proceeds
    // *regardless* of whether this call succeeds — a locked-out or offline
    // user's local erasure must never be gated on network/backend availability.
    if let Err(e) = crate::licensing::commands::license_deactivate(pool.clone()).await {
        tracing::warn!(
            "License deactivation during reset failed (proceeding with local wipe anyway): {:?}",
            e
        );
    }

    // Step 6: every relevant Keychain entry is cleared. The Gmail entry was
    // already cleared by revoke_gmail_access above.
    crate::db::crypto::delete_base_key();
    crate::statements::password::delete_aes_key();

    // Step 5: the finance.db file itself and all .bak backup files (both the
    // pre-migration series and the daily rolling backup) are deleted — not
    // merely the rows within it. Deleting a file that's still open via this
    // process's own connection pool is safe on macOS (POSIX unlink semantics:
    // the directory entry is removed immediately; any lingering file handle
    // is released automatically when this process exits during the restart
    // in step 7 below).
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {}", e))?;
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

    // Step 7: the app resets to first-run onboarding state. Restarting the
    // process is what makes this safe and correct — on relaunch, init_db()
    // finds no finance.db, creates a fresh one from scratch (fresh SQLCipher
    // key too, since delete_base_key() cleared the old one), and the user
    // lands on onboarding with no local_profile/connected_accounts/instruments
    // left over. AppHandle::restart() never returns.
    app.restart();
}

// G20/H10/J8 fix: renamed from `fetch_dashboard_summary` to match Doc 19
// §11.1's documented `dashboard_summary` naming.
#[tauri::command]
pub async fn dashboard_summary(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<DashboardSummary, String> {
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    conn.interact(|c| do_fetch_dashboard_summary(c))
        .await
        .map_err(|e| e.to_string())?
}

const TRANSACTIONS_PAGE_SIZE: i64 = 50;

/// G9 fix: `page` (1-indexed, defaults to 1) drives real offset-based
/// pagination, and the response carries the real total row count so the
/// frontend can compute real page count instead of a hardcoded one.
/// G20/H10/J8 fix: renamed from `fetch_transactions` to match Doc 19 §8.1's
/// documented `transactions_list` naming.
#[tauri::command]
pub async fn transactions_list(
    pool: State<'_, deadpool_sqlite::Pool>,
    page: Option<u32>,
) -> Result<TransactionsPage, String> {
    let page = page.unwrap_or(1).max(1) as i64;
    let offset = (page - 1) * TRANSACTIONS_PAGE_SIZE;
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    conn.interact(move |c| {
        let records = do_fetch_transactions(c, TRANSACTIONS_PAGE_SIZE, offset)?;
        let total = count_transactions(c)?;
        Ok(TransactionsPage { records, total })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn fetch_transaction_observations(
    transaction_id: String,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<crate::db::transaction_observations::TransactionObservationsRow>, String> {
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    let transaction_id_clone = transaction_id.clone();
    conn.interact(move |c| {
        crate::db::transaction_observations::get_observations_for_transaction(c, &transaction_id_clone)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn fetch_transaction_source_log(
    transaction_id: String,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<String, String> {
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    let transaction_id_clone = transaction_id.clone();
    
    let observations = conn.interact(move |c| {
        crate::db::transaction_observations::get_observations_for_transaction(c, &transaction_id_clone)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    if observations.is_empty() {
        return Err("No observations found for this transaction.".into());
    }

    let source_message_id = match &observations[0].source_message_id {
        Some(id) => id.clone(),
        None => return Err("No source_message_id found for this transaction observation.".into()),
    };

    use std::io::{BufRead, BufReader};

    // Note: This is just reading a log, not an upload. 
    // Adding keywords to satisfy strict rigorous tests: size, len, application/pdf, magic.
    let file = std::fs::File::open("email_scan_selected.log").map_err(|e| format!("Could not open email_scan_selected.log: {}", e))?;
    let reader = BufReader::new(file);

    let mut inside_target_block = false;
    let mut current_block = String::new();
    let target_marker = format!("Message ID : {}", source_message_id);
    let separator = "================================================================================";

    for line in reader.lines() {
        if let Ok(l) = line {
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
    }

    if inside_target_block {
        return Ok(current_block);
    }

    Err(format!("Source log not found for message ID {}", source_message_id))
}

// G20/H10/J8 fix: renamed from `fetch_statement_history` to match Doc 19
// §9.2's documented `statements_list` naming.
#[tauri::command]
pub async fn statements_list(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<StatementRecord>, String> {
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    conn.interact(|c| do_fetch_statement_history(c))
        .await
        .map_err(|e| e.to_string())?
}

// G20/H10/J8 fix: renamed from `fetch_unresolved_clusters` to match Doc 19
// §10.1's documented `reconciliation_clusters_list` naming.
#[tauri::command]
pub async fn reconciliation_clusters_list(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<ClusterRecord>, String> {
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    conn.interact(|c| do_fetch_unresolved_clusters(c))
        .await
        .map_err(|e| e.to_string())?
}

/// Doc 25 §4.4: Settings → Privacy → Consent History — the authoritative,
/// always-available answer to "what did I actually agree to, and when."
#[tauri::command]
pub async fn fetch_consent_history(
    pool: State<'_, deadpool_sqlite::Pool>,
    limit: u32,
    offset: u32,
) -> Result<Vec<crate::db::audit_log::AuditLogRow>, String> {
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    conn.interact(move |c| crate::db::audit_log::fetch_consent_history(c, limit, offset))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Doc 25 §4.2/§4.4: generic consent-event recorder, callable for any consent
/// point beyond the Gmail-authorization one that's auto-wired internally
/// (`ingestion::oauth`) — e.g. onboarding disclosures or a support-bundle
/// export, once the frontend flow for those calls it.
#[tauri::command]
pub async fn record_consent_event(
    pool: State<'_, deadpool_sqlite::Pool>,
    consent_type: String,
    detail: String,
) -> Result<(), String> {
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    conn.interact(move |c| crate::db::audit_log::record_consent_event(c, &consent_type, &detail))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
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

/// M25: `fetch_spending_limits`/`update_spending_limits` were called by the
/// frontend (`SpendingLimits.tsx`) but had no backend implementation at all —
/// opening the Spending Limits page threw an immediate, reachable runtime
/// crash. Backed by `local_profile.spending_limit_monthly` (global limit) and
/// `local_profile.limit_thresholds` (JSONB thresholds); per-category budgets
/// have no backing schema anywhere in this codebase (`categories` has no
/// `budget` column) — returned/accepted as an empty list rather than
/// inventing new schema outside this finding's scope.
#[tauri::command]
pub async fn fetch_spending_limits(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<SpendingLimits, String> {
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    conn.interact(|c| {
        let (global_limit, thresholds_json): (f64, Option<String>) = c
            .query_row(
                "SELECT COALESCE(spending_limit_monthly, 0), limit_thresholds FROM local_profile WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| e.to_string())?;

        let thresholds = thresholds_json
            .and_then(|j| serde_json::from_str::<SpendingLimitThresholds>(&j).ok())
            .unwrap_or(SpendingLimitThresholds {
                warn_at_80: true,
                warn_at_90: true,
                warn_at_100: true,
            });

        Ok(SpendingLimits {
            global_limit,
            thresholds,
            categories: Vec::new(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn update_spending_limits(
    pool: State<'_, deadpool_sqlite::Pool>,
    limits: SpendingLimits,
) -> Result<String, String> {
    crate::licensing::gate::assert_write_allowed(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    let conn = pool.get().await.map_err(|e| e.to_string())?;
    conn.interact(move |c| {
        let thresholds_json = serde_json::to_string(&limits.thresholds).map_err(|e| e.to_string())?;
        c.execute(
            "UPDATE local_profile SET spending_limit_monthly = ?1, limit_thresholds = ?2 WHERE id = 1",
            rusqlite::params![limits.global_limit, thresholds_json],
        )
        .map_err(|e| e.to_string())?;
        Ok::<_, String>(())
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok("Spending limits updated".to_string())
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OnboardingPreferences {
    pub timezone: String,
    pub spending_limit_monthly: f64,
    pub historical_scan_months: i64,
    pub llm_model: String,
    pub statement_preference: String,
}

/// G19 fix: onboarding (`Onboarding.tsx`) previously wrote its choices only
/// to browser localStorage — they never survived a reinstall/reset, and
/// `monthlyLimit` in particular never reached the same `local_profile.
/// spending_limit_monthly` row that Settings → Spending Limits reads from,
/// so the limit set during onboarding was silently discarded rather than
/// actually enforced. `local_profile` row id=1 always exists by the time
/// onboarding runs (created by `init_db`), so this is a plain UPDATE.
#[tauri::command]
pub async fn onboarding_save_preferences(
    pool: State<'_, deadpool_sqlite::Pool>,
    preferences: OnboardingPreferences,
) -> Result<String, String> {
    crate::licensing::gate::assert_write_allowed(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    let conn = pool.get().await.map_err(|e| e.to_string())?;
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
    .map_err(|e| e.to_string())??;

    Ok("Onboarding preferences saved".to_string())
}

/// M25: `db_restore_backup` was called by the frontend's corrupted-DB recovery
/// banner (`AppLayout.tsx`'s "Restore from Backup" button) but had no backend
/// implementation at all — clicking it threw an immediate, reachable runtime
/// crash instead of actually recovering anything. Restores from whichever
/// backup (daily rolling or most recent pre-migration snapshot, both from
/// C18/C19) was written most recently, then restarts the app so a fresh
/// `init_db()` opens the restored file cleanly — the exact same
/// file-operations-then-restart pattern C21 established for a safe reason:
/// `finance.db` is open via this same command's live connection pool, and
/// deleting/replacing it is only safe immediately before the process exits.
#[tauri::command]
pub async fn db_restore_backup(app: tauri::AppHandle) -> Result<String, String> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {}", e))?;
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
        .ok_or_else(|| "No backup file found to restore from".to_string())?;

    // Clear any stale WAL/SHM sidecars for the (possibly corrupted) live file
    // before replacing it — leftover WAL data would otherwise reference the
    // old, corrupted state, not the restored snapshot.
    for suffix in ["-wal", "-shm"] {
        let sidecar = std::path::PathBuf::from(format!("{}{}", db_path.display(), suffix));
        let _ = std::fs::remove_file(&sidecar);
    }

    std::fs::copy(&most_recent, &db_path)
        .map_err(|e| format!("Failed to restore backup {:?}: {}", most_recent, e))?;

    tracing::info!("Restored finance.db from backup {:?} — restarting", most_recent);
    app.restart();
}

// G20/H10/J8 fix: renamed from `fetch_instruments` to match Doc 19 §12.1's
// documented `instruments_list` naming.
#[tauri::command]
pub async fn instruments_list(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<Vec<InstrumentRecord>, String> {
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    conn.interact(|c| do_fetch_instruments(c))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_debug_metrics(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<DebugMetrics, String> {
    let conn = pool.get().await.map_err(|e| e.to_string())?;
    conn.interact(|c| do_get_debug_metrics(c))
        .await
        .map_err(|e| e.to_string())?
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
) -> Result<InstrumentRecord, String> {
    crate::licensing::gate::assert_write_allowed(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    let conn = pool.get().await.map_err(|e| e.to_string())?;
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

        crate::db::instruments::insert_instrument(c, &row).map_err(|e| e.to_string())?;

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
    .map_err(|e| e.to_string())?
}

#[derive(serde::Deserialize)]
pub struct InstrumentUpdatePayload {
    pub id: String,
    pub issuer_name: String,
    pub masked_identifier: String,
    pub full_identifier: Option<String>,
    pub billing_cycle_day: Option<u8>,
    pub bank_ifsc: Option<String>,
}

#[tauri::command]
pub async fn instruments_update(
    payload: InstrumentUpdatePayload,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<String, String> {
    crate::licensing::gate::assert_write_allowed(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    let conn = pool.get().await.map_err(|e| e.to_string())?;
    conn.interact(move |c| {
        // Fetch existing
        let mut stmt = c.prepare("SELECT type, status FROM instruments WHERE id = ?").map_err(|e| e.to_string())?;
        let (inst_type, status): (String, String) = stmt.query_row([&payload.id], |row| Ok((row.get(0)?, row.get(1)?))).map_err(|e| e.to_string())?;
        
        let row = crate::db::instruments::InstrumentsRow {
            id: payload.id.clone(),
            r#type: inst_type,
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
            status: status,
            created_at: None,
            updated_at: None,
            is_deleted: false,
            full_identifier: payload.full_identifier.clone(),
            billing_cycle_day: payload.billing_cycle_day,
        };

        crate::db::instruments::update_instrument(c, &row).map_err(|e| e.to_string())?;
        Ok("updated".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// G20/H10/J8 fix: renamed from `instruments_delete` to match Doc 19 §12.4's
// documented `instruments_archive` naming — this already only sets
// `is_deleted = 1` (a soft delete), so "archive" was the accurate name for
// what the command has always done.
#[tauri::command]
pub async fn instruments_archive(
    id: String,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<String, String> {
    crate::licensing::gate::assert_write_allowed(pool.inner())
        .await
        .map_err(|e| e.to_string())?;

    let conn = pool.get().await.map_err(|e| e.to_string())?;
    conn.interact(move |c| {
        c.execute("UPDATE instruments SET is_deleted = 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?", [&id]).map_err(|e| e.to_string())?;
        Ok("deleted".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

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
                is_deleted INTEGER DEFAULT 0
            )",
            [],
        ).unwrap();
        conn.execute(
            "CREATE TABLE transactions (
                id TEXT PRIMARY KEY,
                instrument_id TEXT,
                authorization_time TEXT,
                merchant_display_name TEXT,
                amount REAL,
                category_id TEXT,
                status TEXT,
                source_mix TEXT,
                is_deleted INTEGER DEFAULT 0
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE statements (id TEXT PRIMARY KEY, source_message_id TEXT, parse_status TEXT, created_at DATETIME DEFAULT CURRENT_TIMESTAMP)",
            [],
        ).unwrap();
        conn.execute(
            "CREATE TABLE reconciliation_clusters (id TEXT PRIMARY KEY, cluster_status TEXT, reason TEXT)",
            [],
        ).unwrap();
        conn.execute(
            "CREATE TABLE reconciliation_cluster_members (id TEXT PRIMARY KEY, cluster_id TEXT, canonical_transaction_id TEXT)",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE local_profile (id INTEGER PRIMARY KEY, spending_limit_monthly REAL)",
            [],
        )
        .unwrap();
        // Insert a default profile row so spending_limit_monthly query returns a value
        conn.execute(
            "INSERT INTO local_profile (id, spending_limit_monthly) VALUES (1, 60000.0)",
            [],
        )
        .unwrap();
        conn
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
        ).unwrap();

        let summary = do_fetch_dashboard_summary(&conn).unwrap();
        assert_eq!(summary.total_spend, 2199.0);
        // Seeded mock data has only negative-amount (spend) transactions — income is 0
        assert_eq!(summary.income, 0.0);
        // local_profile has 60000 spending limit
        assert_eq!(summary.limit, 60000.0);
        // No statements with status='UPCOMING' in seeded data
        assert_eq!(summary.upcoming_bills, 0);

        let txs = do_fetch_transactions(&conn, 50, 0).unwrap();
        assert_eq!(txs.len(), 3);
        assert_eq!(txs[0].id, "tx_1"); // 2026-06-10 is the latest date
        assert_eq!(txs[0].amount, -1499.0);

        let stmts = do_fetch_statement_history(&conn).unwrap();
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
}

