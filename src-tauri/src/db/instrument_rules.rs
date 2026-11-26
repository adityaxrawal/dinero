//! User-confirmed instrument rules (owner decision Q-6).
//!
//! An alert that names the bank and card type but no card number cannot be
//! attributed by the natural key. Instead of guessing, the user picks the card
//! once and may confirm a rule for that (issuer, type). The rule is consulted
//! only when the number is absent, and can be reverted under Learned Rules.
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Serialize, PartialEq)]
pub struct InstrumentRule {
    pub id: String,
    pub issuer_name: String,
    pub instrument_type: String,
    pub instrument_id: String,
    pub instrument_label: String,
    pub created_at: Option<String>,
}

/// Creates or re-points the active rule for (issuer, type).
pub fn upsert(conn: &Connection, issuer: &str, kind: &str, instrument_id: &str) -> Result<()> {
    let issuer = crate::db::instruments::canonical_issuer(issuer);
    conn.execute(
        "INSERT INTO instrument_rules (id, issuer_name, instrument_type, instrument_id, status) \
         VALUES (?1, ?2, ?3, ?4, 'active') \
         ON CONFLICT(issuer_name, instrument_type) DO UPDATE SET \
             instrument_id = excluded.instrument_id, status = 'active', created_at = CURRENT_TIMESTAMP",
        params![uuid::Uuid::new_v4().to_string(), issuer, kind, instrument_id],
    )?;
    Ok(())
}

/// The instrument an active rule names, if it is still a live instrument.
pub fn lookup(conn: &Connection, issuer: &str, kind: &str) -> Result<Option<String>> {
    let issuer = crate::db::instruments::canonical_issuer(issuer);
    Ok(conn
        .query_row(
            "SELECT r.instrument_id FROM instrument_rules r \
             JOIN instruments i ON i.id = r.instrument_id AND i.is_deleted = 0 \
             WHERE r.issuer_name = ?1 AND r.instrument_type = ?2 AND r.status = 'active'",
            params![issuer, kind],
            |r| r.get(0),
        )
        .optional()?)
}

/// Active rules, for Settings > Learned Rules.
pub fn list_active(conn: &Connection) -> Result<Vec<InstrumentRule>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.issuer_name, r.instrument_type, r.instrument_id, \
                i.issuer_name || ' ' || i.type || ' ' || i.masked_identifier, r.created_at \
         FROM instrument_rules r JOIN instruments i ON i.id = r.instrument_id \
         WHERE r.status = 'active' ORDER BY r.created_at DESC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(InstrumentRule {
                id: r.get(0)?,
                issuer_name: r.get(1)?,
                instrument_type: r.get(2)?,
                instrument_id: r.get(3)?,
                instrument_label: r.get(4)?,
                created_at: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Turns a rule off. Already-booked transactions are not changed.
pub fn revert(conn: &Connection, id: &str) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE instrument_rules SET status = 'inactive' WHERE id = ?1 AND status = 'active'",
        params![id],
    )? == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_is_used_only_for_its_issuer_and_type_and_can_be_reverted() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier) \
             VALUES ('rupay', 'credit_card', 'Jupiter', '0000')",
            [],
        )
        .unwrap();
        upsert(&conn, "Jupiter", "credit_card", "rupay").unwrap();
        assert_eq!(lookup(&conn, "Jupiter", "credit_card").unwrap().as_deref(), Some("rupay"));
        assert_eq!(lookup(&conn, "Jupiter", "UPI").unwrap(), None, "other card types never match");

        // Through the resolver: only when the number is missing.
        assert_eq!(
            crate::db::instruments::resolve_instrument(&conn, Some("credit_card"), Some("Jupiter"), None, None)
                .unwrap()
                .as_deref(),
            Some("rupay")
        );

        let id = list_active(&conn).unwrap()[0].id.clone();
        assert!(revert(&conn, &id).unwrap());
        assert_eq!(lookup(&conn, "Jupiter", "credit_card").unwrap(), None);
    }
}
