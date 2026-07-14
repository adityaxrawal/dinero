use crate::ingestion::gmail_client::full_fetch_semaphore;
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
