pub mod network_client;
pub mod commands;
pub mod db;
pub mod error;
pub mod extraction;
pub mod ingestion;
pub mod ipc;
pub mod reconciliation;
pub mod statements;
pub mod licensing;
pub mod llm_manager;
pub mod crash_reporter;
pub mod diagnostics;
pub mod feedback;
pub mod integrity;
pub mod startup;

use std::path::PathBuf;
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

/// Doc 28 §4.2 (J4 fix): default retention for rotated `app-logs.log.*`
/// files, overridable via `DINERO_LOG_RETENTION_DAYS` (the doc calls the
/// window "configurable").
const DEFAULT_LOG_RETENTION_DAYS: u64 = 15;

/// Deletes rotated log files older than the retention window. Best-effort —
/// a failure here should never block startup.
fn prune_old_logs(log_dir: &std::path::Path) {
    let retention_days = std::env::var("DINERO_LOG_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_LOG_RETENTION_DAYS);
    let max_age = std::time::Duration::from_secs(retention_days * 24 * 60 * 60);

    let entries = match std::fs::read_dir(log_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Failed to read log directory for pruning: {}", e);
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let is_rotated_log = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("app-logs.log"))
            .unwrap_or(false);
        if !is_rotated_log {
            continue;
        }

        let age = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|modified| modified.elapsed().ok());

        if let Some(age) = age {
            if age > max_age {
                if let Err(e) = std::fs::remove_file(&path) {
                    tracing::warn!("Failed to prune old log file {:?}: {}", path, e);
                } else {
                    tracing::info!("Pruned log file older than {} days: {:?}", retention_days, path);
                }
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut log_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    if log_dir.ends_with("src-tauri") {
        log_dir = log_dir.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
    }
    // J4 fix (Doc 28 §4.2): the debug log previously never rotated or
    // expired at all (`rolling::never` into one ever-growing file). Rotates
    // daily and prunes files older than the configurable retention window
    // (15 days by default, matching the documented default).
    let file_appender = tracing_appender::rolling::daily(&log_dir, "app-logs.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    Box::leak(Box::new(_guard));
    prune_old_logs(&log_dir);

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().with_ansi(true))
        .with(tracing_subscriber::fmt::layer().with_writer(non_blocking).with_ansi(false))
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
        .setup(|app| {
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
                        "Migration failed (backup at {}): {}",
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

            // In-memory-only holding area for PDF bytes blocked on Statement
            // Instrument Gate confirmation (C2 fix) — never written to disk.
            let pending_statement_bytes = crate::statements::pending_bytes::PendingStatementBytes::default();
            app.manage(pending_statement_bytes.clone());

            // Spawn the two isolated ingestion queues (Doc 15 §2 principle 7, §5;
            // Doc 12 §6.2a/§7.2) before any producer (polling/historical scan/manual
            // upload) can start pushing jobs onto them.
            let queue_handles = crate::ingestion::queues::spawn_queues(
                app.handle().clone(),
                pool_clone.clone(),
                pending_statement_bytes,
            );
            app.manage(queue_handles);

            let pool_for_polling = pool_clone.clone();
            let app_handle_for_polling = app.handle().clone();
            let cancel_token = tokio_util::sync::CancellationToken::new();
            app.manage(cancel_token.clone());

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
                        let _ = conn
                            .interact(move |c| {
                                crate::db::maintenance::check_integrity_and_report(c, &app_handle_for_integrity)
                            })
                            .await;
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
                }
            });

            let pool_for_licensing = pool_clone.clone();
            tauri::async_runtime::spawn(async move {
                crate::licensing::worker::start_background_validation(
                    pool_for_licensing,
                    "https://api.dinero-app.com".to_string(),
                )
                .await;
            });


            Ok(())
        })
        .invoke_handler(commands::get_handlers())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
#[cfg(test)]
mod phase8_telemetry_tests;
#[cfg(test)]
mod phase9_security_tests;
#[cfg(test)]
mod phase9_rigorous_tests;
#[cfg(test)]
mod phase11_llm_tests;
#[cfg(test)]
mod llm_manager_tests;
#[cfg(test)]
mod phase11_rigorous_tests;
#[cfg(test)]
mod phase10_quality_gates_tests;
#[cfg(test)]
mod phase10_rigorous_tests;
