use crate::session_workspace::WindowId;
use aether_ui::theme::Corners;
use std::path::{Path, PathBuf};

use aether_git::Branches;
use aether_ui::{TriggerBounds, popup_height, room};
use gpui_kit::component::input::InputState;
use gpui_kit::component::{ActiveTheme, FocusTrapElement, v_flex};
use gpui_kit::{
    AnyElement, App, Context, Entity, FocusHandle, Role, SharedString, Window, actions, div,
    prelude::*, px,
};

use crate::app::ChatApp;

mod branch;
mod gh;
mod model;
mod remote;
mod rows;
mod scripts;

pub(crate) use model::{
    GhState, PopView, ScriptRuns, back_of, choose_app_label, disabled_label, home_short,
    line_totals, machine_name,
};
pub(crate) use model::{Stop, pop_step};
use model::{footer_error, row_after_shrink};
pub(crate) use rows::diff_stat;

actions!(workspace_popover, [PopUp, PopDown, PopFirst, PopLast, PopBack, PopConfirm]);

pub(crate) const POP_CONTEXT: &str = "WorkspacePopover";
pub(crate) const BRANCH_SEARCH_CONTEXT: &str = "PopBranchSearch";
pub(crate) const PUBLISH_NAME_CONTEXT: &str = "PopPublishName";
pub(crate) const SCRIPT_FORM_CONTEXT: &str = "PopScriptForm";

pub(crate) struct WorkspacePopover {
    pub(crate) open: bool,
    pub(crate) view: PopView,
    pub(crate) focus: FocusHandle,
    pub(crate) restore: Option<FocusHandle>,
    pub(crate) machine: Option<SharedString>,
    pub(crate) error: Option<SharedString>,
    pub(crate) script_name: Entity<InputState>,
    pub(crate) script_cmd: Entity<InputState>,
    pub(crate) branch_query: Entity<InputState>,
    pub(crate) branches: Option<(PathBuf, Result<Branches, SharedString>)>,
    pub(crate) publish_name: Entity<InputState>,
    pub(crate) script_setup: bool,
    pub(crate) publish_private: bool,
    pub(crate) publishing: bool,
    pub(crate) gh_checking: bool,
    pub(crate) trigger: TriggerBounds,
    rows: Vec<FocusHandle>,
}

impl WorkspacePopover {
    pub(crate) fn new(window: &mut Window, cx: &mut App) -> Self {
        let focus = cx.focus_handle();
        let branch_query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search or create a branch…"));
        let mut input = |hint| cx.new(|cx| InputState::new(window, cx).placeholder(hint));
        Self {
            open: false,
            view: PopView::Main,
            focus,
            restore: None,
            machine: None,
            error: None,
            script_name: input("dev"),
            script_cmd: input("pnpm dev"),
            branch_query,
            branches: None,
            publish_name: input(""),
            script_setup: false,
            publish_private: true,
            publishing: false,
            gh_checking: false,
            trigger: TriggerBounds::default(),
            rows: Vec::new(),
        }
    }

    pub(crate) fn dismiss(&mut self) {
        self.open = false;
        self.restore = None;
    }

    pub(crate) fn rows(&mut self, n: usize, cx: &mut App) -> &[FocusHandle] {
        self.rows.resize_with(n, || cx.focus_handle().tab_stop(true));
        &self.rows
    }

    fn first_input(&self) -> Option<&Entity<InputState>> {
        match self.view {
            PopView::Script(_) => Some(&self.script_name),
            PopView::Branch => Some(&self.branch_query),
            PopView::Publish => Some(&self.publish_name),
            PopView::Main | PopView::Location | PopView::Apps | PopView::Remote => None,
        }
    }
}

fn read_machine() -> Option<String> {
    let run = |program: &str, args: &[&str]| {
        let out = aether_core::command(program).args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let scutil =
        if cfg!(target_os = "macos") { run("scutil", &["--get", "ComputerName"]) } else { None };
    machine_name(scutil.as_deref(), run("hostname", &[]).as_deref())
}

impl ChatApp {
    pub(crate) fn toggle_workspace_pop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace_pop.open {
            return self.close_workspace_pop(window, cx);
        }
        let pop = &mut self.workspace_pop;
        pop.open = true;
        pop.restore = window.focused(cx);
        if pop.machine.is_none() {
            let task = cx.background_executor().spawn(async { read_machine() });
            cx.spawn(async move |this, cx| {
                let name = task.await;
                this.update(cx, |this, cx| {
                    this.workspace_pop.machine = name.map(SharedString::from);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        self.load_pop_remote(cx);
        self.pop_view(PopView::Main, window, cx);
    }

    pub(crate) fn pop_view(&mut self, view: PopView, window: &mut Window, cx: &mut Context<Self>) {
        let pop = &mut self.workspace_pop;
        pop.view = view;
        pop.error = None;
        if let Some(input) = pop.first_input().cloned() {
            input.update(cx, |s, cx| s.focus(window, cx));
        } else {
            let first = usize::from(view != PopView::Main);
            let n = pop.rows.len().max(first + 1);
            pop.rows(n, cx)[first].focus(window, cx);
        }
        cx.notify();
    }

    pub(crate) fn close_workspace_pop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pop = &mut self.workspace_pop;
        if !std::mem::take(&mut pop.open) {
            return;
        }
        if let Some(previous) = pop.restore.take()
            && pop.focus.contains_focused(window, cx)
        {
            previous.focus(window, cx);
        }
        cx.notify();
    }

    fn pop_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match back_of(&self.workspace_pop.view) {
            Some(view) => self.pop_view(view, window, cx),
            None => self.close_workspace_pop(window, cx),
        }
    }

    fn pop_move(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        let pop = &self.workspace_pop;
        let at = pop.rows.iter().position(|row| row.is_focused(window));
        let field = matches!(pop.view, PopView::Branch | PopView::Publish | PopView::Script(_));
        match pop_step(at, field, down) {
            Some(Stop::Row(to)) => {
                if let Some(row) = pop.rows.get(to) {
                    row.focus(window, cx);
                }
            }
            Some(Stop::Field) => {
                if let Some(input) = pop.first_input().cloned() {
                    input.update(cx, |s, cx| s.focus(window, cx));
                }
            }
            None => {}
        }
    }

    /// Home or End: the first row under the header, or the last.
    fn pop_edge(&mut self, last: bool, window: &mut Window, cx: &mut Context<Self>) {
        let pop = &self.workspace_pop;
        let first = usize::from(pop.view != PopView::Main);
        let row = if last { pop.rows.last() } else { pop.rows.get(first) };
        if let Some(row) = row.cloned() {
            row.focus(window, cx);
        }
    }

    pub(crate) fn pop_git_failed(
        &mut self,
        folder: &Path,
        text: &str,
        detail: &str,
        host: WindowId,
        cx: &mut Context<Self>,
    ) {
        let chat = &self.chats[self.active];
        let shown = self.changes_view_shown(host);
        let here = chat.folder(&self.settings.workspace_root).as_deref() == Some(folder);
        if here && !shown {
            self.pop_error(footer_error(text, detail), cx);
        }
    }

    /// Reopening clears the footer, so a late failure goes to the header line instead.
    pub(crate) fn pop_error(&mut self, error: impl Into<SharedString>, cx: &mut Context<Self>) {
        let error = Some(error.into());
        if self.workspace_pop.open {
            self.workspace_pop.error = error;
        } else {
            self.open_error = error;
        }
        cx.notify();
    }

    pub(crate) fn render_workspace_pop(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.workspace_pop.open {
            return None;
        }
        let max_h =
            popup_height(room(self.workspace_pop.trigger.get(), true, window), f32::INFINITY);
        let focused = self.workspace_pop.rows.iter().position(|row| row.is_focused(window));
        let body = self.pop_body(cx);
        let rows = &self.workspace_pop.rows;
        if let Some(row) = row_after_shrink(focused, rows.len()).map(|ix| rows[ix].clone()) {
            row.focus(window, cx);
        }
        let error = self.workspace_pop.error.clone();
        let danger = aether_ui::theme::danger_text(cx);
        let theme = cx.theme();
        Some(
            v_flex()
                .id("workspace-pop")
                .key_context(POP_CONTEXT)
                .role(Role::Dialog)
                .aria_label("Workspace")
                .on_action(
                    cx.listener(|this, _: &PopUp, window, cx| this.pop_move(false, window, cx)),
                )
                .on_action(
                    cx.listener(|this, _: &PopDown, window, cx| this.pop_move(true, window, cx)),
                )
                .on_action(
                    cx.listener(|this, _: &PopFirst, window, cx| this.pop_edge(false, window, cx)),
                )
                .on_action(
                    cx.listener(|this, _: &PopLast, window, cx| this.pop_edge(true, window, cx)),
                )
                .on_action(cx.listener(|this, _: &PopBack, window, cx| this.pop_back(window, cx)))
                .on_mouse_down_out(
                    cx.listener(|this, _, window, cx| this.close_workspace_pop(window, cx)),
                )
                .w(px(380.))
                .max_h(px(max_h))
                .overflow_y_scroll()
                .p(px(6.))
                .gap(px(2.))
                .corner(px(12.))
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .shadow_lg()
                .children(body)
                .children(error.map(|e| {
                    div()
                        .id("pop-error")
                        .role(Role::Alert)
                        .aria_label(e.clone())
                        .px(px(10.))
                        .py(px(6.))
                        .text_xs()
                        .text_color(danger)
                        .child(e)
                }))
                .focus_trap("workspace-pop-trap", &self.workspace_pop.focus)
                .into_any_element(),
        )
    }
}
