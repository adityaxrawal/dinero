use crate::ingestion::gmail_client::{full_fetch_semaphore, GmailClient};
use base64::Engine as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Doc 30 TASK-GMAIL-002: "Respect Gmail's 250 quota-units/second limit via a
/// Tokio semaphore (max 50 concurrent full-message fetches/second)." Proves
/// the shared semaphore behind `fetch_message(.., FetchFormat::Full)` never
/// lets more than 50 holders in at once, and that the cap is genuinely near
/// 50 rather than accidentally much smaller.
#[tokio::test]
async fn test_quota_throttling_caps_concurrent_fetches() {
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_seen = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();
    for _ in 0..60 {
        let in_flight = in_flight.clone();
        let max_seen = max_seen.clone();
        handles.push(tokio::spawn(async move {
            let _permit = full_fetch_semaphore().acquire().await.unwrap();
            let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            max_seen.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
            in_flight.fetch_sub(1, Ordering::SeqCst);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let observed_max = max_seen.load(Ordering::SeqCst);
    assert!(
        observed_max <= 50,
        "never more than 50 concurrent full-message fetches, saw {}",
        observed_max
    );
    assert!(
        observed_max > 40,
        "cap should be genuinely near 50, not accidentally much smaller: saw {}",
        observed_max
    );
}

/// Doc 30 TASK-GMAIL-008: "PDF bytes are never written to disk or SQLite at
/// any point in this handoff." Mocks the attachment-fetch HTTP call, fetches
/// real bytes through `fetch_attachment`, then scans every file `init_db`'s
/// own side effects leave behind (backup file, hw-uuid marker, etc. — none
/// of this test's concern) for the exact PDF byte sequence, rather than
/// asserting on filenames (which would be fragile to unrelated DB-init
/// implementation details).
#[tokio::test]
async fn test_pdf_bytes_never_touch_filesystem() {
    let mut server = mockito::Server::new_async().await;
    let pdf_bytes = b"%PDF-1.4 fake pdf content for test\x00\x01\x02".to_vec();
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&pdf_bytes);

    let mock = server
        .mock(
            "GET",
            "/gmail/v1/users/me/messages/msg1/attachments/att1",
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::json!({ "data": encoded }).to_string())
        .create_async()
        .await;

    let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let db_path = temp_dir.join("test.db");
    let pool = crate::db::init_db(db_path.clone()).await.unwrap();

    let client = GmailClient::new_with_base_url("fake_token".into(), pool, server.url());
    let fetched = client.fetch_attachment("msg1", "att1").await.unwrap();
    assert_eq!(fetched, pdf_bytes);
    mock.assert_async().await;

    for entry in std::fs::read_dir(&temp_dir).unwrap().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Ok(contents) = std::fs::read(&path) {
            let leaked = pdf_bytes.len() <= contents.len()
                && contents.windows(pdf_bytes.len()).any(|w| w == pdf_bytes.as_slice());
            assert!(
                !leaked,
                "PDF bytes leaked into file on disk: {:?}",
                path
            );
        }
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}
