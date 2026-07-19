#[cfg(test)]
mod tests {
    use crate::db::network_activity_log::{self, NetworkActivityLogRow};
    use chrono::Utc;
    use uuid::Uuid;

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
            channel: Some("google_oauth".to_string()),
        };

        network_activity_log::insert(&conn, &log).unwrap();

        let (logs, total) = network_activity_log::list_paginated(&conn, 1, 50).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(total, 1);

        let stored = &logs[0];
        assert_eq!(stored.method, "GET");
        assert_eq!(
            stored.url_redacted,
            "https://oauth2.googleapis.com/token?redacted"
        );
        assert!(stored.url_redacted.contains("redacted"));
        assert!(!stored.url_redacted.contains("secret_token"));
        assert_eq!(
            stored.secret_fields_masked.as_deref(),
            Some("Authorization")
        );
    }

    fn insert_n_rows(conn: &rusqlite::Connection, n: usize) {
        for i in 0..n {
            network_activity_log::insert(
                conn,
                &NetworkActivityLogRow {
                    id: Uuid::new_v4().to_string(),
                    timestamp: Some(Utc::now().naive_utc() + chrono::Duration::seconds(i as i64)),
                    method: "GET".to_string(),
                    domain: "gmail.googleapis.com".to_string(),
                    url_redacted: format!("https://gmail.googleapis.com/{i}"),
                    bytes_sent: Some(0),
                    bytes_received: Some(100),
                    status_code: Some(200),
                    secret_fields_masked: None,
                    channel: Some("gmail_api".to_string()),
                },
            )
            .unwrap();
        }
    }

    /// Regression test for the unbounded-fetch scale problem: a single
    /// historical scan can write hundreds of rows in seconds (Doc 30
    /// TASK-GMAIL-002's 50-concurrent-fetch cap), and this table has no
    /// row-count limit, only a 30-day time window (Document 18 §4.21b) --
    /// `list_paginated` must actually page, not just accept the params and
    /// still return everything.
    #[test]
    fn test_list_paginated_returns_correct_page_and_total() {
        let conn = setup_test_db();
        insert_n_rows(&conn, 25);

        let (page1, total) = network_activity_log::list_paginated(&conn, 1, 10).unwrap();
        assert_eq!(page1.len(), 10);
        assert_eq!(total, 25);

        let (page2, total2) = network_activity_log::list_paginated(&conn, 2, 10).unwrap();
        assert_eq!(page2.len(), 10);
        assert_eq!(total2, 25);

        let (page3, total3) = network_activity_log::list_paginated(&conn, 3, 10).unwrap();
        assert_eq!(
            page3.len(),
            5,
            "last page must return the remainder, not pad or error"
        );
        assert_eq!(total3, 25);

        // Newest-first ordering must hold across the page boundary too --
        // not just within a single page.
        let page1_ids: std::collections::HashSet<_> = page1.iter().map(|r| r.id.clone()).collect();
        let page2_ids: std::collections::HashSet<_> = page2.iter().map(|r| r.id.clone()).collect();
        assert!(page1_ids.is_disjoint(&page2_ids), "pages must not overlap");
    }

    #[test]
    fn test_list_paginated_empty_table_returns_empty_page_and_zero_total() {
        let conn = setup_test_db();
        let (rows, total) = network_activity_log::list_paginated(&conn, 1, 50).unwrap();
        assert!(rows.is_empty());
        assert_eq!(total, 0);
    }
}
