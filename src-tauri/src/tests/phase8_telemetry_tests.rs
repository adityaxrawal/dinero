use crate::commands::data::do_get_debug_metrics;
use crate::commands::debug::{
    debug_get_pipeline_state, debug_set_gmail_poll_paused, debug_set_scan_queue_paused,
};
use crate::db;
use crate::db::audit_log::{insert as insert_audit_log, AuditLogRow};
use deadpool_sqlite::Pool;
use uuid::Uuid;

async fn setup_test_db() -> Pool {
    let temp_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&temp_dir).unwrap();
    let db_path = temp_dir.join("test.db");
    crate::db::init_db(db_path.clone())
        .await
        .expect("init test db")
}

#[tokio::test]
async fn test_debug_pipeline_pause_resume() {
    let initial_state = debug_get_pipeline_state().await.unwrap();

    debug_set_gmail_poll_paused(true).await.unwrap();
    let state = debug_get_pipeline_state().await.unwrap();
    assert!(state.gmail_poll_paused);

    debug_set_scan_queue_paused(true).await.unwrap();
    let state2 = debug_get_pipeline_state().await.unwrap();
    assert!(state2.scan_queue_paused);

    debug_set_gmail_poll_paused(initial_state.gmail_poll_paused)
        .await
        .unwrap();
    debug_set_scan_queue_paused(initial_state.scan_queue_paused)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_debug_fetch_parse_errors() {
    let pool = setup_test_db().await;
    let conn = pool.get().await.unwrap();

    conn.interact(|c| {
        c.execute("INSERT INTO transaction_observations (id, source_pipeline, extraction_method, raw_payload_json) VALUES ('err1', 'gmail_transaction', 'failed', '{\"err\": true}')", []).unwrap();
        c.execute("INSERT INTO transaction_observations (id, source_pipeline, extraction_method, raw_payload_json) VALUES ('err2', 'gmail_transaction', 'llm', '{\"err\": false}')", []).unwrap();
    }).await.unwrap();

    let errors = conn.interact(|c| {
        let mut stmt = c.prepare("SELECT * FROM transaction_observations WHERE extraction_method = 'failed' ORDER BY created_at DESC").unwrap();
        let iter = stmt.query_map([], db::transaction_observations::row_to_observation).unwrap();
        let mut res = Vec::new();
        for r in iter { res.push(r.unwrap()); }
        res
    }).await.unwrap();

    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].id, "err1");
}

#[tokio::test]
async fn test_debug_fetch_unprocessed_statements() {
    let pool = setup_test_db().await;
    let conn = pool.get().await.unwrap();

    conn.interact(|c| {
        c.execute("INSERT INTO unprocessed_statements (id, statement_source_json, status, failure_type, failure_reason) VALUES ('stmt1', '{}', 'pending_retry', 'password_required', 'password_required')", []).unwrap();
    }).await.unwrap();

    let stmts = conn
        .interact(|c| db::unprocessed_statements::select_pending(c))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stmts.len(), 1);
    assert_eq!(stmts[0].id, "stmt1");
    assert_eq!(stmts[0].failure_reason, "password_required");
}

#[tokio::test]
async fn test_debug_fetch_audit_log() {
    let pool = setup_test_db().await;
    let conn = pool.get().await.unwrap();

    conn.interact(|c| {
        let row1 = AuditLogRow {
            id: "a1".to_string(),
            actor_type: Some("system".to_string()),
            actor_id: Some("sys".to_string()),
            action: Some("create".to_string()),
            resource_type: Some("rule".to_string()),
            resource_id: Some("rule1".to_string()),
            before_json: None,
            after_json: None,
            created_at: chrono::Utc::now(),
        };
        insert_audit_log(c, &row1).unwrap();

        let row2 = AuditLogRow {
            id: "a2".to_string(),
            actor_type: Some("system".to_string()),
            actor_id: Some("sys".to_string()),
            action: Some("delete".to_string()),
            resource_type: Some("cluster".to_string()),
            resource_id: Some("c1".to_string()),
            before_json: None,
            after_json: None,
            created_at: chrono::Utc::now(),
        };
        insert_audit_log(c, &row2).unwrap();
    })
    .await
    .unwrap();

    let logs_all = conn
        .interact(|c| db::audit_log::fetch_all(c, None, 10, 0))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(logs_all.len(), 2);

    let logs_filtered = conn
        .interact(|c| db::audit_log::fetch_all(c, Some("rule".to_string()), 10, 0))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(logs_filtered.len(), 1);
    assert_eq!(logs_filtered[0].id, "a1");
}

#[tokio::test]
async fn test_performance_metrics_tracking() {
    let pool = setup_test_db().await;
    let conn = pool.get().await.unwrap();

    conn.interact(|c| {
        c.execute("INSERT INTO transaction_observations (id, extraction_method) VALUES ('obs1', 'llm')", []).unwrap();
        c.execute("INSERT INTO transaction_observations (id, extraction_method) VALUES ('obs2', 'bank_templates')", []).unwrap();
        c.execute("INSERT INTO transaction_observations (id, extraction_method) VALUES ('obs3', 'bank_templates')", []).unwrap();
    }).await.unwrap();

    conn.interact(|c| {
        c.execute(
            "INSERT INTO match_decisions (id, decision) VALUES ('md1', 'auto_matched_exact')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO match_decisions (id, decision) VALUES ('md2', 'auto_matched_exact')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO match_decisions (id, decision) VALUES ('md3', 'ambiguous_pending')",
            [],
        )
        .unwrap();
    })
    .await
    .unwrap();

    let metrics = conn
        .interact(|c| do_get_debug_metrics(c))
        .await
        .unwrap()
        .unwrap();

    assert!((metrics.llm_fallback_rate - 0.333).abs() < 0.01);

    assert_eq!(metrics.extraction_layer_distribution.get("llm"), Some(&1));
    assert_eq!(
        metrics.extraction_layer_distribution.get("bank_templates"),
        Some(&2)
    );

    assert_eq!(
        metrics
            .reconciliation_decision_distribution
            .get("auto_matched_exact"),
        Some(&2)
    );
    assert_eq!(
        metrics
            .reconciliation_decision_distribution
            .get("ambiguous_pending"),
        Some(&1)
    );
}
