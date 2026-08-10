//! Ingestion progress markers, enabling resumable scans.
//!
//! A mailbox scan can run for a long time and must survive being interrupted, so
//! progress is checkpointed continuously and a resumed scan restarts from the
//! last marker rather than from the beginning.
//!
//! `claim_checkpoint_in_progress` is a claim rather than a read: it is what stops
//! two scans of the same account running concurrently and double-ingesting.
use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProcessingCheckpointRow {
    pub id: String,
    pub job_type: String,
    pub job_key: String,
    pub checkpoint_state_json: String,
    pub last_processed_token: Option<String>,
    pub status: String,
    pub updated_at: Option<NaiveDateTime>,
}

/// Insert or update a checkpoint for a job.
pub fn upsert_checkpoint(conn: &Connection, checkpoint: &ProcessingCheckpointRow) -> Result<()> {
    conn.execute(
        "INSERT INTO processing_checkpoints (
            id, job_type, job_key, checkpoint_state_json, last_processed_token, status
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6
        )
        ON CONFLICT(job_type, job_key) DO UPDATE SET
            checkpoint_state_json = excluded.checkpoint_state_json,
            last_processed_token = excluded.last_processed_token,
            status = excluded.status",
        params![
            checkpoint.id,
            checkpoint.job_type,
            checkpoint.job_key,
            checkpoint.checkpoint_state_json,
            checkpoint.last_processed_token,
            checkpoint.status,
        ],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
/// Updates only the progress counters on a checkpoint.
///
/// Called frequently during a scan, so it deliberately touches just the counters
/// rather than rewriting the whole row on every message processed.
pub fn patch_scan_progress(
    conn: &Connection,
    job_key: &str,
    processed_count: usize,
    transactions_found: usize,
    statements_found: usize,
    mandate_events_found: usize,
    non_financial: usize,
    errors: usize,
    pending_enrichment: usize,
) -> Result<()> {
    conn.execute(
        "UPDATE processing_checkpoints
         SET checkpoint_state_json = json_set(
                 checkpoint_state_json,
                 '$.processed_count', ?2,
                 '$.transactions_found', ?3,
                 '$.statements_found', ?4,
                 '$.mandate_events_found', ?5,
                 '$.non_financial', ?6,
                 '$.errors', ?7,
                 '$.pending_enrichment', ?8
             ),
             status = 'in_progress'
         WHERE job_type = 'historical_scan' AND job_key = ?1",
        params![
            job_key,
            processed_count as i64,
            transactions_found as i64,
            statements_found as i64,
            mandate_events_found as i64,
            non_financial as i64,
            errors as i64,
            pending_enrichment as i64,
        ],
    )?;
    Ok(())
}

#[derive(Debug, Clone)]
pub enum ClaimOutcome {
    Claimed(Option<ProcessingCheckpointRow>),
    AlreadyInProgress,
}

/// Attempts to claim a job, refusing if another run already holds it.
///
/// The guard against concurrent scans of the same account, which would
/// double-ingest every message. Returning AlreadyInProgress rather than an error
/// lets the caller treat a contended claim as an ordinary outcome.
///
/// An existing checkpoint's id and state are carried forward, so reclaiming an
/// interrupted job resumes it instead of restarting from the beginning.
pub fn claim_checkpoint_in_progress(
    conn: &Connection,
    job_type: &str,
    job_key: &str,
) -> Result<ClaimOutcome> {
    let existing = get_checkpoint(conn, job_type, job_key)?;
    if let Some(cp) = &existing {
        if cp.status == "in_progress" {
            return Ok(ClaimOutcome::AlreadyInProgress);
        }
    }

    let id = existing
        .as_ref()
        .map(|cp| cp.id.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let state_json = existing
        .as_ref()
        .map(|cp| cp.checkpoint_state_json.clone())
        .unwrap_or_else(|| "{}".to_string());

    let changed = conn.execute(
        "INSERT INTO processing_checkpoints (
            id, job_type, job_key, checkpoint_state_json, last_processed_token, status
        ) VALUES (?1, ?2, ?3, ?4, NULL, 'in_progress')
        ON CONFLICT(job_type, job_key) DO UPDATE SET
            status = 'in_progress',
            updated_at = CURRENT_TIMESTAMP
        WHERE processing_checkpoints.status != 'in_progress'",
        params![id, job_type, job_key, state_json],
    )?;

    if changed == 0 {
        return Ok(ClaimOutcome::AlreadyInProgress);
    }

    Ok(ClaimOutcome::Claimed(existing))
}

/// Fetch a job's checkpoint.
pub fn get_checkpoint(
    conn: &Connection,
    job_type: &str,
    job_key: &str,
) -> Result<Option<ProcessingCheckpointRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, job_type, job_key, checkpoint_state_json, last_processed_token, status, updated_at
         FROM processing_checkpoints
         WHERE job_type = ?1 AND job_key = ?2",
    )?;

    let checkpoint = stmt
        .query_row(params![job_type, job_key], |row: &Row| {
            Ok(ProcessingCheckpointRow {
                id: row.get(0)?,
                job_type: row.get(1)?,
                job_key: row.get(2)?,
                checkpoint_state_json: row.get(3)?,
                last_processed_token: row.get(4)?,
                status: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .optional()?;

    Ok(checkpoint)
}

/// Timestamp of the most recent checkpoint activity.
///
/// Feeds the health report's staleness measure: no checkpoint movement means
/// ingestion has stopped making progress.
pub fn most_recent_checkpoint_updated_at(conn: &Connection) -> Result<Option<NaiveDateTime>> {
    let ts: Option<NaiveDateTime> = conn.query_row(
        "SELECT MAX(updated_at) FROM processing_checkpoints",
        [],
        |row| row.get(0),
    )?;
    Ok(ts)
}
