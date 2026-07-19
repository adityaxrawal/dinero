//! # Deduplication Gate (Task 4.8)
//!
//! Provides three independent duplicate-detection checks that must all pass before
//! an observation is persisted to the `transaction_observations` table:
//!
//! 1. **Source message ID check** — a Gmail message ID (or equivalent) already exists
//!    in `source_message_id`.
//! 2. **Source record ID check** — the `(source_pipeline, source_record_id)` pair
//!    already exists (mirrors the DB `UNIQUE` constraint but checked proactively so
//!    we can emit a structured log entry instead of a DB error).
//! 3. **Fingerprint hash check** — a SHA-256 fingerprint already exists in the
//!    `fingerprint` column, catching the same *economic event* arriving via a second
//!    pipeline (e.g. email + statement import).
//!
//! On duplicate detection the function returns [`DuplicateDecision::Duplicate`] and
//! the caller **must** silently discard the incoming observation.  A structured
//! tracing log entry is emitted at `warn` level so that the discard is fully
//! observable in production logs without bubbling up as an error.
//!
//! ## Important note on fingerprint semantics
//!
//! The fingerprint is intentionally computed over *economic* fields
//! (`source_pipeline`, `source_record_id`, `amount_minor`, `currency`,
//! `direction`, `event_time_minute_precision`, `masked_identifier`).  Two
//! observations originating from **different** pipelines (e.g. email alert and PDF
//! statement) that represent the **same** economic event *will* collide on the
//! fingerprint.  This is the intended cross-pipeline deduplication behaviour.
//!
//! Conversely, two observations representing the **same** real-world event but
//! ingested via *different* channels before reconciliation — where the fingerprint
//! inputs differ — will **not** collide at this gate; they survive as separate
//! `transaction_observations` rows and the reconciler collapses them later.

use anyhow::Result;
use rusqlite::Connection;
use tracing::warn;

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// The outcome of [`check_duplicate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DuplicateDecision {
    /// Not a duplicate — safe to persist.
    NotDuplicate,
    /// A duplicate was detected.  The caller must silently discard the
    /// observation.  The `reason` field carries a machine-readable tag that
    /// was already emitted to the structured log.
    Duplicate { reason: DuplicateReason },
}

/// Machine-readable reason tag for a duplicate detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DuplicateReason {
    /// A row with the same `source_message_id` already exists.
    SourceMessageId,
    /// A row with the same `(source_pipeline, source_record_id)` pair already
    /// exists.
    SourceRecordId,
    /// A row with the same SHA-256 `fingerprint` already exists.
    Fingerprint,
}

impl DuplicateReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            DuplicateReason::SourceMessageId => "source_message_id",
            DuplicateReason::SourceRecordId => "source_record_id",
            DuplicateReason::Fingerprint => "fingerprint",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Gate entry-point
// ─────────────────────────────────────────────────────────────────────────────

/// Checks whether an incoming observation is a duplicate of any already-persisted
/// observation.
///
/// Checks are performed in priority order:
/// 1. `source_message_id`
/// 2. `(source_pipeline, source_record_id)`
/// 3. `fingerprint`
///
/// Returns on the **first** duplicate found.  If all checks pass the function
/// returns [`DuplicateDecision::NotDuplicate`].
///
/// # Parameters
/// - `conn`              — An open `rusqlite::Connection`.
/// - `source_message_id` — The raw Gmail (or equivalent) message ID.  `None`
///   means the pipeline has no per-message identity and this check is skipped.
/// - `source_pipeline`   — The pipeline name (e.g. `"gmail_polling"`).
/// - `source_record_id`  — The per-pipeline unique record identifier.  `None`
///   means the check is skipped.
/// - `fingerprint`       — The SHA-256 fingerprint of the economic fields.
///   `None` means the check is skipped.
pub fn check_duplicate(
    conn: &Connection,
    source_message_id: Option<&str>,
    source_pipeline: &str,
    source_record_id: Option<&str>,
    fingerprint: Option<&str>,
) -> Result<DuplicateDecision> {
    // ── Check 1: source_message_id ───────────────────────────────────────────
    if let Some(msg_id) = source_message_id {
        if is_message_id_duplicate(conn, msg_id)? {
            warn!(
                duplicate_reason = "source_message_id",
                source_message_id = msg_id,
                source_pipeline = source_pipeline,
                "Deduplication gate: observation discarded — duplicate source_message_id"
            );
            return Ok(DuplicateDecision::Duplicate {
                reason: DuplicateReason::SourceMessageId,
            });
        }
    }

    // ── Check 2: (source_pipeline, source_record_id) ─────────────────────────
    if let Some(rec_id) = source_record_id {
        if is_source_record_duplicate(conn, source_pipeline, rec_id)? {
            warn!(
                duplicate_reason = "source_record_id",
                source_pipeline = source_pipeline,
                source_record_id = rec_id,
                "Deduplication gate: observation discarded — duplicate (source_pipeline, source_record_id)"
            );
            return Ok(DuplicateDecision::Duplicate {
                reason: DuplicateReason::SourceRecordId,
            });
        }
    }

    // ── Check 3: fingerprint ─────────────────────────────────────────────────
    if let Some(fp) = fingerprint {
        if is_fingerprint_duplicate(conn, fp)? {
            warn!(
                duplicate_reason = "fingerprint",
                source_pipeline = source_pipeline,
                fingerprint = fp,
                "Deduplication gate: observation discarded — duplicate fingerprint"
            );
            return Ok(DuplicateDecision::Duplicate {
                reason: DuplicateReason::Fingerprint,
            });
        }
    }

    Ok(DuplicateDecision::NotDuplicate)
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Returns `true` if any non-deleted observation row carries the given
/// `source_message_id`.
fn is_message_id_duplicate(conn: &Connection, source_message_id: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transaction_observations \
         WHERE source_message_id = ?1 AND is_deleted = 0",
        rusqlite::params![source_message_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Returns `true` if any non-deleted observation row carries the given
/// `(source_pipeline, source_record_id)` pair.
fn is_source_record_duplicate(
    conn: &Connection,
    source_pipeline: &str,
    source_record_id: &str,
) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transaction_observations \
         WHERE source_pipeline = ?1 AND source_record_id = ?2 AND is_deleted = 0",
        rusqlite::params![source_pipeline, source_record_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Returns `true` if any non-deleted observation row carries the given
/// SHA-256 `fingerprint`.
fn is_fingerprint_duplicate(conn: &Connection, fingerprint: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transaction_observations \
         WHERE fingerprint = ?1 AND is_deleted = 0",
        rusqlite::params![fingerprint],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rusqlite::Connection;
    use uuid::Uuid;

    // ── Schema helpers ────────────────────────────────────────────────────────

    /// Creates an in-memory SQLite database with the minimal schema required by
    /// the deduplication gate tests.
    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory DB");
        conn.execute_batch(
            "CREATE TABLE transaction_observations (
                id                        TEXT PRIMARY KEY,
                canonical_transaction_id  TEXT,
                source_pipeline           TEXT,
                source_record_id          TEXT,
                source_message_id         TEXT,
                source_thread_id          TEXT,
                statement_id              TEXT,
                statement_entry_id        TEXT,
                instrument_id             TEXT,
                direction                 TEXT,
                amount                    REAL,
                amount_minor              INTEGER,
                currency                  TEXT,
                event_time                TEXT,
                event_time_confidence     TEXT,
                posting_date              TEXT,
                merchant_raw              TEXT,
                merchant_normalized       TEXT,
                reference_id              TEXT,
                original_amount_minor     INTEGER,
                original_currency         TEXT,
                exchange_rate             REAL,
                balance_after_transaction REAL,
                timezone_at_ingestion     TEXT,
                fingerprint               TEXT,
                extraction_method         TEXT,
                confidence_score          REAL,
                raw_payload_json          TEXT,
                parser_version            TEXT,
                emi_total_installments    INTEGER,
                emi_installment_number    INTEGER,
                emi_original_amount_minor INTEGER,
                is_deleted                INTEGER NOT NULL DEFAULT 0,
                created_at                TEXT,
                updated_at                TEXT,
                UNIQUE(source_pipeline, source_record_id),
                UNIQUE(fingerprint)
            );",
        )
        .expect("create schema");
        conn
    }

    /// Inserts a minimal observation row for deduplication testing.
    fn insert_obs(
        conn: &Connection,
        id: &str,
        source_pipeline: &str,
        source_record_id: &str,
        source_message_id: &str,
        fingerprint: &str,
    ) {
        conn.execute(
            "INSERT INTO transaction_observations (
                id, source_pipeline, source_record_id, source_message_id,
                fingerprint, is_deleted, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)",
            rusqlite::params![
                id,
                source_pipeline,
                source_record_id,
                source_message_id,
                fingerprint,
                Utc::now().naive_utc().to_string(),
            ],
        )
        .expect("insert obs");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Required test: duplicate source_message_id is suppressed
    // ─────────────────────────────────────────────────────────────────────────

    /// Verifies that an incoming observation whose `source_message_id` matches an
    /// already-persisted row is detected and the gate returns
    /// `DuplicateDecision::Duplicate` with reason `DuplicateReason::SourceMessageId`.
    #[test]
    fn test_duplicate_message_id_suppressed() {
        let conn = setup_test_db();

        // Seed an existing observation with a known message ID.
        let existing_msg_id = "gmail_msg_abc123";
        insert_obs(
            &conn,
            &Uuid::new_v4().to_string(),
            "gmail_polling",
            "rec_001",
            existing_msg_id,
            "fp_aaaaaaa",
        );

        // Incoming observation has the same source_message_id.
        let decision = check_duplicate(
            &conn,
            Some(existing_msg_id), // <── collision on message ID
            "gmail_polling",
            Some("rec_002"),    // different record id — would not collide alone
            Some("fp_bbbbbbb"), // different fingerprint — would not collide alone
        )
        .expect("check_duplicate must not error");

        assert_eq!(
            decision,
            DuplicateDecision::Duplicate {
                reason: DuplicateReason::SourceMessageId
            },
            "Expected SourceMessageId duplicate decision"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Required test: duplicate fingerprint is suppressed
    // ─────────────────────────────────────────────────────────────────────────

    /// Verifies that an incoming observation whose SHA-256 fingerprint matches an
    /// already-persisted row is detected and the gate returns
    /// `DuplicateDecision::Duplicate` with reason `DuplicateReason::Fingerprint`.
    #[test]
    fn test_duplicate_fingerprint_suppressed() {
        let conn = setup_test_db();

        // Seed an existing observation with a known fingerprint.
        let existing_fp = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        insert_obs(
            &conn,
            &Uuid::new_v4().to_string(),
            "gmail_polling",
            "rec_100",
            "gmail_msg_100",
            existing_fp,
        );

        // Incoming observation has a different message ID and record ID but the
        // same fingerprint — i.e. same economic event, different pipeline / retry.
        let decision = check_duplicate(
            &conn,
            Some("gmail_msg_999"), // different message id — will not trigger check 1
            "statement_import",
            Some("rec_999"),   // different record id — will not trigger check 2
            Some(existing_fp), // <── fingerprint collision
        )
        .expect("check_duplicate must not error");

        assert_eq!(
            decision,
            DuplicateDecision::Duplicate {
                reason: DuplicateReason::Fingerprint
            },
            "Expected Fingerprint duplicate decision"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Required test: same event from email and statement not suppressed at
    // observation layer
    // ─────────────────────────────────────────────────────────────────────────

    /// Verifies that two observations representing the **same** economic event that
    /// arrive via **different** source pipelines, with **different** fingerprints
    /// (because the fingerprint inputs from each pipeline differ), and **different**
    /// message IDs, are **not** suppressed by the deduplication gate.
    ///
    /// The gate operates at the per-observation level.  Cross-pipeline
    /// reconciliation of the same event happens downstream; the gate only
    /// suppresses exact identifier or fingerprint collisions.
    #[test]
    fn test_same_event_from_email_and_statement_not_suppressed_at_observation() {
        let conn = setup_test_db();

        // Seed an email-pipeline observation for a given economic event.
        let email_fp = "email_fingerprint_aaaaaa";
        insert_obs(
            &conn,
            &Uuid::new_v4().to_string(),
            "gmail_polling",
            "gmail_rec_200",
            "gmail_msg_200",
            email_fp,
        );

        // Incoming statement-pipeline observation for the same real-world event
        // but with a different fingerprint (different pipeline + record_id fields).
        let statement_fp = "statement_fingerprint_bbbbbb";
        let decision = check_duplicate(
            &conn,
            None, // statement pipeline has no Gmail message ID
            "statement_import",
            Some("stmt_entry_200"), // different record id
            Some(statement_fp),     // different fingerprint
        )
        .expect("check_duplicate must not error");

        assert_eq!(
            decision,
            DuplicateDecision::NotDuplicate,
            "A statement observation for the same real-world event must NOT be \
             suppressed at the observation layer when fingerprints differ"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Boundary: fresh observation is not suppressed
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_fresh_observation_not_suppressed() {
        let conn = setup_test_db();

        let decision = check_duplicate(
            &conn,
            Some("brand_new_msg_id"),
            "gmail_polling",
            Some("brand_new_rec_id"),
            Some("brand_new_fingerprint"),
        )
        .expect("check_duplicate must not error");

        assert_eq!(
            decision,
            DuplicateDecision::NotDuplicate,
            "A fresh observation must not be flagged as a duplicate"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Boundary: soft-deleted duplicate does not trigger the gate
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_soft_deleted_duplicate_not_suppressed() {
        let conn = setup_test_db();

        // Insert an observation and immediately soft-delete it.
        let msg_id = "softdel_msg_id";
        let obs_id = Uuid::new_v4().to_string();
        insert_obs(&conn, &obs_id, "gmail_polling", "rec_sd", msg_id, "fp_sd");
        conn.execute(
            "UPDATE transaction_observations SET is_deleted = 1 WHERE id = ?1",
            rusqlite::params![obs_id],
        )
        .expect("soft delete");

        // The gate must not see the deleted row as a collision.
        let decision = check_duplicate(
            &conn,
            Some(msg_id),
            "gmail_polling",
            Some("rec_sd"),
            Some("fp_sd"),
        )
        .expect("check_duplicate must not error");

        assert_eq!(
            decision,
            DuplicateDecision::NotDuplicate,
            "Soft-deleted observations must not trigger the deduplication gate"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Boundary: None message_id skips check 1
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_none_message_id_skips_check_one() {
        let conn = setup_test_db();

        // Seed a row whose message ID would collide — but we pass None.
        insert_obs(
            &conn,
            &Uuid::new_v4().to_string(),
            "gmail_polling",
            "rec_skip",
            "msg_skip",
            "fp_skip",
        );

        // Completely different pipeline + record_id + fingerprint with msg_id=None.
        let decision = check_duplicate(
            &conn,
            None, // skip check 1
            "statement_import",
            Some("rec_new"),
            Some("fp_new"),
        )
        .expect("check_duplicate must not error");

        assert_eq!(decision, DuplicateDecision::NotDuplicate);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Boundary: (pipeline, record_id) collision is suppressed
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_duplicate_source_record_id_suppressed() {
        let conn = setup_test_db();

        let pipeline = "gmail_polling";
        let rec_id = "rec_collide";
        insert_obs(
            &conn,
            &Uuid::new_v4().to_string(),
            pipeline,
            rec_id,
            "msg_original",
            "fp_original",
        );

        let decision = check_duplicate(
            &conn,
            Some("msg_different"), // different message id — will not trigger check 1
            pipeline,
            Some(rec_id),         // <── collision on (pipeline, record_id)
            Some("fp_different"), // different fingerprint — will not reach check 3
        )
        .expect("check_duplicate must not error");

        assert_eq!(
            decision,
            DuplicateDecision::Duplicate {
                reason: DuplicateReason::SourceRecordId
            },
            "Expected SourceRecordId duplicate decision"
        );
    }
}
