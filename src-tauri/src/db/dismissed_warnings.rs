//! audit_07 #10: persistence for user-dismissed `system_warning`s.
//!
//! The in-memory registry in `ipc::system_warnings` answers "what is wrong
//! right now". This table answers "what has the user already told us they
//! know about", which has to outlive the process — a machine that is
//! permanently below the RAM threshold otherwise re-prompts on every launch.

use anyhow::Result;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Identifies *what a warning said*, not just which kind it was, so a
/// dismissal cannot silence a materially different message under the same
/// `warning_type`.
pub fn message_hash(message: &str) -> String {
    format!("{:x}", Sha256::digest(message.as_bytes()))
}

pub fn record_dismissal(conn: &Connection, warning_type: &str, message: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO dismissed_system_warnings (warning_type, message_hash)
         VALUES (?1, ?2)
         ON CONFLICT(warning_type) DO UPDATE SET
             message_hash = excluded.message_hash,
             dismissed_at = CURRENT_TIMESTAMP",
        params![warning_type, message_hash(message)],
    )?;
    Ok(())
}

/// Called when a warning's underlying condition resolves. Dropping the
/// dismissal re-arms it: if the same condition recurs later it is a new event
/// the user has not seen the resolution of, and silently swallowing it would
/// be a worse failure than re-prompting.
pub fn clear_dismissal(conn: &Connection, warning_type: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM dismissed_system_warnings WHERE warning_type = ?1",
        params![warning_type],
    )?;
    Ok(())
}

pub fn load_all(conn: &Connection) -> Result<HashMap<String, String>> {
    let mut stmt =
        conn.prepare("SELECT warning_type, message_hash FROM dismissed_system_warnings")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (k, v) = row?;
        out.insert(k, v);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dismissal_is_keyed_to_the_exact_message() {
        let conn = crate::db::test_helpers::setup_test_db();

        record_dismissal(&conn, "low_ram", "Low RAM: 9 GB free").unwrap();
        let stored = load_all(&conn);
        let stored = stored.unwrap();
        assert_eq!(
            stored.get("low_ram"),
            Some(&message_hash("Low RAM: 9 GB free"))
        );
        assert_ne!(
            stored.get("low_ram"),
            Some(&message_hash("Low RAM: 1 GB free")),
            "a materially different message must not match a prior dismissal"
        );

        // Re-dismissing the same type replaces rather than duplicating —
        // `warning_type` is the primary key, so a second row would fail.
        record_dismissal(&conn, "low_ram", "Low RAM: 1 GB free").unwrap();
        assert_eq!(
            load_all(&conn).unwrap().get("low_ram"),
            Some(&message_hash("Low RAM: 1 GB free"))
        );

        // Condition resolved -> dismissal dropped, so a recurrence is shown.
        clear_dismissal(&conn, "low_ram").unwrap();
        assert!(load_all(&conn).unwrap().is_empty());
    }
}
