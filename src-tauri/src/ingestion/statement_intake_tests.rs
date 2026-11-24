use super::*;
use tauri::test::{mock_builder, mock_context};

/// Audit A-4: an attachment that cannot be fetched is recorded for retry and
/// leaves no `statements` row (which would mark the email processed).
#[tokio::test]
async fn failed_attachment_fetch_is_recorded_for_retry() {
    let app = mock_builder().build(mock_context(tauri::test::noop_assets())).unwrap();
    let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let pool = crate::db::init_db(dir.join("t.db")).await.unwrap();

    let mut server = mockito::Server::new_async().await;
    let _fetch = server
        .mock("GET", mockito::Matcher::Regex("^/gmail/v1/users/me/messages/msg_1/attachments/".into()))
        .match_query(mockito::Matcher::Any)
        .with_status(404)
        .create_async()
        .await;
    let client = GmailClient::new_with_base_url("fake_token".into(), pool.clone(), server.url(), None);
    let attachments = vec![PdfAttachmentMeta {
        attachment_id: Some("att_1".into()),
        filename: "statement.pdf".into(),
        mime_type: "application/pdf".into(),
        size: None,
        inline_bytes: None,
    }];

    intake_email_statement(app.handle(), &pool, &client, "acc", "msg_1", &attachments, None).await;

    let (failed, statements): (Vec<String>, i64) = pool
        .get()
        .await
        .unwrap()
        .interact(|c| {
            let failed = crate::db::scan_failed_messages::distinct_msg_ids(c, "acc").unwrap();
            let statements = c.query_row("SELECT COUNT(*) FROM statements", [], |r| r.get(0))?;
            Ok::<_, rusqlite::Error>((failed, statements))
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(failed, vec!["msg_1".to_string()]);
    assert_eq!(statements, 0);
    let _ = std::fs::remove_dir_all(&dir);
}
