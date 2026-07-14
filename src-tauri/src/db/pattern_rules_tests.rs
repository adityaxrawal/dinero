#[cfg(test)]
mod tests {
    use crate::db::pattern_rules::{self, PatternRulesRow};
    use chrono::Utc;
    use rusqlite::Connection;
    use serde_json::json;

    fn setup_db() -> Connection {
        crate::db::test_helpers::setup_test_db()
    }

    #[test]
    fn test_pattern_rules_state_transitions() {
        let conn = setup_db();

        let mut row = PatternRulesRow {
            id: "pr-1".to_string(),
            bank_name: "Chase".to_string(),
            template_hash: "hash123".to_string(),
            field_name: "amount".to_string(),
            rule_payload_json: json!({"regex": "\\$([0-9.]+)"}),
            status: "pending".to_string(),
            success_count: 0,
            failure_count: 0,
            confidence: 0.1,
            created_at: Some(Utc::now().naive_utc()),
            updated_at: Some(Utc::now().naive_utc()),
        };

        pattern_rules::insert(&conn, &row).unwrap();

        // pending -> active
        assert!(pattern_rules::update_status(&conn, "pr-1", "active").is_ok());

        // active -> trusted
        assert!(pattern_rules::update_status(&conn, "pr-1", "trusted").is_ok());

        // trusted -> inactive
        assert!(pattern_rules::update_status(&conn, "pr-1", "inactive").is_ok());

        // Invalid transitions
        row.id = "pr-2".to_string();
        row.status = "pending".to_string();
        pattern_rules::insert(&conn, &row).unwrap();

        // pending -> trusted (invalid)
        assert!(pattern_rules::update_status(&conn, "pr-2", "trusted").is_err());

        row.id = "pr-3".to_string();
        row.status = "active".to_string();
        pattern_rules::insert(&conn, &row).unwrap();

        // active -> pending (invalid)
        assert!(pattern_rules::update_status(&conn, "pr-3", "pending").is_err());
    }
}
