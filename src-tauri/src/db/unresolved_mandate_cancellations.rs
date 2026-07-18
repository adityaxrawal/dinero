use anyhow::Result;
use rusqlite::{params, Connection};

/// Mirrors unprocessed_statements' blocking-on-user-input shape (Doc 18
/// §4.16-4.21): a mandate cancellation email that couldn't be matched to
/// exactly one active recurring_payments row (zero or multiple candidates)
/// is never guessed at -- logged here instead
/// (docs/superpowers/specs/2026-07-18-mandate-tracking-design.md §5).
pub fn insert_unresolved(conn: &Connection, raw_signal: &str, candidate_ids: &[String]) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let candidate_ids_json = serde_json::to_string(candidate_ids)?;
    conn.execute(
        "INSERT INTO unresolved_mandate_cancellations (id, raw_signal, candidate_ids) VALUES (?1, ?2, ?3)",
        params![id, raw_signal, candidate_ids_json],
    )?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        crate::db::test_helpers::setup_test_db()
    }

    #[test]
    fn test_insert_unresolved_with_zero_candidates() {
        let conn = setup();
        let id = insert_unresolved(&conn, r#"{"merchant":"ScribdInc"}"#, &[]).unwrap();
        let raw: String = conn
            .query_row(
                "SELECT raw_signal FROM unresolved_mandate_cancellations WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw, r#"{"merchant":"ScribdInc"}"#);
    }

    #[test]
    fn test_insert_unresolved_with_multiple_candidates() {
        let conn = setup();
        let id = insert_unresolved(&conn, "{}", &["id-1".to_string(), "id-2".to_string()]).unwrap();
        let candidates_json: String = conn
            .query_row(
                "SELECT candidate_ids FROM unresolved_mandate_cancellations WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        let candidates: Vec<String> = serde_json::from_str(&candidates_json).unwrap();
        assert_eq!(candidates, vec!["id-1", "id-2"]);
    }
}
