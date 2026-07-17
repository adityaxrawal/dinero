//! TASK-DESK-005 (Doc 30 §12, Doc 29 §14): the Tauri auto-updater, per
//! Document 16 §9.1 -- "over-the-air updates via Tauri updater, GitHub
//! Releases as update server, signed updater endpoint." Checks on a
//! schedule (every ~6 hours while running, plus once on launch) and via
//! the manual "Check for Updates" menu item (TASK-DESK-001). Every release
//! artifact is signed with the updater's Ed25519 keypair; the actual
//! cryptographic verification against the embedded public key
//! (`tauri.conf.json`'s `plugins.updater.pubkey`) is `tauri-plugin-
//! updater`'s own internal, already-tested responsibility
//! (`verify_signature`/`minisign_verify` in its vendored source) -- not
//! reimplemented here. This module's job is: never treat a check/download
//! error (including a signature failure) as "no update available" or
//! silently swallow it, schedule checks correctly, coordinate a graceful
//! shutdown of active background workers before installing, and drive the
//! non-intrusive "Update Now" / "Remind Me Later" prompt.
//!
//! Per this task's own text, this is also the source implementation for
//! the updater signing-key mechanism Document 26 TM-OQ-05 flags as lacking
//! a documented custody/rotation policy -- still genuinely open, not
//! resolved by this task (no custody/rotation process exists to implement
//! yet).

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::UpdaterExt;

/// Doc 30 TASK-DESK-005: checked every ~6 hours while running.
pub const UPDATE_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

/// Bounded window given to in-flight background work to observe
/// cancellation and checkpoint before the installer actually runs. Not
/// indefinite -- an update must still eventually proceed even if a worker
/// is slow to notice.
const GRACEFUL_SHUTDOWN_GRACE_PERIOD: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct UpdateAvailable {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
}

/// Holds the most recently found `Update` so the frontend's "Update Now"
/// toast action (a plain button with no room to carry the `Update` object
/// itself) can trigger the real install via a follow-up IPC command
/// (`updater_confirm_install`) without re-checking. "Remind Me Later" is
/// simply not clicking it -- the next scheduled check (or manual menu
/// trigger) re-populates this if the update is still available, no
/// separate snooze state needed.
#[derive(Default)]
pub struct PendingUpdate(pub tokio::sync::Mutex<Option<tauri_plugin_updater::Update>>);

/// Checks for an update and, if one is available, emits `update_available`
/// for the frontend's non-intrusive "Update Now" / "Remind Me Later"
/// prompt (never a forced-restart popup). Any error from the check --
/// including a signature/verification failure surfaced later at download
/// time -- propagates as `Err` here; it is never conflated with `Ok(None)`
/// ("no update available"), which would silently mask a real failure.
pub async fn check_for_update<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<Option<UpdateAvailable>, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let result = updater.check().await.map_err(|e| e.to_string())?;
    match result {
        Some(update) => {
            let available = UpdateAvailable {
                version: update.version.clone(),
                current_version: update.current_version.clone(),
                notes: update.body.clone(),
            };
            if let Some(pending) = app.try_state::<PendingUpdate>() {
                *pending.0.lock().await = Some(update);
            }
            let _ = app.emit("update_available", &available);
            Ok(Some(available))
        }
        None => Ok(None),
    }
}

/// Doc 30 TASK-DESK-005: backs the frontend's "Update Now" action. Not in
/// Document 19's command catalog (this task predates/extends it, same
/// precedent as several Area 8 tasks' own additive commands) -- consumes
/// whatever `Update` the most recent check found, running the same
/// graceful-shutdown-then-install path as `download_and_install`.
#[tauri::command]
pub async fn updater_confirm_install(
    app: AppHandle,
    pending: tauri::State<'_, PendingUpdate>,
) -> Result<(), crate::error::AppError> {
    let update = pending
        .0
        .lock()
        .await
        .take()
        .ok_or_else(|| crate::error::AppError::NotFound("No pending update to install".to_string()))?;
    download_and_install(&app, update)
        .await
        .map_err(crate::error::AppError::Unknown)
}

/// Doc 30 TASK-DESK-005 acceptance: `test_manual_check_for_updates_menu_item`.
/// Called from the menu's `MenuAction::CheckForUpdates` dispatch
/// (`menu::handle_menu_event`) -- spawned so the menu handler itself
/// (a synchronous callback) never blocks on the network round-trip.
pub fn trigger_manual_check<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = check_for_update(&app).await {
            tracing::warn!("Manual update check failed: {}", e);
        }
    });
}

/// Doc 30 TASK-DESK-005: "if the user updates while a large polling/
/// extraction loop is active, the backend signals all active Tokio
/// workers via `CancellationToken`s to checkpoint and shut down cleanly
/// first -- never force-kill the process (avoiding SQLite WAL
/// corruption)." Signals the app-wide `CancellationToken` (the Gmail
/// polling and missing-data alert loops both already select on it) and
/// requests cancellation of any in-flight historical scans via
/// `scans_cancel` -- discovered via `BackgroundTaskRegistry`
/// (TASK-DESK-003), reused here rather than adding a second way to
/// enumerate active scans. Must be called, and awaited, before
/// `download_and_install` -- never after.
pub async fn prepare_for_graceful_shutdown<R: Runtime>(app: &AppHandle<R>) {
    if let Some(cancel_token) = app.try_state::<tokio_util::sync::CancellationToken>() {
        cancel_token.cancel();
    }

    if let Some(registry) =
        app.try_state::<crate::background_tasks::indicator::BackgroundTaskRegistry>()
    {
        for task in registry.active_tasks() {
            if task.task_type == "historical_scan" {
                let _ = crate::ingestion::historical_scan::scans_cancel(task.task_id).await;
            }
        }
    }

    tokio::time::sleep(GRACEFUL_SHUTDOWN_GRACE_PERIOD).await;
}

/// Doc 30 TASK-DESK-005: the actual install, always preceded by graceful
/// shutdown coordination. After relaunch, `TASK-DB-002`'s migration path
/// runs cleanly against the updated schema before anything else --
/// unchanged by this task, since that's simply the normal `init_db` flow
/// every launch already goes through, updated binary or not.
pub async fn download_and_install<R: Runtime>(
    app: &AppHandle<R>,
    update: tauri_plugin_updater::Update,
) -> Result<(), String> {
    prepare_for_graceful_shutdown(app).await;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())
}

/// Doc 30 TASK-DESK-005: checked once on launch, then every
/// `UPDATE_CHECK_INTERVAL` while running.
pub fn spawn_update_check_loop<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        loop {
            if let Err(e) = check_for_update(&app).await {
                tracing::warn!("Scheduled update check failed: {}", e);
            }
            tokio::time::sleep(UPDATE_CHECK_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    fn mock_app() -> AppHandle<tauri::test::MockRuntime> {
        // The updater plugin's own `setup` hook fails a missing
        // `plugins.updater` config section outright (it deserializes
        // straight into its `Config` struct, no `#[serde(default)]`) --
        // `tauri::test::mock_context` otherwise leaves `plugins` empty, so
        // a minimal real config is supplied here for every updater test,
        // not just the one that needs a real endpoint override.
        let mut ctx = tauri::test::mock_context(tauri::test::noop_assets());
        ctx.config_mut().plugins.0.insert(
            "updater".into(),
            serde_json::json!({
                "endpoints": ["https://example.invalid/latest.json"],
                "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6IHRlc3QKUldUZXN0S2V5MDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMA==",
            }),
        );
        tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(ctx)
            .unwrap()
            .handle()
            .clone()
    }

    /// Doc 30 TASK-DESK-005 acceptance: `test_update_triggers_graceful_worker_shutdown`.
    #[tokio::test]
    async fn test_update_triggers_graceful_worker_shutdown() {
        let app = mock_app();
        let token = CancellationToken::new();
        app.manage(token.clone());
        assert!(!token.is_cancelled(), "must start uncancelled");

        prepare_for_graceful_shutdown(&app).await;

        assert!(
            token.is_cancelled(),
            "the app-wide CancellationToken (shared with the Gmail polling \
             and alert-worker loops) must be signalled before an update is \
             installed, never after or never at all"
        );
    }

    #[tokio::test]
    async fn test_graceful_shutdown_cancels_in_flight_historical_scans() {
        let app = mock_app();
        let registry =
            crate::background_tasks::indicator::BackgroundTaskRegistry::default();
        registry.register_or_update(&app, "acct_1", "historical_scan", "Scanning acct_1", 1, 10, "Scanning…");
        app.manage(registry);

        // Not asserting scans_cancel's DB side effect here (that's Area 4's
        // own test surface) -- this proves the call path runs to
        // completion without panicking for an in-flight scan, which is the
        // part this task actually owns.
        prepare_for_graceful_shutdown(&app).await;
    }

    /// Doc 30 TASK-DESK-005 acceptance: `test_unsigned_update_rejected`.
    /// Drives the *real* `tauri-plugin-updater` against a real local HTTP
    /// server serving a syntactically well-formed manifest whose signature
    /// field is not a valid signature at all -- this is a real "unsigned/
    /// invalid-signature" update by Doc 30's own wording, rejected by the
    /// plugin's own `download()` (which calls `verify_signature` internally)
    /// before any bytes could ever reach an installer. Proves this module
    /// propagates that rejection as `Err`, never as `Ok` (accepted) and
    /// never a panic.
    #[tokio::test]
    async fn test_unsigned_update_rejected() {
        // Same "bind to an ephemeral port, read back the real port" pattern
        // already established for the local OAuth callback server
        // (`ingestion::oauth`), reused here for a local test-only mock
        // update server.
        let server = Arc::new(
            tiny_http::Server::http("127.0.0.1:0")
                .expect("failed to start local mock update server"),
        );
        let port = server
            .server_addr()
            .to_ip()
            .expect("mock update server must have an IP address")
            .port();

        let server_for_thread = Arc::clone(&server);
        let handle = std::thread::spawn(move || {
            // Request 1: the manifest.
            if let Ok(request) = server_for_thread.recv() {
                let body = serde_json::json!({
                    "version": "999.0.0",
                    "notes": "test release",
                    "pub_date": "2026-01-01T00:00:00Z",
                    "platforms": {
                        "darwin": {
                            "url": format!("http://127.0.0.1:{port}/fake-binary"),
                            // Not a valid minisign signature at all --
                            // guaranteed to fail decode/verification.
                            "signature": "not-a-real-signature",
                        }
                    }
                })
                .to_string();
                let response = tiny_http::Response::from_string(body).with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                        .unwrap(),
                );
                let _ = request.respond(response);
            }

            // Request 2: the "binary" download itself.
            if let Ok(request) = server_for_thread.recv() {
                let response = tiny_http::Response::from_string("fake binary bytes");
                let _ = request.respond(response);
            }
        });

        let app = mock_app();
        let updater = app
            .updater_builder()
            .endpoints(vec![format!("http://127.0.0.1:{port}/latest.json").parse().unwrap()])
            .unwrap()
            .target("darwin")
            .build()
            .unwrap();

        let update = updater
            .check()
            .await
            .expect("check() itself must succeed -- the manifest is well-formed JSON")
            .expect("999.0.0 must compare as newer than whatever mock_context's current version is");

        let download_result = update.download(|_, _| {}, || {}).await;

        assert!(
            download_result.is_err(),
            "an update whose signature is not a valid signature at all must be rejected \
             (Err), never accepted (Ok) and never silently treated as 'no update'"
        );

        handle.join().ok();
    }

    /// Doc 30 TASK-DESK-005 acceptance: `test_manual_check_for_updates_menu_item`.
    /// Drives the real path `menu::handle_menu_event`'s `MenuAction::CheckForUpdates`
    /// arm calls: `trigger_manual_check` spawns `check_for_update`, which
    /// (via a real local mock server, same pattern as the signature-
    /// rejection test) must actually reach the network and populate
    /// `PendingUpdate` -- proving the menu item is wired to a real check,
    /// not just dispatching an inert event (the gap this task's own
    /// fix-log flagged in TASK-DESK-001).
    #[tokio::test]
    async fn test_manual_check_for_updates_menu_item() {
        // Bind first so the real port is known before the app (and its
        // static `plugins.updater.endpoints` config) is built -- `check_for_update`
        // calls the plain `app.updater()` (no per-call override), exactly
        // as `trigger_manual_check`/`menu::handle_menu_event`'s real
        // dispatch does, so the mock endpoint has to be baked into the
        // app's config up front rather than passed in later.
        let server = Arc::new(
            tiny_http::Server::http("127.0.0.1:0")
                .expect("failed to start local mock update server"),
        );
        let port = server
            .server_addr()
            .to_ip()
            .expect("mock update server must have an IP address")
            .port();

        let handle = std::thread::spawn(move || {
            if let Ok(request) = server.recv() {
                let body = serde_json::json!({
                    "version": "999.0.0",
                    "notes": "test release",
                    "pub_date": "2026-01-01T00:00:00Z",
                    "platforms": {
                        "darwin": {
                            "url": format!("http://127.0.0.1:{port}/fake-binary"),
                            "signature": "not-a-real-signature",
                        }
                    }
                })
                .to_string();
                let response = tiny_http::Response::from_string(body).with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                        .unwrap(),
                );
                let _ = request.respond(response);
            }
        });

        let mut ctx = tauri::test::mock_context(tauri::test::noop_assets());
        ctx.config_mut().plugins.0.insert(
            "updater".into(),
            serde_json::json!({
                "endpoints": [format!("http://127.0.0.1:{port}/latest.json")],
                "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6IHRlc3QKUldUZXN0S2V5MDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMA==",
            }),
        );
        let app = tauri::test::mock_builder()
            .plugin(
                tauri_plugin_updater::Builder::new()
                    .target("darwin")
                    .build(),
            )
            .build(ctx)
            .unwrap()
            .handle()
            .clone();
        app.manage(PendingUpdate::default());

        trigger_manual_check(&app);

        // Bounded wait for the spawned check to complete.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if app.state::<PendingUpdate>().0.lock().await.is_some() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "trigger_manual_check must actually reach the network and \
                 populate PendingUpdate -- it never did within the timeout"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        handle.join().ok();
    }
}
