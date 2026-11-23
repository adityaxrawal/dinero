//! Durable record of consent given and withdrawn.
//!
//! Consent is stored as an event history rather than a boolean flag, so the
//! record shows what was agreed, when, and whether it was later withdrawn --
//! which a single mutable flag could not.
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

/// Records a consent event.
pub fn insert_consent_event(
    conn: &Connection,
    event_type: &str,
    disclosure_text: &str,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO consent_events (id, event_type, disclosure_text, consented_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, event_type, disclosure_text, Utc::now()],
    )?;
    Ok(id)
}

/// Records withdrawal of a previously given consent.
///
/// Appended as a new event rather than deleting the original, so the history
/// shows what was agreed and when it was revoked.
pub fn withdraw_consent_event(conn: &Connection, event_type: &str) -> Result<()> {
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

/// Whether a consent is currently active.
pub fn has_active_consent(conn: &Connection, event_type: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM consent_events WHERE event_type = ?1 AND withdrawn_at IS NULL",
        params![event_type],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Waits until the network-communication disclosure has been acknowledged.
///
/// Background network calls made without a user action (licence revalidation,
/// update checks) start only after it, so nothing leaves the machine before
/// the user has read what will (review P4). Returns false if `cancel` fires
/// first.
pub async fn wait_for_network_consent(
    pool: &deadpool_sqlite::Pool,
    cancel: &tokio_util::sync::CancellationToken,
) -> bool {
    loop {
        if let Ok(conn) = pool.get().await {
            let given = conn
                .interact(|c| {
                    has_active_consent(c, crate::notifications::NETWORK_DISCLOSURE_CONSENT_EVENT_TYPE)
                })
                .await;
            if matches!(given, Ok(Ok(true))) {
                return true;
            }
        }
        tokio::select! {
            _ = cancel.cancelled() => return false,
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
        }
    }
}

/// The full consent history, for the settings view.
pub fn fetch_consent_history(
    conn: &Connection,
    limit: u32,
    offset: u32,
) -> Result<Vec<ConsentEventsRow>> {
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

/// Fetch one consent event.
pub fn get_by_id(conn: &Connection, id: &str) -> Result<Option<ConsentEventsRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, event_type, disclosure_text, consented_at, withdrawn_at FROM consent_events WHERE id = ?1",
    )?;
    Ok(stmt
        .query_row(params![id], row_to_consent_event)
        .optional()?)
}

/// Maps a result row onto a consent event.
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

    async fn setup_pool() -> deadpool_sqlite::Pool {
        let path = crate::db::test_helpers::fresh_temp_db_path();
        crate::db::migrations::run_migrations(&path, None).await.unwrap();
        let mgr = deadpool_sqlite::Manager::from_config(
            &deadpool_sqlite::Config { path, pool: Some(deadpool_sqlite::PoolConfig::new(1)) },
            deadpool_sqlite::Runtime::Tokio1,
        );
        deadpool_sqlite::Pool::builder(mgr).build().unwrap()
    }

    /// Review P4: background network calls wait for the disclosure.
    #[tokio::test]
    async fn network_consent_wait_holds_until_cancelled_without_consent() {
        let pool = setup_pool().await;
        let cancel = tokio_util::sync::CancellationToken::new();
        let waiter = tokio::spawn({
            let (pool, cancel) = (pool.clone(), cancel.clone());
            async move { wait_for_network_consent(&pool, &cancel).await }
        });
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(!waiter.is_finished(), "must not proceed without consent");
        cancel.cancel();
        assert!(!waiter.await.unwrap());
    }

    #[tokio::test]
    async fn network_consent_wait_proceeds_once_acknowledged() {
        let pool = setup_pool().await;
        pool.get()
            .await
            .unwrap()
            .interact(|c| {
                insert_consent_event(c, crate::notifications::NETWORK_DISCLOSURE_CONSENT_EVENT_TYPE, "t")
            })
            .await
            .unwrap()
            .unwrap();
        let cancel = tokio_util::sync::CancellationToken::new();
        assert!(wait_for_network_consent(&pool, &cancel).await);
    }

    #[test]
    fn insert_and_fetch_roundtrip() {
        let conn = setup_db();
        let id =
            insert_consent_event(&conn, "gmail_oauth_consent", "verbatim disclosure text").unwrap();

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

        let history = fetch_consent_history(&conn, 10, 0).unwrap();
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn withdraw_with_no_matching_event_is_a_no_op() {
        let conn = setup_db();
        withdraw_consent_event(&conn, "gmail_oauth_consent").unwrap();
        assert_eq!(fetch_consent_history(&conn, 10, 0).unwrap().len(), 0);
    }

    #[test]
    fn withdraw_only_affects_the_most_recent_unwithdrawn_row() {
        let conn = setup_db();
        let first = insert_consent_event(&conn, "gmail_oauth_consent", "first").unwrap();
        let second = insert_consent_event(&conn, "gmail_oauth_consent", "second").unwrap();

        withdraw_consent_event(&conn, "gmail_oauth_consent").unwrap();

        assert!(get_by_id(&conn, &second)
            .unwrap()
            .unwrap()
            .withdrawn_at
            .is_some());
        assert!(get_by_id(&conn, &first)
            .unwrap()
            .unwrap()
            .withdrawn_at
            .is_none());
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
