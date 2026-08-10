//! macOS menu-bar extra and dock badge.
//!
//! Surfaces pending review counts and headline figures while the main window is
//! closed, so the app remains informative without needing to be open.
use std::path::{Path, PathBuf};
use tauri::menu::{Menu, MenuBuilder, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{ActivationPolicy, AppHandle, Manager, Runtime};

pub const MENU_BAR_EXTRA_ID_OPEN_DASHBOARD: &str = "tray_open_dashboard";
pub const MENU_BAR_EXTRA_ID_OPEN_REVIEW_QUEUE: &str = "tray_open_review_queue";
pub const MENU_BAR_EXTRA_ID_QUIT: &str = "tray_quit";

const MENU_BAR_EXTRA_SETTING_FILE: &str = "menu_bar_extra_enabled";
const TRAY_ICON_ID: &str = "dinero-menu-bar-extra";

/// Badge count to display, or None when there is nothing pending.
///
/// None rather than zero, so the badge disappears entirely instead of showing a
/// zero that reads as an unread item.
pub fn badge_count_for(pending_review_count: i64) -> Option<i64> {
    if pending_review_count > 0 {
        Some(pending_review_count)
    } else {
        None
    }
}

/// Updates the dock badge with the pending review count.
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

/// Formats the tray's at-a-glance summary line.
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

/// Builds the tray menu.
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

/// Creates the menu-bar tray icon.
pub fn build_tray_icon<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<TrayIcon<R>> {
    let menu = build_tray_menu(app)?;
    TrayIconBuilder::with_id(TRAY_ICON_ID)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .title(format_tray_summary(0.0, 0, 0))
        .tooltip("Dinero")
        .icon(tauri::image::Image::from_bytes(include_bytes!(
            "../../icons/tray@2x.png"
        ))?)
        .icon_as_template(true)
        .build(app)
}

/// Updates the tray's summary figures.
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

/// Updates the tray only if it is currently shown.
///
/// The extra is optional, so this avoids constructing figures for a tray the user
/// has turned off.
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

/// Shows or hides the menu-bar extra at runtime.
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

/// Shows or hides the dock icon.
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

/// Path of the menu-bar extra setting.
fn menu_bar_extra_settings_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(MENU_BAR_EXTRA_SETTING_FILE)
}

/// Whether the menu-bar extra is enabled.
pub fn read_menu_bar_extra_enabled(app_data_dir: &Path) -> bool {
    std::fs::read_to_string(menu_bar_extra_settings_path(app_data_dir))
        .map(|s| s.trim() == "true")
        .unwrap_or(false)
}

/// Persists the menu-bar extra preference.
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

    #[test]
    fn test_dock_badge_reflects_pending_review_count() {
        assert_eq!(badge_count_for(1), Some(1));
        assert_eq!(badge_count_for(5), Some(5));
        assert_eq!(badge_count_for(42), Some(42));
    }

    #[test]
    fn test_dock_badge_clears_at_zero() {
        assert_eq!(badge_count_for(0), None);
        assert_eq!(
            badge_count_for(-1),
            None,
            "a negative count is not meaningful; must also clear"
        );
    }

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
        update_dock_badge(&app, 5);
    }
}
