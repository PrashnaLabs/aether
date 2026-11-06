use gpui_kit::{Axis, Context, Window};

use crate::{
    ClearTerminal, CloseTerminalPane, FocusNextTerminalGroup, FocusNextTerminalPane,
    FocusPrevTerminalGroup, FocusPrevTerminalPane, NewTerminalGroup, SplitTerminalDown,
    SplitTerminalRight, ToggleTerminalFind, ToggleTerminalMaximize, ToggleTerminalSidebar,
};

use super::TerminalWorkspace;

impl TerminalWorkspace {
    pub(super) fn on_new_group(
        &mut self,
        _: &NewTerminalGroup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_group(window, cx);
    }

    pub(super) fn on_split_right(
        &mut self,
        _: &SplitTerminalRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.model.active_pane_id();
        self.split_pane(target, Axis::Horizontal, window, cx);
    }

    pub(super) fn on_split_down(
        &mut self,
        _: &SplitTerminalDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.model.active_pane_id();
        self.split_pane(target, Axis::Vertical, window, cx);
    }

    pub(super) fn on_close_pane(
        &mut self,
        _: &CloseTerminalPane,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.model.active_pane_id();
        self.request_close_pane(target, window, cx);
    }

    pub(super) fn on_focus_next_pane(
        &mut self,
        _: &FocusNextTerminalPane,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = self.model.focus_next_pane();
        if let Some(pane) = self.panes.get(&next) {
            let focus = pane.read(cx).focus.clone();
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    pub(super) fn on_focus_prev_pane(
        &mut self,
        _: &FocusPrevTerminalPane,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prev = self.model.focus_prev_pane();
        if let Some(pane) = self.panes.get(&prev) {
            let focus = pane.read(cx).focus.clone();
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    pub(super) fn on_focus_next_group(
        &mut self,
        _: &FocusNextTerminalGroup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = self.model.focus_next_group();
        if let Some(pane) = self.panes.get(&next) {
            let focus = pane.read(cx).focus.clone();
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    pub(super) fn on_focus_prev_group(
        &mut self,
        _: &FocusPrevTerminalGroup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prev = self.model.focus_prev_group();
        if let Some(pane) = self.panes.get(&prev) {
            let focus = pane.read(cx).focus.clone();
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    pub(super) fn on_toggle_find(
        &mut self,
        _: &ToggleTerminalFind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pane) = self.active_pane() {
            pane.update(cx, |p, cx| p.toggle_find(window, cx));
            cx.notify();
        }
    }

    pub(super) fn on_toggle_sidebar(
        &mut self,
        _: &ToggleTerminalSidebar,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model.sidebar_open = !self.model.sidebar_open;
        self.focus_active_pane(window, cx);
        cx.notify();
    }

    pub(super) fn on_toggle_maximize(
        &mut self,
        _: &ToggleTerminalMaximize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model.maximized = !self.model.maximized;
        self.focus_active_pane(window, cx);
        cx.notify();
    }

    pub(super) fn on_clear_terminal(
        &mut self,
        _: &ClearTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pane) = self.active_pane() {
            pane.update(cx, |p, cx| p.clear_terminal(cx));
            let focus = pane.read(cx).focus.clone();
            window.focus(&focus, cx);
        }
    }
}
