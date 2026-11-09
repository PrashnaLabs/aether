mod actions;
mod header;
mod model;
mod rename;
mod sidebar;
mod split;

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use aether_core::{TerminalCursor, TerminalTheme};
use gpui_kit::base::resize_handle;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::resizable::resize_handle_appearance;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{
    App, Axis, Context, DragMoveEvent, Entity, EventEmitter, FocusHandle, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, ParentElement, SharedString, Styled, Subscription,
    Window, div, prelude::*, px,
};

use super::grid::Palette;
use super::view::{TerminalEvent, TerminalPaneEvent, TerminalView, hsla, resolve_terminal_palette};
use super::{TERMINAL_CONTEXT, TerminalTab};

pub use model::WorkspaceModel;
pub(crate) use model::{CloseOutcome, GroupId, PaneId};

pub(crate) const WORKSPACE_HEADER_HEIGHT: f32 = 34.;
pub(crate) const SIDEBAR_WIDTH: f32 = 190.;
pub(crate) const SIDEBAR_COLLAPSED_WIDTH: f32 = 36.;
pub(crate) const SPLIT_DIVIDER_SIZE: f32 = 4.;

#[derive(Clone)]
struct DrawerResize;

impl Render for DrawerResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::Empty
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenameTarget {
    Group(GroupId),
    Pane(PaneId),
}

pub struct TerminalWorkspace {
    pub model: WorkspaceModel,
    pub(crate) panes: HashMap<PaneId, Entity<TerminalView>>,
    pane_subscriptions: HashMap<PaneId, Subscription>,
    commands: HashMap<SharedString, PaneId>,
    pub(crate) folder: PathBuf,
    pub open: bool,
    pub shown: bool,
    pub full_page: bool,
    /// Header sits at the window's top edge, under the floating window controls.
    pub window_top: bool,
    /// Shows the hide (X) button.
    pub hideable: bool,
    pub watched: Option<bool>,
    pub(crate) option_as_meta: bool,
    pub(crate) font_family: SharedString,
    pub(crate) font_size: f32,
    pub(crate) terminal_theme: TerminalTheme,
    pub(crate) terminal_cursor: TerminalCursor,
    pub(crate) active_tab: TerminalTab,
    pub(crate) focus: FocusHandle,
    resizing: bool,
    renaming: Option<RenameTarget>,
    rename_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<TerminalEvent> for TerminalWorkspace {}

impl TerminalWorkspace {
    pub fn new(folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let model = WorkspaceModel::new();
        let initial_pane_id = model.active_pane_id();
        let rename_input = cx.new(|cx| InputState::new(window, cx).placeholder("Terminal name"));
        let rename_sub = cx.subscribe_in(
            &rename_input,
            window,
            |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { .. } => {
                    this.commit_rename(cx);
                    this.focus_active_pane(window, cx);
                }
                InputEvent::Blur => this.commit_rename(cx),
                _ => {}
            },
        );
        let mut this = Self {
            model,
            panes: HashMap::new(),
            pane_subscriptions: HashMap::new(),
            commands: HashMap::new(),
            folder,
            open: false,
            shown: false,
            full_page: false,
            window_top: false,
            hideable: false,
            watched: None,
            option_as_meta: false,
            font_family: "".into(),
            font_size: 13.0,
            terminal_theme: TerminalTheme::default(),
            terminal_cursor: TerminalCursor::default(),
            active_tab: TerminalTab::Global,
            focus,
            resizing: false,
            renaming: None,
            rename_input,
            _subscriptions: vec![rename_sub],
        };
        this.spawn_pane(initial_pane_id, window, cx);
        this
    }

    fn spawn_pane(
        &mut self,
        pane_id: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TerminalView> {
        let folder = self.folder.clone();
        let option_as_meta = self.option_as_meta;
        let font_family = self.font_family.clone();
        let font_size = self.font_size;
        let terminal_theme = self.terminal_theme;
        let terminal_cursor = self.terminal_cursor;
        let active_tab = self.active_tab;
        let full_page = self.full_page;
        let open = self.open;
        let pane = cx.new(|cx| {
            let mut v = TerminalView::new_pane(pane_id, folder, window, cx);
            v.option_as_meta = option_as_meta;
            v.font_family = font_family;
            v.font_size = font_size;
            v.terminal_theme = terminal_theme;
            v.terminal_cursor = terminal_cursor;
            v.active_tab = active_tab;
            v.full_page = full_page;
            v.open = open;
            v
        });
        let sub =
            cx.subscribe_in(&pane, window, |this, view, event: &TerminalPaneEvent, window, cx| {
                match *event {
                    TerminalPaneEvent::Focused(id) => {
                        if this.model.focus_pane(id) {
                            cx.notify();
                        }
                    }
                    TerminalPaneEvent::SplitRight(id) => {
                        this.split_pane(id, Axis::Horizontal, window, cx);
                    }
                    TerminalPaneEvent::SplitDown(id) => {
                        this.split_pane(id, Axis::Vertical, window, cx);
                    }
                    TerminalPaneEvent::ClosePane(id) => {
                        this.request_close_pane(id, window, cx);
                    }
                    TerminalPaneEvent::Exited(id) => {
                        let p = view.read(cx);
                        let (hold, key, code, stopped, failed_to_start) =
                            (p.hold, p.key.clone(), p.exit_code(), p.stopped, p.failed_to_start());
                        if let Some(key) = key {
                            cx.emit(TerminalEvent::CommandEnded {
                                key,
                                code,
                                stopped,
                                failed_to_start,
                            });
                        }
                        if !hold {
                            this.close_pane(id, window, cx);
                        }
                    }
                    TerminalPaneEvent::TitleChanged(_) => {
                        cx.notify();
                    }
                }
            });
        self.panes.insert(pane_id, pane.clone());
        self.pane_subscriptions.insert(pane_id, sub);
        pane
    }

    pub(crate) fn active_pane(&self) -> Option<Entity<TerminalView>> {
        self.panes.get(&self.model.active_pane_id()).cloned()
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.active_pane().map_or_else(|| self.focus.clone(), |p| p.read(cx).focus.clone())
    }

    pub(crate) fn focus_active_pane(&self, window: &mut Window, cx: &mut App) {
        let focus = self.focus_handle(cx);
        window.focus(&focus, cx);
    }

    pub fn contains_focused(&self, window: &Window, cx: &App) -> bool {
        self.focus.contains_focused(window, cx)
            || self.panes.values().any(|p| p.read(cx).focus.contains_focused(window, cx))
    }

    pub fn is_maximized(&self) -> bool {
        self.model.maximized
    }

    /// A tab or page terminal stops filling the window; a drawer keeps its size.
    pub fn leave_fullscreen(&mut self) {
        if self.full_page {
            self.model.maximized = false;
        }
    }

    pub fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.shown = shown;
        if !shown {
            for pane in self.panes.values() {
                pane.update(cx, |p, _| p.shown = false);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn sync_settings(
        &mut self,
        folder: PathBuf,
        option_as_meta: bool,
        font_family: SharedString,
        font_size: f32,
        terminal_theme: TerminalTheme,
        terminal_cursor: TerminalCursor,
        active_tab: TerminalTab,
        full_page: bool,
        cx: &mut Context<Self>,
    ) {
        let changed_meta = self.folder != folder
            || self.option_as_meta != option_as_meta
            || self.font_family != font_family
            || self.active_tab != active_tab
            || self.full_page != full_page;
        self.folder = folder.clone();
        self.option_as_meta = option_as_meta;
        self.font_family = font_family.clone();
        self.font_size = font_size;
        self.terminal_theme = terminal_theme;
        self.terminal_cursor = terminal_cursor;
        self.active_tab = active_tab;
        self.full_page = full_page;
        for pane in self.panes.values() {
            pane.update(cx, |p, cx| {
                let folder = if p.own_folder { p.folder.clone() } else { folder.clone() };
                p.full_page = full_page;
                p.active_tab = active_tab;
                p.open = self.open;
                if p.folder != folder
                    || p.option_as_meta != option_as_meta
                    || p.font_family != font_family
                {
                    p.folder = folder;
                    p.option_as_meta = option_as_meta;
                    p.font_family = font_family.clone();
                    cx.notify();
                }
                p.set_font_size(font_size, cx);
                p.set_terminal_theme(terminal_theme, cx);
                p.set_terminal_cursor(terminal_cursor, cx);
            });
        }
        if changed_meta {
            cx.notify();
        }
    }

    pub fn shutdown(&mut self, cx: &mut Context<Self>) {
        for pane in self.panes.values() {
            pane.update(cx, |p, _| p.shutdown());
        }
    }

    pub fn is_running(&self, cx: &App) -> bool {
        self.panes.values().any(|p| p.read(cx).is_running())
    }

    pub fn running_job(&self, cx: &App) -> Option<String> {
        if let Some(active) = self.active_pane()
            && let Some(job) = active.read(cx).running_job()
        {
            return Some(job);
        }
        self.panes.values().find_map(|p| p.read(cx).running_job())
    }

    pub fn run_command(
        &mut self,
        key: SharedString,
        title: SharedString,
        command: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = match self.commands.get(&key).and_then(|id| self.panes.get(id)).cloned() {
            Some(pane) => {
                self.model.focus_pane(pane.read(cx).pane_id);
                pane.update(cx, |p, cx| {
                    p.command = Some(command);
                    p.restart(cx);
                });
                pane
            }
            None => {
                let (group_id, pane_id) = self.model.new_group();
                self.model.set_group_title(group_id, Some(title));
                let pane = self.spawn_pane(pane_id, window, cx);
                pane.update(cx, |p, _| {
                    p.command = Some(command);
                    p.hold = true;
                    p.key = Some(key.clone());
                });
                self.commands.insert(key, pane_id);
                pane
            }
        };
        let palette = self.palette();
        pane.update(cx, |p, cx| p.start_command(palette, window, cx));
        self.open = true;
        cx.notify();
    }

    pub fn stop_command(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(pane) = self.commands.get(key).and_then(|id| self.panes.get(id)) {
            pane.update(cx, |p, cx| p.stop(cx));
        }
    }

    pub fn focus_command(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(&id) = self.commands.get(key)
            && self.model.focus_pane(id)
        {
            cx.notify();
        }
    }

    pub fn command_running(&self, key: &str, cx: &App) -> bool {
        self.commands
            .get(key)
            .and_then(|id| self.panes.get(id))
            .is_some_and(|p| p.read(cx).command_live())
    }

    fn command_closed(&mut self, pane: &Entity<TerminalView>, cx: &mut Context<Self>) {
        let p = pane.read(cx);
        let Some(key) = p.key.clone() else {
            return;
        };
        let live = p.command_live();
        self.commands.remove(&key);
        if live {
            cx.emit(TerminalEvent::CommandEnded {
                key,
                code: None,
                stopped: true,
                failed_to_start: false,
            });
        }
    }

    fn palette(&self) -> Palette {
        resolve_terminal_palette(self.terminal_theme)
    }

    pub(crate) fn new_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (_, pane_id) = self.model.new_group();
        let pane = self.spawn_pane(pane_id, window, cx);
        let focus = pane.read(cx).focus.clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    /// New tab whose shell starts, and stays, in `folder` rather than the workspace folder.
    pub fn open_folder(&mut self, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // a fresh workspace's lone shell hasn't started yet; take it over instead of a 2nd tab
        let fresh =
            self.panes.len() == 1 && self.active_pane().is_some_and(|p| p.read(cx).unstarted());
        if fresh {
            self.focus_active_pane(window, cx);
        } else {
            self.new_group(window, cx);
        }
        if let Some(pane) = self.active_pane() {
            pane.update(cx, |p, _| {
                p.folder = folder;
                p.own_folder = true;
            });
        }
    }

    pub(crate) fn split_pane(
        &mut self,
        target: PaneId,
        axis: Axis,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(new_pane_id) = self.model.split_pane(target, axis) {
            let own = self.panes.get(&target).and_then(|p| {
                let p = p.read(cx);
                p.own_folder.then(|| p.folder.clone())
            });
            let pane = self.spawn_pane(new_pane_id, window, cx);
            if let Some(folder) = own {
                pane.update(cx, |p, _| {
                    p.folder = folder;
                    p.own_folder = true;
                });
            }
            let focus = pane.read(cx).focus.clone();
            window.focus(&focus, cx);
            cx.notify();
        }
    }

    pub(crate) fn request_close_pane(
        &mut self,
        target: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.panes.contains_key(&target) {
            return;
        }
        let label = self.pane_display_label(target, cx);
        let running_job = self
            .panes
            .get(&target)
            .and_then(|p| p.read(cx).running_job())
            .map(|job| if job.is_empty() { "A program".to_string() } else { job });
        let description = match running_job {
            Some(job) => format!("{job} is still running in \"{label}\" and will be stopped."),
            None => format!("Close \"{label}\" and terminate its terminal session?"),
        };
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let view = view.clone();
            alert
                .confirm()
                .title("Delete terminal?")
                .description(SharedString::from(description.clone()))
                .ok_text("Delete")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        this.close_pane(target, window, cx);
                    });
                    true
                })
        });
    }

    pub(crate) fn request_close_group(
        &mut self,
        group_id: GroupId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(group) = self.model.group(group_id) else {
            return;
        };
        let leaves = group.leaves();
        let pane_count = leaves.len();
        let label = self.group_display_label(group_id, cx);
        let running_job = leaves
            .iter()
            .find_map(|id| self.panes.get(id).and_then(|p| p.read(cx).running_job()))
            .map(|job| if job.is_empty() { "A program".to_string() } else { job });
        let title = if pane_count > 1 { "Close terminal group?" } else { "Close terminal?" };
        let description = match (pane_count > 1, running_job) {
            (true, Some(job)) => format!(
                "{job} is still running in \"{label}\". Closing this group will terminate all {pane_count} split terminals."
            ),
            (true, None) => {
                format!("Close \"{label}\" and terminate all {pane_count} split terminals?")
            }
            (false, Some(job)) => {
                format!("{job} is still running in \"{label}\" and will be stopped.")
            }
            (false, None) => format!("Close \"{label}\" and terminate its terminal session?"),
        };
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let view = view.clone();
            alert
                .confirm()
                .title(title)
                .description(SharedString::from(description.clone()))
                .ok_text("Close")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        this.close_group(group_id, window, cx);
                    });
                    true
                })
        });
    }

    pub(crate) fn close_pane(
        &mut self,
        target: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.renaming == Some(RenameTarget::Pane(target)) {
            self.renaming = None;
        }
        if let Some(pane) = self.panes.remove(&target) {
            self.command_closed(&pane, cx);
            pane.update(cx, |p, _| p.shutdown());
        }
        self.pane_subscriptions.remove(&target);
        match self.model.close_pane(target) {
            CloseOutcome::Focused(next_pane) => {
                if let Some(pane) = self.panes.get(&next_pane) {
                    let focus = pane.read(cx).focus.clone();
                    window.focus(&focus, cx);
                }
                cx.notify();
            }
            CloseOutcome::HideWorkspace { replacement_pane } => {
                self.open = false;
                self.shown = false;
                self.spawn_pane(replacement_pane, window, cx);
                cx.emit(TerminalEvent::Hide);
                cx.notify();
            }
            CloseOutcome::NotFound => {}
        }
    }

    pub(crate) fn close_group(
        &mut self,
        group_id: GroupId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.renaming == Some(RenameTarget::Group(group_id)) {
            self.renaming = None;
        }
        let leaves = self.model.group(group_id).map(|g| g.leaves()).unwrap_or_default();
        for pane_id in leaves {
            if self.renaming == Some(RenameTarget::Pane(pane_id)) {
                self.renaming = None;
            }
            if let Some(pane) = self.panes.remove(&pane_id) {
                self.command_closed(&pane, cx);
                pane.update(cx, |p, _| p.shutdown());
            }
            self.pane_subscriptions.remove(&pane_id);
        }
        match self.model.close_group(group_id) {
            CloseOutcome::Focused(next_pane) => {
                if let Some(pane) = self.panes.get(&next_pane) {
                    let focus = pane.read(cx).focus.clone();
                    window.focus(&focus, cx);
                }
                cx.notify();
            }
            CloseOutcome::HideWorkspace { replacement_pane } => {
                self.open = false;
                self.shown = false;
                self.spawn_pane(replacement_pane, window, cx);
                cx.emit(TerminalEvent::Hide);
                cx.notify();
            }
            CloseOutcome::NotFound => {}
        }
    }

    fn select_group(&mut self, group_id: GroupId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pane_id) = self.model.select_group(group_id) {
            if let Some(pane) = self.panes.get(&pane_id) {
                let focus = pane.read(cx).focus.clone();
                window.focus(&focus, cx);
            }
            cx.notify();
        }
    }

    fn focus_pane_by_id(&mut self, pane_id: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.focus_pane(pane_id) {
            if let Some(pane) = self.panes.get(&pane_id) {
                let focus = pane.read(cx).focus.clone();
                window.focus(&focus, cx);
            }
            cx.notify();
        }
    }

    fn group_display_label(&self, group_id: GroupId, cx: &App) -> SharedString {
        if let Some(group) = self.model.group(group_id) {
            if let Some(custom) = &group.custom_title {
                return custom.clone();
            }
            if let Some(pane) = self.panes.get(&group.active_pane) {
                return pane.read(cx).display_label();
            }
        }
        "sh".into()
    }

    fn pane_display_label(&self, pane_id: PaneId, cx: &App) -> SharedString {
        if let Some(pane) = self.panes.get(&pane_id) {
            let p = pane.read(cx);
            if p.custom_title.is_none()
                && let Some(group_id) = self.model.group_of_pane(pane_id)
                && let Some(group) = self.model.group(group_id)
                && group.pane_count() == 1
                && let Some(custom) = &group.custom_title
            {
                return custom.clone();
            }
            return p.display_label();
        }
        "sh".into()
    }

    fn mouse_up(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.resizing) {
            cx.emit(TerminalEvent::ResizeEnd);
        }
    }
}

impl Render for TerminalWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.shown = true;
        let palette = self.palette();

        let active_group = self.model.active_group().clone();
        let active_leaves = active_group.leaves();
        let multi_pane = active_leaves.len() > 1;
        let active_pane = active_group.active_pane;

        for (&pane_id, pane) in &self.panes {
            let in_active_group = active_leaves.contains(&pane_id);
            pane.update(cx, |p, _| {
                if !in_active_group {
                    p.shown = false;
                }
                p.show_pane_header = false;
                p.is_active_pane = pane_id == active_pane;
            });
        }

        let split_canvas = self.render_split_node(&active_group.root, active_pane, multi_pane, cx);

        v_flex()
            .id("terminal-workspace")
            .debug_selector(|| "terminal-workspace".into())
            .size_full()
            .min_w_0()
            .min_h_0()
            .relative()
            .bg(hsla(palette.bg))
            .track_focus(&self.focus)
            .when(self.renaming.is_none(), |el| el.key_context(TERMINAL_CONTEXT))
            .on_action(cx.listener(Self::on_new_group))
            .on_action(cx.listener(Self::on_split_right))
            .on_action(cx.listener(Self::on_split_down))
            .on_action(cx.listener(Self::on_close_pane))
            .on_action(cx.listener(Self::on_focus_next_pane))
            .on_action(cx.listener(Self::on_focus_prev_pane))
            .on_action(cx.listener(Self::on_focus_next_group))
            .on_action(cx.listener(Self::on_focus_prev_group))
            .on_action(cx.listener(Self::on_toggle_find))
            .on_action(cx.listener(Self::on_toggle_sidebar))
            .on_action(cx.listener(Self::on_toggle_maximize))
            .on_action(cx.listener(Self::on_clear_terminal))
            .on_drag_move(cx.listener(|this, e: &DragMoveEvent<Rc<DrawerResize>>, _, cx| {
                this.resizing = true;
                this.model.maximized = false;
                cx.emit(TerminalEvent::Resize(f32::from(e.bounds.bottom() - e.event.position.y)));
            }))
            .capture_any_mouse_up(cx.listener(|this, _, _, cx| this.mouse_up(cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.mouse_up(cx)))
            .child(self.render_header(window, cx))
            .child(
                h_flex()
                    .flex_1()
                    .w_full()
                    .min_w_0()
                    .min_h_0()
                    .child(div().flex_1().h_full().min_w_0().min_h_0().child(split_canvas))
                    .when(!self.full_page || self.model.sidebar_open, |row| {
                        row.child(self.render_sidebar(cx))
                    }),
            )
            .when(!self.full_page && !self.model.maximized, |this| {
                this.child(
                    resize_handle::<DrawerResize, DrawerResize>("terminal-resize", Axis::Vertical)
                        .with_appearance(resize_handle_appearance())
                        .on_drag(DrawerResize, |_: Rc<DrawerResize>, _, _, cx| {
                            cx.new(|_| DrawerResize)
                        }),
                )
                .child(
                    div()
                        .id("terminal-resize-reset")
                        .absolute()
                        .top(px(-4.))
                        .left_0()
                        .w_full()
                        .h(px(9.))
                        .cursor_row_resize()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, e: &MouseDownEvent, _, cx| {
                                if e.click_count == 2 {
                                    this.model.maximized = false;
                                    cx.emit(TerminalEvent::ResetHeight);
                                }
                            }),
                        ),
                )
            })
    }
}
