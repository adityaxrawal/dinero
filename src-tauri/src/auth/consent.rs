//! TASK-AUTH-003: Consent Event Recording Table and Write Path.
//!
//! `consent_events` (Document 18 §4.21a) is a dedicated table, separate from
//! `audit_log`, backing the DPDP-relevant Consent History viewer (Document
//! 06 §7). Consent events are never auto-deleted — even Gmail disconnect
//! (TASK-AUTH-006) only sets `withdrawn_at`, never removes the row; the
//! purge exemption extends through account deletion until the final purge
//! step (Document 28 §7 row 15).

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsentEventsRow {
    pub id: String,
    pub event_type: String,
    pub disclosure_text: String,
    pub consented_at: DateTime<Utc>,
    pub withdrawn_at: Option<DateTime<Utc>>,
}

/// Records a consent event. `disclosure_text` must be the exact verbatim
/// text shown to the user at consent time (Document 18 §4.21) — not a
/// paraphrase of what happened.
pub fn insert_consent_event(conn: &Connection, event_type: &str, disclosure_text: &str) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO consent_events (id, event_type, disclosure_text, consented_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, event_type, disclosure_text, Utc::now()],
    )?;
    Ok(id)
}

/// On withdrawal (e.g. Gmail disconnect, TASK-AUTH-006), sets `withdrawn_at`
/// on the most recent not-yet-withdrawn row for `event_type` — never
/// deletes. A no-op (not an error) if there's no matching row to withdraw,
/// since revoke flows must complete even if consent was never recorded
/// (e.g. a pre-TASK-AUTH-003 connection).
pub fn withdraw_consent_event(conn: &Connection, event_type: &str) -> Result<()> {
    // Ordered by `rowid`, not `consented_at` — SQLite's DATETIME storage
    // truncates to whole-second precision, so two events recorded within
    // the same second would otherwise be indistinguishable; `rowid` is
    // monotonically increasing per insert regardless.
    conn.execute(
        "UPDATE consent_events SET withdrawn_at = ?2
         WHERE id = (
             SELECT id FROM consent_events
             WHERE event_type = ?1 AND withdrawn_at IS NULL
             ORDER BY rowid DESC LIMIT 1
         )",
        params![event_type, Utc::now()],
    )?;
    Ok(())
}

/// Whether an un-withdrawn consent event of this type has ever been
/// recorded. TASK-DESK-002 uses this (`event_type =
/// "network_disclosure_acknowledged"`) to gate the native-notification
/// permission request on the user having actually seen the network
/// disclosure screen, rather than requesting it proactively at cold launch.
pub fn has_active_consent(conn: &Connection, event_type: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM consent_events WHERE event_type = ?1 AND withdrawn_at IS NULL",
        params![event_type],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Read-only history for Settings → Privacy → Consent History
/// (`auth_get_consent_history`, Document 19 §5.6).
pub fn fetch_consent_history(conn: &Connection, limit: u32, offset: u32) -> Result<Vec<ConsentEventsRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, event_type, disclosure_text, consented_at, withdrawn_at
         FROM consent_events ORDER BY rowid DESC LIMIT ?1 OFFSET ?2",
    )?;
    let rows = stmt.query_map(params![limit, offset], row_to_consent_event)?;
    let mut events = Vec::new();
    for row in rows {
        events.push(row?);
    }
    Ok(events)
}

pub fn get_by_id(conn: &Connection, id: &str) -> Result<Option<ConsentEventsRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, event_type, disclosure_text, consented_at, withdrawn_at FROM consent_events WHERE id = ?1",
    )?;
    Ok(stmt.query_row(params![id], row_to_consent_event).optional()?)
}

fn row_to_consent_event(row: &rusqlite::Row) -> rusqlite::Result<ConsentEventsRow> {
    Ok(ConsentEventsRow {
        id: row.get(0)?,
        event_type: row.get(1)?,
        disclosure_text: row.get(2)?,
        consented_at: row.get(3)?,
        withdrawn_at: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        crate::db::test_helpers::setup_test_db()
    }

    #[test]
    fn insert_and_fetch_roundtrip() {
        let conn = setup_db();
        let id = insert_consent_event(&conn, "gmail_oauth_consent", "verbatim disclosure text").unwrap();

        let fetched = get_by_id(&conn, &id).unwrap().unwrap();
        assert_eq!(fetched.event_type, "gmail_oauth_consent");
        assert_eq!(fetched.disclosure_text, "verbatim disclosure text");
        assert!(fetched.withdrawn_at.is_none());

        let history = fetch_consent_history(&conn, 10, 0).unwrap();
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn withdraw_sets_withdrawn_at_without_deleting() {
        let conn = setup_db();
        let id = insert_consent_event(&conn, "gmail_oauth_consent", "text").unwrap();

        withdraw_consent_event(&conn, "gmail_oauth_consent").unwrap();

        let fetched = get_by_id(&conn, &id).unwrap().unwrap();
        assert!(fetched.withdrawn_at.is_some());

        // Never deleted.
        let history = fetch_consent_history(&conn, 10, 0).unwrap();
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn withdraw_with_no_matching_event_is_a_no_op() {
        let conn = setup_db();
        // No prior consent event exists for this type at all.
        withdraw_consent_event(&conn, "gmail_oauth_consent").unwrap();
        assert_eq!(fetch_consent_history(&conn, 10, 0).unwrap().len(), 0);
    }

    #[test]
    fn withdraw_only_affects_the_most_recent_unwithdrawn_row() {
        let conn = setup_db();
        let first = insert_consent_event(&conn, "gmail_oauth_consent", "first").unwrap();
        let second = insert_consent_event(&conn, "gmail_oauth_consent", "second").unwrap();

        withdraw_consent_event(&conn, "gmail_oauth_consent").unwrap();

        assert!(get_by_id(&conn, &second).unwrap().unwrap().withdrawn_at.is_some());
        assert!(get_by_id(&conn, &first).unwrap().unwrap().withdrawn_at.is_none());
    }

    #[test]
    fn has_active_consent_false_until_recorded_then_true() {
        let conn = setup_db();
        assert!(!has_active_consent(&conn, "network_disclosure_acknowledged").unwrap());

        insert_consent_event(&conn, "network_disclosure_acknowledged", "text").unwrap();
        assert!(has_active_consent(&conn, "network_disclosure_acknowledged").unwrap());
    }

    #[test]
    fn has_active_consent_false_again_after_withdrawal() {
        let conn = setup_db();
        insert_consent_event(&conn, "network_disclosure_acknowledged", "text").unwrap();
        withdraw_consent_event(&conn, "network_disclosure_acknowledged").unwrap();
        assert!(!has_active_consent(&conn, "network_disclosure_acknowledged").unwrap());
    }
}
