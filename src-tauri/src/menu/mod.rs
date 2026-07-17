//! TASK-DESK-001 (Doc 30 §12, Doc 29 §14): the native macOS application menu
//! bar -- distinct from the in-app React navigation (Area 9). Menu wiring is
//! a pure dispatch layer: each custom item either emits a Tauri event for
//! React (`AppShell`) to act on, or directly invokes an existing backend
//! command -- no business logic lives here. Predefined items (About, Hide,
//! Quit, Cut/Copy/Paste, Undo/Redo, window controls) are handled entirely
//! natively by the OS and never reach `handle_menu_event`.
//!
//! The menu's structure is described once, as the pure `MENU_SPEC` data
//! table below, and `build_menu` is the only thing that walks it to
//! construct a real `tauri::menu::Menu`. This is deliberate: `muda` (the
//! native menu backend Tauri uses) enforces an actual OS-level "must be
//! constructed on the main thread" check on macOS, which a plain `cargo
//! test` process cannot satisfy -- so unlike most Tauri IPC/event code in
//! this codebase, a live `Menu` object can't be built inside a unit test
//! here. Keeping the spec free of any Tauri type means the acceptance tests
//! can assert against it directly without ever constructing one.

use tauri::menu::{Menu, MenuBuilder, MenuItem, Submenu, SubmenuBuilder};
use tauri::{AppHandle, Manager, Runtime};

use crate::ipc::events::{emit_event, AppEvent};

/// Custom (non-predefined) menu item ids. Named constants so `MENU_SPEC`
/// (construction) and `resolve_menu_action` (interpretation) can never drift
/// out of sync with each other.
pub const MENU_ID_CHECK_FOR_UPDATES: &str = "check_for_updates";
pub const MENU_ID_PREFERENCES: &str = "preferences";
pub const MENU_ID_UPLOAD_STATEMENT: &str = "upload_statement";
pub const MENU_ID_EXPORT_DATA: &str = "export_data";
pub const MENU_ID_REFRESH: &str = "refresh";
pub const MENU_ID_TOGGLE_SIDEBAR: &str = "toggle_sidebar";
pub const MENU_ID_DOCUMENTATION: &str = "documentation";
pub const MENU_ID_REPORT_ISSUE: &str = "report_issue";
pub const MENU_ID_PRIVACY_POLICY: &str = "privacy_policy";

/// Production marketing/docs domain (Document 49 §7: `dinero.app` resolved
/// 2026-07-14 as the production domain name).
const DOCS_URL: &str = "https://dinero.app/docs";
const PRIVACY_POLICY_URL: &str = "https://dinero.app/privacy";

/// A custom menu item's static shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub accelerator: Option<&'static str>,
}

/// One entry in a submenu: either a custom item, a separator, or one of the
/// OS-native predefined items (About/Hide/Quit/Cut/Copy/.../Bring All to
/// Front) that carry no id of our own and are never routed through
/// `handle_menu_event`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntrySpec {
    Custom(ItemSpec),
    Separator,
    PredefinedAbout,
    PredefinedHide,
    PredefinedHideOthers,
    PredefinedShowAll,
    PredefinedQuit,
    PredefinedCloseWindow,
    PredefinedUndo,
    PredefinedRedo,
    PredefinedCut,
    PredefinedCopy,
    PredefinedPaste,
    PredefinedSelectAll,
    PredefinedMinimize,
    PredefinedMaximize,
    PredefinedBringAllToFront,
}

/// A top-level submenu's static shape.
#[derive(Debug, Clone, Copy)]
pub struct SubmenuSpec {
    pub title: &'static str,
    pub entries: &'static [EntrySpec],
}

/// The standard macOS application menu bar (Doc 30 TASK-DESK-001): App /
/// File / Edit / View / Window / Help, in macOS convention order. The sole
/// source of truth for both `build_menu` (construction) and this module's
/// tests (assertion) -- see the module doc comment for why.
pub const MENU_SPEC: &[SubmenuSpec] = &[
    SubmenuSpec {
        title: "Dinero",
        entries: &[
            EntrySpec::PredefinedAbout,
            EntrySpec::Separator,
            EntrySpec::Custom(ItemSpec {
                id: MENU_ID_CHECK_FOR_UPDATES,
                label: "Check for Updates…",
                accelerator: None,
            }),
            EntrySpec::Separator,
            EntrySpec::Custom(ItemSpec {
                id: MENU_ID_PREFERENCES,
                label: "Preferences…",
                accelerator: Some("CmdOrCtrl+,"),
            }),
            EntrySpec::Separator,
            EntrySpec::PredefinedHide,
            EntrySpec::PredefinedHideOthers,
            EntrySpec::PredefinedShowAll,
            EntrySpec::Separator,
            EntrySpec::PredefinedQuit,
        ],
    },
    SubmenuSpec {
        title: "File",
        entries: &[
            EntrySpec::Custom(ItemSpec {
                id: MENU_ID_UPLOAD_STATEMENT,
                label: "Upload Statement…",
                accelerator: Some("CmdOrCtrl+U"),
            }),
            EntrySpec::Custom(ItemSpec {
                id: MENU_ID_EXPORT_DATA,
                label: "Export Data…",
                accelerator: None,
            }),
            EntrySpec::Separator,
            EntrySpec::PredefinedCloseWindow,
        ],
    },
    SubmenuSpec {
        // Standard Cut/Copy/Paste/Undo/Redo/Select All -- required for
        // WebKit text-field compatibility (Doc 30: without a native Edit
        // menu, keyboard shortcuts for text editing inside the WebView
        // don't work on macOS).
        title: "Edit",
        entries: &[
            EntrySpec::PredefinedUndo,
            EntrySpec::PredefinedRedo,
            EntrySpec::Separator,
            EntrySpec::PredefinedCut,
            EntrySpec::PredefinedCopy,
            EntrySpec::PredefinedPaste,
            EntrySpec::PredefinedSelectAll,
        ],
    },
    SubmenuSpec {
        title: "View",
        entries: &[
            EntrySpec::Custom(ItemSpec {
                id: MENU_ID_REFRESH,
                label: "Refresh",
                accelerator: Some("CmdOrCtrl+R"),
            }),
            EntrySpec::Custom(ItemSpec {
                id: MENU_ID_TOGGLE_SIDEBAR,
                label: "Toggle Sidebar",
                accelerator: Some("CmdOrCtrl+Alt+S"),
            }),
        ],
    },
    SubmenuSpec {
        title: "Window",
        entries: &[
            EntrySpec::PredefinedMinimize,
            EntrySpec::PredefinedMaximize,
            EntrySpec::Separator,
            EntrySpec::PredefinedBringAllToFront,
        ],
    },
    SubmenuSpec {
        title: "Help",
        entries: &[
            EntrySpec::Custom(ItemSpec {
                id: MENU_ID_DOCUMENTATION,
                label: "Documentation",
                accelerator: None,
            }),
            EntrySpec::Custom(ItemSpec {
                id: MENU_ID_REPORT_ISSUE,
                label: "Report an Issue…",
                accelerator: None,
            }),
            EntrySpec::Separator,
            EntrySpec::Custom(ItemSpec {
                id: MENU_ID_PRIVACY_POLICY,
                label: "Privacy Policy",
                accelerator: None,
            }),
        ],
    },
];

/// What a custom menu item resolves to -- the pure half of dispatch, kept
/// free of any `AppHandle` so it is trivially unit-testable (Doc 30
/// TASK-DESK-001 acceptance criterion
/// `test_upload_statement_shortcut_dispatches_correctly`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    /// Ask React (`AppShell`) to navigate to this route.
    Navigate(&'static str),
    /// Ask React to toggle the sidebar's collapsed state.
    ToggleSidebar,
    /// Ask React to open the statement-upload flow.
    UploadStatement,
    /// Ask for an update check. The check itself is TASK-DESK-005's scope --
    /// this item only dispatches the request.
    CheckForUpdates,
    /// Trigger an immediate Gmail poll directly (no UI round-trip needed).
    RefreshNow,
    /// Generate a diagnostic bundle directly.
    ReportIssue,
    /// Open an external URL in the system browser.
    OpenUrl(&'static str),
}

/// Maps a menu item id to the action it should dispatch. Returns `None` for
/// any id not recognized here -- in practice this only ever happens for
/// predefined items, which Tauri never routes through `on_menu_event` at all.
pub fn resolve_menu_action(id: &str) -> Option<MenuAction> {
    match id {
        MENU_ID_CHECK_FOR_UPDATES => Some(MenuAction::CheckForUpdates),
        MENU_ID_PREFERENCES => Some(MenuAction::Navigate("/settings")),
        MENU_ID_UPLOAD_STATEMENT => Some(MenuAction::UploadStatement),
        MENU_ID_EXPORT_DATA => Some(MenuAction::Navigate("/settings")),
        MENU_ID_REFRESH => Some(MenuAction::RefreshNow),
        MENU_ID_TOGGLE_SIDEBAR => Some(MenuAction::ToggleSidebar),
        MENU_ID_DOCUMENTATION => Some(MenuAction::OpenUrl(DOCS_URL)),
        MENU_ID_REPORT_ISSUE => Some(MenuAction::ReportIssue),
        MENU_ID_PRIVACY_POLICY => Some(MenuAction::OpenUrl(PRIVACY_POLICY_URL)),
        _ => None,
    }
}

/// Builds one submenu from its spec. Generic over `R: Runtime` so
/// production code can call it with the real Wry runtime -- this function
/// itself is never exercised in tests (see module doc comment).
fn build_submenu<R: Runtime>(app: &AppHandle<R>, spec: &SubmenuSpec) -> tauri::Result<Submenu<R>> {
    let mut builder = SubmenuBuilder::new(app, spec.title);
    for entry in spec.entries {
        builder = match entry {
            EntrySpec::Separator => builder.separator(),
            EntrySpec::PredefinedAbout => builder.about(None),
            EntrySpec::PredefinedHide => builder.hide(),
            EntrySpec::PredefinedHideOthers => builder.hide_others(),
            EntrySpec::PredefinedShowAll => builder.show_all(),
            EntrySpec::PredefinedQuit => builder.quit(),
            EntrySpec::PredefinedCloseWindow => builder.close_window(),
            EntrySpec::PredefinedUndo => builder.undo(),
            EntrySpec::PredefinedRedo => builder.redo(),
            EntrySpec::PredefinedCut => builder.cut(),
            EntrySpec::PredefinedCopy => builder.copy(),
            EntrySpec::PredefinedPaste => builder.paste(),
            EntrySpec::PredefinedSelectAll => builder.select_all(),
            EntrySpec::PredefinedMinimize => builder.minimize(),
            EntrySpec::PredefinedMaximize => builder.maximize(),
            EntrySpec::PredefinedBringAllToFront => builder.bring_all_to_front(),
            EntrySpec::Custom(item) => {
                let menu_item =
                    MenuItem::with_id(app, item.id, item.label, true, item.accelerator)?;
                builder.item(&menu_item)
            }
        };
    }
    builder.build()
}

/// Builds the standard macOS application menu bar from `MENU_SPEC`. Generic
/// over `R: Runtime` for the same reason as `build_submenu`.
pub fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let mut builder = MenuBuilder::new(app);
    for submenu_spec in MENU_SPEC {
        let submenu = build_submenu(app, submenu_spec)?;
        builder = builder.item(&submenu);
    }
    builder.build()
}

/// Executes the effect a resolved `MenuAction` implies. This is the one
/// place custom menu items are allowed to touch backend state -- everything
/// else in this module is pure dispatch. Tied to the concrete default
/// (`Wry`) runtime, matching the existing calling convention of the backend
/// commands invoked directly from here (`sync_force_poll_now`,
/// `export_logs`), which are likewise not generic over `R`.
pub fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let Some(action) = resolve_menu_action(event.id().0.as_str()) else {
        return;
    };

    match action {
        MenuAction::Navigate(route) => {
            let _ = emit_event(app, AppEvent::MenuNavigate, serde_json::json!({ "route": route }));
        }
        MenuAction::ToggleSidebar => {
            let _ = emit_event(app, AppEvent::MenuToggleSidebar, serde_json::json!({}));
        }
        MenuAction::UploadStatement => {
            let _ = emit_event(
                app,
                AppEvent::MenuUploadStatementRequested,
                serde_json::json!({}),
            );
        }
        MenuAction::CheckForUpdates => {
            let _ = emit_event(app, AppEvent::MenuCheckForUpdates, serde_json::json!({}));
        }
        MenuAction::RefreshNow => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let pool = app.state::<deadpool_sqlite::Pool>();
                if let Err(e) = crate::ingestion::polling::sync_force_poll_now(app.clone(), pool).await {
                    tracing::warn!("Menu-triggered refresh failed: {}", e);
                }
            });
        }
        MenuAction::ReportIssue => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let pool = app.state::<deadpool_sqlite::Pool>();
                match crate::commands::export_logs(app.clone(), pool).await {
                    Ok(_) => tracing::info!("Menu-triggered diagnostic bundle export succeeded"),
                    Err(e) => tracing::warn!("Menu-triggered diagnostic export failed: {}", e),
                }
            });
        }
        MenuAction::OpenUrl(url) => {
            if let Err(e) = tauri_plugin_opener::open_url(url, None::<&str>) {
                tracing::warn!("Failed to open URL {}: {}", url, e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_custom<'a>(spec: &'a SubmenuSpec, id: &str) -> Option<&'a ItemSpec> {
        spec.entries.iter().find_map(|e| match e {
            EntrySpec::Custom(item) if item.id == id => Some(item),
            _ => None,
        })
    }

    /// Doc 30 TASK-DESK-001 acceptance: `test_menu_structure_matches_macos_conventions`.
    #[test]
    fn test_menu_structure_matches_macos_conventions() {
        let titles: Vec<&str> = MENU_SPEC.iter().map(|s| s.title).collect();
        assert_eq!(
            titles,
            vec!["Dinero", "File", "Edit", "View", "Window", "Help"],
            "macOS convention: App menu first (named after the running process), \
             then File/Edit/View/Window/Help in that order"
        );

        let app_menu = &MENU_SPEC[0];
        assert!(matches!(app_menu.entries[0], EntrySpec::PredefinedAbout));
        assert!(matches!(
            app_menu.entries.last(),
            Some(EntrySpec::PredefinedQuit)
        ));

        let file_menu = &MENU_SPEC[1];
        let upload = find_custom(file_menu, MENU_ID_UPLOAD_STATEMENT)
            .expect("File menu must contain the Upload Statement item");
        assert_eq!(upload.label, "Upload Statement…");
        assert_eq!(upload.accelerator, Some("CmdOrCtrl+U"));

        let edit_menu = &MENU_SPEC[2];
        assert!(!edit_menu.entries.is_empty());
        assert!(edit_menu.entries.iter().any(|e| matches!(e, EntrySpec::PredefinedCut)));
        assert!(edit_menu.entries.iter().any(|e| matches!(e, EntrySpec::PredefinedCopy)));
        assert!(edit_menu.entries.iter().any(|e| matches!(e, EntrySpec::PredefinedPaste)));

        let view_menu = &MENU_SPEC[3];
        assert!(find_custom(view_menu, MENU_ID_REFRESH).is_some());
        assert!(find_custom(view_menu, MENU_ID_TOGGLE_SIDEBAR).is_some());

        let help_menu = &MENU_SPEC[5];
        assert!(find_custom(help_menu, MENU_ID_DOCUMENTATION).is_some());
        assert!(find_custom(help_menu, MENU_ID_REPORT_ISSUE).is_some());
        assert!(find_custom(help_menu, MENU_ID_PRIVACY_POLICY).is_some());
    }

    /// Doc 30 TASK-DESK-001 acceptance: `test_upload_statement_shortcut_dispatches_correctly`.
    #[test]
    fn test_upload_statement_shortcut_dispatches_correctly() {
        let file_menu = &MENU_SPEC[1];
        let upload = find_custom(file_menu, MENU_ID_UPLOAD_STATEMENT).unwrap();
        assert_eq!(upload.accelerator, Some("CmdOrCtrl+U"));
        assert_eq!(
            resolve_menu_action(upload.id),
            Some(MenuAction::UploadStatement)
        );
    }

    #[test]
    fn test_preferences_and_export_data_both_navigate_to_settings() {
        assert_eq!(
            resolve_menu_action(MENU_ID_PREFERENCES),
            Some(MenuAction::Navigate("/settings"))
        );
        assert_eq!(
            resolve_menu_action(MENU_ID_EXPORT_DATA),
            Some(MenuAction::Navigate("/settings"))
        );
    }

    #[test]
    fn test_unknown_id_resolves_to_no_action() {
        // Predefined items (Cut/Copy/Quit/etc.) carry no id of ours and
        // never reach resolve_menu_action with a recognized id -- confirm
        // the fallthrough is inert.
        assert_eq!(resolve_menu_action("some_predefined_item_id"), None);
    }

    #[test]
    fn test_refresh_and_toggle_sidebar_resolve_correctly() {
        assert_eq!(resolve_menu_action(MENU_ID_REFRESH), Some(MenuAction::RefreshNow));
        assert_eq!(
            resolve_menu_action(MENU_ID_TOGGLE_SIDEBAR),
            Some(MenuAction::ToggleSidebar)
        );
    }

    #[test]
    fn test_help_menu_items_resolve_correctly() {
        assert_eq!(
            resolve_menu_action(MENU_ID_DOCUMENTATION),
            Some(MenuAction::OpenUrl(DOCS_URL))
        );
        assert_eq!(
            resolve_menu_action(MENU_ID_PRIVACY_POLICY),
            Some(MenuAction::OpenUrl(PRIVACY_POLICY_URL))
        );
        assert_eq!(resolve_menu_action(MENU_ID_REPORT_ISSUE), Some(MenuAction::ReportIssue));
        assert_eq!(
            resolve_menu_action(MENU_ID_CHECK_FOR_UPDATES),
            Some(MenuAction::CheckForUpdates)
        );
    }

    /// Every custom item id referenced in `MENU_SPEC` must have a
    /// `resolve_menu_action` arm -- guards against the two definitions
    /// silently drifting apart as the menu grows.
    #[test]
    fn test_every_custom_item_in_spec_resolves_to_an_action() {
        for submenu in MENU_SPEC {
            for entry in submenu.entries {
                if let EntrySpec::Custom(item) = entry {
                    assert!(
                        resolve_menu_action(item.id).is_some(),
                        "menu item '{}' (in submenu '{}') has no resolve_menu_action arm",
                        item.id,
                        submenu.title
                    );
                }
            }
        }
    }
}
