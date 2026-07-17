//! Doc 30 TASK-DESK-010 (Doc 29 §12): "Launch at Login" + "Continue syncing
//! when app is closed" Settings toggles, plus the battery-aware background
//! polling-interval policy this task's own source material frames as the
//! resolution to Document 16 §20 OQ-01 -- already reflected as [RESOLVED]
//! in the currently-published Document 16 (v1.8, Documentation Audit
//! finding H-01) and Doc 30 §21.2, so there is no open conflict left to
//! flag here.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Manager, Runtime};

// ---------------------------------------------------------------------
// Launch at Login
// ---------------------------------------------------------------------

/// Thin abstraction over the real OS-level login-item registration
/// (`tauri_plugin_autostart`, backed by a real Launch Agent plist under
/// this Mac's `~/Library/LaunchAgents`) so the orchestration logic below
/// is unit-testable without actually mutating this machine's real login
/// items -- the same "never touch the real system in a unit test"
/// convention already established for Keychain access (`db::crypto`).
pub trait LoginItemController {
    fn enable(&self) -> Result<(), String>;
    fn disable(&self) -> Result<(), String>;
    fn is_enabled(&self) -> Result<bool, String>;
}

/// The real controller backing `settings_set_launch_at_login`. Deliberately
/// not exercised in `cargo test` (see `LoginItemController`'s doc comment);
/// `apply_launch_at_login`'s orchestration is tested against a fake instead.
pub struct TauriAutoLaunchController<'a, R: Runtime> {
    app: &'a AppHandle<R>,
}

impl<'a, R: Runtime> TauriAutoLaunchController<'a, R> {
    pub fn new(app: &'a AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<'a, R: Runtime> LoginItemController for TauriAutoLaunchController<'a, R> {
    fn enable(&self) -> Result<(), String> {
        use tauri_plugin_autostart::ManagerExt;
        self.app.autolaunch().enable().map_err(|e| e.to_string())
    }

    fn disable(&self) -> Result<(), String> {
        use tauri_plugin_autostart::ManagerExt;
        self.app.autolaunch().disable().map_err(|e| e.to_string())
    }

    fn is_enabled(&self) -> Result<bool, String> {
        use tauri_plugin_autostart::ManagerExt;
        self.app.autolaunch().is_enabled().map_err(|e| e.to_string())
    }
}

/// Doc 30 TASK-DESK-010 acceptance: `test_launch_at_login_toggle_registers_login_item`.
/// Pure orchestration over any `LoginItemController` -- tested here against
/// a fake; the real controller's actual system effect (writing/removing the
/// Launch Agent plist) is exercised only by manual QA, the same limitation
/// already documented for every other real-OS-side-effect path this run has
/// touched (Keychain writes, code signing/notarization).
pub fn apply_launch_at_login<C: LoginItemController>(
    controller: &C,
    enabled: bool,
) -> Result<(), String> {
    if enabled {
        controller.enable()
    } else {
        controller.disable()
    }
}

// ---------------------------------------------------------------------
// "Continue syncing when app is closed" (background-only mode)
// ---------------------------------------------------------------------

const BACKGROUND_SYNC_SETTING_FILE: &str = "background_sync_enabled";

fn background_sync_settings_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(BACKGROUND_SYNC_SETTING_FILE)
}

/// Defaults to `false` -- disabled, matching the pre-existing behavior of
/// fully quitting the process on window close until a user opts in.
pub fn read_background_sync_enabled(app_data_dir: &Path) -> bool {
    std::fs::read_to_string(background_sync_settings_path(app_data_dir))
        .map(|s| s.trim() == "true")
        .unwrap_or(false)
}

pub fn write_background_sync_enabled(app_data_dir: &Path, enabled: bool) -> std::io::Result<()> {
    std::fs::write(
        background_sync_settings_path(app_data_dir),
        if enabled { "true" } else { "false" },
    )
}

/// Pure: whether the main window's close request should be intercepted
/// (hidden, kept running) rather than left to quit the process normally.
pub fn should_prevent_close(background_sync_enabled: bool) -> bool {
    background_sync_enabled
}

/// Effectful, wired into the app-level `on_window_event` handler in
/// `lib.rs::run`. When "continue syncing when closed" is enabled, hides
/// the window and the Dock icon instead of letting the close proceed --
/// the ingestion queues/polling loop/reconciliation worker spawned in
/// `lib.rs::run` are already fully independent of window lifetime, so
/// nothing else needs pausing or resuming here. When disabled, does
/// nothing: the close proceeds and Tauri's default behavior (quit when the
/// last window closes) takes over, deferring to the next launch's
/// checkpoint-resume to catch up on missed history, per this task's spec.
/// The application menu/tray's own Quit items (`PredefinedMenuItem::quit`)
/// call `AppHandle::exit` directly and never raise `CloseRequested` at all,
/// so an explicit Quit always fully quits regardless of this setting.
pub fn handle_main_window_close_requested<R: Runtime>(
    window: &tauri::Window<R>,
    api: &tauri::CloseRequestApi,
    background_sync_enabled: bool,
) {
    if !should_prevent_close(background_sync_enabled) {
        return;
    }
    api.prevent_close();
    if let Err(e) = window.hide() {
        tracing::warn!("Failed to hide main window for background-only mode: {}", e);
    }
    crate::menu::status_item::apply_dock_visibility(window.app_handle(), true);
}

// ---------------------------------------------------------------------
// Battery-aware background polling interval
// ---------------------------------------------------------------------

/// Doc 30: "the normal 30-90s" cadence -- `ingestion::polling`'s loop
/// currently sleeps a fixed 60s per cycle (within that documented range);
/// this task increases that interval under the stated condition, it does
/// not change the normal-cadence value itself.
pub const NORMAL_POLL_INTERVAL_SECS: u64 = 60;
pub const LOW_BATTERY_POLL_INTERVAL_SECS: u64 = 300;

/// No document specifies an exact default charge threshold -- mirrors
/// macOS's own Low Power Mode prompt, which commonly triggers at 20%.
/// Configurable via `settings_set_low_battery_poll_threshold_percent`.
pub const DEFAULT_LOW_BATTERY_THRESHOLD_PERCENT: f32 = 20.0;

/// Pure decision. Doc 30 TASK-DESK-010 acceptance:
/// `test_polling_interval_increases_on_low_battery`.
pub fn effective_poll_interval_secs(
    background_only_mode_active: bool,
    on_battery: bool,
    battery_percent: Option<f32>,
    threshold_percent: f32,
) -> u64 {
    if !background_only_mode_active {
        return NORMAL_POLL_INTERVAL_SECS;
    }
    match (on_battery, battery_percent) {
        (true, Some(percent)) if percent < threshold_percent => LOW_BATTERY_POLL_INTERVAL_SECS,
        _ => NORMAL_POLL_INTERVAL_SECS,
    }
}

/// Shared, atomically-updated poll interval: read every cycle by
/// `ingestion::polling::start_polling_loop`, written by
/// `run_battery_aware_polling_interval_loop` below.
pub struct PollingIntervalState(AtomicU64);

impl Default for PollingIntervalState {
    fn default() -> Self {
        Self(AtomicU64::new(NORMAL_POLL_INTERVAL_SECS))
    }
}

impl PollingIntervalState {
    pub fn load_secs(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    pub fn store_secs(&self, secs: u64) {
        self.0.store(secs, Ordering::Relaxed);
    }
}

/// Real battery query. `None` on a desktop Mac with no battery (Mac
/// mini/Studio/iMac) or if the platform battery API is unavailable --
/// `effective_poll_interval_secs` treats that identically to "on AC power,"
/// which is the correct, safe default (never throttle polling on a machine
/// that has no battery to run low on).
pub fn read_battery_power_state() -> Option<(bool, f32)> {
    let manager = battery::Manager::new().ok()?;
    let mut batteries = manager.batteries().ok()?;
    let battery = batteries.next()?.ok()?;
    let on_battery = matches!(
        battery.state(),
        battery::State::Discharging | battery::State::Empty
    );
    let percent = battery.state_of_charge().get::<battery::units::ratio::percent>();
    Some((on_battery, percent))
}

const LOW_BATTERY_THRESHOLD_SETTING_FILE: &str = "low_battery_poll_threshold_percent";

fn low_battery_threshold_settings_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(LOW_BATTERY_THRESHOLD_SETTING_FILE)
}

pub fn read_low_battery_threshold_percent(app_data_dir: &Path) -> f32 {
    std::fs::read_to_string(low_battery_threshold_settings_path(app_data_dir))
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok())
        .unwrap_or(DEFAULT_LOW_BATTERY_THRESHOLD_PERCENT)
}

pub fn write_low_battery_threshold_percent(
    app_data_dir: &Path,
    threshold_percent: f32,
) -> std::io::Result<()> {
    std::fs::write(
        low_battery_threshold_settings_path(app_data_dir),
        threshold_percent.to_string(),
    )
}

/// Spawned once at startup (`lib.rs::run`). Refreshes `PollingIntervalState`
/// on its own 30s cadence, independent of the polling loop's own cycle --
/// only relevant while "continue syncing when closed" is enabled AND the
/// main window is currently hidden (i.e. actually operating in
/// background-only mode right now, not merely eligible to); with the
/// window visible, always resolves to the normal cadence regardless of
/// battery state, since a foreground app isn't the background-only
/// scenario Document 16 §20 OQ-01 concerns.
pub async fn run_battery_aware_polling_interval_loop<R: Runtime>(
    app: AppHandle<R>,
    cancel_token: tokio_util::sync::CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => break,
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                let Ok(app_dir) = app.path().app_data_dir() else { continue; };
                let background_sync_enabled = read_background_sync_enabled(&app_dir);
                let window_visible = app
                    .get_webview_window("main")
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(true);
                let background_only_mode_active = background_sync_enabled && !window_visible;

                let (on_battery, percent) = read_battery_power_state()
                    .map(|(on_battery, percent)| (on_battery, Some(percent)))
                    .unwrap_or((false, None));
                let threshold = read_low_battery_threshold_percent(&app_dir);

                let interval = effective_poll_interval_secs(
                    background_only_mode_active,
                    on_battery,
                    percent,
                    threshold,
                );
                app.state::<PollingIntervalState>().store_secs(interval);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dinero_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A fake controller recording calls, never touching the real system --
    /// exactly the substitution `LoginItemController`'s doc comment exists for.
    struct FakeLoginItemController {
        enabled: RefCell<bool>,
        enable_calls: RefCell<u32>,
        disable_calls: RefCell<u32>,
    }

    impl FakeLoginItemController {
        fn new(initial: bool) -> Self {
            Self {
                enabled: RefCell::new(initial),
                enable_calls: RefCell::new(0),
                disable_calls: RefCell::new(0),
            }
        }
    }

    impl LoginItemController for FakeLoginItemController {
        fn enable(&self) -> Result<(), String> {
            *self.enabled.borrow_mut() = true;
            *self.enable_calls.borrow_mut() += 1;
            Ok(())
        }
        fn disable(&self) -> Result<(), String> {
            *self.enabled.borrow_mut() = false;
            *self.disable_calls.borrow_mut() += 1;
            Ok(())
        }
        fn is_enabled(&self) -> Result<bool, String> {
            Ok(*self.enabled.borrow())
        }
    }

    /// Doc 30 TASK-DESK-010 acceptance: `test_launch_at_login_toggle_registers_login_item`.
    #[test]
    fn test_launch_at_login_toggle_registers_login_item() {
        let controller = FakeLoginItemController::new(false);
        assert!(!controller.is_enabled().unwrap());

        apply_launch_at_login(&controller, true).unwrap();
        assert!(controller.is_enabled().unwrap(), "toggling on must register the login item");
        assert_eq!(*controller.enable_calls.borrow(), 1);

        apply_launch_at_login(&controller, false).unwrap();
        assert!(!controller.is_enabled().unwrap(), "toggling off must remove the login item");
        assert_eq!(*controller.disable_calls.borrow(), 1);
    }

    #[test]
    fn test_background_sync_enabled_persists_and_defaults_to_false() {
        let dir = temp_dir();
        assert!(
            !read_background_sync_enabled(&dir),
            "must default to disabled -- fully quit on close until opted in"
        );

        write_background_sync_enabled(&dir, true).unwrap();
        assert!(read_background_sync_enabled(&dir));

        write_background_sync_enabled(&dir, false).unwrap();
        assert!(!read_background_sync_enabled(&dir));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Doc 30 TASK-DESK-010 acceptance: `test_background_only_mode_hides_dock_icon`.
    /// `apply_dock_visibility` (TASK-DESK-008) is the real effectful call --
    /// this exercises the decision that must trigger it: only when
    /// "continue syncing when closed" is enabled does closing the window
    /// hide the Dock icon; otherwise the close is left alone entirely.
    #[test]
    fn test_background_only_mode_hides_dock_icon() {
        assert!(
            should_prevent_close(true),
            "background sync enabled -- close must be intercepted, which is what leads to hiding the Dock icon"
        );
        assert!(
            !should_prevent_close(false),
            "background sync disabled -- close must proceed normally (process quits), no Dock-hiding involved"
        );
    }

    /// A window-less smoke test confirming `handle_main_window_close_requested`'s
    /// downstream `apply_dock_visibility` call is a safe no-op against a
    /// `MockRuntime` app with no real window -- mirrors
    /// `status_item::test_update_dock_badge_is_a_safe_noop_without_a_main_window`.
    #[test]
    fn test_apply_dock_visibility_safe_noop_under_mock_runtime() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone();
        crate::menu::status_item::apply_dock_visibility(&app, true);
        crate::menu::status_item::apply_dock_visibility(&app, false);
    }

    /// Doc 30 TASK-DESK-010 acceptance: `test_polling_interval_increases_on_low_battery`.
    #[test]
    fn test_polling_interval_increases_on_low_battery() {
        // Not in background-only mode at all -- always normal, regardless of battery.
        assert_eq!(
            effective_poll_interval_secs(false, true, Some(5.0), 20.0),
            NORMAL_POLL_INTERVAL_SECS
        );

        // Background-only mode, on battery, below threshold -- throttle.
        assert_eq!(
            effective_poll_interval_secs(true, true, Some(15.0), 20.0),
            LOW_BATTERY_POLL_INTERVAL_SECS
        );

        // Background-only mode, on battery, at/above threshold -- normal cadence.
        assert_eq!(
            effective_poll_interval_secs(true, true, Some(20.0), 20.0),
            NORMAL_POLL_INTERVAL_SECS
        );
        assert_eq!(
            effective_poll_interval_secs(true, true, Some(85.0), 20.0),
            NORMAL_POLL_INTERVAL_SECS
        );

        // Background-only mode, on AC power -- normal cadence regardless of a
        // stale/irrelevant percent reading.
        assert_eq!(
            effective_poll_interval_secs(true, false, Some(5.0), 20.0),
            NORMAL_POLL_INTERVAL_SECS
        );

        // Background-only mode, no battery hardware at all (desktop Mac) --
        // treated the same as AC power.
        assert_eq!(
            effective_poll_interval_secs(true, false, None, 20.0),
            NORMAL_POLL_INTERVAL_SECS
        );
    }

    #[test]
    fn test_low_battery_threshold_persists_and_has_a_sane_default() {
        let dir = temp_dir();
        assert_eq!(
            read_low_battery_threshold_percent(&dir),
            DEFAULT_LOW_BATTERY_THRESHOLD_PERCENT
        );

        write_low_battery_threshold_percent(&dir, 35.0).unwrap();
        assert_eq!(read_low_battery_threshold_percent(&dir), 35.0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_polling_interval_state_default_and_store() {
        let state = PollingIntervalState::default();
        assert_eq!(state.load_secs(), NORMAL_POLL_INTERVAL_SECS);
        state.store_secs(LOW_BATTERY_POLL_INTERVAL_SECS);
        assert_eq!(state.load_secs(), LOW_BATTERY_POLL_INTERVAL_SECS);
    }
}
