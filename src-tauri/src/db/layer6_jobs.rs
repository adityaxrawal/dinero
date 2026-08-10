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

/// Outstanding jobs, replayed at startup.
pub fn select_all(conn: &Connection) -> Result<Vec<PendingLayer6Job>> {
    let mut stmt = conn.prepare(
        "SELECT id, observation_id, bank_name, body_text, internal_date_seconds \
         FROM layer6_pending_jobs",
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
}
