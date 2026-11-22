//! Durable queue for pending LLM extraction jobs.
//!
//! Persisted rather than held in memory so that work in flight when the app
//! quits is replayed at the next launch instead of being lost. Rows are deleted
//! only once the job has genuinely completed.
use anyhow::Result;
use rusqlite::{params, Connection};

pub struct PendingLayer6Job {
    pub id: String,
    pub observation_id: String,
    pub bank_name: String,
    pub body_text: String,
    pub internal_date_seconds: Option<i64>,
}

/// Persists a pending LLM extraction job.
///
/// Written before the job is queued, so work in flight when the app exits is
/// replayed at the next launch rather than lost.
pub fn insert(conn: &Connection, job: &PendingLayer6Job) -> Result<()> {
    conn.execute(
        "INSERT INTO layer6_pending_jobs (
            id, observation_id, bank_name, body_text, internal_date_seconds
        ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            job.id,
            job.observation_id,
            job.bank_name,
            job.body_text,
            job.internal_date_seconds,
        ],
    )?;
    Ok(())
}

/// Removes a job once it has genuinely completed.
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM layer6_pending_jobs WHERE id = ?1", params![id])?;
    Ok(())
}

/// Failed attempts allowed before a job is marked dead.
pub const MAX_ATTEMPTS: i64 = 3;

/// Records a failed attempt; returns true once the job has been marked dead.
pub fn record_failed_attempt(conn: &Connection, id: &str) -> Result<bool> {
    conn.execute(
        "UPDATE layer6_pending_jobs
         SET attempts = attempts + 1,
             dead_at = CASE WHEN attempts + 1 >= ?2 THEN datetime('now') ELSE dead_at END
         WHERE id = ?1",
        params![id, MAX_ATTEMPTS],
    )?;
    let dead: bool = conn
        .query_row(
            "SELECT dead_at IS NOT NULL FROM layer6_pending_jobs WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap_or(false);
    Ok(dead)
}

/// Outstanding (not dead) jobs, replayed at startup.
pub fn select_all(conn: &Connection) -> Result<Vec<PendingLayer6Job>> {
    let mut stmt = conn.prepare(
        "SELECT id, observation_id, bank_name, body_text, internal_date_seconds \
         FROM layer6_pending_jobs WHERE dead_at IS NULL",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(PendingLayer6Job {
            id: row.get(0)?,
            observation_id: row.get(1)?,
            bank_name: row.get(2)?,
            body_text: row.get(3)?,
            internal_date_seconds: row.get(4)?,
        })
    })?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;

    #[tokio::test]
    async fn insert_select_delete_round_trip() {
        let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let pool = init_db(temp_dir.join("test.db"))
            .await
            .expect("DB init failed");
        let conn = pool.get().await.unwrap();

        conn.interact(|c| {
            insert(
                c,
                &PendingLayer6Job {
                    id: "unassigned-1".to_string(),
                    observation_id: "obs-1".to_string(),
                    bank_name: "HDFC Bank".to_string(),
                    body_text: "Rs. 100 debited".to_string(),
                    internal_date_seconds: Some(1_700_000_000),
                },
            )
        })
        .await
        .unwrap()
        .unwrap();

        let all = conn.interact(|c| select_all(c)).await.unwrap().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "unassigned-1");
        assert_eq!(all[0].bank_name, "HDFC Bank");

        conn.interact(|c| delete(c, "unassigned-1"))
            .await
            .unwrap()
            .unwrap();
        let all = conn.interact(|c| select_all(c)).await.unwrap().unwrap();
        assert!(all.is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn job_is_marked_dead_after_max_attempts_and_not_replayed() {
        let conn = crate::db::test_helpers::setup_test_db();
        insert(
            &conn,
            &PendingLayer6Job {
                id: "u1".to_string(),
                observation_id: "o1".to_string(),
                bank_name: "HDFC Bank".to_string(),
                body_text: "x".to_string(),
                internal_date_seconds: None,
            },
        )
        .unwrap();
        for attempt in 1..MAX_ATTEMPTS {
            assert!(!record_failed_attempt(&conn, "u1").unwrap(), "attempt {attempt}");
            assert_eq!(select_all(&conn).unwrap().len(), 1);
        }
        assert!(record_failed_attempt(&conn, "u1").unwrap());
        assert!(select_all(&conn).unwrap().is_empty(), "dead jobs are not replayed");
        let kept: i64 = conn
            .query_row("SELECT COUNT(*) FROM layer6_pending_jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, 1, "dead jobs are kept for diagnosis");
    }
}
