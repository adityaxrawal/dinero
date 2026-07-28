use crate::db::scan_failed_messages::{insert, select_by_account, ScanFailedMessageRow};

fn setup_db() -> rusqlite::Connection {
    crate::db::test_helpers::setup_test_db()
}

#[test]
fn insert_and_select_by_account_round_trips() {
    let conn = setup_db();

    let row_a = ScanFailedMessageRow {
        id: "sfm_1".to_string(),
        account_id: "acc_1".to_string(),
        msg_id: "msg_a".to_string(),
        error: "Failed to send fetch_message request".to_string(),
        failed_at: None,
    };
    let row_b = ScanFailedMessageRow {
        id: "sfm_2".to_string(),
        account_id: "acc_1".to_string(),
        msg_id: "msg_b".to_string(),
        error: "401 Unauthorized".to_string(),
        failed_at: None,
    };
    let row_other_account = ScanFailedMessageRow {
        id: "sfm_3".to_string(),
        account_id: "acc_2".to_string(),
        msg_id: "msg_c".to_string(),
        error: "timeout".to_string(),
        failed_at: None,
    };

    insert(&conn, &row_a).unwrap();
    insert(&conn, &row_b).unwrap();
    insert(&conn, &row_other_account).unwrap();

    let acc_1_failures = select_by_account(&conn, "acc_1").unwrap();
    assert_eq!(acc_1_failures.len(), 2);
    let mut msg_ids: Vec<String> = acc_1_failures.iter().map(|r| r.msg_id.clone()).collect();
    msg_ids.sort();
    assert_eq!(msg_ids, vec!["msg_a".to_string(), "msg_b".to_string()]);

    let acc_2_failures = select_by_account(&conn, "acc_2").unwrap();
    assert_eq!(acc_2_failures.len(), 1);
    assert_eq!(acc_2_failures[0].msg_id, "msg_c");
}
