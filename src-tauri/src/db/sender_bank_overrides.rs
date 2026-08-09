//! Gate 1's runtime relabel layer: "mail from this verified domain is actually
//! <bank>" (design 2026-07-29).
//!
//! Deliberately **not** folded into `pending_senders`, which looks similar and
//! sits at the same call site. That table promotes an *unverified* domain to
//! verified — a security decision the user makes explicitly about a sender that
//! was rejected. This one only renames the bank on a sender that already
//! passed verification. Sharing one table would mean a "wrong bank" report
//! could silently grant verification to any domain the user typed, which is a
//! much larger thing than the question they were answering.
//!
//! Domain-scoped, one row per domain. A per-address variant was considered and
//! dropped: this is the highest-blast-radius write in the whole feedback
//! pipeline (misroute a domain and every future email from it lands under the
//! wrong bank), and two scopes would double the ways to get it wrong for a
//! distinction real bank senders do not actually make.

use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SenderBankOverride {
    pub id: String,
    pub domain: String,
    pub bank_name: String,
    pub display_name: Option<String>,
    pub status: String,
    pub created_at: Option<NaiveDateTime>,
}

fn map_row(row: &rusqlite::Row) -> rusqlite::Result<SenderBankOverride> {
    Ok(SenderBankOverride {
        id: row.get(0)?,
        domain: row.get(1)?,
        bank_name: row.get(2)?,
        display_name: row.get(3)?,
        status: row.get(4)?,
        created_at: row.get(5)?,
    })
}

/// Records (or replaces) the override for one domain, reactivating it if a
/// previous report had been reverted. Returns the row id.
pub fn upsert(
    conn: &Connection,
    domain: &str,
    bank_name: &str,
    display_name: Option<&str>,
    feedback_log_id: Option<&str>,
) -> Result<String> {
    let domain = domain.trim().to_lowercase();
    if domain.is_empty() {
        return Err(anyhow::anyhow!("sender override requires a domain"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO sender_bank_overrides
            (id, domain, bank_name, display_name, status, triggering_feedback_log_id)
         VALUES (?1, ?2, ?3, ?4, 'active', ?5)
         ON CONFLICT(domain) DO UPDATE SET
            bank_name = excluded.bank_name,
            display_name = excluded.display_name,
            status = 'active',
            triggering_feedback_log_id = excluded.triggering_feedback_log_id,
            updated_at = CURRENT_TIMESTAMP",
        params![id, domain, bank_name, display_name, feedback_log_id],
    )?;
    let stored_id: String = conn.query_row(
        "SELECT id FROM sender_bank_overrides WHERE domain = ?1",
        params![domain],
        |r| r.get(0),
    )?;
    Ok(stored_id)
}

/// What Gate 1 consults. Kept small and indexed — this runs per message.
pub fn select_active(conn: &Connection) -> Result<Vec<SenderBankOverride>> {
    let mut stmt = conn.prepare(
        "SELECT id, domain, bank_name, display_name, status, created_at
         FROM sender_bank_overrides WHERE status = 'active' ORDER BY domain ASC",
    )?;
    let rows = stmt.query_map([], map_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Everything, including reverted rows, for the Settings review panel.
pub fn select_all(conn: &Connection) -> Result<Vec<SenderBankOverride>> {
    let mut stmt = conn.prepare(
        "SELECT id, domain, bank_name, display_name, status, created_at
         FROM sender_bank_overrides ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], map_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Retires an override. Soft, not a delete: the row is the only record that
/// this domain was ever relabelled, and "why did my mail move banks last
/// month" needs to stay answerable.
pub fn deactivate(conn: &Connection, id: &str) -> Result<()> {
    let updated = conn.execute(
        "UPDATE sender_bank_overrides SET status = 'inactive', updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![id],
    )?;
    if updated == 0 {
        return Err(anyhow::anyhow!("Sender override not found"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        crate::db::test_helpers::setup_test_db()
    }

    #[test]
    fn upsert_creates_then_replaces_by_domain() {
        let conn = setup_db();
        let first = upsert(
            &conn,
            "alerts.hdfcbank.net",
            "HDFC Bank",
            Some("HDFC"),
            Some("fb1"),
        )
        .unwrap();
        let second = upsert(
            &conn,
            "alerts.hdfcbank.net",
            "ICICI Bank",
            None,
            Some("fb2"),
        )
        .unwrap();

        assert_eq!(first, second, "a domain must have exactly one override row");
        let all = select_all(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].bank_name, "ICICI Bank");
        assert_eq!(all[0].status, "active");
    }

    #[test]
    fn deactivate_hides_the_override_from_the_gate() {
        let conn = setup_db();
        let id = upsert(&conn, "mail.axisbank.com", "Axis Bank", None, None).unwrap();
        assert_eq!(select_active(&conn).unwrap().len(), 1);

        deactivate(&conn, &id).unwrap();
        assert!(select_active(&conn).unwrap().is_empty());
        assert_eq!(
            select_all(&conn).unwrap().len(),
            1,
            "a deactivated override stays visible to the review panel"
        );
    }

    #[test]
    fn domains_are_stored_lowercased() {
        let conn = setup_db();
        upsert(&conn, "Alerts.HDFCBank.NET", "HDFC Bank", None, None).unwrap();
        let all = select_all(&conn).unwrap();
        assert_eq!(
            all[0].domain, "alerts.hdfcbank.net",
            "the gate compares against an already-lowercased domain, so storage must match"
        );
    }

    #[test]
    fn reactivating_a_deactivated_domain_works() {
        let conn = setup_db();
        let id = upsert(&conn, "sbi.co.in", "SBI", None, None).unwrap();
        deactivate(&conn, &id).unwrap();

        upsert(&conn, "sbi.co.in", "State Bank of India", None, None).unwrap();
        let active = select_active(&conn).unwrap();
        assert_eq!(active.len(), 1, "a fresh report must revive the override");
        assert_eq!(active[0].bank_name, "State Bank of India");
    }
}
