//! Local-lifecycle data retention sweeps (Doc 28 §4.2, J2/J3 fixes).
//!
//! These are storage-minimization measures cited as DPDP-compliance evidence
//! in Doc 25/28 but previously had zero implementing code — this module is
//! that implementation.

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

/// Doc 28 §4.2: canonical transactions older than this are eligible for
/// archival into `finance_archive_YYYY.db`.
const ARCHIVE_AGE_YEARS: i64 = 5;

/// J3 fix (Doc 28 §4.2): copies canonical transactions older than 5 years
/// into a same-encryption `finance_archive_<year>.db` file (one per calendar
/// year of `best_event_time`), using `ATTACH DATABASE ... KEY` so the
/// archive is protected by the identical SQLCipher key as the live database
/// — no new encryption scheme, no plaintext financial data ever written.
///
/// Deliberately additive-only: this copies into the archive but does **not**
/// delete/prune the source rows from `transactions`. Whether archived
/// transactions should still appear in the live Transactions list, reports,
/// etc. is a product decision this function does not make unilaterally —
/// pruning can be layered on once that's decided, reusing the same
/// `is_deleted`/status convention already used elsewhere in this schema.
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

        // SQLCipher extends ATTACH itself with a KEY clause — a separate
        // `PRAGMA archive.key = ...` after a plain ATTACH does not actually
        // encrypt the newly-created attached file with that key.
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

/// How long a matched record keeps its raw payload before the sweep nulls it.
///
/// Raised from 90 days to 1 year for issue #12: the LLM merchant-cleanup pass
/// reads `raw_payload_json`'s stored email body to work out what the real
/// merchant was, and the Evidence pane renders the same body next to the
/// transaction. At 90 days both went blind on anything older than a quarter —
/// which is exactly the backlog a cleanup pass exists to fix. Still a bounded
/// retention window, so the Doc 28 §4.2 storage-minimisation control holds;
/// only the horizon moved.
const RAW_PAYLOAD_RETENTION: &str = "-1 year";

/// J2 fix: nulls `raw_payload_json`/`raw_row_json` on records that are both
/// (a) matched — i.e. reconciled into a canonical transaction, not still
/// pending/unmatched — and (b) older than [`RAW_PAYLOAD_RETENTION`].
/// Unmatched/pending records are deliberately left alone: their raw payload
/// may still be needed for reconciliation or user-facing "why wasn't this
/// matched" debugging.
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

/// How long a row that has reached a terminal state is kept. Same horizon as
/// [`RAW_PAYLOAD_RETENTION`] and for the same reason: past it, the raw payload
/// these rows explain has already been nulled, so what remains is a score and
/// a decision string with nothing left to check them against.
///
/// One constant rather than one per table — every caller means the same thing
/// by it, and a second knob with the same value is a knob nobody will keep in
/// sync.
const SETTLED_ROW_RETENTION: &str = "-1 year";

/// audit_04 #7: `statement_drafts` rows were never deleted. A draft holds
/// `rows_json` — the full parsed row set of a statement — so a `committed`
/// draft is a verbatim second copy of data already in `statement_entries`,
/// and a `discarded` one is a copy of data the user explicitly rejected.
/// Neither has a reader after it leaves `pending_review`.
///
/// `pending_review` drafts are never swept at any age: that is the user's
/// review queue, and an email-scan draft they have not noticed yet is exactly
/// the case the audit worried about — deleting it would silently discard a
/// parsed statement rather than relieve a backlog.
pub fn sweep_settled_statement_drafts(conn: &Connection) -> Result<usize> {
    let deleted = conn.execute(
        "DELETE FROM statement_drafts
         WHERE status IN ('committed', 'discarded')
           AND updated_at < datetime('now', ?1)",
        [SETTLED_ROW_RETENTION],
    )?;

    if deleted > 0 {
        tracing::info!("Retention sweep: deleted {} settled statement drafts", deleted);
    }

    Ok(deleted)
}

/// audit_05 #7 / audit_03 #5: neither `match_decisions` nor
/// `reconciliation_clusters` was ever pruned — both grow with every
/// observation, forever.
///
/// Deletes only settled rows. Specifically **not** deleted:
/// * anything a human touched (`reviewed_by IS NOT NULL`) or still owes a
///   decision on (`pending_review`, `open`, `deferred`) — that is the review
///   backlog itself, not debris;
/// * the original auto-match row behind a `manually_corrected` decision, which
///   Doc 11 §9.1 requires be preserved for traceability. It is found via the
///   `audit_log` entry `append_correction_decision` writes, the only pointer
///   at it (there is no FK);
/// * decisions for observations that never reconciled — the "why wasn't this
///   matched" evidence `sweep_raw_payloads` deliberately keeps too.
///
/// audit_03 #5 also proposes auto-promoting a stale cluster to an auto-match
/// after a TTL. That is not implemented and should not be: the engine routed
/// those candidates to a cluster precisely because it could not tell them
/// apart, so a timer would silently pick a winner among them — Doc 12 §8.2's
/// "ambiguous matches must be kept unresolved rather than forced" exists to
/// prevent exactly that. Growth is bounded here by clearing *settled* clusters
/// instead; an open cluster is a real question for the user and stays put.
///
/// `reconciliation_cluster_members` needs no clause of its own — its FK is
/// `ON DELETE CASCADE`.
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

    // COALESCE, not `created_at`: a cluster opened two years ago and resolved
    // yesterday is a fresh resolution and keeps the full window.
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

        // canonical_transaction_id is a real FK — matched observations need
        // a real row to point at.
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

        // Old + matched — should be cleared.
        conn.execute(
            "INSERT INTO transaction_observations (id, canonical_transaction_id, raw_payload_json, created_at) \
             VALUES ('obs_old_matched', 'txn_1', '{\"secret\":true}', datetime('now', '-400 days'))",
            [],
        )
        .unwrap();

        // Old but unmatched — should NOT be cleared.
        conn.execute(
            "INSERT INTO transaction_observations (id, canonical_transaction_id, raw_payload_json, created_at) \
             VALUES ('obs_old_unmatched', NULL, '{\"secret\":true}', datetime('now', '-400 days'))",
            [],
        )
        .unwrap();

        // Recent + matched — should NOT be cleared yet.
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

    /// audit_05 #7 / audit_03 #5: settled reconciliation audit rows must age
    /// out, but every category the sweep is supposed to protect must survive —
    /// the human decisions, the open review backlog, and the original
    /// auto-match row behind a correction (Doc 11 §9.1).
    #[test]
    fn reconciliation_sweep_clears_settled_rows_and_keeps_the_review_trail() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, amount_minor, currency, direction, is_deleted) \
             VALUES ('txn_1', 'inst_1', 1000, 'INR', 'debit', 0)",
            [],
        )
        .unwrap();
        // Matched, so its decisions are eligible.
        conn.execute(
            "INSERT INTO transaction_observations (id, canonical_transaction_id, created_at) \
             VALUES ('obs_m', 'txn_1', datetime('now', '-400 days'))",
            [],
        )
        .unwrap();
        // Never reconciled — its decision is the "why wasn't this matched"
        // evidence `sweep_raw_payloads` keeps too.
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
        decision("d_old_pending", "obs_m", "pending_review", None, "-400 days");
        decision("d_old_human", "obs_m", "reviewed", Some("user"), "-400 days");
        decision("d_old_unmatched", "obs_u", "not_required", None, "-400 days");
        // The row a `manually_corrected` decision points back at. Only the
        // audit_log entry links them, so that is what has to protect it.
        decision("d_old_corrected", "obs_m", "not_required", None, "-400 days");
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
        // Open and deferred are the user's backlog, never swept regardless of age.
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

    /// audit_04 #7: settled drafts must age out, but a `pending_review` draft
    /// is the user's queue — sweeping one would silently discard a parsed
    /// statement, which is worse than the accumulation the finding describes.
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
        // Created long ago and still unreviewed -- the email-scan draft the
        // user never noticed. Must survive.
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

        // Old transaction (well past the 5-year threshold) — should be archived.
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, amount_minor, currency, direction, best_event_time, is_deleted) \
             VALUES ('old_tx', 'inst_1', 1000, 'INR', 'debit', '2015-03-01 12:00:00', 0)",
            [],
        )
        .unwrap();
        // Recent transaction — should NOT be archived.
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

        // Re-open the archive file fresh (as a future restore/audit tool
        // would) and confirm the row is really there, under the same key.
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

        // Running it again must not error or duplicate the row (idempotent).
        let archived_again = archive_old_transactions(&conn, &dir, test_key).unwrap();
        assert_eq!(archived_again, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
