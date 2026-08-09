use crate::ingestion::gmail_client::GmailClient;
use base64::Engine as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

async fn client_with_temp_db(base_url: String) -> GmailClient {
    let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let pool = crate::db::init_db(temp_dir.join("test.db")).await.unwrap();
    GmailClient::new_with_base_url("fake_token".into(), pool, base_url, None)
}

/// TASK-GMAIL: a successful HTTP status doesn't guarantee a clean body --
/// Google can serve a truncated/malformed payload behind a 200. Previously
/// `fetch_message` parsed the body once with no retry, so this bailed
/// immediately as "Failed to parse Message JSON" (25 occurrences in the
/// field error log) even though the very next attempt would have worked.
/// `execute_with_retry_and_parse` must retry the whole request when the
/// body fails to parse, not just when the transport call itself fails.
#[tokio::test]
async fn test_fetch_message_retries_on_malformed_json_body() {
    let mut server = mockito::Server::new_async().await;

    let bad_mock = server
        .mock("GET", "/gmail/v1/users/me/messages/msg1")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{not valid json")
        .expect(1)
        .create_async()
        .await;

    let good_mock = server
        .mock("GET", "/gmail/v1/users/me/messages/msg1")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "id": "msg1",
                "threadId": "thread1",
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let client = client_with_temp_db(server.url()).await;
    let message = client
        .fetch_message(
            "msg1",
            crate::ingestion::gmail_client::FetchFormat::Metadata,
        )
        .await
        .expect("fetch_message should succeed after retrying the malformed body");

    assert_eq!(message.id, "msg1");
    bad_mock.assert_async().await;
    good_mock.assert_async().await;
}

/// Regression test for the "0 / 0" progress-freeze bug: `search_messages`'s
/// pagination loop previously gave the caller no signal at all until every
/// page had been fetched. `on_page` must fire once per page with the
/// running total found so far, so a caller (the historical scan) can show
/// the user real, moving numbers during what can otherwise be a long,
/// silent phase.
#[tokio::test]
async fn test_search_messages_invokes_on_page_callback_and_paginates() {
    let mut server = mockito::Server::new_async().await;

    // Page 1: the real request has exactly one query param ("q=..."), no
    // "&" at all -- distinguishes it from page 2's request without needing
    // regex lookaround (the `regex` crate this codebase uses throughout
    // doesn't support it).
    let _mock_page1 = server
        .mock("GET", "/gmail/v1/users/me/messages")
        .match_query(mockito::Matcher::Regex(r"^q=[^&]+$".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "messages": [{"id": "m1"}, {"id": "m2"}],
                "nextPageToken": "page2_tok"
            })
            .to_string(),
        )
        .create_async()
        .await;

    let _mock_page2 = server
        .mock("GET", "/gmail/v1/users/me/messages")
        .match_query(mockito::Matcher::UrlEncoded(
            "pageToken".into(),
            "page2_tok".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::json!({ "messages": [{"id": "m3"}] }).to_string())
        .create_async()
        .await;

    let client = client_with_temp_db(server.url()).await;

    let seen_counts = Arc::new(Mutex::new(Vec::<usize>::new()));
    let seen_counts_clone = seen_counts.clone();
    let ids = client
        .search_messages("after:2020/01/01", move |count| {
            seen_counts_clone.lock().unwrap().push(count);
        })
        .await
        .unwrap();

    assert_eq!(
        ids,
        vec!["m1".to_string(), "m2".to_string(), "m3".to_string()]
    );
    assert_eq!(
        *seen_counts.lock().unwrap(),
        vec![2, 3],
        "on_page must fire once per page with the running total found so far"
    );
}

/// Regression test: an unbounded date range (e.g. a decade-wide scan) on a
/// mailbox that keeps returning a `nextPageToken` must not paginate
/// forever -- `MAX_SEARCH_PAGES` bounds it, so the caller still gets a
/// result back in finite time instead of the search phase (and the UI's
/// progress counters) hanging indefinitely.
#[tokio::test]
async fn test_search_messages_caps_at_max_search_pages() {
    let mut server = mockito::Server::new_async().await;

    // Every page returns one message and always another `nextPageToken` --
    // simulates a pathologically wide range that would otherwise paginate
    // forever.
    let _mock = server
        .mock("GET", "/gmail/v1/users/me/messages")
        // Without an explicit `match_query`, mockito requires the *whole*
        // path+query to exactly equal the bare path -- every real request
        // here always has a `?q=...` query string, so it would never match
        // at all (observed as every request falling through to mockito's
        // default 501 response). `Matcher::Any` switches the matcher into
        // path/query-split mode and accepts any query.
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({ "messages": [{"id": "m"}], "nextPageToken": "always_more" })
                .to_string(),
        )
        .create_async()
        .await;

    let client = client_with_temp_db(server.url()).await;

    let page_count = Arc::new(AtomicUsize::new(0));
    let page_count_clone = page_count.clone();
    let ids = client
        .search_messages("after:2016/01/01", move |_| {
            page_count_clone.fetch_add(1, Ordering::SeqCst);
        })
        .await
        .unwrap();

    assert_eq!(
        ids.len(),
        500,
        "must stop at MAX_SEARCH_PAGES, not paginate forever"
    );
    assert_eq!(page_count.load(Ordering::SeqCst), 500);
}

/// Doc 2026-07-26 mail scan performance: proves the limiter itself blocks
/// correctly under real elapsed time (a small, deliberately real sleep
/// budget — not `start_paused`, since this test also drives a real mockito
/// socket in the same file, and paused virtual time breaks real TCP
/// connections).
#[tokio::test]
async fn quota_limiter_paces_a_drained_bucket_before_the_request_fires() {
    let limiter = crate::ingestion::gmail_client::QuotaLimiter::new_for_test(2.0); // 2 units/sec, capacity 2
    limiter.acquire(2.0).await; // drain the starting bucket
    let start = std::time::Instant::now();
    limiter.acquire(1.0).await; // needs 0.5s of real refill at 2/sec
    assert!(start.elapsed() >= Duration::from_millis(400));
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
        .mock("GET", "/gmail/v1/users/me/messages/msg1/attachments/att1")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::json!({ "data": encoded }).to_string())
        .create_async()
        .await;

    let temp_dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let db_path = temp_dir.join("test.db");
    let pool = crate::db::init_db(db_path.clone()).await.unwrap();

    let client = GmailClient::new_with_base_url("fake_token".into(), pool, server.url(), None);
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
                && contents
                    .windows(pdf_bytes.len())
                    .any(|w| w == pdf_bytes.as_slice());
            assert!(!leaked, "PDF bytes leaked into file on disk: {:?}", path);
        }
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}
