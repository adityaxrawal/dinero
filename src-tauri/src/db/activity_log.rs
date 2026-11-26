//! Persistent record of background work outcomes (audit F5-4, F19-3).
//!
//! Scan runs, statement imports, duplicate-check decisions and local-AI
//! results used to be reported only as toasts and an in-memory notification
//! feed, so a restart erased what had happened.
use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

/// Outcome names for scan runs.
pub const SCAN_COMPLETED: &str = "completed";
pub const SCAN_CANCELLED: &str = "cancelled";
pub const SCAN_FAILED: &str = "failed";

/// Entries kept; older ones are pruned on insert. Sized so one scan's AI
/// results (one entry per email the AI finished) don't push out older scans.
const KEEP: i64 = 500;

/// "₹450 at Swiggy" for an observation, from the email's own values, or
/// "a transaction" when they are missing.
pub fn describe_observation(conn: &Connection, observation_id: &str) -> String {
    let row: Option<(Option<i64>, Option<String>)> = conn
        .query_row(
            "SELECT amount_minor, merchant_raw FROM transaction_observations WHERE id = ?1",
            params![observation_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    match row {
        Some((Some(amount), merchant)) => {
            let (whole, paise) = (amount.abs() / 100, amount.abs() % 100);
            let rupees = if paise == 0 {
                format!("₹{whole}")
            } else {
                format!("₹{whole}.{paise:02}")
            };
            match merchant.filter(|m| !m.trim().is_empty()) {
                Some(m) => format!("{rupees} at {}", m.trim()),
                None => rupees,
            }
        }
        _ => "a transaction".to_string(),
    }
}

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
    fn describes_an_observation_from_its_own_values() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute(
            "INSERT INTO transaction_observations (id, source_pipeline, source_record_id, amount_minor, merchant_raw) \
             VALUES ('o1', 'gmail_transaction', 'm1', 45050, 'Swiggy'), ('o2', 'gmail_transaction', 'm2', 20000, NULL)",
            [],
        )
        .unwrap();
        assert_eq!(describe_observation(&conn, "o1"), "₹450.50 at Swiggy");
        assert_eq!(describe_observation(&conn, "o2"), "₹200");
        assert_eq!(describe_observation(&conn, "missing"), "a transaction");
        // The widened kind list (migration 080) accepts the new kinds.
        append(&conn, "cluster", "matched", "x", None).unwrap();
        append(&conn, "ai", "booked", "y", None).unwrap();
    }

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
