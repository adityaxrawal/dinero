//! Application root: module tree and the Tauri startup sequence.
//!
//! `run()` is the single place the desktop application is assembled, and the
//! order of what happens there is load-bearing:
//!
//!   1. Logging is initialised first, so every later step -- including a failure
//!      in one -- is recorded rather than lost.
//!   2. A panic hook is installed, which routes panics into the same log. A
//!      panic in a Tauri command would otherwise unwind into the runtime and
//!      leave no trace on disk.
//!   3. Plugins are registered, then the window close handler, then `setup`.
//!   4. Inside `setup`: the integrity check gates everything else in release
//!      builds, and only once it passes are the database, background services
//!      and IPC handlers brought up.
//!
//! The integrity check deliberately terminates the process on failure rather
//! than degrading, since a bundle that fails signature verification may have
//! been modified.

pub mod auth;
pub mod background_tasks;
pub mod billing;
pub mod commands;
pub mod crash_reporter;
pub mod db;
pub mod diagnostics;
pub mod error;
pub mod extraction;
pub mod feedback;
pub mod health;
pub mod ingestion;
pub mod integrity;
pub mod ipc;
pub mod learning;
pub mod licensing;
pub mod lifecycle;
pub mod llama_sidecar;
pub mod llm_manager;
pub mod llm_pipeline;
pub mod logging;
pub mod menu;
pub mod network_client;
pub mod notifications;
pub mod permissions;
pub mod reconciliation;
pub mod security;
pub mod startup;
pub mod statements;
pub mod updater;

use std::path::PathBuf;
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Builds and runs the desktop application.
pub fn run() {
    // Logs belong at the repository/app root, not inside src-tauri. During
    // development cargo runs from src-tauri, so that one level is stripped.
    let mut log_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    if log_dir.ends_with("src-tauri") {
        log_dir = log_dir
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf();
    }
    let (categorized_writers, guards) = crate::logging::CategorizedLogWriters::init(&log_dir);
    // The writer guards must outlive every log call, and logging continues
    // until the process exits. Leaking them is the simplest correct lifetime
    // here -- dropping them early would silently truncate the log files.
    Box::leak(Box::new(guards));

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    // RUST_LOG wins when set; otherwise a default that keeps this app's own
    // targets verbose while leaving dependencies at info.
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,dinero_app_lib=trace,dinero_app=trace,frontend=trace,api_calls=trace,network=trace,llm_calls=info,ingestion_extraction=info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().with_ansi(true))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(categorized_writers)
                .with_ansi(false)
                .with_target(true)
                .with_file(true)
                .with_line_number(true)
                .with_thread_names(true)
                .with_level(true),
        )
        .init();

    // Panics are otherwise invisible in a packaged desktop build -- there is no
    // console attached. The payload is a `&str` or a `String` depending on how
    // the panic was raised, so both are attempted before giving up.
    std::panic::set_hook(Box::new(|panic_info| {
        let location = panic_info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());
        let message = panic_info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| panic_info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".to_string());
        tracing::error!("PANIC at {}: {}", location, message);
    }));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // Closing the main window does not necessarily quit: with background sync
        // enabled the app keeps running to continue scheduled work.
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app_dir = window
                    .app_handle()
                    .path()
                    .app_data_dir()
                    .unwrap_or_else(|_| PathBuf::from(".dinero"));
                let background_sync_enabled =
                    crate::lifecycle::launch_agent::read_background_sync_enabled(&app_dir);
                crate::lifecycle::launch_agent::handle_main_window_close_requested(
                    window,
                    api,
                    background_sync_enabled,
                );
            }
        })
        .setup(|app| {
            let app_menu = crate::menu::build_menu(app.handle())?;
            app.set_menu(app_menu)?;
            app.on_menu_event(|app_handle, event| {
                crate::menu::handle_menu_event(app_handle, event);
            });

            // Release builds only -- development binaries are unsigned by
            // nature. A failure here is fatal by design: the app explains why
            // and exits rather than running from a bundle it cannot vouch for.
            #[cfg(not(debug_assertions))]
            if let Err(e) = crate::integrity::verify_binary_integrity() {
                tracing::error!("Fatal: binary integrity check failed — {}", e);
                let msg = format!(
                    "Dinero's code signature could not be verified and the app will not start:\n\n{e}\n\n\
                    This can happen if the application bundle has been modified or corrupted. \
                    Please reinstall Dinero from a trusted source."
                );
                let _ = app.dialog()
                    .message(&msg)
                    .title("Dinero — Integrity Check Failed")
                    .kind(tauri_plugin_dialog::MessageDialogKind::Error)
                    .blocking_show();
                std::process::exit(1);
            }

            crate::startup::check_ram_and_set_llm_eligibility(&app.handle().clone());

            let app_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from(".dinero"));
            std::fs::create_dir_all(&app_dir).unwrap();

            crate::crash_reporter::init(app_dir.clone());
            app.manage(crate::feedback::FeedbackManager::new(app_dir.clone()));

            // Database bring-up. This is the most failure-prone step in startup:
            // the file is SQLCipher-encrypted with a key held in the OS keychain,
            // so it can fail for reasons the user must be told about explicitly
            // rather than seeing a blank window.
            let db_path = app_dir.join("finance.db");

            // Detected before opening: the hardware UUID marker changing means
            // the database file has moved to a different Mac, which the user is
            // notified about after a successful open.
            let looks_like_hardware_migration =
                crate::db::crypto::hw_uuid_marker_indicates_migration(&app_dir);

            let pool = match tauri::async_runtime::block_on(async {
                db::init_db(db_path.clone()).await
            }) {
                Ok(p) => {
                    if looks_like_hardware_migration {
                        let _ = app.handle().emit(
                            crate::ipc::events::AppEvent::DbHardwareMigrated.as_str(),
                            serde_json::json!({
                                "message": "Database migrated to new Mac.",
                            }),
                        );
                    }

                    // Connected Gmail accounts survive a database reset via this
                    // plaintext-metadata backup. It is consumed once and deleted,
                    // so a later reset does not silently resurrect old accounts.
                    let backup_path = app_dir.join("gmail_accounts_backup.json");
                    if backup_path.exists() {
                        if let Ok(json) = std::fs::read_to_string(&backup_path) {
                            if let Ok(accounts) = serde_json::from_str::<Vec<crate::db::connected_accounts::ConnectedAccountsRow>>(&json) {
                                let pool_clone = p.clone();
                                tauri::async_runtime::block_on(async move {
                                    if let Ok(conn) = pool_clone.get().await {
                                        let _ = conn.interact(move |c| {
                                            for account in accounts {
                                                let _ = crate::db::connected_accounts::insert_account(c, &account);
                                            }
                                        }).await;
                                    }
                                });
                            }
                        }
                        let _ = std::fs::remove_file(backup_path);
                    }

                    p
                }
                // Each failure below is terminal but distinct, and each explains
                // the specific recovery path -- a generic "could not start"
                // would leave the user with no way forward.
                Err(db::DbInitError::KeyMismatch) => {
                    let msg = concat!(
                        "Dinero cannot open its database.\n\n",
                        "The encryption key stored in your Keychain no longer matches the database file on disk. ",
                        "This can happen after a Keychain reset, a macOS migration, or an OS reinstall.\n\n",
                        "To recover your data, use Settings → Recovery Phrase after the app restarts.\n",
                        "If you have no Recovery Phrase, you must reset App Data from Settings to start fresh.\n\n",
                        "The app will now exit."
                    );
                    tracing::error!("Fatal: DB key mismatch — {}", msg);
                    let _ = app.dialog()
                        .message(msg)
                        .title("Dinero — Database Key Mismatch")
                        .kind(tauri_plugin_dialog::MessageDialogKind::Error)
                        .blocking_show();
                    std::process::exit(1);
                }
                Err(db::DbInitError::KeychainAccessDenied) => {
                    let msg = concat!(
                        "Dinero needs access to the macOS Keychain to encrypt your financial data — ",
                        "this is required and cannot be skipped.\n\n",
                        "If macOS showed a Keychain permission prompt and it was denied, or the login ",
                        "Keychain is locked, please:\n",
                        "  1. Open Keychain Access.app\n",
                        "  2. Unlock your login keychain if it shows as locked\n",
                        "  3. Restart Dinero and allow the Keychain access prompt\n\n",
                        "The app will now exit."
                    );
                    tracing::error!("Fatal: Keychain access denied — {}", msg);
                    let _ = app.dialog()
                        .message(msg)
                        .title("Dinero — Keychain Access Required")
                        .kind(tauri_plugin_dialog::MessageDialogKind::Error)
                        .blocking_show();
                    std::process::exit(1);
                }
                // A failed schema migration is recoverable: a backup is taken
                // immediately before every migration, so the user is offered a
                // rollback rather than being left with a half-migrated database.
                Err(db::DbInitError::MigrationFailed { source, backup_path }) => {
                    tracing::error!(
                        "Migration failed (backup at {}): {:?}",
                        backup_path.display(),
                        source
                    );
                    let msg = format!(
                        "Dinero encountered an error while updating its database:\n\n{source}\n\n\
                        A backup taken immediately before this update is available at:\n{}\n\n\
                        Restore this backup? Your data will be reverted to its state just before \
                        this update attempt. Choosing \"No\" exits the app without making changes.",
                        backup_path.display()
                    );
                    let restore = app.dialog()
                        .message(&msg)
                        .title("Dinero — Database Update Failed")
                        .kind(tauri_plugin_dialog::MessageDialogKind::Warning)
                        .buttons(tauri_plugin_dialog::MessageDialogButtons::YesNo)
                        .blocking_show();

                    if !restore {
                        tracing::error!("User declined migration rollback — exiting.");
                        std::process::exit(1);
                    }

                    // Restore, then re-open. If the reopen still fails the
                    // situation is beyond automatic recovery and the app exits
                    // rather than looping.
                    if let Err(e) = db::restore_backup_file(&db_path, &backup_path) {
                        let msg = format!(
                            "Failed to restore the backup:\n\n{e}\n\nThe app will now exit."
                        );
                        tracing::error!("Backup restore failed: {}", e);
                        let _ = app.dialog()
                            .message(&msg)
                            .title("Dinero — Restore Failed")
                            .kind(tauri_plugin_dialog::MessageDialogKind::Error)
                            .blocking_show();
                        std::process::exit(1);
                    }

                    match tauri::async_runtime::block_on(async {
                        db::init_db(db_path.clone()).await
                    }) {
                        Ok(p) => {
                            tracing::info!("Restored pre-migration backup — app resuming with reverted data.");
                            p
                        }
                        Err(e) => {
                            let msg = format!(
                                "The backup was restored, but Dinero still could not start:\n\n{e}\n\n\
                                The app will now exit."
                            );
                            tracing::error!("DB init still failing after backup restore: {}", e);
                            let _ = app.dialog()
                                .message(&msg)
                                .title("Dinero — Database Error")
                                .kind(tauri_plugin_dialog::MessageDialogKind::Error)
                                .blocking_show();
                            std::process::exit(1);
                        }
                    }
                }
                // Corruption. Recoverable only if a daily backup exists; without
                // one there is nothing to fall back to.
                Err(db::DbInitError::IntegrityCheckFailed { details }) => {
                    tracing::error!("Fatal: DB integrity check failed — {}", details);
                    let daily_backup = app_dir.join("backups").join("finance.db.daily.bak");
                    if !daily_backup.exists() {
                        let msg = format!(
                            "Dinero's database failed an integrity check and appears to be corrupted:\n\n{details}\n\n\
                            No daily backup was found to restore from. The app will now exit.\n\
                            You may need to reset App Data to start fresh."
                        );
                        let _ = app.dialog()
                            .message(&msg)
                            .title("Dinero — Database Corrupted")
                            .kind(tauri_plugin_dialog::MessageDialogKind::Error)
                            .blocking_show();
                        std::process::exit(1);
                    }

                    let msg = format!(
                        "Dinero's database failed an integrity check and appears to be corrupted:\n\n{details}\n\n\
                        Restore the most recent daily backup? Any changes since that backup will be lost. \
                        Choosing \"No\" exits the app without making changes."
                    );
                    let restore = app.dialog()
                        .message(&msg)
                        .title("Dinero — Database Corrupted")
                        .kind(tauri_plugin_dialog::MessageDialogKind::Warning)
                        .buttons(tauri_plugin_dialog::MessageDialogButtons::YesNo)
                        .blocking_show();

                    if !restore {
                        tracing::error!("User declined corruption recovery — exiting.");
                        std::process::exit(1);
                    }

                    if let Err(e) = db::restore_backup_file(&db_path, &daily_backup) {
                        let msg = format!("Failed to restore the daily backup:\n\n{e}\n\nThe app will now exit.");
                        tracing::error!("Daily backup restore failed: {}", e);
                        let _ = app.dialog()
                            .message(&msg)
                            .title("Dinero — Restore Failed")
                            .kind(tauri_plugin_dialog::MessageDialogKind::Error)
                            .blocking_show();
                        std::process::exit(1);
                    }

                    match tauri::async_runtime::block_on(async {
                        db::init_db(db_path.clone()).await
                    }) {
                        Ok(p) => {
                            tracing::info!("Restored daily backup — app resuming with reverted data.");
                            p
                        }
                        Err(e) => {
                            let msg = format!(
                                "The daily backup was restored, but Dinero still could not start:\n\n{e}\n\n\
                                The app will now exit."
                            );
                            tracing::error!("DB init still failing after daily backup restore: {}", e);
                            let _ = app.dialog()
                                .message(&msg)
                                .title("Dinero — Database Error")
                                .kind(tauri_plugin_dialog::MessageDialogKind::Error)
                                .blocking_show();
                            std::process::exit(1);
                        }
                    }
                }
                // Anything not handled above. No specific remedy to offer, so
                // the message is generic and the app exits.
                Err(e) => {
                    let msg = format!(
                        "Dinero could not initialise its database and must exit.\n\nError: {e}\n\n\
                        If this problem persists, please contact support."
                    );
                    tracing::error!("Fatal: DB init failed — {}", e);
                    let _ = app.dialog()
                        .message(&msg)
                        .title("Dinero — Database Error")
                        .kind(tauri_plugin_dialog::MessageDialogKind::Error)
                        .blocking_show();
                    std::process::exit(1);
                }
            };

            // Owner-only permissions on the database file. It holds financial
            // data and, although encrypted at rest, must not be readable by other
            // local accounts. A failure is a warning rather than fatal, since the
            // encryption remains the primary protection.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Err(e) = std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600)) {
                    tracing::warn!("Failed to chmod 600 {}: {}", db_path.display(), e);
                }
            }

            // Shared application state. Everything registered here is reachable
            // from any Tauri command handler through the app handle.
            let pool_clone = pool.clone();
            app.manage(pool);

            app.manage(crate::auth::session::SessionState::default());
            app.manage(crate::security::incident_response::IncidentMonitor::default());
            app.manage(crate::background_tasks::indicator::BackgroundTaskRegistry::default());
            app.manage(crate::llm_manager::DownloadRegistry::default());
            app.manage(crate::llm_pipeline::LlmPipeline::new());
            {
                // Warning dismissals are persisted, so they are loaded into the
                // in-memory set before any warning can be raised -- otherwise a
                // previously dismissed warning would reappear on every launch.
                let pool_for_dismissals = pool_clone.clone();
                let loaded = tauri::async_runtime::block_on(async move {
                    let conn = pool_for_dismissals.get().await.ok()?;
                    conn.interact(|c| crate::db::dismissed_warnings::load_all(c))
                        .await
                        .ok()?
                        .ok()
                });
                crate::ipc::system_warnings::load_dismissals(loaded.unwrap_or_default());
            }
            {
                // Spawned rather than blocking: session setup is not needed for
                // the window to appear, and blocking here would delay first paint.
                let pool_for_session = pool_clone.clone();
                let session_state_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let state = session_state_handle.state::<crate::auth::session::SessionState>();
                    if let Err(e) = crate::auth::session::ensure_active_session(&pool_for_session, state.inner()).await {
                        tracing::warn!("Failed to establish local session: {}", e);
                    }
                });
            }

            crate::permissions::macos_permissions::check_permissions_at_launch(&app.handle().clone());

            {
                let pool_for_notif_permission = pool_clone.clone();
                let app_handle_for_notif_permission = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    crate::notifications::request_permission_if_disclosed(
                        &app_handle_for_notif_permission,
                        &pool_for_notif_permission,
                    )
                    .await;
                });
            }

            // Background services from here on. Each is spawned onto the async
            // runtime so startup completes and the window renders while they come
            // up in the background.
            let pipeline = app.state::<crate::llm_pipeline::LlmPipeline>().inner().clone();
            let learning_handle = crate::learning::spawn_learning_worker(pool_clone.clone(), pipeline);
            app.manage(learning_handle.clone());

            let queue_handles = crate::ingestion::queues::spawn_queues(
                app.handle().clone(),
                pool_clone.clone(),
                learning_handle,
            );
            // The layer-6 sender is captured before the handles are moved into
            // managed state, so the replay task below can still reach the queue.
            let layer6_tx_for_replay = queue_handles.layer6_tx.clone();
            app.manage(queue_handles);

            // Re-enqueues extraction jobs that were still pending when the app
            // last exited, so work interrupted by a quit is not silently dropped.
            let pool_for_layer6_replay = pool_clone.clone();
            let app_dir_for_layer6_replay = app_dir.clone();
            tauri::async_runtime::spawn(async move {
                crate::ingestion::queues::replay_pending_layer6_jobs(
                    &pool_for_layer6_replay,
                    &layer6_tx_for_replay,
                    app_dir_for_layer6_replay,
                )
                .await;
            });

            // Statement PDFs are retained only briefly; expired ones are purged
            // on every launch so raw financial documents do not accumulate.
            let pool_for_cleanup = pool_clone.clone();
            let app_data_dir = app_dir.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = crate::statements::pdf_storage::cleanup_expired_pdfs(&app_data_dir, &pool_for_cleanup).await {
                    tracing::error!("Failed to cleanup expired PDFs: {}", e);
                }
            });

            let pool_for_polling = pool_clone.clone();
            let app_handle_for_polling = app.handle().clone();
            // One cancellation token shared by the long-running loops, so app
            // shutdown can stop them together rather than one at a time.
            let cancel_token = tokio_util::sync::CancellationToken::new();
            app.manage(cancel_token.clone());

            // Polling cadence adapts to power state -- a laptop on battery polls
            // less aggressively than one on mains.
            app.manage(crate::lifecycle::launch_agent::PollingIntervalState::default());
            let app_handle_for_battery_loop = app.handle().clone();
            let cancel_token_for_battery_loop = cancel_token.clone();
            tauri::async_runtime::spawn(async move {
                crate::lifecycle::launch_agent::run_battery_aware_polling_interval_loop(
                    app_handle_for_battery_loop,
                    cancel_token_for_battery_loop,
                )
                .await;
            });

            tauri::async_runtime::spawn(async move {
                crate::ingestion::polling::start_polling_loop(
                    app_handle_for_polling,
                    pool_for_polling,
                    cancel_token,
                )
                .await;
            });

            // Watches for gaps in ingested data (a missing statement period, a
            // silent account) and raises reconciliation alerts.
            let pool_for_alerts = pool_clone.clone();
            let app_handle_for_alerts = app.handle().clone();
            let cancel_token_alerts = app
                .state::<tokio_util::sync::CancellationToken>()
                .inner()
                .clone();
            tauri::async_runtime::spawn(async move {
                crate::reconciliation::alert_worker::start_missing_data_polling_loop(
                    app_handle_for_alerts,
                    pool_for_alerts,
                    cancel_token_alerts,
                )
                .await;
            });

            // Daily encrypted backup loop. The backup is written with VACUUM
            // INTO, which produces a consistent snapshot without stopping writers
            // and compacts the file in the same pass.
            let backup_dir = app_dir.join("backups");
            std::fs::create_dir_all(&backup_dir).unwrap();
            let archive_dir = app_dir.join("archives");
            let archive_db_key = crate::db::crypto::derive_database_key().ok();
            let pool_for_backup = pool_clone.clone();
            let app_handle_for_backup = app.handle().clone();
            let db_path_for_backup = db_path.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(60 * 60 * 24)).await;
                    tracing::info!("Running daily encrypted SQLite backup background task...");

                    let backup_file = backup_dir.join("finance.db.daily.bak");
                    // Written to a temporary file and renamed on success. The
                    // rename is atomic, so a crash mid-backup can never leave a
                    // truncated file sitting where the recovery path expects a
                    // usable backup.
                    let backup_tmp_file = backup_dir.join("finance.db.daily.bak.tmp");
                    let _ = std::fs::remove_file(&backup_tmp_file);
                    let backup_tmp_file_str = backup_tmp_file.to_string_lossy().to_string();

                    if let Ok(conn) = pool_for_backup.get().await {
                        let res = conn
                            .interact(move |c| {
                                c.execute("VACUUM INTO ?", [&backup_tmp_file_str])
                            })
                            .await;

                        match res {
                            Ok(Ok(_)) => {
                                match std::fs::rename(&backup_tmp_file, &backup_file) {
                                    Ok(_) => {
                                        tracing::info!(
                                            "Backup successful: {}",
                                            backup_file.display()
                                        );
                                        // A backup that cannot be verified is
                                        // worse than none, because it invites
                                        // false confidence -- so the user is
                                        // warned rather than left assuming it
                                        // would work.
                                        if let Err(e) = crate::db::backup::verify_backup_integrity(&backup_file) {
                                            tracing::error!(
                                                "Daily backup verification failed for {}: {}",
                                                backup_file.display(),
                                                e
                                            );
                                            crate::ipc::system_warnings::emit_system_warning(
                                                &app_handle_for_backup,
                                                crate::ipc::system_warnings::SystemWarningPayload {
                                                    warning_type: "backup_verification_failed".to_string(),
                                                    message: "Today's automatic backup could not be verified. \
                                                    Your data is still safe, but this backup may not be usable \
                                                    for restore.".to_string(),
                                                    severity: crate::ipc::system_warnings::WarningSeverity::Degraded,
                                                    action_hint: None,
                                                },
                                            );
                                        }
                                        let _ = app_handle_for_backup.emit(
                                            crate::ipc::events::AppEvent::DbBackupCompleted.as_str(),
                                            serde_json::json!({
                                                "completed_at": chrono::Utc::now().to_rfc3339(),
                                            }),
                                        );
                                        if let Ok(conn) = pool_for_backup.get().await {
                                            let _ = conn
                                                .interact(|c| {
                                                    crate::db::audit_log::insert(
                                                        c,
                                                        &crate::db::audit_log::AuditLogRow {
                                                            id: uuid::Uuid::new_v4().to_string(),
                                                            actor_type: Some("system".to_string()),
                                                            actor_id: None,
                                                            action: Some("db_backup_completed".to_string()),
                                                            resource_type: Some("database".to_string()),
                                                            resource_id: None,
                                                            before_json: None,
                                                            after_json: None,
                                                            created_at: chrono::Utc::now(),
                                                        },
                                                    )
                                                })
                                                .await;
                                        }
                                    }
                                    Err(e) => tracing::error!(
                                        "Backup succeeded but failed to replace {}: {}",
                                        backup_file.display(),
                                        e
                                    ),
                                }
                            }
                            _ => {
                                tracing::error!("Backup failed");
                                let _ = std::fs::remove_file(&backup_tmp_file);
                            }
                        }
                    }

                    if let Ok(conn) = pool_for_backup.get().await {
                        let app_handle_for_integrity = app_handle_for_backup.clone();
                    // Integrity is re-checked on the same daily cadence. A
                    // failure is escalated through the incident monitor rather
                    // than merely logged, since silent corruption of financial
                    // records is exactly what must not go unnoticed.
                        let integrity_ok = conn
                            .interact(move |c| {
                                crate::db::maintenance::check_integrity_and_report(c, &app_handle_for_integrity)
                            })
                            .await;
                        if let Ok(Ok(false)) = integrity_ok {
                            let monitor = app_handle_for_backup.state::<crate::security::incident_response::IncidentMonitor>();
                            if crate::security::incident_response::record_trigger(
                                monitor.inner(),
                                crate::security::incident_response::TriggerKind::IntegrityCheckFailure,
                            ) {
                                let session_state = app_handle_for_backup.state::<crate::auth::session::SessionState>();
                                let _ = crate::security::incident_response::respond_to_incident(
                                    crate::security::incident_response::TriggerKind::IntegrityCheckFailure,
                                    &app_handle_for_backup,
                                    &pool_for_backup,
                                    session_state.inner(),
                                )
                                .await;
                            }
                        }
                    }
                    // Daily maintenance sweeps follow. Each reclaims space or
                    // enforces a retention policy, and each is independently
                    // fallible -- a failure in one must not skip the rest, which
                    // is why they are separate blocks rather than one chain.
                    if let Ok(conn) = pool_for_backup.get().await {
                        let _ = conn
                            .interact(|c| crate::db::maintenance::run_incremental_vacuum(c))
                            .await;
                    }
                    let _ = crate::db::maintenance::check_db_size_warning(&app_handle_for_backup, &db_path_for_backup);

                    if let Ok(conn) = pool_for_backup.get().await {
                        let _ = conn
                            .interact(|c| crate::db::retention::sweep_raw_payloads(c))
                            .await;
                    }

                    if let Ok(conn) = pool_for_backup.get().await {
                        let _ = conn
                            .interact(|c| crate::db::retention::sweep_reconciliation_audit(c))
                            .await;
                    }

                    if let Ok(conn) = pool_for_backup.get().await {
                        let _ = conn
                            .interact(|c| crate::db::retention::sweep_settled_statement_drafts(c))
                            .await;
                    }

                    if let Ok(conn) = pool_for_backup.get().await {
                        let _ = conn
                            .interact(|c| crate::db::ignored_messages::purge_expired(c))
                            .await;
                    }

                    // Old transactions move to a separately encrypted archive
                    // rather than being deleted, keeping the working database
                    // small without losing history. Skipped entirely if the key
                    // could not be derived.
                    if let Some(db_key) = archive_db_key.clone() {
                        let archive_dir = archive_dir.clone();
                        if let Ok(conn) = pool_for_backup.get().await {
                            let _ = conn
                                .interact(move |c| {
                                    crate::db::retention::archive_old_transactions(c, &archive_dir, &db_key)
                                })
                                .await;
                        }
                    }

                    if let Ok(conn) = pool_for_backup.get().await {
                        let today = chrono::Utc::now().date_naive();
                    // Bill reminders ride the same daily loop rather than
                    // running their own timer, since a once-a-day check is
                    // exactly the right cadence for a three-day warning.
                        let due_soon = conn
                            .interact(move |c| crate::db::instruments::list_upcoming_bills(c, &today))
                            .await;
                        if let Ok(Ok(instruments)) = due_soon {
                            for instrument in instruments {
                                let Some(due_date) = instrument.statement_due_date else {
                                    continue;
                                };
                                if crate::notifications::is_three_days_before_due(due_date, today) {
                                    let label = instrument
                                        .nickname
                                        .clone()
                                        .unwrap_or_else(|| instrument.issuer_name.clone());
                                    crate::notifications::send_notification(
                                        &app_handle_for_backup,
                                        crate::notifications::NotificationKind::UpcomingBillDue,
                                        "Upcoming Bill Due",
                                        &format!("{} is due in 3 days", label),
                                        Some(&instrument.id),
                                    );
                                }
                            }
                        }
                    }

                    crate::permissions::macos_permissions::check_permissions_at_launch(
                        &app_handle_for_backup,
                    );
                }
            });

            // Periodic licence revalidation against the licensing service, so a
            // subscription that lapsed or was cancelled elsewhere takes effect
            // without waiting for a restart.
            let pool_for_licensing = pool_clone.clone();
            let app_handle_for_licensing = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                crate::licensing::worker::start_background_validation(
                    pool_for_licensing,
                    "https://api.dinero-app.com".to_string(),
                    app_handle_for_licensing,
                )
                .await;
            });

            // Health poll. A backend that stops reporting ready is escalated to
            // the incident monitor, which is what surfaces the offline indicator
            // in the sidebar.
            let pool_for_health = pool_clone.clone();
            let app_handle_for_health = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    let report = crate::health::compute_health_report(&pool_for_health).await;
                    if !report.backend_ready {
                        let monitor = app_handle_for_health
                            .state::<crate::security::incident_response::IncidentMonitor>();
                        if crate::security::incident_response::record_trigger(
                            &monitor,
                            crate::security::incident_response::TriggerKind::BackendStartupFailure,
                        ) {
                            crate::security::incident_response::emit_health_alert(
                                crate::security::incident_response::TriggerKind::BackendStartupFailure,
                                &app_handle_for_health,
                            );
                        }
                    }
                }
            });

            // Update checks are release-only: a dev build would otherwise be
            // prompted to replace itself with the published version.
            app.manage(crate::updater::PendingUpdate::default());
            if !cfg!(debug_assertions) {
                crate::updater::spawn_update_check_loop(app.handle().clone());
            }

            {
            // macOS menu-bar extra and dock badge, refreshed on a loop so the
            // at-a-glance figures stay current while the window is closed.
                let menu_bar_extra_enabled =
                    crate::menu::status_item::read_menu_bar_extra_enabled(&app_dir);
                crate::menu::status_item::apply_menu_bar_extra_runtime_state(
                    &app.handle().clone(),
                    menu_bar_extra_enabled,
                );

                let pool_for_status = pool_clone.clone();
                let app_handle_for_status = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        if let Ok(conn) = pool_for_status.get().await {
                            let app_handle = app_handle_for_status.clone();
                            let _ = conn
                                .interact(move |c| {
                                    let today = chrono::Utc::now().naive_utc().date();
                                    let pending =
                                        crate::commands::data::compute_unassigned_amount_pending_review(c)
                                            .unwrap_or(crate::commands::data::PendingReviewMetric {
                                                count: 0,
                                                amount_minor: 0,
                                            });
                                    let month_spend = crate::commands::data::do_fetch_dashboard_summary(c)
                                        .map(|s| s.month_to_date_spend)
                                        .unwrap_or(0.0);
                                    let upcoming_count =
                                        crate::commands::data::do_fetch_upcoming_bills(c, &today)
                                            .map(|bills| bills.len() as i64)
                                            .unwrap_or(0);

                                    crate::menu::status_item::update_dock_badge(&app_handle, pending.count);
                                    crate::menu::status_item::update_tray_summary_if_present(
                                        &app_handle,
                                        month_spend,
                                        pending.count,
                                        upcoming_count,
                                    );
                                })
                                .await;
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    }
                });
            }

            Ok(())
        })
        // Registers every #[tauri::command] the frontend can invoke.
        .invoke_handler(commands::get_handlers())
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        // Shutdown: cancelling the shared token stops the polling, battery and
        // alert loops together, so they do not keep touching the database while
        // the process is tearing down.
        .run(|app_handle, event| match event {
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit => {
                if let Some(cancel_token) = app_handle.try_state::<tokio_util::sync::CancellationToken>() {
                    cancel_token.cancel();
                }
            }
            _ => {}
        });
}
#[cfg(test)]
mod tests;
