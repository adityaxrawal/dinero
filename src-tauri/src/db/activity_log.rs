//! Persistent record of background work outcomes (audit F5-4).
//!
//! Scan runs and statement imports used to be reported only as toasts and an
//! in-memory notification feed, so a restart erased what the last scan did.
use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

/// Outcome names for scan runs.
pub const SCAN_COMPLETED: &str = "completed";
pub const SCAN_CANCELLED: &str = "cancelled";
pub const SCAN_FAILED: &str = "failed";

/// Entries kept; older ones are pruned on insert.
const KEEP: i64 = 200;

#[derive(Debug, Serialize, PartialEq)]
pub struct ActivityEntry {
    pub id: String,
    pub kind: String,
    pub outcome: String,
    pub summary: String,
    pub details_json: Option<String>,
    pub created_at: Option<String>,
}

/// Records one outcome and keeps the log to the newest entries.
pub fn append(
    conn: &Connection,
    kind: &str,
    outcome: &str,
    summary: &str,
    details: Option<&serde_json::Value>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO activity_log (id, kind, outcome, summary, details_json) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            uuid::Uuid::new_v4().to_string(),
            kind,
            outcome,
            summary,
            details.map(|d| d.to_string())
        ],
    )?;
    conn.execute(
        "DELETE FROM activity_log WHERE id NOT IN \
         (SELECT id FROM activity_log ORDER BY created_at DESC, rowid DESC LIMIT ?1)",
        params![KEEP],
    )?;
    Ok(())
}

/// Newest entries first.
pub fn list(conn: &Connection, limit: i64) -> Result<Vec<ActivityEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, outcome, summary, details_json, created_at FROM activity_log \
         ORDER BY created_at DESC, rowid DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |r| {
            Ok(ActivityEntry {
                id: r.get(0)?,
                kind: r.get(1)?,
                outcome: r.get(2)?,
                summary: r.get(3)?,
                details_json: r.get(4)?,
                created_at: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_newest_first_and_prunes_to_the_cap() {
        let conn = crate::db::test_helpers::setup_test_db();
        for i in 0..(KEEP + 5) {
            append(&conn, "scan", "completed", &format!("run {i}"), None).unwrap();
        }
        let all = list(&conn, 1000).unwrap();
        assert_eq!(all.len() as i64, KEEP);
        assert_eq!(all[0].summary, format!("run {}", KEEP + 4));
    }
}
