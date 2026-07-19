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

/// J2 fix: nulls `raw_payload_json`/`raw_row_json` on records that are both
/// (a) matched — i.e. reconciled into a canonical transaction, not still
/// pending/unmatched — and (b) older than 90 days. Unmatched/pending records
/// are deliberately left alone: their raw payload may still be needed for
/// reconciliation or user-facing "why wasn't this matched" debugging.
pub fn sweep_raw_payloads(conn: &Connection) -> Result<(usize, usize)> {
    let observations_cleared = conn.execute(
        "UPDATE transaction_observations
         SET raw_payload_json = NULL
         WHERE canonical_transaction_id IS NOT NULL
           AND raw_payload_json IS NOT NULL
           AND created_at < datetime('now', '-90 days')",
        [],
    )?;

    let entries_cleared = conn.execute(
        "UPDATE statement_entries
         SET raw_row_json = NULL
         WHERE raw_row_json IS NOT NULL
           AND id IN (
             SELECT statement_entry_id FROM transaction_observations
             WHERE canonical_transaction_id IS NOT NULL
               AND statement_entry_id IS NOT NULL
               AND created_at < datetime('now', '-90 days')
           )",
        [],
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
             VALUES ('obs_old_matched', 'txn_1', '{\"secret\":true}', datetime('now', '-100 days'))",
            [],
        )
        .unwrap();

        // Old but unmatched — should NOT be cleared.
        conn.execute(
            "INSERT INTO transaction_observations (id, canonical_transaction_id, raw_payload_json, created_at) \
             VALUES ('obs_old_unmatched', NULL, '{\"secret\":true}', datetime('now', '-100 days'))",
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
