use super::*;
use crate::app::ChatApp;
use aether_core::TileLayout;
use gpui_kit::{Context, Window};

impl ChatApp {
    pub(crate) fn run_session_pane_action(
        &mut self,
        pane: PaneId,
        action: PaneAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self.workspace.model.window_of(pane) else { return };
        let result = match action {
            PaneAction::SplitRight => {
                self.split_session(pane, SplitAxis::Horizontal, window, cx);
                return;
            }
            PaneAction::SplitDown => {
                self.split_session(pane, SplitAxis::Vertical, window, cx);
                return;
            }
            PaneAction::Float => self.workspace.model.float(pane, Rect::new(40., 40., 640., 480.)),
            PaneAction::Dock => self.workspace.model.dock(pane, None, DockSide::Right),
            PaneAction::Detach => {
                self.detach_session(pane, window, cx);
                return;
            }
            PaneAction::Return => {
                self.return_session(pane, window, cx);
                return;
            }
            PaneAction::Maximize => {
                if let Some(w) = self.workspace.model.windows.get_mut(&host) {
                    w.maximized = if w.maximized == Some(pane) { None } else { Some(pane) };
                }
                Ok(())
            }
            PaneAction::Promote => {
                self.workspace.model.promote(pane);
                Ok(())
            }
            PaneAction::Close => {
                self.workspace.views.remove(&pane);
                self.workspace.model.close(pane)
            }
        };
        if let Err(e) = result {
            self.workspace.error = Some(e.into());
        }
        cx.notify();
    }
    pub(super) fn split_existing_session(
        &mut self,
        pane: PaneId,
        chat: i64,
        axis: SplitAxis,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(size) = self.workspace.bounds.get(&pane).copied() else { return };
        if !self.chats.iter().any(|c| c.id == chat && !c.archived) {
            return;
        }
        match self.workspace.model.split(pane, chat, axis, size) {
            Ok(_) => {
                self.ensure_session_ui(chat, window, cx);
                self.in_session(chat, cx, |app, cx| {
                    app.composer.update(cx, |s, cx| s.focus(window, cx))
                });
            }
            Err(error) => self.workspace.error = Some(error.into()),
        }
        cx.notify();
    }
    pub(super) fn split_session(
        &mut self,
        pane: PaneId,
        axis: SplitAxis,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let size = self.workspace.bounds.get(&pane).copied().unwrap_or([
            f32::from(window.viewport_size().width),
            f32::from(window.viewport_size().height) - 120.,
        ]);
        if let Err(error) = self.workspace.model.can_split(pane, axis, size) {
            self.workspace.error = Some(error.into());
            cx.notify();
            return;
        }
        let source = self.workspace.model.panes.get(&pane).copied();
        if let Some(id) = source
            && let Some(ix) = self.chats.iter().position(|c| c.id == id)
        {
            self.active = ix;
            self.activate_session_ui(id, window, cx);
        }
        self.workspace.splitting = true;
        self.new_chat_in_current_project(window, cx);
        self.workspace.splitting = false;
        let id = self.chats[self.active].id;
        match self.workspace.model.split(pane, id, axis, size) {
            Ok(_) => {}
            Err(e) => self.workspace.error = Some(e.into()),
        }
        cx.notify();
    }
    pub(super) fn focus_session_direction(
        &mut self,
        host: WindowId,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let monocle = self.settings.tile_layout == TileLayout::Monocle;
        if let Some(pane) =
            self.workspace.model.directional(host, direction, &self.workspace.pane_rects, monocle)
            && let Some(id) = self.workspace.model.panes.get(&pane).copied()
        {
            self.in_session(id, cx, |app, cx| app.composer.update(cx, |s, cx| s.focus(window, cx)));
        }
        cx.notify();
    }
    pub(super) fn cycle_session(
        &mut self,
        host: WindowId,
        back: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(p) = self.workspace.model.cycle(host, back)
            && let Some(id) = self.workspace.model.panes.get(&p).copied()
        {
            self.in_session(id, cx, |app, cx| app.composer.update(cx, |s, cx| s.focus(window, cx)));
        }
        cx.notify();
    }
    pub(super) fn swap_session(
        &mut self,
        host: WindowId,
        direction: Direction,
        cx: &mut Context<Self>,
    ) {
        let monocle = self.settings.tile_layout == TileLayout::Monocle;
        self.workspace.model.swap_toward(host, direction, &self.workspace.pane_rects, monocle);
        cx.notify();
    }
    pub(super) fn focused_pane_action(
        &mut self,
        host: WindowId,
        action: PaneAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(layout) = self.workspace.model.windows.get(&host) else { return };
        let Some(pane) = layout.focused else { return };
        let action = match action {
            PaneAction::Float if layout.floats.iter().any(|f| f.pane == pane) => PaneAction::Dock,
            action => action,
        };
        self.run_session_pane_action(pane, action, window, cx);
    }
    pub(super) fn cycle_tile_layout(&mut self, cx: &mut Context<Self>) {
        let all = TileLayout::ALL;
        let i = all.iter().position(|l| *l == self.settings.tile_layout).unwrap_or(0);
        self.update_settings(cx, |s| s.tile_layout = all[(i + 1) % all.len()]);
    }
    pub(super) fn nudge_tile_ratio(&mut self, by: f32, cx: &mut Context<Self>) {
        if self.settings.tile_layout == TileLayout::Stack {
            self.update_settings(cx, |s| s.tile_ratio = (s.tile_ratio + by).clamp(0.3, 0.8));
        }
    }
}
