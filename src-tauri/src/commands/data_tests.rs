#[cfg(test)]
mod tests {
    use crate::commands::data::do_get_debug_metrics;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        crate::db::test_helpers::setup_test_db()
    }

    #[test]
    fn test_do_get_debug_metrics_empty() {
        let conn = setup_test_db();
        let metrics = do_get_debug_metrics(&conn).unwrap();
        
        assert_eq!(metrics.total_transactions, 0);
        assert_eq!(metrics.total_statements, 0);
        assert_eq!(metrics.unresolved_clusters, 0);
        assert_eq!(metrics.llm_fallback_rate, 0.0);
        assert_eq!(metrics.queue_depth, 0);
        assert!(metrics.extraction_layer_distribution.is_empty());
        assert!(metrics.reconciliation_decision_distribution.is_empty());
    }

    #[test]
    fn test_do_get_debug_metrics_populated() {
        let conn = setup_test_db();
        
        // Insert dummy observations to test extraction_layer_distribution
        conn.execute(
            "INSERT INTO transaction_observations (id, source_pipeline, source_record_id, extraction_method) 
             VALUES ('obs1', 'gmail_transaction', 'rec1', 'llm')",
            []
        ).unwrap();
        conn.execute(
            "INSERT INTO transaction_observations (id, source_pipeline, source_record_id, extraction_method) 
             VALUES ('obs2', 'gmail_transaction', 'rec2', 'llm')",
            []
        ).unwrap();
        conn.execute(
            "INSERT INTO transaction_observations (id, source_pipeline, source_record_id, extraction_method) 
             VALUES ('obs3', 'gmail_transaction', 'rec3', 'regex')",
            []
        ).unwrap();

        // Insert dummy match decisions to test reconciliation_decision_distribution
        conn.execute(
            "INSERT INTO match_decisions (id, decision) VALUES ('dec1', 'auto_matched_exact')",
            []
        ).unwrap();
        conn.execute(
            "INSERT INTO match_decisions (id, decision) VALUES ('dec2', 'auto_matched_exact')",
            []
        ).unwrap();
        conn.execute(
            "INSERT INTO match_decisions (id, decision) VALUES ('dec3', 'ambiguous_pending')",
            []
        ).unwrap();

        let metrics = do_get_debug_metrics(&conn).unwrap();
        
        // llm_fallback_rate should be 2/3 = 0.666...
        assert!((metrics.llm_fallback_rate - 0.6666666666666666).abs() < f64::EPSILON);
        
        // check extraction_layer_distribution
        assert_eq!(*metrics.extraction_layer_distribution.get("llm").unwrap(), 2);
        assert_eq!(*metrics.extraction_layer_distribution.get("regex").unwrap(), 1);
        
        // check reconciliation_decision_distribution
        assert_eq!(*metrics.reconciliation_decision_distribution.get("auto_matched_exact").unwrap(), 2);
        assert_eq!(*metrics.reconciliation_decision_distribution.get("ambiguous_pending").unwrap(), 1);
    }
}
