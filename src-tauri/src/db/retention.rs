//! Data retention sweeps, run daily.
//!
//! Enforces the promise that raw source material is not kept indefinitely: raw
//! payloads, settled drafts and reconciliation audit rows are all aged out.
//!
//! Old transactions are archived to a separately encrypted file rather than
//! deleted, which keeps the working database small without discarding the user's
//! financial history.
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

const ARCHIVE_AGE_YEARS: i64 = 5;

/// Moves old transactions into a separately encrypted archive.
///
/// Archived rather than deleted: the working database stays small without the
/// user losing their financial history. The archive carries its own encryption,
/// so it is no more readable than the live database.
pub fn archive_old_transactions(
    conn: &Connection,
    archive_dir: &Path,
    db_key: &str,
) -> Result<usize> {
    std::fs::create_dir_all(archive_dir).context("Failed to create archive directory")?;

    let years: Vec<i64> = {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT CAST(strftime('%Y', best_event_time) AS INTEGER)
             FROM transactions
             WHERE best_event_time IS NOT NULL
               AND best_event_time < datetime('now', ?1)
               AND is_deleted = 0",
        )?;
        let offset = format!("-{} years", ARCHIVE_AGE_YEARS);
        let rows = stmt.query_map([&offset], |row| row.get::<_, i64>(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let mut total_archived = 0usize;
    for year in years {
        let archive_path = archive_dir.join(format!("finance_archive_{}.db", year));
        let archive_path_str = archive_path.to_string_lossy().to_string();

        conn.execute_batch(&format!(
            "ATTACH DATABASE '{}' AS archive KEY '{}';",
            archive_path_str.replace('\'', "''"),
            db_key
        ))?;
        let attach_result = (|| -> Result<usize> {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS archive.transactions AS
                 SELECT * FROM transactions WHERE 0;",
            )?;
            let inserted = conn.execute(
                "INSERT INTO archive.transactions
                 SELECT * FROM transactions
                 WHERE is_deleted = 0
                   AND CAST(strftime('%Y', best_event_time) AS INTEGER) = ?1
                   AND id NOT IN (SELECT id FROM archive.transactions)",
                [year],
            )?;
            Ok(inserted)
        })();

        conn.execute("DETACH DATABASE archive", [])?;

        let inserted = attach_result?;
        if inserted > 0 {
            tracing::info!(
                "Archived {} transactions from {} into {}",
                inserted,
                year,
                archive_path.display()
            );
        }
        total_archived += inserted;
    }

    Ok(total_archived)
}

const RAW_PAYLOAD_RETENTION: &str = "-1 year";

/// Deletes raw source payloads past their retention window.
///
/// Raw payloads are whole bank emails -- the most sensitive material the app
/// holds -- so they are kept only as long as reprocessing might need them.
pub fn sweep_raw_payloads(conn: &Connection) -> Result<(usize, usize)> {
    let observations_cleared = conn.execute(
        "UPDATE transaction_observations
         SET raw_payload_json = NULL
         WHERE canonical_transaction_id IS NOT NULL
           AND raw_payload_json IS NOT NULL
           AND created_at < datetime('now', ?1)",
        [RAW_PAYLOAD_RETENTION],
    )?;

    let entries_cleared = conn.execute(
        "UPDATE statement_entries
         SET raw_row_json = NULL
         WHERE raw_row_json IS NOT NULL
           AND id IN (
             SELECT statement_entry_id FROM transaction_observations
             WHERE canonical_transaction_id IS NOT NULL
               AND statement_entry_id IS NOT NULL
               AND created_at < datetime('now', ?1)
           )",
        [RAW_PAYLOAD_RETENTION],
    )?;

    if observations_cleared > 0 || entries_cleared > 0 {
        tracing::info!(
            "Retention sweep: cleared raw_payload_json on {} observations, raw_row_json on {} statement entries",
            observations_cleared,
            entries_cleared
        );
    }

    Ok((observations_cleared, entries_cleared))
}

const SETTLED_ROW_RETENTION: &str = "-1 year";

/// Removes drafts that have been committed or discarded.
pub fn sweep_settled_statement_drafts(conn: &Connection) -> Result<usize> {
    let deleted = conn.execute(
        "DELETE FROM statement_drafts
         WHERE status IN ('committed', 'discarded')
           AND updated_at < datetime('now', ?1)",
        [SETTLED_ROW_RETENTION],
    )?;

    if deleted > 0 {
        tracing::info!(
            "Retention sweep: deleted {} settled statement drafts",
            deleted
        );
    }

    Ok(deleted)
}

/// Ages out reconciliation audit rows.
pub fn sweep_reconciliation_audit(conn: &Connection) -> Result<(usize, usize)> {
    let decisions_deleted = conn.execute(
        "DELETE FROM match_decisions
         WHERE reviewed_by IS NULL
           AND review_status <> 'pending_review'
           AND created_at < datetime('now', ?1)
           AND id NOT IN (
             SELECT resource_id FROM audit_log
             WHERE resource_type = 'match_decision' AND resource_id IS NOT NULL
           )
           AND observation_id IN (
             SELECT id FROM transaction_observations
             WHERE canonical_transaction_id IS NOT NULL
           )",
        [SETTLED_ROW_RETENTION],
    )?;

    let clusters_deleted = conn.execute(
        "DELETE FROM reconciliation_clusters
         WHERE cluster_status IN ('resolved', 'rejected')
           AND COALESCE(resolved_at, created_at) < datetime('now', ?1)",
        [SETTLED_ROW_RETENTION],
    )?;

    if decisions_deleted > 0 || clusters_deleted > 0 {
        tracing::info!(
            "Retention sweep: deleted {} settled match_decisions, {} settled reconciliation_clusters",
            decisions_deleted,
            clusters_deleted
        );
    }

    Ok((decisions_deleted, clusters_deleted))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute("INSERT INTO local_profile (id) VALUES (1)", [])
            .unwrap();
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier, status) \
             VALUES ('inst_1', 'credit_card', 'HDFC', '1234', 'active')",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn sweep_clears_old_matched_observation_but_not_recent_or_unmatched() {
        let conn = setup_db();

        conn.execute(
            "INSERT INTO transactions (id, instrument_id, amount_minor, currency, direction, is_deleted) \
             VALUES ('txn_1', 'inst_1', 1000, 'INR', 'debit', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, amount_minor, currency, direction, is_deleted) \
             VALUES ('txn_2', 'inst_1', 2000, 'INR', 'debit', 0)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO transaction_observations (id, canonical_transaction_id, raw_payload_json, created_at) \
             VALUES ('obs_old_matched', 'txn_1', '{\"secret\":true}', datetime('now', '-400 days'))",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO transaction_observations (id, canonical_transaction_id, raw_payload_json, created_at) \
             VALUES ('obs_old_unmatched', NULL, '{\"secret\":true}', datetime('now', '-400 days'))",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO transaction_observations (id, canonical_transaction_id, raw_payload_json, created_at) \
             VALUES ('obs_recent_matched', 'txn_2', '{\"secret\":true}', datetime('now', '-1 days'))",
            [],
        )
        .unwrap();

        let (cleared, _) = sweep_raw_payloads(&conn).unwrap();
        assert_eq!(cleared, 1);

        let get_payload = |id: &str| -> Option<String> {
            conn.query_row(
                "SELECT raw_payload_json FROM transaction_observations WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap()
        };

        assert_eq!(get_payload("obs_old_matched"), None);
        assert!(get_payload("obs_old_unmatched").is_some());
        assert!(get_payload("obs_recent_matched").is_some());
    }

    #[test]
    fn reconciliation_sweep_clears_settled_rows_and_keeps_the_review_trail() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, amount_minor, currency, direction, is_deleted) \
             VALUES ('txn_1', 'inst_1', 1000, 'INR', 'debit', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transaction_observations (id, canonical_transaction_id, created_at) \
             VALUES ('obs_m', 'txn_1', datetime('now', '-400 days'))",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transaction_observations (id, canonical_transaction_id, created_at) \
             VALUES ('obs_u', NULL, datetime('now', '-400 days'))",
            [],
        )
        .unwrap();

        let decision = |id: &str, obs: &str, status: &str, by: Option<&str>, age: &str| {
            conn.execute(
                "INSERT INTO match_decisions (id, observation_id, decision, score, review_status, reviewed_by, created_at) \
                 VALUES (?1, ?2, 'auto_matched_scored', 0.9, ?3, ?4, datetime('now', ?5))",
                rusqlite::params![id, obs, status, by, age],
            )
            .unwrap();
        };

        decision("d_old_auto", "obs_m", "not_required", None, "-400 days");
        decision("d_recent_auto", "obs_m", "not_required", None, "-1 days");
        decision(
            "d_old_pending",
            "obs_m",
            "pending_review",
            None,
            "-400 days",
        );
        decision(
            "d_old_human",
            "obs_m",
            "reviewed",
            Some("user"),
            "-400 days",
        );
        decision(
            "d_old_unmatched",
            "obs_u",
            "not_required",
            None,
            "-400 days",
        );
        decision(
            "d_old_corrected",
            "obs_m",
            "not_required",
            None,
            "-400 days",
        );
        conn.execute(
            "INSERT INTO audit_log (id, actor_type, action, resource_type, resource_id) \
             VALUES ('al_1', 'user', 'manual_correction', 'match_decision', 'd_old_corrected')",
            [],
        )
        .unwrap();

        let cluster = |id: &str, status: &str, age: &str| {
            conn.execute(
                "INSERT INTO reconciliation_clusters (id, cluster_status, created_at, resolved_at) \
                 VALUES (?1, ?2, datetime('now', '-400 days'), datetime('now', ?3))",
                rusqlite::params![id, status, age],
            )
            .unwrap();
        };
        cluster("c_old_resolved", "resolved", "-400 days");
        cluster("c_recent_resolved", "resolved", "-1 days");
        cluster("c_old_rejected", "rejected", "-400 days");
        conn.execute(
            "INSERT INTO reconciliation_clusters (id, cluster_status, created_at) \
             VALUES ('c_old_open', 'open', datetime('now', '-400 days'))",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_clusters (id, cluster_status, created_at) \
             VALUES ('c_old_deferred', 'deferred', datetime('now', '-400 days'))",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reconciliation_cluster_members (id, cluster_id, member_role) \
             VALUES ('m_1', 'c_old_resolved', 'incoming')",
            [],
        )
        .unwrap();

        let (decisions, clusters) = sweep_reconciliation_audit(&conn).unwrap();
        assert_eq!(decisions, 1, "only the settled aged auto-decision");
        assert_eq!(clusters, 2, "the aged resolved and rejected clusters");

        let survives = |table: &str, id: &str| -> bool {
            conn.query_row(
                &format!("SELECT COUNT(*) FROM {} WHERE id = ?1", table),
                [id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
                > 0
        };

        assert!(!survives("match_decisions", "d_old_auto"));
        assert!(survives("match_decisions", "d_recent_auto"));
        assert!(survives("match_decisions", "d_old_pending"));
        assert!(survives("match_decisions", "d_old_human"));
        assert!(survives("match_decisions", "d_old_unmatched"));
        assert!(
            survives("match_decisions", "d_old_corrected"),
            "Doc 11 §9.1: the original row behind a correction is preserved"
        );

        assert!(!survives("reconciliation_clusters", "c_old_resolved"));
        assert!(!survives("reconciliation_clusters", "c_old_rejected"));
        assert!(survives("reconciliation_clusters", "c_recent_resolved"));
        assert!(survives("reconciliation_clusters", "c_old_open"));
        assert!(survives("reconciliation_clusters", "c_old_deferred"));
        assert!(
            !survives("reconciliation_cluster_members", "m_1"),
            "members must go with their cluster via ON DELETE CASCADE"
        );
    }

    #[test]
    fn draft_sweep_clears_settled_drafts_but_never_the_review_queue() {
        let conn = setup_db();

        let draft = |id: &str, status: &str, age: &str| {
            conn.execute(
                "INSERT INTO statement_drafts (id, origin, file_hash, rows_json, status, created_at, updated_at) \
                 VALUES (?1, 'manual_upload', ?1, '[]', ?2, datetime('now', '-500 days'), datetime('now', ?3))",
                rusqlite::params![id, status, age],
            )
            .unwrap();
        };

        draft("d_old_committed", "committed", "-400 days");
        draft("d_old_discarded", "discarded", "-400 days");
        draft("d_recent_committed", "committed", "-1 days");
        draft("d_old_pending", "pending_review", "-400 days");

        assert_eq!(sweep_settled_statement_drafts(&conn).unwrap(), 2);

        let survives = |id: &str| -> bool {
            conn.query_row(
                "SELECT COUNT(*) FROM statement_drafts WHERE id = ?1",
                [id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
                > 0
        };
        assert!(!survives("d_old_committed"));
        assert!(!survives("d_old_discarded"));
        assert!(survives("d_recent_committed"));
        assert!(
            survives("d_old_pending"),
            "an unreviewed draft is the review queue, not debris"
        );
    }

    #[test]
    fn archive_copies_old_transactions_into_an_encrypted_yearly_file() {
        let conn = setup_db();
        let test_key = "test_archive_key_0123456789abcdef";

        conn.execute(
            "INSERT INTO transactions (id, instrument_id, amount_minor, currency, direction, best_event_time, is_deleted) \
             VALUES ('old_tx', 'inst_1', 1000, 'INR', 'debit', '2015-03-01 12:00:00', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, amount_minor, currency, direction, best_event_time, is_deleted) \
             VALUES ('recent_tx', 'inst_1', 2000, 'INR', 'debit', datetime('now'), 0)",
            [],
        )
        .unwrap();

        let dir =
            std::env::temp_dir().join(format!("dinero_archive_test_{}", uuid::Uuid::new_v4()));
        let archived = archive_old_transactions(&conn, &dir, test_key).unwrap();
        assert_eq!(archived, 1);

        let archive_path = dir.join("finance_archive_2015.db");
        assert!(archive_path.exists());

        let archive_conn = Connection::open(&archive_path).unwrap();
        archive_conn
            .execute_batch(&format!("PRAGMA key = '{}';", test_key))
            .unwrap();
        let id: String = archive_conn
            .query_row("SELECT id FROM transactions WHERE id = 'old_tx'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(id, "old_tx");

        let archived_again = archive_old_transactions(&conn, &dir, test_key).unwrap();
        assert_eq!(archived_again, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
