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
