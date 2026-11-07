use gpui_kit::component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
use gpui_kit::{App, Menu, MenuItem, OsAction, PromptLevel, Window, actions};

use crate::actions::{
    NewChat, OpenBoard, OpenSettings, OpenUsage, QuickOpen, SaveBuffer, SearchWorkspace,
    ToggleBrowser, ToggleChanges, ToggleCommandMenu, ToggleExplorer, ToggleSidePanel,
    ToggleSidebar, ToggleTerminal,
};

actions!(aether, [About, Hide, HideOthers, ShowAll, Quit, Minimize, Zoom]);

/// Installs the native menu bar; call after `bind_keys` so items show their shortcuts.
pub fn init(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Minimize, cx| with_window(cx, |window, _| window.minimize_window()));
    cx.on_action(|_: &Zoom, cx| with_window(cx, |window, _| window.zoom_window()));
    cx.on_action(|_: &About, cx| {
        with_window(cx, |window, cx| {
            let version = format!("Version {}", env!("CARGO_PKG_VERSION"));
            drop(window.prompt(PromptLevel::Info, "Aether", Some(&version), &["OK"], cx));
        })
    });
    cx.set_menus(menus());
}

fn with_window(cx: &mut App, f: impl FnOnce(&mut Window, &mut App)) {
    if let Some(window) = cx.active_window() {
        window.update(cx, |_, window, cx| f(window, cx)).ok();
    }
}

fn menus() -> Vec<Menu> {
    vec![
        Menu::new("Aether").items([
            MenuItem::action("About Aether", About),
            MenuItem::separator(),
            MenuItem::action("Settings…", OpenSettings),
            MenuItem::separator(),
            MenuItem::action("Hide Aether", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
            MenuItem::action("Quit Aether", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("New Chat", NewChat),
            MenuItem::action("Quick Open…", QuickOpen),
            MenuItem::action("Search in Workspace…", SearchWorkspace),
            MenuItem::separator(),
            MenuItem::action("Save", SaveBuffer),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", Undo, OsAction::Undo),
            MenuItem::os_action("Redo", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("View").items([
            MenuItem::action("Toggle Sidebar", ToggleSidebar),
            MenuItem::action("Toggle Side Panel", ToggleSidePanel),
            MenuItem::action("Toggle Terminal", ToggleTerminal),
            MenuItem::action("Toggle Explorer", ToggleExplorer),
            MenuItem::action("Toggle Changes", ToggleChanges),
            MenuItem::action("Toggle Browser", ToggleBrowser),
            MenuItem::separator(),
            MenuItem::action("Command Menu", ToggleCommandMenu),
            MenuItem::action("Usage", OpenUsage),
            MenuItem::action("Tasks Board", OpenBoard),
        ]),
        Menu::new("Window")
            .items([MenuItem::action("Minimize", Minimize), MenuItem::action("Zoom", Zoom)]),
        Menu::new("Help").items([MenuItem::action("Search Commands…", ToggleCommandMenu)]),
    ]
}

#[cfg(test)]
#[path = "../tests/unit/menus.rs"]
mod tests;
