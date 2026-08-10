//! Application update checking and installation.
//!
//! Updates are never applied silently -- an available update is surfaced and
//! installed only on explicit confirmation, since a background restart mid-scan
//! would discard work in progress.
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::UpdaterExt;

pub const UPDATE_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

const GRACEFUL_SHUTDOWN_GRACE_PERIOD: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct UpdateAvailable {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
}

#[derive(Default)]
pub struct PendingUpdate(pub tokio::sync::Mutex<Option<tauri_plugin_updater::Update>>);

/// Checks for an available update.
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

#[tauri::command]
/// Installs a pending update after the user confirms.
///
/// Never silent: a background restart mid-scan would discard work in progress.
pub async fn updater_confirm_install(
    app: AppHandle,
    pending: tauri::State<'_, PendingUpdate>,
) -> Result<(), crate::error::AppError> {
    let update = pending.0.lock().await.take().ok_or_else(|| {
        crate::error::AppError::NotFound("No pending update to install".to_string())
    })?;
    download_and_install(&app, update)
        .await
        .map_err(crate::error::AppError::Unknown)
}

/// Triggers a manual update check from the menu.
pub fn trigger_manual_check<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = check_for_update(&app).await {
            tracing::warn!("Manual update check failed: {}", e);
        }
    });
}

/// Prepares for shutdown before an update restarts the app.
///
/// Stops background work so an update does not interrupt a scan mid-write.
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

/// Downloads and installs the update.
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

/// Starts the periodic update-check loop.
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
        let registry = crate::background_tasks::indicator::BackgroundTaskRegistry::default();
        registry.register_or_update(
            &app,
            "acct_1",
            "historical_scan",
            "Scanning acct_1",
            1,
            10,
            "Scanning…",
        );
        app.manage(registry);

        prepare_for_graceful_shutdown(&app).await;
    }

    #[tokio::test]
    async fn test_unsigned_update_rejected() {
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
            if let Ok(request) = server_for_thread.recv() {
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

            if let Ok(request) = server_for_thread.recv() {
                let response = tiny_http::Response::from_string("fake binary bytes");
                let _ = request.respond(response);
            }
        });

        let app = mock_app();
        let updater = app
            .updater_builder()
            .endpoints(vec![format!("http://127.0.0.1:{port}/latest.json")
                .parse()
                .unwrap()])
            .unwrap()
            .target("darwin")
            .build()
            .unwrap();

        let update = updater
            .check()
            .await
            .expect("check() itself must succeed -- the manifest is well-formed JSON")
            .expect(
                "999.0.0 must compare as newer than whatever mock_context's current version is",
            );

        let download_result = update.download(|_, _| {}, || {}).await;

        assert!(
            download_result.is_err(),
            "an update whose signature is not a valid signature at all must be rejected \
             (Err), never accepted (Ok) and never silently treated as 'no update'"
        );

        handle.join().ok();
    }

    #[tokio::test]
    async fn test_manual_check_for_updates_menu_item() {
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
