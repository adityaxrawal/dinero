#[cfg(test)]
mod tests {
    use crate::db::feedback_log::{self, FeedbackLogRow};
    use chrono::Utc;
    use rusqlite::Connection;
    use serde_json::json;

    fn setup_db() -> Connection {
        crate::db::test_helpers::setup_test_db()
    }

    #[test]
    fn test_feedback_log_crud() {
        let conn = setup_db();

        // Setup an instrument and transaction first
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier) VALUES (?1, ?2, ?3, ?4)",
            ["inst-1", "credit_card", "Test Issuer", "1234"],
        ).unwrap();

        conn.execute(
            "INSERT INTO transactions (id, instrument_id, merchant_display_name, amount_minor, currency, best_event_time) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params!["tx-1", "inst-1", "Test Tx", 1000, "USD", "2023-01-01"],
        ).unwrap();

        let row = FeedbackLogRow {
            id: "fb-1".to_string(),
            transaction_id: "tx-1".to_string(),
            observation_id: None,
            source_pipeline: Some("statement_pdf".to_string()),
            field_name: "amount".to_string(),
            old_value: Some("100".to_string()),
            new_value: "1000".to_string(),
            source_context_json: json!({"page": 1, "box": [10, 20, 30, 40]}),
            created_at: Some(Utc::now().naive_utc()),
        };

        feedback_log::insert(&conn, &row).unwrap();

        let retrieved = feedback_log::select_by_transaction(&conn, "tx-1").unwrap();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].id, "fb-1");
        assert_eq!(
            retrieved[0].source_pipeline,
            Some("statement_pdf".to_string())
        );
        assert_eq!(retrieved[0].new_value, "1000");
    }
}
