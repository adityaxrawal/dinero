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

/// Outcome of [`claim_checkpoint_in_progress`].
#[derive(Debug, Clone)]
pub enum ClaimOutcome {
    /// This call claimed the job. Carries whatever checkpoint existed
    /// *before* the claim (`None` for a brand-new job_type/job_key), for
    /// resume-state purposes.
    Claimed(Option<ProcessingCheckpointRow>),
    /// Someone else already holds this job in `in_progress` status.
    AlreadyInProgress,
}

/// Doc 30 TASK-API-009 / Document 19 (`scans_historical` -> dedupe active
/// scans via `SCAN_ALREADY_RUNNING`): atomically claims a checkpoint for
/// `in_progress` in a single `INSERT ... ON CONFLICT ... DO UPDATE ...
/// WHERE` statement, rather than a separate read-then-write. SQLite skips
/// the `DO UPDATE` (and reports zero rows changed) when the `WHERE`
/// condition is false, and serializes writers against each other at the
/// single-statement level regardless of what each caller's own preceding
/// read saw -- so of two near-simultaneous callers for the same
/// `job_type`/`job_key`, only one can ever have this function return
/// `Claimed`, even though both may have raced past an earlier read-only
/// check. Preserves the existing row's `checkpoint_state_json` on claim
/// (only `status`/`updated_at` are touched by the `DO UPDATE` branch) so a
/// paused/failed/cancelled job's resume state survives being re-claimed.
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
        // Someone else's claim won the race between our read above and
        // this write.
        return Ok(ClaimOutcome::AlreadyInProgress);
    }

    Ok(ClaimOutcome::Claimed(existing))
}

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

/// Doc 30 TASK-OPS-003: the freshest `updated_at` across every job's
/// checkpoint row, used to report checkpoint-freshness in the local health
/// report. `None` means no checkpoint has ever been written (a brand-new
/// install), which is not itself unhealthy.
pub fn most_recent_checkpoint_updated_at(conn: &Connection) -> Result<Option<NaiveDateTime>> {
    let ts: Option<NaiveDateTime> = conn.query_row(
        "SELECT MAX(updated_at) FROM processing_checkpoints",
        [],
        |row| row.get(0),
    )?;
    Ok(ts)
}
