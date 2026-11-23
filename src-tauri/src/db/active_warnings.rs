//! Persisted active system warnings, reloaded at startup.
use anyhow::Result;
use rusqlite::{params, Connection};

pub fn upsert(conn: &Connection, warning_type: &str, payload_json: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO active_system_warnings (warning_type, payload_json) VALUES (?1, ?2)
         ON CONFLICT(warning_type) DO UPDATE SET payload_json = excluded.payload_json,
             updated_at = datetime('now')",
        params![warning_type, payload_json],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, warning_type: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM active_system_warnings WHERE warning_type = ?1",
        params![warning_type],
    )?;
    Ok(())
}

pub fn load_all(conn: &Connection) -> Result<Vec<String>> {
    let rows = conn
        .prepare("SELECT payload_json FROM active_system_warnings")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(rows)
}
