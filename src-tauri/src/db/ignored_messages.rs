//! Messages deliberately skipped during ingestion.
//!
//! Prevents rescanning mail already judged non-financial. Entries expire rather
//! than persisting forever, so a message wrongly classified once is eventually
//! reconsidered rather than being excluded permanently.
use anyhow::Result;
use chrono::{Duration, NaiveDateTime, Utc};
use rusqlite::{params, Connection};

pub const IGNORED_MESSAGE_TTL_DAYS: i64 = 30;

#[derive(Debug, Clone)]
pub struct IgnoredMessageRow {
    pub id: String,
    pub message_id: String,
    pub bank_name: Option<String>,
    pub reason: String,
    pub subject: Option<String>,
    pub snippet: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub expires_at: NaiveDateTime,
    /// The From: domain, so a wrongly rejected sender can be trusted (Q-8).
    pub sender_domain: Option<String>,
    /// Which gate skipped it: `sender` (Gate 1) or `content` (Gate 2).
    pub gate: Option<String>,
}

impl IgnoredMessageRow {
    /// Builds an ignore record, stamping its expiry from the retention window.
    ///
    /// Expiry is set at construction rather than at query time so each record
    /// carries its own lifetime, and a later change to the window does not
    /// retroactively resurrect or purge messages already recorded.
    pub fn new(
        message_id: &str,
        bank_name: Option<&str>,
        reason: &str,
        subject: &str,
        snippet: &str,
    ) -> Self {
        let now = Utc::now().naive_utc();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            message_id: message_id.to_string(),
            bank_name: bank_name.map(str::to_string),
            reason: reason.to_string(),
            subject: Some(subject.to_string()),
            snippet: Some(snippet.to_string()),
            created_at: Some(now),
            expires_at: now + Duration::days(IGNORED_MESSAGE_TTL_DAYS),
            sender_domain: None,
            gate: None,
        }
    }

    /// Adds the sender domain and gate shown in the Skipped emails list.
    pub fn with_source(mut self, sender_domain: Option<&str>, gate: &str) -> Self {
        self.sender_domain = sender_domain.map(str::to_string);
        self.gate = Some(gate.to_string());
        self
    }
}

/// Marks a message as deliberately skipped.
pub fn insert(conn: &Connection, row: &IgnoredMessageRow) -> Result<()> {
    conn.execute(
        "INSERT INTO ignored_messages (
            id, message_id, bank_name, reason, subject, snippet, expires_at, sender_domain, gate
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            row.id,
            row.message_id,
            row.bank_name,
            row.reason,
            row.subject,
            row.snippet,
            row.expires_at,
            row.sender_domain,
            row.gate,
        ],
    )?;
    Ok(())
}

/// Recently ignored messages, so a rescan can skip them cheaply.
pub fn select_recent(conn: &Connection, limit: i64) -> Result<Vec<IgnoredMessageRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, message_id, bank_name, reason, subject, snippet, created_at, expires_at, \
                sender_domain, gate \
         FROM ignored_messages ORDER BY created_at DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |r| {
            Ok(IgnoredMessageRow {
                id: r.get(0)?,
                message_id: r.get(1)?,
                bank_name: r.get(2)?,
                reason: r.get(3)?,
                subject: r.get(4)?,
                snippet: r.get(5)?,
                created_at: r.get(6)?,
                expires_at: r.get(7)?,
                sender_domain: r.get(8)?,
                gate: r.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Re-admits a sender's skipped emails: removes their ignore records and the
/// rejection marks that made rescans skip them, so the next scan reads them
/// again. Returns how many emails were re-admitted.
pub fn readmit_sender(conn: &Connection, sender_domain: &str) -> Result<usize> {
    conn.execute(
        "DELETE FROM processed_messages WHERE decision LIKE 'rejected:%' AND message_id IN \
         (SELECT message_id FROM ignored_messages WHERE sender_domain = ?1)",
        params![sender_domain],
    )?;
    Ok(conn.execute(
        "DELETE FROM ignored_messages WHERE sender_domain = ?1",
        params![sender_domain],
    )?)
}

/// Drops expired ignore records.
///
/// Expiry is deliberate: a message misclassified once is eventually reconsidered
/// rather than excluded permanently by a decision that may have been wrong.
pub fn purge_expired(conn: &Connection) -> Result<usize> {
    let deleted = conn.execute(
        "DELETE FROM ignored_messages WHERE expires_at < datetime('now')",
        [],
    )?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    /// Trusting a sender from Skipped emails clears its ignore records and
    /// rejection marks so the next scan reads those emails again (Q-8); other
    /// senders' records stay.
    #[test]
    fn readmit_sender_clears_only_that_senders_skips() {
        let conn = crate::db::test_helpers::setup_test_db();
        for (id, domain) in [("m1", "mail.hsbc.bank.in"), ("m2", "promo.example.com")] {
            let row = IgnoredMessageRow::new(id, None, "Suspicious domain", "Subject", "snippet")
                .with_source(Some(domain), "sender");
            insert(&conn, &row).unwrap();
            crate::db::processed_messages::record(&conn, id, "rejected:Suspicious domain").unwrap();
        }
        assert_eq!(readmit_sender(&conn, "mail.hsbc.bank.in").unwrap(), 1);
        let left: Vec<String> = select_recent(&conn, 10).unwrap().into_iter().map(|r| r.message_id).collect();
        assert_eq!(left, vec!["m2".to_string()]);
        let processed: i64 = conn
            .query_row("SELECT COUNT(*) FROM processed_messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(processed, 1, "only the trusted sender's rejection mark is cleared");
    }

    use super::*;

    #[test]
    fn insert_and_select_recent_round_trips() {
        let conn = crate::db::test_helpers::setup_test_db();
        let row = IgnoredMessageRow::new(
            "msg_1",
            Some("HDFC Bank"),
            "gate2_reject_Noise",
            "Some subject",
            "Some snippet",
        );
        insert(&conn, &row).unwrap();
        let recent = select_recent(&conn, 10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].message_id, "msg_1");
        assert_eq!(recent[0].bank_name.as_deref(), Some("HDFC Bank"));
    }

    #[test]
    fn purge_expired_only_deletes_past_expiry() {
        let conn = crate::db::test_helpers::setup_test_db();
        let mut expired = IgnoredMessageRow::new("msg_old", None, "gate2_reject_Noise", "s", "s");
        expired.expires_at = Utc::now().naive_utc() - Duration::days(1);
        insert(&conn, &expired).unwrap();

        let fresh = IgnoredMessageRow::new("msg_new", None, "gate2_reject_Noise", "s", "s");
        insert(&conn, &fresh).unwrap();

        let deleted = purge_expired(&conn).unwrap();
        assert_eq!(deleted, 1);
        let remaining = select_recent(&conn, 10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].message_id, "msg_new");
    }
}
