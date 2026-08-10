//! Records which system warnings the user has dismissed.
//!
//! Keyed by a hash of the message rather than the text itself, which keeps the
//! table compact and stable while ensuring a materially reworded warning is
//! treated as new and shown again.
use anyhow::Result;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Hashes a warning message to key its dismissal.
///
/// Keying on content rather than an identifier means a materially reworded
/// warning counts as new and is shown again, while a repeat of the same warning
/// stays dismissed.
pub fn message_hash(message: &str) -> String {
    format!("{:x}", Sha256::digest(message.as_bytes()))
}

/// Persists a dismissal so it survives a restart.
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

/// Clears a dismissal, allowing the warning to reappear.
pub fn clear_dismissal(conn: &Connection, warning_type: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM dismissed_system_warnings WHERE warning_type = ?1",
        params![warning_type],
    )?;
    Ok(())
}

/// Loads every dismissal at startup, before any warning can be raised.
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

        record_dismissal(&conn, "low_ram", "Low RAM: 1 GB free").unwrap();
        assert_eq!(
            load_all(&conn).unwrap().get("low_ram"),
            Some(&message_hash("Low RAM: 1 GB free"))
        );

        clear_dismissal(&conn, "low_ram").unwrap();
        assert!(load_all(&conn).unwrap().is_empty());
    }
}
