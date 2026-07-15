//! Doc 30 TASK-DEDUP-001: Fingerprint Pre-Filter Lookup.
//!
//! Before the full windowed candidate search (TASK-DEDUP-003) and scoring
//! engine (TASK-DEDUP-004) ever run, an indexed lookup via
//! `idx_transaction_observations_fingerprint`: an exact fingerprint match
//! against an already-reconciled observation is treated as an extremely
//! strong exact-match candidate, skipping the more expensive fuzzy scoring
//! entirely for performance (Document 29 §11). No match falls through to
//! windowed candidate generation.
//!
//! The fingerprint (Doc 30 TASK-TXN-008) is only ever set on observations
//! whose source pipeline could compute one (Gmail-sourced observations with
//! a resolved `instrument_id` and `connected_account_id`); observations
//! without one (manual entries, and currently statement-PDF rows -- flagged
//! separately, see `src-tauri/src/statements/observation_builder.rs`)
//! simply skip this pre-filter and fall through to windowed search, which is
//! always correctness-preserving, just not the performance shortcut.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

/// Looks up an existing, already-reconciled observation sharing the given
/// fingerprint (excluding the incoming observation's own row, which is
/// already persisted by the time reconciliation runs, Doc 30 TASK-TXN-009).
///
/// Returns the linked `canonical_transaction_id` of the oldest such match,
/// if any -- the caller (`reconciliation::engine::reconcile`) is responsible
/// for verifying the strict exact-match conditions (TASK-DEDUP-002) before
/// treating this as a confirmed match.
pub fn fingerprint_prefilter_lookup(
    conn: &Connection,
    fingerprint: &str,
    exclude_observation_id: &str,
) -> Result<Option<String>> {
    let canonical_id: Option<String> = conn
        .query_row(
            "SELECT canonical_transaction_id FROM transaction_observations \
             WHERE fingerprint = ?1 AND id != ?2 AND canonical_transaction_id IS NOT NULL \
             AND is_deleted = 0 \
             ORDER BY created_at ASC LIMIT 1",
            rusqlite::params![fingerprint, exclude_observation_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(canonical_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory DB");
        conn.execute_batch(
            "CREATE TABLE transaction_observations (
                id                       TEXT PRIMARY KEY,
                canonical_transaction_id TEXT,
                fingerprint              TEXT,
                is_deleted               INTEGER NOT NULL DEFAULT 0,
                created_at               TEXT
            );",
        )
        .expect("create schema");
        conn
    }

    fn insert_obs(
        conn: &Connection,
        id: &str,
        fingerprint: &str,
        canonical_transaction_id: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO transaction_observations (id, fingerprint, canonical_transaction_id, is_deleted, created_at) \
             VALUES (?1, ?2, ?3, 0, ?4)",
            rusqlite::params![id, fingerprint, canonical_transaction_id, Utc::now().naive_utc().to_string()],
        )
        .expect("insert obs");
    }

    /// Doc 30 TASK-DEDUP-001 acceptance test.
    #[test]
    fn test_prefilter_finds_exact_fingerprint_match() {
        let conn = setup_test_db();
        insert_obs(&conn, "obs_prior", "fp_shared", Some("txn_canonical_1"));

        let hit = fingerprint_prefilter_lookup(&conn, "fp_shared", "obs_incoming").unwrap();
        assert_eq!(hit, Some("txn_canonical_1".to_string()));
    }

    /// Doc 30 TASK-DEDUP-001 acceptance test.
    #[test]
    fn test_prefilter_falls_through_to_full_scoring_on_miss() {
        let conn = setup_test_db();
        insert_obs(&conn, "obs_prior", "fp_other", Some("txn_canonical_1"));

        let hit = fingerprint_prefilter_lookup(&conn, "fp_nonexistent", "obs_incoming").unwrap();
        assert_eq!(hit, None);
    }

    /// A fingerprint match against a row that hasn't been reconciled yet
    /// (`canonical_transaction_id IS NULL`) is not a usable pre-filter hit --
    /// there is nothing yet to auto-match against.
    #[test]
    fn test_prefilter_ignores_unlinked_observation() {
        let conn = setup_test_db();
        insert_obs(&conn, "obs_prior", "fp_shared", None);

        let hit = fingerprint_prefilter_lookup(&conn, "fp_shared", "obs_incoming").unwrap();
        assert_eq!(hit, None);
    }

    /// The incoming observation's own row (already persisted, Doc 30
    /// TASK-TXN-009) must never match itself.
    #[test]
    fn test_prefilter_excludes_self() {
        let conn = setup_test_db();
        insert_obs(&conn, "obs_self", "fp_shared", Some("txn_canonical_1"));

        let hit = fingerprint_prefilter_lookup(&conn, "fp_shared", "obs_self").unwrap();
        assert_eq!(hit, None);
    }

    #[test]
    fn test_prefilter_ignores_soft_deleted() {
        let conn = setup_test_db();
        insert_obs(&conn, "obs_deleted", "fp_shared", Some("txn_canonical_1"));
        conn.execute(
            "UPDATE transaction_observations SET is_deleted = 1 WHERE id = 'obs_deleted'",
            [],
        )
        .unwrap();

        let hit = fingerprint_prefilter_lookup(&conn, "fp_shared", "obs_incoming").unwrap();
        assert_eq!(hit, None);
    }
}
