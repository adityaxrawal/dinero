mod llm_manager_tests;
mod phase10_quality_gates_tests;
mod phase10_rigorous_tests;
mod phase11_llm_tests;
mod phase11_rigorous_tests;
mod phase8_telemetry_tests;
mod phase9_rigorous_tests;
mod phase9_security_tests;

/// Review L2: background loops wake and stop as soon as shutdown is requested.
#[tokio::test]
async fn sleep_or_cancelled_returns_early_on_shutdown() {
    let token = tokio_util::sync::CancellationToken::new();
    assert!(!crate::sleep_or_cancelled(&token, std::time::Duration::from_millis(10)).await);
    let t = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        t.cancel();
    });
    let start = std::time::Instant::now();
    assert!(crate::sleep_or_cancelled(&token, std::time::Duration::from_secs(60)).await);
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
}
