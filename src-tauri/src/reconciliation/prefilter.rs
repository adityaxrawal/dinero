//! Narrows candidates by fingerprint before scoring.
//!
//! Scoring every observation against the entire ledger would not scale. The
//! fingerprint lookup reduces the field to plausible matches cheaply, so the
//! expensive comparison runs over a handful of rows rather than all of them.
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

/// Narrows candidates by fingerprint before the expensive scoring pass.
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

    #[test]
    fn test_prefilter_finds_exact_fingerprint_match() {
        let conn = setup_test_db();
        insert_obs(&conn, "obs_prior", "fp_shared", Some("txn_canonical_1"));

        let hit = fingerprint_prefilter_lookup(&conn, "fp_shared", "obs_incoming").unwrap();
        assert_eq!(hit, Some("txn_canonical_1".to_string()));
    }

    #[test]
    fn test_prefilter_falls_through_to_full_scoring_on_miss() {
        let conn = setup_test_db();
        insert_obs(&conn, "obs_prior", "fp_other", Some("txn_canonical_1"));

        let hit = fingerprint_prefilter_lookup(&conn, "fp_nonexistent", "obs_incoming").unwrap();
        assert_eq!(hit, None);
    }

    #[test]
    fn test_prefilter_ignores_unlinked_observation() {
        let conn = setup_test_db();
        insert_obs(&conn, "obs_prior", "fp_shared", None);

        let hit = fingerprint_prefilter_lookup(&conn, "fp_shared", "obs_incoming").unwrap();
        assert_eq!(hit, None);
    }

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
