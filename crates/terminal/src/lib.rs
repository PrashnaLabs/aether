mod boxes;
mod dark;
mod grid;
mod keys;
pub(crate) mod palette;
mod session;
pub mod view;
pub mod workspace;

use std::path::{Path, PathBuf};

pub use dark::dark;

use gpui_kit::{App, KeyBinding, NoAction, Pixels, actions};

use grid::MIN_ROWS;
use view::{GRID_PAD_Y, HEADER_HEIGHT};

actions!(aether, [ToggleTerminal]);

actions!(
    terminal,
    [
        FocusTerminal,
        TerminalCopy,
        TerminalPaste,
        TerminalSelectAll,
        ClearTerminal,
        NewTerminalGroup,
        SplitTerminalRight,
        SplitTerminalDown,
        CloseTerminalPane,
        FocusNextTerminalPane,
        FocusPrevTerminalPane,
        FocusNextTerminalGroup,
        FocusPrevTerminalGroup,
        ToggleTerminalFind,
        ToggleTerminalSidebar,
        ToggleTerminalMaximize,
    ]
);

/// Key context around the terminal grid.
pub(crate) const TERMINAL_CONTEXT: &str = "Terminal";
pub const DEFAULT_HEIGHT: f32 = 260.;

/// Ctrl+` toggles the drawer everywhere; Cmd+J (macOS) moves focus between it and
/// the composer. Inside the grid the shell gets every other key, except the app
/// shortcuts that leave the terminal (Cmd+N, Cmd+, on macOS).
pub fn bind_keys(cx: &mut App) {
    let t = Some(TERMINAL_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("ctrl-`", ToggleTerminal, None),
        // the window root moves focus on Tab
        KeyBinding::new("tab", NoAction, t),
        KeyBinding::new("shift-tab", NoAction, t),
        // Esc is the shell's; an ancestor's Esc action (a peek card's BackToChat) would eat it
        KeyBinding::new("escape", NoAction, t),
    ]);
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-j", FocusTerminal, None),
            KeyBinding::new("cmd-c", TerminalCopy, t),
            KeyBinding::new("cmd-v", TerminalPaste, t),
            KeyBinding::new("cmd-a", TerminalSelectAll, t),
            KeyBinding::new("cmd-k", ClearTerminal, t),
            KeyBinding::new("cmd-r", NoAction, t),
            KeyBinding::new("cmd-shift-`", NewTerminalGroup, t),
            KeyBinding::new("cmd-\\", SplitTerminalRight, t),
            KeyBinding::new("cmd-shift-\\", SplitTerminalDown, t),
            KeyBinding::new("cmd-w", CloseTerminalPane, t),
            KeyBinding::new("cmd-alt-right", FocusNextTerminalPane, t),
            KeyBinding::new("cmd-alt-left", FocusPrevTerminalPane, t),
            KeyBinding::new("cmd-shift-]", FocusNextTerminalGroup, t),
            KeyBinding::new("cmd-shift-[", FocusPrevTerminalGroup, t),
            KeyBinding::new("cmd-f", ToggleTerminalFind, t),
        ]);
    } else {
        // the app's Ctrl shortcuts are the shell's own keys here
        cx.bind_keys([
            KeyBinding::new("ctrl-j", FocusTerminal, None),
            KeyBinding::new("ctrl-shift-c", TerminalCopy, t),
            KeyBinding::new("ctrl-shift-v", TerminalPaste, t),
            KeyBinding::new("ctrl-shift-a", TerminalSelectAll, t),
            KeyBinding::new("ctrl-l", ClearTerminal, t),
            KeyBinding::new("ctrl-shift-`", NewTerminalGroup, t),
            KeyBinding::new("ctrl-shift-5", SplitTerminalRight, t),
            KeyBinding::new("ctrl-alt-\\", SplitTerminalDown, t),
            KeyBinding::new("ctrl-shift-w", CloseTerminalPane, t),
            KeyBinding::new("alt-right", FocusNextTerminalPane, t),
            KeyBinding::new("alt-left", FocusPrevTerminalPane, t),
            KeyBinding::new("ctrl-pagedown", FocusNextTerminalGroup, t),
            KeyBinding::new("ctrl-pageup", FocusPrevTerminalGroup, t),
            KeyBinding::new("ctrl-shift-f", ToggleTerminalFind, t),
            KeyBinding::new("ctrl-c", NoAction, t),
            KeyBinding::new("ctrl-k", NoAction, t),
            KeyBinding::new("ctrl-n", NoAction, t),
            KeyBinding::new("ctrl-,", NoAction, t),
            KeyBinding::new("ctrl-r", NoAction, t),
        ]);
    }
}

pub(crate) fn row_height(font_size: Pixels) -> Pixels {
    (font_size * 1.44).round()
}

/// Smallest drawer that still shows the header and the minimum grid.
pub fn min_height(font_size: Pixels) -> f32 {
    HEADER_HEIGHT + GRID_PAD_Y + MIN_ROWS as f32 * f32::from(row_height(font_size))
}

/// Drawer height for `desired`: at least `min`, at most 75% of the window (the floor wins).
pub fn drawer_height(desired: f32, window: f32, min: f32) -> f32 {
    desired.min(window * 0.75).max(min)
}

/// Drawer height honoring maximize mode (`75%` of viewport when `maximized`).
pub fn effective_drawer_height(desired: f32, window: f32, min: f32, maximized: bool) -> f32 {
    let target = if maximized { window * 0.75 } else { desired };
    drawer_height(target, window, min)
}

/// Resolves the working directory for the global terminal.
/// Falls back to the home folder if workspace is empty or invalid.
pub fn resolve_global_folder(workspace: &str) -> PathBuf {
    let path = Path::new(workspace);
    if !workspace.is_empty() && path.is_dir() {
        path.to_path_buf()
    } else {
        Some(aether_core::home_dir())
            .filter(|home| !home.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("/"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalTab {
    Global,
    Project(i64),
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
