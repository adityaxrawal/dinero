use super::*;

/// Doc 30 TASK-STMT-003: proves the isolation/timeout mechanism itself —
/// that a call really does round-trip through a separate OS process (not an
/// in-process stub), and that a hung child is killed rather than blocking
/// the caller for the full hang duration. Uses `/bin/cat` and `/bin/sleep`
/// rather than the real `pdf_sidecar` binary or a real PDF, since this is a
/// property of the spawn/timeout primitive, not of pdfium specifically.
#[tokio::test]
async fn test_sidecar_process_isolated_from_main() {
    // `cat` echoes stdin back on stdout verbatim — a real process boundary,
    // not a function call: if this worked in-process, there would be no
    // child to spawn at all.
    let result = run_with_timeout(
        PathBuf::from("/bin/cat"),
        &[],
        b"{\"marker\":\"hello-sidecar\"}",
        b"payload-bytes",
        Duration::from_secs(5),
    )
    .await
    .expect("cat must echo stdin back successfully");

    assert!(
        result
            .windows(b"hello-sidecar".len())
            .any(|w| w == b"hello-sidecar"),
        "round-tripped bytes must contain what was written to stdin"
    );

    // A hung child (`sleep 5`) with a short timeout must be killed quickly —
    // proving the 30s-per-page enforcement mechanism actually bounds wall
    // time rather than merely being a documented intention.
    let start = std::time::Instant::now();
    let timeout_result = run_with_timeout(
        PathBuf::from("/bin/sleep"),
        &["5"],
        b"{}",
        b"x",
        Duration::from_millis(200),
    )
    .await;

    assert!(
        timeout_result.is_err(),
        "a hung child must be reported as a timeout error, not silently succeed"
    );
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "must be killed near the 200ms timeout, not wait out the 5s hang: took {:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn test_run_with_timeout_reports_spawn_failure_for_missing_binary() {
    let result = run_with_timeout(
        PathBuf::from("/definitely/not/a/real/binary/path"),
        &[],
        b"{}",
        b"x",
        Duration::from_secs(5),
    )
    .await;
    assert!(result.is_err());
}
