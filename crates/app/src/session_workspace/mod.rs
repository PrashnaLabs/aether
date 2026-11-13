mod commands;
mod model;
mod pane;
mod render;
mod windows;
pub(crate) use windows::initial_window_bounds;
mod persistence;
pub(crate) use model::*;
pub(crate) use persistence::SessionSnapshot;
#[derive(Clone, Copy)]
pub(crate) enum PaneAction {
    SplitRight,
    SplitLeft,
    SplitDown,
    SplitUp,
    Float,
    Dock,
    Detach,
    Return,
    Maximize,
    Promote,
    Close,
}
use gpui_kit::{AnyWindowHandle, Entity};
use std::collections::HashMap;
#[derive(Default)]
pub(crate) struct Workspace {
    pub model: WorkspaceModel,
    pub views: HashMap<PaneId, Entity<render::SessionPane>>,
    pub bounds: HashMap<PaneId, [f32; 2]>,
    pub surface_sizes: HashMap<WindowId, [f32; 2]>,
    pub pane_rects: std::collections::BTreeMap<PaneId, Rect>,
    pub windows: HashMap<WindowId, AnyWindowHandle>,
    pub error: Option<String>,
    pub save_error: Option<String>,
    pub splitting: bool,
    pub persistence_enabled: bool,
    pub restored_drafts: std::collections::BTreeMap<i64, SessionSnapshot>,
    pub save: persistence::SaveState,
}

/// Interactive children keep their own context behavior rather than opening the session menu.
pub(crate) fn block_session_menu(
    _: &gpui_kit::MouseDownEvent,
    _: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) {
    cx.stop_propagation();
}
