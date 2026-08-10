//! Controls background running and the close-vs-quit decision.
//!
//! With background sync enabled, closing the window hides it rather than exiting,
//! so scheduled ingestion continues. Polling cadence is also adapted to power
//! state here -- polling aggressively on battery costs real runtime.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Manager, Runtime};

pub trait LoginItemController {
    /// Enables the login item.
    fn enable(&self) -> Result<(), String>;
    /// Disables the login item.
    fn disable(&self) -> Result<(), String>;
    /// Whether the login item is currently enabled.
    fn is_enabled(&self) -> Result<bool, String>;
}

pub struct TauriAutoLaunchController<'a, R: Runtime> {
    app: &'a AppHandle<R>,
}

impl<'a, R: Runtime> TauriAutoLaunchController<'a, R> {
    /// Wraps a Tauri app handle as a login-item controller.
    pub fn new(app: &'a AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<'a, R: Runtime> LoginItemController for TauriAutoLaunchController<'a, R> {
    /// Registers the app to launch at login.
    fn enable(&self) -> Result<(), String> {
        use tauri_plugin_autostart::ManagerExt;
        self.app.autolaunch().enable().map_err(|e| e.to_string())
    }

    /// Removes the login-item registration.
    fn disable(&self) -> Result<(), String> {
        use tauri_plugin_autostart::ManagerExt;
        self.app.autolaunch().disable().map_err(|e| e.to_string())
    }

    /// Queries the current login-item state.
    fn is_enabled(&self) -> Result<bool, String> {
        use tauri_plugin_autostart::ManagerExt;
        self.app
            .autolaunch()
            .is_enabled()
            .map_err(|e| e.to_string())
    }
}

/// Applies the launch-at-login preference.
///
/// Takes the controller as a parameter so the decision logic is testable without
/// touching the real system login items.
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

const BACKGROUND_SYNC_SETTING_FILE: &str = "background_sync_enabled";

/// Path of the background-sync setting file.
fn background_sync_settings_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(BACKGROUND_SYNC_SETTING_FILE)
}

/// Whether background sync is enabled.
///
/// Read from a plain file rather than the database, because it is needed during
/// the window-close handler -- before the database pool is necessarily reachable.
pub fn read_background_sync_enabled(app_data_dir: &Path) -> bool {
    std::fs::read_to_string(background_sync_settings_path(app_data_dir))
        .map(|s| s.trim() == "true")
        .unwrap_or(false)
}

/// Persists the background-sync preference.
pub fn write_background_sync_enabled(app_data_dir: &Path, enabled: bool) -> std::io::Result<()> {
    std::fs::write(
        background_sync_settings_path(app_data_dir),
        if enabled { "true" } else { "false" },
    )
}

/// Whether closing the window should hide it instead of quitting.
pub fn should_prevent_close(background_sync_enabled: bool) -> bool {
    background_sync_enabled
}

/// Handles a close request, hiding the window when background sync is on.
///
/// Hiding rather than exiting is what lets scheduled ingestion keep running after
/// the user closes the window.
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

pub const NORMAL_POLL_INTERVAL_SECS: u64 = 60;
pub const LOW_BATTERY_POLL_INTERVAL_SECS: u64 = 300;

pub const DEFAULT_LOW_BATTERY_THRESHOLD_PERCENT: f32 = 20.0;

/// Chooses the polling interval for the current power state.
///
/// Only throttles while running in background-only mode: with the window open the
/// user is present and expects prompt updates. On battery below the threshold the
/// interval widens, since aggressive polling costs real runtime for mail that
/// will still be there later.
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

pub struct PollingIntervalState(AtomicU64);

impl Default for PollingIntervalState {
    /// Starts at the normal interval until power state is known.
    fn default() -> Self {
        Self(AtomicU64::new(NORMAL_POLL_INTERVAL_SECS))
    }
}

impl PollingIntervalState {
    /// Reads the current interval.
    pub fn load_secs(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    /// Updates the interval.
    ///
    /// Atomic because the polling loop and the battery watcher touch this
    /// concurrently, without either taking a lock.
    pub fn store_secs(&self, secs: u64) {
        self.0.store(secs, Ordering::Relaxed);
    }
}

/// Reads whether the machine is on battery, and its charge percentage.
///
/// Returns None where no battery exists, as on a desktop, which the caller treats
/// as mains power.
pub fn read_battery_power_state() -> Option<(bool, f32)> {
    let manager = battery::Manager::new().ok()?;
    let mut batteries = manager.batteries().ok()?;
    let battery = batteries.next()?.ok()?;
    let on_battery = matches!(
        battery.state(),
        battery::State::Discharging | battery::State::Empty
    );
    let percent = battery
        .state_of_charge()
        .get::<battery::units::ratio::percent>();
    Some((on_battery, percent))
}

const LOW_BATTERY_THRESHOLD_SETTING_FILE: &str = "low_battery_poll_threshold_percent";

/// Path of the low-battery threshold setting.
fn low_battery_threshold_settings_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(LOW_BATTERY_THRESHOLD_SETTING_FILE)
}

/// Reads the low-battery threshold percentage.
pub fn read_low_battery_threshold_percent(app_data_dir: &Path) -> f32 {
    std::fs::read_to_string(low_battery_threshold_settings_path(app_data_dir))
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok())
        .unwrap_or(DEFAULT_LOW_BATTERY_THRESHOLD_PERCENT)
}

/// Persists the low-battery threshold percentage.
pub fn write_low_battery_threshold_percent(
    app_data_dir: &Path,
    threshold_percent: f32,
) -> std::io::Result<()> {
    std::fs::write(
        low_battery_threshold_settings_path(app_data_dir),
        threshold_percent.to_string(),
    )
}

/// Watches power state and adjusts the polling interval to match.
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

    #[test]
    fn test_launch_at_login_toggle_registers_login_item() {
        let controller = FakeLoginItemController::new(false);
        assert!(!controller.is_enabled().unwrap());

        apply_launch_at_login(&controller, true).unwrap();
        assert!(
            controller.is_enabled().unwrap(),
            "toggling on must register the login item"
        );
        assert_eq!(*controller.enable_calls.borrow(), 1);

        apply_launch_at_login(&controller, false).unwrap();
        assert!(
            !controller.is_enabled().unwrap(),
            "toggling off must remove the login item"
        );
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

    #[test]
    fn test_polling_interval_increases_on_low_battery() {
        assert_eq!(
            effective_poll_interval_secs(false, true, Some(5.0), 20.0),
            NORMAL_POLL_INTERVAL_SECS
        );

        assert_eq!(
            effective_poll_interval_secs(true, true, Some(15.0), 20.0),
            LOW_BATTERY_POLL_INTERVAL_SECS
        );

        assert_eq!(
            effective_poll_interval_secs(true, true, Some(20.0), 20.0),
            NORMAL_POLL_INTERVAL_SECS
        );
        assert_eq!(
            effective_poll_interval_secs(true, true, Some(85.0), 20.0),
            NORMAL_POLL_INTERVAL_SECS
        );

        assert_eq!(
            effective_poll_interval_secs(true, false, Some(5.0), 20.0),
            NORMAL_POLL_INTERVAL_SECS
        );

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
