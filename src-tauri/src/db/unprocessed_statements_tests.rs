use crate::db::unprocessed_statements::{
    insert_unprocessed_statement, select_pending, update_status, UnprocessedStatementRow,
};
use rusqlite::Connection;

fn setup_db() -> Connection {
    crate::db::test_helpers::setup_test_db()
}

#[test]
fn test_unprocessed_statements_lifecycle() {
    let conn = setup_db();

    let stmt1 = UnprocessedStatementRow {
        id: "us_1".to_string(),
        statement_source_json: r#"{"source": "gmail"}"#.to_string(),
        failure_type: "password_protected".to_string(),
        failure_reason: "PDF requires a password".to_string(),
        status: "awaiting_password".to_string(),
        resolved_statement_id: None,
        created_at: None,
        updated_at: None,
    };

    let stmt2 = UnprocessedStatementRow {
        id: "us_2".to_string(),
        statement_source_json: r#"{"source": "gmail"}"#.to_string(),
        failure_type: "corrupt_pdf".to_string(),
        failure_reason: "File is truncated".to_string(),
        status: "failed".to_string(),
        resolved_statement_id: None,
        created_at: None,
        updated_at: None,
    };

    insert_unprocessed_statement(&conn, &stmt1).unwrap();
    insert_unprocessed_statement(&conn, &stmt2).unwrap();

    let pending = select_pending(&conn).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "us_1");

    use crate::db::statements::{insert, StatementsRow};
    use chrono::Utc;

    let stmt = StatementsRow {
        id: "stmt_123".to_string(),
        instrument_id: None,
        statement_type: "pdf".to_string(),
        billing_period_start: Utc::now().naive_utc().date(),
        billing_period_end: Utc::now().naive_utc().date(),
        due_date: None,
        statement_date: None,
        current_balance: None,
        minimum_due: None,
        rewards_summary_json: None,
        source_message_id: None,
        parse_status: "parsed".to_string(),
        is_duplicate: false,
        created_at: None,
        updated_at: None,
    };
    insert(&conn, &stmt).unwrap();

    // Update status to resolved
    update_status(&conn, "us_1", "resolved", Some("stmt_123")).unwrap();

    let pending_after = select_pending(&conn).unwrap();
    assert_eq!(pending_after.len(), 0);
}
