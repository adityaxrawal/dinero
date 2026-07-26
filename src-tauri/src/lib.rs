pub mod auth;
pub mod background_tasks;
pub mod billing;
pub mod commands;
pub mod crash_reporter;
pub mod db;
pub mod dev_review;
pub mod diagnostics;
pub mod error;
pub mod extraction;
pub mod feedback;
pub mod health;
pub mod ingestion;
pub mod integrity;
pub mod ipc;
pub mod licensing;
pub mod lifecycle;
pub mod llama_sidecar;
pub mod llm_manager;
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
pub fn run() {
    let mut log_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    if log_dir.ends_with("src-tauri") {
        log_dir = log_dir
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf();
    }
    // J4 fix (Doc 28 §4.2): the debug log previously never rotated or
    // expired at all (`rolling::never` into one ever-growing file). Rotates
    // daily and prunes files older than the configurable retention window
    // (15 days by default, matching the documented default).
    let file_appender = tracing_appender::rolling::daily(&log_dir, "app-logs.log");
    // TASK-OPS-007: redact at write time, not only lazily when a diagnostic
    // bundle is later exported -- `app-logs.log` itself must never hold
    // unredacted PII on disk in the interim. The console/stdout layer below
    // is deliberately left unredacted for local-dev ergonomics; only the
    // persisted file is wrapped.
    let redacting_appender = crate::logging::RedactingWriter::new(file_appender);
    let (non_blocking, _guard) = tracing_appender::non_blocking(redacting_appender);
    Box::leak(Box::new(_guard));
    crate::logging::prune_old_logs(&log_dir);

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,dinero_app_lib=trace,dinero_app=trace"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().with_ansi(true))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false),
        )
        .init();

    // H4 fix (Doc 19 §3.4): a global panic hook so any panic — inside a
    // command handler, a spawned background task, or elsewhere — is captured
    // in app-logs.log (which the diagnostic bundle and crash_reporter both
    // read from) instead of only going to stderr, which is invisible in a
    // packaged app. Tokio's own task boundary already isolates a panic inside
    // an async command/spawned task from bringing down the whole process;
    // this hook adds the "detect and log" half of that story, which was
    // previously entirely absent.
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
        // TASK-DESK-002: native macOS notifications (UNUserNotificationCenter).
        .plugin(tauri_plugin_notification::init())
        // Doc 30 TASK-API-004: read-only, capability-scoped (tauri.conf.json's
        // `fs:allow-read-file`) -- lets the frontend read the bytes of a
        // dialog-selected statement PDF so `statements_upload` can be sent
        // real file content, matching Document 19 §9.1's actual contract.
        .plugin(tauri_plugin_fs::init())
        // TASK-DESK-010: "Launch at Login" -- a real macOS Launch Agent
        // (not a mere app-side preference), registered/removed via this
        // plugin only in response to an explicit user toggle.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // TASK-DESK-010: "Continue syncing when app is closed." Disabled by
        // default (`read_background_sync_enabled` defaults to `false`), the
        // window's close ("red traffic light") button quits the process
        // normally -- Tauri's own default when the last window closes.
        // Enabled, the close is intercepted (hidden + Dock icon hidden)
        // instead, keeping the already-independent background workers
        // (polling/queues/reconciliation) running. The Quit menu/tray items
        // bypass this entirely (`AppHandle::exit`, no `CloseRequested`).
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
            // TASK-DESK-001: the native macOS application menu bar. Built and
            // attached before anything else so a working Quit item exists
            // even if a later fatal-startup dialog (DB init failure, etc.)
            // is the only other thing the user sees.
            let app_menu = crate::menu::build_menu(app.handle())?;
            app.set_menu(app_menu)?;
            app.on_menu_event(|app_handle, event| {
                crate::menu::handle_menu_event(app_handle, event);
            });

            // I11 fix (Doc 26 T-10): verify the running binary's code signature
            // before doing anything else. Release-build-only — local dev builds
            // are typically unsigned/ad-hoc signed and would always fail this.
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

            // TASK-SETUP-006: RAM check must never block startup — runs
            // before DB init, which can itself fail/exit in several ways.
            crate::startup::check_ram_and_set_llm_eligibility(&app.handle().clone());

            // Setup DB in app data dir
            let app_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from(".dinero"));
            std::fs::create_dir_all(&app_dir).unwrap();

            crate::crash_reporter::init(app_dir.clone());
            app.manage(crate::feedback::FeedbackManager::new(app_dir.clone()));

            // TASK-DB-001: Document 18 §7.2 names the file `finance.db`
            // (was `data.db` — a drift already visible in the mismatched
            // `finance.db.bak.*` backup naming migrations.rs already used).
            let db_path = app_dir.join("finance.db");

            // TASK-DB-021: peek at the hardware-UUID marker before init_db
            // consumes/updates it, so a successful open below can show the
            // non-blocking "Database migrated to new Mac" toast Document 30
            // describes — without threading a migration flag through
            // init_db's own return type.
            let looks_like_hardware_migration =
                crate::db::crypto::hw_uuid_marker_indicates_migration(&app_dir);

            // Initialize SQLCipher database and run migrations.
            // Handle key-mismatch separately so the user sees a clear dialog
            // (with recovery instructions) rather than a raw panic.
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
                    
                    // Restore connected_accounts if they were backed up during "Delete My Data"
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
                    // best-effort dialog; ignore errors (dialog plugin may not be fully ready)
                    let _ = app.dialog()
                        .message(msg)
                        .title("Dinero — Database Key Mismatch")
                        .kind(tauri_plugin_dialog::MessageDialogKind::Error)
                        .blocking_show();
                    std::process::exit(1);
                }
                // G3 fix: a dedicated Keychain-access-denial screen — previously
                // this fell through to the generic "could not start" dialog,
                // giving no indication that granting Keychain access would fix it.
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
                // Doc 18 §12.1 (C18 fix): a failed migration doesn't just exit —
                // a pre-migration backup was already taken, so offer a one-click
                // rollback to it before giving up.
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
                // I6 fix: PRAGMA integrity_check found corruption — fail closed
                // and offer to restore the daily backup rather than silently
                // continuing to operate on a corrupt database.
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

            // I12 fix: finance.db contains a user's full financial history —
            // restrict it to owner-only read/write, matching the Keychain-only
            // secrets posture elsewhere in the app. Best-effort: a permissions
            // failure here shouldn't block startup, but is worth logging loudly.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Err(e) = std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600)) {
                    tracing::warn!("Failed to chmod 600 {}: {}", db_path.display(), e);
                }
            }

            let pool_clone = pool.clone();
            app.manage(pool);

            // TASK-AUTH-005: ensure an active session row exists for this
            // device, storing its id only in Tauri's managed in-memory
            // state (never sent to React, never persisted outside the
            // `sessions` table itself). Best-effort: a session-establishment
            // hiccup should not block startup — `current_session_id()`
            // simply returns `None` until the next successful call.
            app.manage(crate::auth::session::SessionState::default());
            // TASK-AUTH-014: in-memory-only incident counters, registered
            // once for the lifetime of this run.
            app.manage(crate::security::incident_response::IncidentMonitor::default());
            // TASK-DESK-003: single aggregated registry of long-running
            // background tasks, backing the global background-task indicator.
            app.manage(crate::background_tasks::indicator::BackgroundTaskRegistry::default());
            // Per-model-id cancellation tokens for in-progress local LLM
            // downloads (Settings' model picker Cancel button).
            app.manage(crate::llm_manager::DownloadRegistry::default());
            {
                let pool_for_session = pool_clone.clone();
                let session_state_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let state = session_state_handle.state::<crate::auth::session::SessionState>();
                    if let Err(e) = crate::auth::session::ensure_active_session(&pool_for_session, state.inner()).await {
                        tracing::warn!("Failed to establish local session: {}", e);
                    }
                });
            }

            // TASK-DESK-004: proactive permission-state check at launch --
            // by this point DB init has already succeeded (Keychain access
            // is therefore already confirmed), but this still runs the real
            // check-and-emit path so a later mid-session Keychain revocation
            // check (the daily maintenance loop, below) and this one share
            // the exact same code path and event contract.
            crate::permissions::macos_permissions::check_permissions_at_launch(&app.handle().clone());

            // TASK-DESK-002: request native-notification permission only if
            // the user has already passed the onboarding network-disclosure
            // screen -- never proactively at cold launch before that. A
            // no-op (returns immediately) on every launch until that
            // consent event exists.
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

            // In-memory-only holding area for PDF bytes blocked on Statement
            // Instrument Gate confirmation (C2 fix) — never written to disk.

            // Spawn the two isolated ingestion queues (Doc 15 §2 principle 7, §5;
            // Doc 12 §6.2a/§7.2) before any producer (polling/historical scan/manual
            // upload) can start pushing jobs onto them.
            let queue_handles = crate::ingestion::queues::spawn_queues(
                app.handle().clone(),
                pool_clone.clone(),
            );
            app.manage(queue_handles);

            let pool_for_cleanup = pool_clone.clone();
            let app_data_dir = app_dir.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = crate::statements::pdf_storage::cleanup_expired_pdfs(&app_data_dir, &pool_for_cleanup).await {
                    tracing::error!("Failed to cleanup expired PDFs: {}", e);
                }
            });

            let pool_for_polling = pool_clone.clone();
            let app_handle_for_polling = app.handle().clone();
            let cancel_token = tokio_util::sync::CancellationToken::new();
            app.manage(cancel_token.clone());

            // TASK-DESK-010: the battery-aware polling-interval policy this
            // task frames as the resolution to Document 16 §20 OQ-01 --
            // shared state the polling loop below reads every cycle.
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

            let backup_dir = app_dir.join("backups");
            std::fs::create_dir_all(&backup_dir).unwrap();
            // J3 fix: same directory family, one file per calendar year
            // (finance_archive_YYYY.db) — resolved once here since deriving
            // it needs a Keychain round-trip, not worth repeating every loop tick.
            let archive_dir = app_dir.join("archives");
            let archive_db_key = crate::db::crypto::derive_database_key().ok();
            let pool_for_backup = pool_clone.clone();
            let app_handle_for_backup = app.handle().clone();
            let db_path_for_backup = db_path.clone();
            tauri::async_runtime::spawn(async move {
                // Background loop for backups
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(60 * 60 * 24)).await; // 24 hours
                    tracing::info!("Running daily encrypted SQLite backup background task...");

                    // Doc 28 §4.2/§4.8: the daily backup is a single rolling file
                    // (finance.db.daily.bak), silently overwritten each day — not
                    // one uniquely-timestamped file per day accumulating forever.
                    // VACUUM INTO errors if its target already exists, so write to
                    // a temp path first and atomically rename over the previous
                    // backup on success, rather than deleting it up front (which
                    // would leave a window with no valid backup at all if this
                    // process were interrupted mid-VACUUM).
                    let backup_file = backup_dir.join("finance.db.daily.bak");
                    let backup_tmp_file = backup_dir.join("finance.db.daily.bak.tmp");
                    let _ = std::fs::remove_file(&backup_tmp_file);
                    let backup_tmp_file_str = backup_tmp_file.to_string_lossy().to_string();

                    if let Ok(conn) = pool_for_backup.get().await {
                        let res = conn
                            .interact(move |c| {
                                // VACUUM INTO creates a transactionally consistent, encrypted copy
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
                                        // Doc 30 TASK-OPS-002: "a scheduled verification step
                                        // loads a recent backup in a temporary sandbox to
                                        // confirm it opens cleanly, preventing silent backup
                                        // rot" -- opens the just-written backup fresh (a
                                        // separate connection, not the one that wrote it) and
                                        // runs a real integrity_check immediately, so rot is
                                        // caught the same day it happens, not months later
                                        // when a restore is actually needed.
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
                                        // TASK-DB-020: log completion + notify Settings so it
                                        // can show the last-backup timestamp.
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

                    // TASK-DB-019 steps 1/3/4: corruption check, bounded
                    // incremental vacuum, and a size warning once finance.db
                    // exceeds 2GB. Run on the same daily cadence, after the
                    // backup/retention/archive steps above.
                    if let Ok(conn) = pool_for_backup.get().await {
                        let app_handle_for_integrity = app_handle_for_backup.clone();
                        let integrity_ok = conn
                            .interact(move |c| {
                                crate::db::maintenance::check_integrity_and_report(c, &app_handle_for_integrity)
                            })
                            .await;
                        // TASK-AUTH-014: a corrupt database is itself the
                        // incident-response trigger this task names
                        // ("PRAGMA integrity_check failures") — fires on the
                        // very first occurrence (see IncidentMonitor's
                        // per-trigger threshold).
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
                    if let Ok(conn) = pool_for_backup.get().await {
                        let _ = conn
                            .interact(|c| crate::db::maintenance::run_incremental_vacuum(c))
                            .await;
                    }
                    let _ = crate::db::maintenance::check_db_size_warning(&app_handle_for_backup, &db_path_for_backup);

                    // J2 fix (Doc 28 §4.2 row 1): nulls raw_payload_json/
                    // raw_row_json on matched records older than 90 days —
                    // previously cited as a compliance control with no
                    // implementing code at all. Runs on the same daily cadence
                    // as the backup above, after it, so the backup still
                    // captures the pre-sweep state for one more day.
                    if let Ok(conn) = pool_for_backup.get().await {
                        let _ = conn
                            .interact(|c| crate::db::retention::sweep_raw_payloads(c))
                            .await;
                    }

                    // Doc-30-style optimization #5: purges `ignored_messages`
                    // rows past their 30-day TTL. Runs on the same daily
                    // cadence as the raw-payload sweep above — no dedicated
                    // background task needed for a monthly-scale cleanup.
                    if let Ok(conn) = pool_for_backup.get().await {
                        let _ = conn
                            .interact(|c| crate::db::ignored_messages::purge_expired(c))
                            .await;
                    }

                    // J3 fix (Doc 28 §4.2): copies transactions older than 5
                    // years into finance_archive_YYYY.db — additive only, see
                    // db::retention::archive_old_transactions doc comment.
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

                    // TASK-DESK-002: native "bill due in 3 days" reminder --
                    // runs once per day alongside the other daily maintenance
                    // work above; `is_three_days_before_due`'s exact-equality
                    // check (not a range) makes this self-deduplicating, so
                    // no separate "already reminded" state is needed.
                    if let Ok(conn) = pool_for_backup.get().await {
                        let today = chrono::Utc::now().date_naive();
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

                    // TASK-DESK-004: re-checks permission state on the same
                    // daily cadence as the rest of this maintenance loop --
                    // the "proactive, ongoing" half of detection, covering a
                    // Keychain access revoked (or notification permission
                    // changed) mid-session, not just at cold launch.
                    crate::permissions::macos_permissions::check_permissions_at_launch(
                        &app_handle_for_backup,
                    );
                }
            });

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

            // TASK-OPS-003: a cheap (single `SELECT 1`) liveness re-check
            // every 60s. Startup itself already fails closed with a blocking
            // dialog (see the `db::init_db` match above) — this catches the
            // case where the pool goes unresponsive *after* a successful
            // start (disk unmounted, pool exhausted), which is the only way
            // "backend startup failure" is an ongoing, alertable condition
            // rather than a one-time launch-time check.
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

            // TASK-DESK-005: checks once on launch, then every ~6 hours
            // while running (Document 16 §9.1). Skipped in debug builds --
            // the updater endpoint (tauri.conf.json) points at GitHub
            // Releases, which doesn't exist until the first release ships,
            // so every dev run would otherwise log a spurious error on a
            // fixed interval.
            app.manage(crate::updater::PendingUpdate::default());
            if !cfg!(debug_assertions) {
                crate::updater::spawn_update_check_loop(app.handle().clone());
            }

            // TASK-DESK-008: initialize the menu bar extra to match the
            // persisted setting, then keep the Dock badge and (if enabled)
            // the tray summary refreshed periodically. 30s, not instant --
            // a Dock badge doesn't need sub-second latency, and this avoids
            // needing to hook every single reconciliation-state-mutating
            // call site individually (a fragile, easy-to-miss-one approach
            // this run has repeatedly found bugs from elsewhere).
            {
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
        .invoke_handler(commands::get_handlers())
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
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
