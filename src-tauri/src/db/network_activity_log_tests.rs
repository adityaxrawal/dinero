#[cfg(test)]
mod tests {
    use crate::db::network_activity_log::{self, NetworkActivityLogRow};
    use uuid::Uuid;
    use chrono::Utc;

    fn setup_test_db() -> rusqlite::Connection {
        crate::db::test_helpers::setup_test_db()
    }

    #[test]
    fn test_network_activity_log_sanitization() {
        let conn = setup_test_db();

        let log = NetworkActivityLogRow {
            id: Uuid::new_v4().to_string(),
            timestamp: Some(Utc::now().naive_utc()),
            method: "GET".to_string(),
            domain: "oauth2.googleapis.com".to_string(),
            url_redacted: "https://oauth2.googleapis.com/token?redacted".to_string(),
            bytes_sent: Some(100),
            bytes_received: Some(200),
            status_code: Some(200),
            secret_fields_masked: Some("Authorization".to_string()),
        };

        network_activity_log::insert(&conn, &log).unwrap();

        let logs = network_activity_log::list_all(&conn).unwrap();
        assert_eq!(logs.len(), 1);
        
        let stored = &logs[0];
        assert_eq!(stored.method, "GET");
        assert_eq!(stored.url_redacted, "https://oauth2.googleapis.com/token?redacted");
        assert!(stored.url_redacted.contains("redacted"));
        assert!(!stored.url_redacted.contains("secret_token"));
        assert_eq!(stored.secret_fields_masked.as_deref(), Some("Authorization"));
    }
}
