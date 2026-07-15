use anyhow::Result;
use chrono::{NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TransactionObservationsRow {
    pub id: String,
    pub canonical_transaction_id: Option<String>,
    pub source_pipeline: Option<String>,
    pub source_record_id: Option<String>,
    pub source_message_id: Option<String>,
    pub source_thread_id: Option<String>,
    pub statement_id: Option<String>,
    pub statement_entry_id: Option<String>,
    pub instrument_id: Option<String>,
    pub direction: Option<String>,
    pub amount: Option<f64>,
    pub amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub event_time: Option<NaiveDateTime>,
    pub event_time_confidence: Option<String>,
    pub posting_date: Option<NaiveDate>,
    pub merchant_raw: Option<String>,
    pub merchant_normalized: Option<String>,
    pub reference_id: Option<String>,
    pub original_amount_minor: Option<i64>,
    pub original_currency: Option<String>,
    pub exchange_rate: Option<f64>,
    pub balance_after_transaction: Option<f64>,
    pub timezone_at_ingestion: Option<String>,
    pub fingerprint: Option<String>,
    pub extraction_method: Option<String>,
    pub confidence_score: Option<f64>,
    pub raw_payload_json: Option<String>,
    pub parser_version: Option<String>,
    pub emi_total_installments: Option<i32>,
    pub emi_installment_number: Option<i32>,
    pub emi_original_amount_minor: Option<i64>,
    pub is_deleted: bool,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

pub fn insert_observation(conn: &Connection, obs: &TransactionObservationsRow) -> Result<()> {
    conn.execute(
        "INSERT INTO transaction_observations (
            id, canonical_transaction_id, source_pipeline, source_record_id, source_message_id, source_thread_id,
            statement_id, statement_entry_id, instrument_id, direction, amount, amount_minor, currency,
            event_time, event_time_confidence, posting_date, merchant_raw, merchant_normalized, reference_id,
            original_amount_minor, original_currency, exchange_rate, balance_after_transaction, timezone_at_ingestion,
            fingerprint, extraction_method, confidence_score, raw_payload_json, parser_version,
            emi_total_installments, emi_installment_number, emi_original_amount_minor, is_deleted, created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
            ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35
        )",
        params![
            obs.id, obs.canonical_transaction_id, obs.source_pipeline, obs.source_record_id, obs.source_message_id, obs.source_thread_id,
            obs.statement_id, obs.statement_entry_id, obs.instrument_id, obs.direction, obs.amount, obs.amount_minor, obs.currency,
            obs.event_time, obs.event_time_confidence, obs.posting_date, obs.merchant_raw, obs.merchant_normalized, obs.reference_id,
            obs.original_amount_minor, obs.original_currency, obs.exchange_rate, obs.balance_after_transaction, obs.timezone_at_ingestion,
            obs.fingerprint, obs.extraction_method, obs.confidence_score, obs.raw_payload_json, obs.parser_version,
            obs.emi_total_installments, obs.emi_installment_number, obs.emi_original_amount_minor, obs.is_deleted, obs.created_at, obs.updated_at,
        ],
    )?;
    Ok(())
}

/// Doc 30 TASK-TXN-009: a re-processed message (e.g. from an overlapping
/// historical scan re-reading a window it already covered) must be silently
/// skipped, never surfaced as an error to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertObservationOutcome {
    Inserted,
    /// The `UNIQUE(source_pipeline, source_record_id)` idempotency
    /// constraint rejected this row because it already exists.
    DuplicateSkipped,
}

/// Enforces `UNIQUE(source_pipeline, source_record_id)` as an application-
/// layer idempotency check: lets the database's own constraint do the
/// actual detection (avoids a check-then-insert race between concurrent
/// Transaction Queue workers), but specifically recognizes *that*
/// constraint's violation and reports it as a clean `DuplicateSkipped`
/// outcome rather than a generic error — any other failure (a CHECK
/// violation, a malformed row) still propagates as a real `Err`, since
/// silently swallowing those would hide genuine bugs.
pub fn insert_observation_idempotent(
    conn: &Connection,
    obs: &TransactionObservationsRow,
) -> Result<InsertObservationOutcome> {
    match insert_observation(conn, obs) {
        Ok(()) => Ok(InsertObservationOutcome::Inserted),
        Err(e) => {
            let is_source_record_duplicate = e
                .downcast_ref::<rusqlite::Error>()
                .map(|re| match re {
                    rusqlite::Error::SqliteFailure(err, Some(msg)) => {
                        err.code == rusqlite::ErrorCode::ConstraintViolation
                            && msg.contains("source_pipeline")
                            && msg.contains("source_record_id")
                    }
                    _ => false,
                })
                .unwrap_or(false);
            if is_source_record_duplicate {
                Ok(InsertObservationOutcome::DuplicateSkipped)
            } else {
                Err(e)
            }
        }
    }
}

pub fn get_observation(conn: &Connection, id: &str) -> Result<Option<TransactionObservationsRow>> {
    let mut stmt =
        conn.prepare("SELECT * FROM transaction_observations WHERE id = ?1 AND is_deleted = 0")?;
    let mut rows = stmt.query([id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_observation(row)?))
    } else {
        Ok(None)
    }
}

pub fn get_observations_for_transaction(
    conn: &Connection,
    transaction_id: &str,
) -> Result<Vec<TransactionObservationsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM transaction_observations WHERE canonical_transaction_id = ?1 AND is_deleted = 0")?;
    let rows = stmt.query_map([transaction_id], row_to_observation)?;

    let mut observations = Vec::new();
    for row in rows {
        observations.push(row?);
    }
    Ok(observations)
}

pub fn select_all_paginated(
    conn: &Connection,
    limit: i64,
    offset: i64,
) -> Result<Vec<TransactionObservationsRow>> {
    let mut stmt = conn.prepare("SELECT * FROM transaction_observations WHERE is_deleted = 0 ORDER BY created_at DESC LIMIT ?1 OFFSET ?2")?;
    let rows = stmt.query_map(params![limit, offset], row_to_observation)?;

    let mut observations = Vec::new();
    for row in rows {
        observations.push(row?);
    }
    Ok(observations)
}

pub fn update_observation(conn: &Connection, obs: &TransactionObservationsRow) -> Result<()> {
    let updated = conn.execute(
        "UPDATE transaction_observations SET
            canonical_transaction_id = ?2, source_pipeline = ?3, source_record_id = ?4, source_message_id = ?5, source_thread_id = ?6,
            statement_id = ?7, statement_entry_id = ?8, instrument_id = ?9, direction = ?10, amount = ?11, amount_minor = ?12, currency = ?13,
            event_time = ?14, event_time_confidence = ?15, posting_date = ?16, merchant_raw = ?17, merchant_normalized = ?18, reference_id = ?19,
            original_amount_minor = ?20, original_currency = ?21, exchange_rate = ?22, balance_after_transaction = ?23, timezone_at_ingestion = ?24,
            fingerprint = ?25, extraction_method = ?26, confidence_score = ?27, raw_payload_json = ?28, parser_version = ?29,
            emi_total_installments = ?30, emi_installment_number = ?31, emi_original_amount_minor = ?32, is_deleted = ?33
        WHERE id = ?1 AND is_deleted = 0",
        params![
            obs.id, obs.canonical_transaction_id, obs.source_pipeline, obs.source_record_id, obs.source_message_id, obs.source_thread_id,
            obs.statement_id, obs.statement_entry_id, obs.instrument_id, obs.direction, obs.amount, obs.amount_minor, obs.currency,
            obs.event_time, obs.event_time_confidence, obs.posting_date, obs.merchant_raw, obs.merchant_normalized, obs.reference_id,
            obs.original_amount_minor, obs.original_currency, obs.exchange_rate, obs.balance_after_transaction, obs.timezone_at_ingestion,
            obs.fingerprint, obs.extraction_method, obs.confidence_score, obs.raw_payload_json, obs.parser_version,
            obs.emi_total_installments, obs.emi_installment_number, obs.emi_original_amount_minor, obs.is_deleted,
        ],
    )?;

    if updated == 0 {
        return Err(anyhow::anyhow!(
            "Transaction observation not found or already deleted"
        ));
    }

    Ok(())
}

pub fn soft_delete(conn: &Connection, id: &str) -> Result<()> {
    let updated = conn.execute(
        "UPDATE transaction_observations SET is_deleted = 1 WHERE id = ?1 AND is_deleted = 0",
        params![id],
    )?;

    if updated == 0 {
        return Err(anyhow::anyhow!(
            "Transaction observation not found or already deleted"
        ));
    }

    Ok(())
}

pub fn row_to_observation(row: &Row) -> rusqlite::Result<TransactionObservationsRow> {
    Ok(TransactionObservationsRow {
        id: row.get("id")?,
        canonical_transaction_id: row.get("canonical_transaction_id")?,
        source_pipeline: row.get("source_pipeline")?,
        source_record_id: row.get("source_record_id")?,
        source_message_id: row.get("source_message_id")?,
        source_thread_id: row.get("source_thread_id")?,
        statement_id: row.get("statement_id")?,
        statement_entry_id: row.get("statement_entry_id")?,
        instrument_id: row.get("instrument_id")?,
        direction: row.get("direction")?,
        amount: row.get("amount")?,
        amount_minor: row.get("amount_minor")?,
        currency: row.get("currency")?,
        event_time: row.get("event_time")?,
        event_time_confidence: row.get("event_time_confidence")?,
        posting_date: row.get("posting_date")?,
        merchant_raw: row.get("merchant_raw")?,
        merchant_normalized: row.get("merchant_normalized")?,
        reference_id: row.get("reference_id")?,
        original_amount_minor: row.get("original_amount_minor")?,
        original_currency: row.get("original_currency")?,
        exchange_rate: row.get("exchange_rate")?,
        balance_after_transaction: row.get("balance_after_transaction")?,
        timezone_at_ingestion: row.get("timezone_at_ingestion")?,
        fingerprint: row.get("fingerprint")?,
        extraction_method: row.get("extraction_method")?,
        confidence_score: row.get("confidence_score")?,
        raw_payload_json: row.get("raw_payload_json")?,
        parser_version: row.get("parser_version")?,
        emi_total_installments: row.get("emi_total_installments")?,
        emi_installment_number: row.get("emi_installment_number")?,
        emi_original_amount_minor: row.get("emi_original_amount_minor")?,
        is_deleted: row.get("is_deleted")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn update_canonical_transaction_id(
    conn: &Connection,
    observation_id: &str,
    canonical_transaction_id: Option<&str>,
) -> Result<()> {
    let count = conn.execute(
        "UPDATE transaction_observations SET canonical_transaction_id = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![observation_id, canonical_transaction_id],
    )?;
    if count == 0 {
        return Err(anyhow::anyhow!("Observation not found"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn setup_db() -> Connection {
        crate::db::test_helpers::setup_test_db()
    }

    fn make_row(id: &str, source_pipeline: &str, source_record_id: &str) -> TransactionObservationsRow {
        TransactionObservationsRow {
            id: id.to_string(),
            canonical_transaction_id: None,
            source_pipeline: Some(source_pipeline.to_string()),
            source_record_id: Some(source_record_id.to_string()),
            source_message_id: Some(source_record_id.to_string()),
            source_thread_id: None,
            statement_id: None,
            statement_entry_id: None,
            instrument_id: None,
            direction: Some("debit".to_string()),
            amount: None,
            amount_minor: Some(150000),
            currency: Some("INR".to_string()),
            event_time: None,
            event_time_confidence: None,
            posting_date: None,
            merchant_raw: Some("Amazon".to_string()),
            merchant_normalized: None,
            reference_id: None,
            original_amount_minor: None,
            original_currency: None,
            exchange_rate: None,
            balance_after_transaction: None,
            timezone_at_ingestion: None,
            fingerprint: None,
            extraction_method: Some("bank_templates".to_string()),
            confidence_score: None,
            raw_payload_json: Some(r#"{"body":"test"}"#.to_string()),
            parser_version: None,
            emi_total_installments: None,
            emi_installment_number: None,
            emi_original_amount_minor: None,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        }
    }

    /// Doc 30 TASK-TXN-009 acceptance test.
    #[test]
    fn test_observation_insert_success() {
        let conn = setup_db();
        let id = Uuid::new_v4().to_string();
        let row = make_row(&id, "gmail_transaction", "msg_1");

        let outcome = insert_observation_idempotent(&conn, &row).unwrap();
        assert_eq!(outcome, InsertObservationOutcome::Inserted);

        let fetched = get_observation(&conn, &id).unwrap().unwrap();
        assert_eq!(fetched.amount_minor, Some(150000));
        assert_eq!(fetched.merchant_raw, Some("Amazon".to_string()));
        assert_eq!(fetched.raw_payload_json, Some(r#"{"body":"test"}"#.to_string()));
    }

    /// Doc 30 TASK-TXN-009 acceptance test: "a re-processed message... is
    /// silently skipped, never an error."
    #[test]
    fn test_duplicate_source_record_silently_skipped() {
        let conn = setup_db();
        let id1 = Uuid::new_v4().to_string();
        let row1 = make_row(&id1, "gmail_transaction", "msg_dup");
        let outcome1 = insert_observation_idempotent(&conn, &row1).unwrap();
        assert_eq!(outcome1, InsertObservationOutcome::Inserted);

        // Same (source_pipeline, source_record_id), different row id --
        // simulates a re-processed message (e.g. overlapping historical scan).
        let id2 = Uuid::new_v4().to_string();
        let row2 = make_row(&id2, "gmail_transaction", "msg_dup");
        let outcome2 = insert_observation_idempotent(&conn, &row2).unwrap();
        assert_eq!(
            outcome2,
            InsertObservationOutcome::DuplicateSkipped,
            "a duplicate (source_pipeline, source_record_id) must be silently skipped, not error"
        );

        // Only the first insert actually landed.
        assert!(get_observation(&conn, &id1).unwrap().is_some());
        assert!(get_observation(&conn, &id2).unwrap().is_none());
    }

    /// A genuine, unrelated failure (not the source_pipeline/source_record_id
    /// duplicate case) must still propagate as a real error, not be silently
    /// swallowed -- e.g. a source_pipeline value the CHECK constraint rejects.
    #[test]
    fn test_genuine_constraint_violation_still_errors() {
        let conn = setup_db();
        let id = Uuid::new_v4().to_string();
        let mut row = make_row(&id, "not_a_valid_pipeline_value", "msg_x");
        row.source_pipeline = Some("not_a_valid_pipeline_value".to_string());

        let result = insert_observation_idempotent(&conn, &row);
        assert!(
            result.is_err(),
            "a CHECK constraint violation must still propagate as an error"
        );
    }

    /// Doc 30 TASK-TXN-008's fingerprint fix depends on `fingerprint` no
    /// longer being UNIQUE (migration 037) -- two observations sharing a
    /// fingerprint (the same real transaction from two sources) must both
    /// be insertable, since deduplication happens downstream via
    /// reconciliation, not by rejecting the second insert outright.
    #[test]
    fn test_duplicate_fingerprint_across_different_sources_both_insert() {
        let conn = setup_db();
        let id1 = Uuid::new_v4().to_string();
        let mut row1 = make_row(&id1, "gmail_transaction", "msg_a");
        row1.fingerprint = Some("shared_fingerprint_abc".to_string());
        assert_eq!(
            insert_observation_idempotent(&conn, &row1).unwrap(),
            InsertObservationOutcome::Inserted
        );

        let id2 = Uuid::new_v4().to_string();
        let mut row2 = make_row(&id2, "statement_pdf", "entry_b");
        row2.fingerprint = Some("shared_fingerprint_abc".to_string());
        assert_eq!(
            insert_observation_idempotent(&conn, &row2).unwrap(),
            InsertObservationOutcome::Inserted,
            "a second observation with the same fingerprint (different \
             source_pipeline/source_record_id) must insert cleanly, not hit \
             a UNIQUE(fingerprint) constraint"
        );
    }
}
