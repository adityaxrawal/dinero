//! TASK-DESK-008 (Doc 30 §12, Doc 29 §14): the Dock icon badge
//! (`analytics_pending_review_count`, TASK-API-006) and the optional macOS
//! menu bar extra (status item) -- distinct from the app's own application
//! menu bar (TASK-DESK-001). Also implements the "Hide Dock icon, show
//! only menu bar extra" mode via `NSApplication.setActivationPolicy`.

use std::path::{Path, PathBuf};
use tauri::menu::{Menu, MenuBuilder, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{ActivationPolicy, AppHandle, Manager, Runtime};

pub const MENU_BAR_EXTRA_ID_OPEN_DASHBOARD: &str = "tray_open_dashboard";
pub const MENU_BAR_EXTRA_ID_OPEN_REVIEW_QUEUE: &str = "tray_open_review_queue";
pub const MENU_BAR_EXTRA_ID_QUIT: &str = "tray_quit";

const MENU_BAR_EXTRA_SETTING_FILE: &str = "menu_bar_extra_enabled";
const TRAY_ICON_ID: &str = "dinero-menu-bar-extra";

// ---------------------------------------------------------------------
// Dock icon badge
// ---------------------------------------------------------------------

/// Pure: what Dock badge label a given pending-review count implies.
/// `None` clears the badge -- macOS shows no badge at all for `None`/`Some(0)`.
/// Doc 30 TASK-DESK-008 acceptance: `test_dock_badge_reflects_pending_review_count`,
/// `test_dock_badge_clears_at_zero`.
pub fn badge_count_for(pending_review_count: i64) -> Option<i64> {
    if pending_review_count > 0 {
        Some(pending_review_count)
    } else {
        None
    }
}

/// Updates the Dock icon badge on the main window. Best-effort: no main
/// window (e.g. a test `AppHandle`) or a platform without Dock badges must
/// never surface as an application error.
///
/// Dispatched via `run_on_main_thread` -- this is called from the periodic
/// refresh loop (`lib.rs`) inside a `deadpool_sqlite` `conn.interact`
/// closure, which runs on a blocking-pool thread, never the main thread.
/// AppKit window/status-bar APIs are main-thread-only; see this file's
/// other `run_on_main_thread` uses for the crash this exact pattern causes
/// when skipped.
pub fn update_dock_badge<R: Runtime>(app: &AppHandle<R>, pending_review_count: i64) {
    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(window) = app_handle.get_webview_window("main") else {
            return;
        };
        if let Err(e) = window.set_badge_count(badge_count_for(pending_review_count)) {
            tracing::warn!("Failed to update Dock badge: {}", e);
        }
    });
}

// ---------------------------------------------------------------------
// Menu bar extra (status item)
// ---------------------------------------------------------------------

/// Pure: the quick-summary text shown as the status item's title (macOS
/// tray icons can show text directly in the menu bar, not just an icon).
/// `month_to_date_spend` is already-converted display rupees (the same
/// value `dashboard_summary`'s `DashboardSummary.month_to_date_spend`
/// field holds) -- reused directly rather than round-tripping back
/// through `amount_minor` a second time just to convert it back to
/// display rupees here.
pub fn format_tray_summary(
    month_to_date_spend: f64,
    pending_review_count: i64,
    upcoming_bills_count: i64,
) -> String {
    let mut parts = vec![format!("₹{:.0}", month_to_date_spend)];
    if pending_review_count > 0 {
        parts.push(format!("{} pending", pending_review_count));
    }
    if upcoming_bills_count > 0 {
        parts.push(format!("{} due", upcoming_bills_count));
    }
    parts.join(" · ")
}

/// Builds the menu bar extra's dropdown menu -- a small, fixed set of
/// quick actions, not the full application menu (TASK-DESK-001's
/// `menu::build_menu` is a completely separate menu).
fn build_tray_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    MenuBuilder::new(app)
        .item(&MenuItem::with_id(
            app,
            MENU_BAR_EXTRA_ID_OPEN_DASHBOARD,
            "Open Dinero",
            true,
            None::<&str>,
        )?)
        .item(&MenuItem::with_id(
            app,
            MENU_BAR_EXTRA_ID_OPEN_REVIEW_QUEUE,
            "Review Pending Items…",
            true,
            None::<&str>,
        )?)
        .separator()
        .quit()
        .build()
}

/// Builds and registers the tray icon. Called only when the menu bar extra
/// is enabled (`read_menu_bar_extra_enabled`) -- the app runs perfectly
/// well with no tray icon at all when it's off, matching Doc 30's "optional"
/// framing.
pub fn build_tray_icon<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<TrayIcon<R>> {
    let menu = build_tray_menu(app)?;
    TrayIconBuilder::with_id(TRAY_ICON_ID)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .title(format_tray_summary(0.0, 0, 0))
        .tooltip("Dinero")
        .icon(
            app.default_window_icon()
                .cloned()
                .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".into()))?,
        )
        .icon_as_template(true)
        .build(app)
}

/// Updates an already-built tray icon's summary text.
pub fn update_tray_summary<R: Runtime>(
    tray: &TrayIcon<R>,
    month_to_date_spend: f64,
    pending_review_count: i64,
    upcoming_bills_count: i64,
) {
    let summary = format_tray_summary(
        month_to_date_spend,
        pending_review_count,
        upcoming_bills_count,
    );
    if let Err(e) = tray.set_title(Some(&summary)) {
        tracing::warn!("Failed to update menu bar extra summary: {}", e);
    }
}

/// Looks up the tray icon by this module's own id and updates it if (and
/// only if) the menu bar extra is currently enabled -- a safe no-op
/// otherwise, so callers (the periodic refresh loop) don't need to know
/// the tray icon's id or track whether it currently exists themselves.
pub fn update_tray_summary_if_present<R: Runtime>(
    app: &AppHandle<R>,
    month_to_date_spend: f64,
    pending_review_count: i64,
    upcoming_bills_count: i64,
) {
    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(tray) = app_handle.tray_by_id(TRAY_ICON_ID) {
            update_tray_summary(
                &tray,
                month_to_date_spend,
                pending_review_count,
                upcoming_bills_count,
            );
        }
    });
}

/// Builds (if not already present) or removes the tray icon to match
/// `enabled`, using Tauri's own tray registry (`tray_by_id`/`remove_tray_by_id`)
/// rather than tracking a separate handle -- idempotent, so it's safe to
/// call both at startup (to match the persisted setting) and from the
/// settings-toggle command.
///
/// Real crash, not just a theoretical race: `settings_set_menu_bar_extra_enabled`
/// (the Settings toggle's command handler) runs on Tauri's async command
/// runtime, not the main thread. `TrayIcon::new` (the enable path, via
/// `build_tray_icon`) checks for the main thread itself and fails soft
/// (`Error::NotMainThread`, caught below) -- but `remove_tray_by_id`'s
/// underlying `NSStatusBar.removeStatusItem` call (the disable path) has no
/// such guard at all (`tray-icon` crate v0.24.1,
/// `platform_impl::macos::TrayIcon::remove`) and calls straight into AppKit
/// unconditionally. AppKit status-bar mutations off the main thread crash
/// the process -- exactly the "enabling works, disabling crashes" asymmetry
/// this was reported as. `run_on_main_thread` schedules both branches onto
/// the actual main thread instead of assuming the caller is already there.
pub fn apply_menu_bar_extra_runtime_state<R: Runtime>(app: &AppHandle<R>, enabled: bool) {
    let app_handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || {
        if enabled {
            if app_handle.tray_by_id(TRAY_ICON_ID).is_none() {
                if let Err(e) = build_tray_icon(&app_handle) {
                    tracing::warn!("Failed to build menu bar extra: {}", e);
                }
            }
        } else {
            app_handle.remove_tray_by_id(TRAY_ICON_ID);
        }
    }) {
        tracing::warn!(
            "Failed to schedule menu bar extra toggle on main thread: {}",
            e
        );
    }
}

/// Applies "Hide Dock icon, show only menu bar extra" mode.
/// `NSApplicationActivationPolicyAccessory` (Tauri's `ActivationPolicy::Accessory`)
/// hides the Dock icon while keeping the app fully running; `Regular`
/// (the default) shows it normally.
pub fn apply_dock_visibility<R: Runtime>(app: &AppHandle<R>, hide_dock_icon: bool) {
    let policy = if hide_dock_icon {
        ActivationPolicy::Accessory
    } else {
        ActivationPolicy::Regular
    };
    if let Err(e) = app.set_activation_policy(policy) {
        tracing::warn!("Failed to set Dock activation policy: {}", e);
    }
}

// ---------------------------------------------------------------------
// Persisted setting: menu bar extra enabled (Doc 30: "toggleable in Settings")
// ---------------------------------------------------------------------

fn menu_bar_extra_settings_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(MENU_BAR_EXTRA_SETTING_FILE)
}

/// Doc 30 TASK-DESK-008 acceptance: `test_menu_bar_extra_toggle_persists_setting`.
/// A single-line marker file rather than a new schema column/table -- this
/// is one boolean UI preference, not data that belongs in `local_profile`
/// (whose existing JSON column, `limit_thresholds`, is already a
/// different, already-resolved concept -- TASK-API-008 -- not a spare
/// place to stash unrelated settings). Defaults to `false` (disabled,
/// Dock icon shown normally) when the file doesn't exist yet.
pub fn read_menu_bar_extra_enabled(app_data_dir: &Path) -> bool {
    std::fs::read_to_string(menu_bar_extra_settings_path(app_data_dir))
        .map(|s| s.trim() == "true")
        .unwrap_or(false)
}

pub fn write_menu_bar_extra_enabled(app_data_dir: &Path, enabled: bool) -> std::io::Result<()> {
    std::fs::write(
        menu_bar_extra_settings_path(app_data_dir),
        if enabled { "true" } else { "false" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Doc 30 TASK-DESK-008 acceptance: `test_dock_badge_reflects_pending_review_count`.
    #[test]
    fn test_dock_badge_reflects_pending_review_count() {
        assert_eq!(badge_count_for(1), Some(1));
        assert_eq!(badge_count_for(5), Some(5));
        assert_eq!(badge_count_for(42), Some(42));
    }

    /// Doc 30 TASK-DESK-008 acceptance: `test_dock_badge_clears_at_zero`.
    #[test]
    fn test_dock_badge_clears_at_zero() {
        assert_eq!(badge_count_for(0), None);
        assert_eq!(
            badge_count_for(-1),
            None,
            "a negative count is not meaningful; must also clear"
        );
    }

    /// Doc 30 TASK-DESK-008 acceptance: `test_menu_bar_extra_toggle_persists_setting`.
    #[test]
    fn test_menu_bar_extra_toggle_persists_setting() {
        let dir = temp_dir();
        assert!(
            !read_menu_bar_extra_enabled(&dir),
            "must default to disabled before any setting has ever been written"
        );

        write_menu_bar_extra_enabled(&dir, true).unwrap();
        assert!(read_menu_bar_extra_enabled(&dir));

        write_menu_bar_extra_enabled(&dir, false).unwrap();
        assert!(!read_menu_bar_extra_enabled(&dir));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_format_tray_summary() {
        assert_eq!(format_tray_summary(0.0, 0, 0), "₹0");
        assert_eq!(format_tray_summary(24500.60, 0, 0), "₹24501");
        assert_eq!(format_tray_summary(24500.0, 3, 0), "₹24500 · 3 pending");
        assert_eq!(
            format_tray_summary(24500.0, 3, 2),
            "₹24500 · 3 pending · 2 due"
        );
        assert_eq!(format_tray_summary(24500.0, 0, 1), "₹24500 · 1 due");
    }

    #[test]
    fn test_update_dock_badge_is_a_safe_noop_without_a_main_window() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone();
        // MockRuntime never has a real "main" window -- must not panic.
        update_dock_badge(&app, 5);
    }
}
