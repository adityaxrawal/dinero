//! Native application menu.
//!
//! Menu structure is declared as data and actions resolved from it, which keeps
//! the mapping from menu item to behaviour testable without constructing a real
//! menu or a running app.
pub mod status_item;

use tauri::menu::{Menu, MenuBuilder, MenuItem, Submenu, SubmenuBuilder};
use tauri::{AppHandle, Manager, Runtime};

use crate::ipc::events::{emit_event, AppEvent};

pub const MENU_ID_CHECK_FOR_UPDATES: &str = "check_for_updates";
pub const MENU_ID_PREFERENCES: &str = "preferences";
pub const MENU_ID_UPLOAD_STATEMENT: &str = "upload_statement";
pub const MENU_ID_EXPORT_DATA: &str = "export_data";
pub const MENU_ID_REFRESH: &str = "refresh";
pub const MENU_ID_TOGGLE_SIDEBAR: &str = "toggle_sidebar";
pub const MENU_ID_DOCUMENTATION: &str = "documentation";
pub const MENU_ID_REPORT_ISSUE: &str = "report_issue";
pub const MENU_ID_PRIVACY_POLICY: &str = "privacy_policy";

const DOCS_URL: &str = "https://dinero.app/docs";
const PRIVACY_POLICY_URL: &str = "https://dinero.app/privacy";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub accelerator: Option<&'static str>,
}

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

#[derive(Debug, Clone, Copy)]
pub struct SubmenuSpec {
    pub title: &'static str,
    pub entries: &'static [EntrySpec],
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    Navigate(&'static str),
    ToggleSidebar,
    UploadStatement,
    CheckForUpdates,
    RefreshNow,
    ReportIssue,
    OpenUrl(&'static str),
}

/// Resolves a menu item id to the action it triggers.
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
        status_item::MENU_BAR_EXTRA_ID_OPEN_DASHBOARD => Some(MenuAction::Navigate("/")),
        status_item::MENU_BAR_EXTRA_ID_OPEN_REVIEW_QUEUE => {
            Some(MenuAction::Navigate("/reconciliation"))
        }
        _ => None,
    }
}

/// Builds one submenu from its declarative spec.
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

/// Builds the application menu.
pub fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let mut builder = MenuBuilder::new(app);
    for submenu_spec in MENU_SPEC {
        let submenu = build_submenu(app, submenu_spec)?;
        builder = builder.item(&submenu);
    }
    builder.build()
}

/// Dispatches a menu event to its action.
pub fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let Some(action) = resolve_menu_action(event.id().0.as_str()) else {
        return;
    };

    match action {
        MenuAction::Navigate(route) => {
            let _ = emit_event(
                app,
                AppEvent::MenuNavigate,
                serde_json::json!({ "route": route }),
            );
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
            crate::updater::trigger_manual_check(app);
        }
        MenuAction::RefreshNow => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let pool = app.state::<deadpool_sqlite::Pool>();
                if let Err(e) =
                    crate::ingestion::polling::sync_force_poll_now(app.clone(), pool).await
                {
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
        assert!(edit_menu
            .entries
            .iter()
            .any(|e| matches!(e, EntrySpec::PredefinedCut)));
        assert!(edit_menu
            .entries
            .iter()
            .any(|e| matches!(e, EntrySpec::PredefinedCopy)));
        assert!(edit_menu
            .entries
            .iter()
            .any(|e| matches!(e, EntrySpec::PredefinedPaste)));

        let view_menu = &MENU_SPEC[3];
        assert!(find_custom(view_menu, MENU_ID_REFRESH).is_some());
        assert!(find_custom(view_menu, MENU_ID_TOGGLE_SIDEBAR).is_some());

        let help_menu = &MENU_SPEC[5];
        assert!(find_custom(help_menu, MENU_ID_DOCUMENTATION).is_some());
        assert!(find_custom(help_menu, MENU_ID_REPORT_ISSUE).is_some());
        assert!(find_custom(help_menu, MENU_ID_PRIVACY_POLICY).is_some());
    }

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
        assert_eq!(resolve_menu_action("some_predefined_item_id"), None);
    }

    #[test]
    fn test_refresh_and_toggle_sidebar_resolve_correctly() {
        assert_eq!(
            resolve_menu_action(MENU_ID_REFRESH),
            Some(MenuAction::RefreshNow)
        );
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
        assert_eq!(
            resolve_menu_action(MENU_ID_REPORT_ISSUE),
            Some(MenuAction::ReportIssue)
        );
        assert_eq!(
            resolve_menu_action(MENU_ID_CHECK_FOR_UPDATES),
            Some(MenuAction::CheckForUpdates)
        );
    }

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
