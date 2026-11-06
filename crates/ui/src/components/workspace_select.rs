use crate::theme::Corners;
use gpui_kit::rems;
use std::path::PathBuf;

use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{InputEvent, InputState},
    scroll::ScrollableElement,
    v_flex,
};
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding, Render, Role,
    ScrollStrategy, Subscription, UniformListScrollHandle, Window, actions, div, prelude::*, px,
    uniform_list,
};

use super::{TriggerBounds, anchored_popup, popup_height, room};

actions!(
    workspace_select,
    [
        WorkspaceUp,
        WorkspaceDown,
        WorkspaceFirst,
        WorkspaceLast,
        WorkspaceConfirm,
        WorkspaceDismiss,
        WorkspaceTabNext,
        WorkspaceActionNext,
        WorkspaceLocal,
        WorkspaceNewBranch
    ]
);

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum WorkspaceTab {
    #[default]
    Branches,
    Worktrees,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub enum WorkspaceSelection {
    Branch(String),
    NewWorktree(String),
    Worktree(PathBuf),
    #[default]
    Local,
}

#[derive(Clone, PartialEq, Eq)]
pub struct WorktreeRow {
    pub path: PathBuf,
    pub branch: String,
    /// An agent is working in it.
    pub running: bool,
}

#[derive(Default)]
struct WorkspaceRows {
    branches: Vec<String>,
    worktrees: Vec<WorktreeRow>,
    selected: WorkspaceSelection,
    filtered: Vec<usize>,
    highlight: Option<usize>,
    tab: WorkspaceTab,
}

impl WorkspaceRows {
    fn new_branch_event(&self) -> Option<WorkspaceSelectEvent> {
        (self.tab == WorkspaceTab::Branches && !self.branches.is_empty())
            .then_some(WorkspaceSelectEvent::NewBranch)
    }

    fn event(&self, new_worktree: bool) -> Option<WorkspaceSelectEvent> {
        let ix = *self.filtered.get(self.highlight?)?;
        Some(match self.tab {
            WorkspaceTab::Branches if new_worktree => {
                WorkspaceSelectEvent::NewWorktree(self.branches[ix].clone())
            }
            WorkspaceTab::Branches => WorkspaceSelectEvent::SwitchBranch(self.branches[ix].clone()),
            WorkspaceTab::Worktrees => {
                WorkspaceSelectEvent::UseWorktree(self.worktrees[ix].path.clone())
            }
        })
    }

    fn filter(&mut self, query: &str) {
        let query = query.to_lowercase();
        self.filtered = match self.tab {
            WorkspaceTab::Branches => self
                .branches
                .iter()
                .enumerate()
                .filter(|(_, branch)| branch.to_lowercase().contains(&query))
                .map(|(ix, _)| ix)
                .collect(),
            WorkspaceTab::Worktrees => self
                .worktrees
                .iter()
                .enumerate()
                .filter(|(_, row)| {
                    row.branch.to_lowercase().contains(&query)
                        || row.path.to_string_lossy().to_lowercase().contains(&query)
                })
                .map(|(ix, _)| ix)
                .collect(),
        };
        self.highlight = (!self.filtered.is_empty()).then_some(0);
    }

    fn change_tab(&mut self, tab: WorkspaceTab, query: &str) {
        self.tab = tab;
        self.filter(query);
    }

    fn navigate(&mut self, delta: isize) {
        self.highlight = super::choice_menu::next_enabled(
            self.highlight,
            &vec![false; self.filtered.len()],
            delta,
        );
    }

    fn is_selected(&self, ix: usize) -> bool {
        let Some(&ix) = self.filtered.get(ix) else { return false };
        match (&self.tab, &self.selected) {
            (
                WorkspaceTab::Branches,
                WorkspaceSelection::Branch(branch) | WorkspaceSelection::NewWorktree(branch),
            ) => self.branches.get(ix) == Some(branch),
            (WorkspaceTab::Worktrees, WorkspaceSelection::Worktree(path)) => {
                self.worktrees.get(ix).is_some_and(|row| &row.path == path)
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceSelectEvent {
    NewBranch,
    SwitchBranch(String),
    NewWorktree(String),
    UseWorktree(PathBuf),
    Local,
}

pub struct WorkspaceSelectState {
    rows: WorkspaceRows,
    query: Entity<InputState>,
    new_worktree: bool,
    disabled: bool,
    open: bool,
    trigger: TriggerBounds,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    restore_focus: Option<FocusHandle>,
    _query_changes: Subscription,
}

impl EventEmitter<WorkspaceSelectEvent> for WorkspaceSelectState {}

impl WorkspaceSelectState {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        for context in ["WorkspaceSelect", "WorkspaceSelect > Input"] {
            cx.bind_keys([
                KeyBinding::new("up", WorkspaceUp, Some(context)),
                KeyBinding::new("down", WorkspaceDown, Some(context)),
                KeyBinding::new("home", WorkspaceFirst, Some(context)),
                KeyBinding::new("end", WorkspaceLast, Some(context)),
                KeyBinding::new("enter", WorkspaceConfirm, Some(context)),
                KeyBinding::new("escape", WorkspaceDismiss, Some(context)),
                KeyBinding::new("ctrl-tab", WorkspaceTabNext, Some(context)),
                KeyBinding::new("ctrl-shift-tab", WorkspaceTabNext, Some(context)),
                KeyBinding::new("tab", WorkspaceActionNext, Some(context)),
                KeyBinding::new("shift-tab", WorkspaceActionNext, Some(context)),
                KeyBinding::new("ctrl-l", WorkspaceLocal, Some(context)),
                KeyBinding::new("ctrl-n", WorkspaceNewBranch, Some(context)),
            ]);
        }
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search branches or worktrees"));
        let _query_changes = cx.subscribe(&query, |this, _, ev: &InputEvent, cx| {
            if let InputEvent::Change = ev {
                this.rows.filter(&this.query.read(cx).value());
                this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            }
        });
        Self {
            rows: WorkspaceRows::default(),
            query,
            new_worktree: false,
            disabled: false,
            open: false,
            trigger: TriggerBounds::default(),
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            restore_focus: None,
            _query_changes,
        }
    }

    pub fn sync(
        &mut self,
        branches: Vec<String>,
        worktrees: Vec<WorktreeRow>,
        selected: WorkspaceSelection,
        disabled: bool,
        cx: &mut Context<Self>,
    ) {
        if self.rows.branches != branches
            || self.rows.worktrees != worktrees
            || self.rows.selected != selected
            || self.disabled != disabled
        {
            self.rows.branches = branches;
            self.rows.worktrees = worktrees;
            self.rows.selected = selected;
            self.disabled = disabled;
            self.rows.filter(&self.query.read(cx).value());
            if disabled {
                self.dismiss(cx);
            }
            cx.notify();
        }
    }

    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.restore_focus = window.focused(cx);
        self.query.update(cx, |q, cx| q.set_value("", window, cx));
        self.rows.filter("");
        self.new_worktree = false;
        self.open = true;
        self.query.update(cx, |q, cx| q.focus(window, cx));
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.open) {
            return;
        }
        if let Some(previous) = self.restore_focus.take()
            && (self.focus.contains_focused(window, cx)
                || self.query.focus_handle(cx).is_focused(window))
        {
            previous.focus(window, cx);
        }
        cx.notify();
    }

    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.restore_focus = None;
        cx.notify();
    }

    fn change_tab(&mut self, tab: WorkspaceTab, cx: &mut Context<Self>) {
        self.rows.change_tab(tab, &self.query.read(cx).value());
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn navigate(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.rows.navigate(delta);
        if let Some(ix) = self.rows.highlight {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    fn pick(&mut self, event: WorkspaceSelectEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.close(window, cx);
        cx.emit(event);
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(event) = self.rows.event(self.new_worktree) {
            self.pick(event, window, cx);
        }
    }

    fn new_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(event) = self.rows.new_branch_event() {
            self.pick(event, window, cx);
        }
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let row_ix = self.rows.filtered[ix];
        let selected = self.rows.is_selected(ix);
        let highlighted = self.rows.highlight == Some(ix);
        let (label, path, running) = match self.rows.tab {
            WorkspaceTab::Branches => (self.rows.branches[row_ix].clone(), None, false),
            WorkspaceTab::Worktrees => {
                let row = &self.rows.worktrees[row_ix];
                (row.branch.clone(), Some(row.path.display().to_string()), row.running)
            }
        };
        let branch = label.clone();
        let mut row = h_flex()
            .id(("workspace-row", ix))
            .w_full()
            .h(px(36.))
            .px_2()
            .gap_2()
            .corner(rems(0.375))
            .role(Role::Group)
            .aria_label(branch.clone())
            .when(selected, |row| row.aria_description("Selected workspace"))
            .when(running && !selected, |row| row.aria_description("Agent running"))
            .when(highlighted, |row| row.bg(cx.theme().accent).aria_active_descendant())
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.rows.highlight = Some(ix);
                    cx.notify();
                }
            }))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_xs().truncate().child(label))
                    .when_some(path, |col, path| {
                        col.child(
                            div()
                                .text_size(px(10.))
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(path),
                        )
                    }),
            )
            .when(running, |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(px(10.))
                        .text_color(cx.theme().muted_foreground)
                        .child("Agent running"),
                )
            })
            .when(selected, |row| {
                row.child(Icon::new(IconName::Check).small().text_color(cx.theme().primary))
            });
        if self.rows.tab == WorkspaceTab::Branches {
            for (new, label, icon) in [
                (false, "Switch here", IconName::ArrowRight),
                (true, "New worktree", IconName::Plus),
            ] {
                let branch = branch.clone();
                row = row.child(
                    crate::components::button(("workspace-action", ix * 2 + usize::from(new)))
                        .xsmall()
                        .ghost()
                        .icon(icon)
                        .tooltip(label)
                        .accessibility_label(format!("{label}: {branch}"))
                        .selected(highlighted && self.new_worktree == new)
                        .disabled(self.disabled)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let event = if new {
                                WorkspaceSelectEvent::NewWorktree(branch.clone())
                            } else {
                                WorkspaceSelectEvent::SwitchBranch(branch.clone())
                            };
                            this.pick(event, window, cx);
                        })),
                );
            }
        } else {
            let path = self.rows.worktrees[row_ix].path.clone();
            row = row.child(
                crate::components::button(("workspace-use", ix))
                    .xsmall()
                    .ghost()
                    .label("Use worktree")
                    .accessibility_label(format!("Use worktree {}", path.display()))
                    .disabled(self.disabled)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick(WorkspaceSelectEvent::UseWorktree(path.clone()), window, cx)
                    })),
            );
        }
        row
    }
}

impl Render for WorkspaceSelectState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut tabs =
            h_flex().id("workspace-tabs").role(Role::TabList).aria_label("Workspace type").gap_1();
        for (tab, label) in
            [(WorkspaceTab::Branches, "Branches"), (WorkspaceTab::Worktrees, "Worktrees")]
        {
            tabs = tabs.child(
                crate::components::button(label)
                    .xsmall()
                    .ghost()
                    .label(label)
                    .role(Role::Tab)
                    .selected(self.rows.tab == tab)
                    .on_click(cx.listener(move |this, _, _, cx| this.change_tab(tab, cx))),
            );
        }
        let height = popup_height(room(self.trigger.get(), false, window), 280.);
        let width = (f32::from(window.viewport_size().width) - 16.).min(400.);
        div().id("workspace-select-panel").key_context("WorkspaceSelect").role(Role::Dialog).aria_label("Choose branch or worktree")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &WorkspaceUp, _, cx| this.navigate(-1, cx)))
            .on_action(cx.listener(|this, _: &WorkspaceDown, _, cx| this.navigate(1, cx)))
            .on_action(cx.listener(|this, _: &WorkspaceFirst, _, cx| { this.rows.highlight = None; this.navigate(1, cx) }))
            .on_action(cx.listener(|this, _: &WorkspaceLast, _, cx| { this.rows.highlight = None; this.navigate(-1, cx) }))
            .on_action(cx.listener(|this, _: &WorkspaceConfirm, window, cx| this.confirm(window, cx)))
            .on_action(cx.listener(|this, _: &WorkspaceDismiss, window, cx| this.close(window, cx)))
            .on_action(cx.listener(|this, _: &WorkspaceLocal, window, cx| this.pick(WorkspaceSelectEvent::Local, window, cx)))
            .on_action(cx.listener(|this, _: &WorkspaceNewBranch, window, cx| this.new_branch(window, cx)))
            .on_action(cx.listener(|this, _: &WorkspaceTabNext, _, cx| {
                let tab = if this.rows.tab == WorkspaceTab::Branches { WorkspaceTab::Worktrees } else { WorkspaceTab::Branches };
                this.change_tab(tab, cx);
            }))
            .on_action(cx.listener(|this, _: &WorkspaceActionNext, _, cx| { this.new_worktree = !this.new_worktree; cx.notify(); }))
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close(window, cx)))
            .w(px(width)).h(px(height)).overflow_hidden().corner(rems(0.5)).border_1().border_color(cx.theme().border)
            .bg(cx.theme().popover).text_color(cx.theme().popover_foreground).shadow_lg()
            .child(v_flex().size_full()
                .child(h_flex().px_2().py_1().justify_between().child(tabs)
                    .child(crate::components::button("workspace-local").xsmall().ghost().label("Local")
                        .accessibility_label("Use project folder").disabled(self.disabled)
                        .on_click(cx.listener(|this, _, window, cx| this.pick(WorkspaceSelectEvent::Local, window, cx)))))
                .child(div().px_2().pb_2().child(super::field(&self.query).small().icon(IconName::Search).aria_label("Search branches or worktrees")))
                .child(div().id("workspace-results").flex_1().min_h_0().px_2()
                    .child(if self.rows.filtered.is_empty() {
                        div().p_4().text_sm().text_color(cx.theme().muted_foreground)
                            .child(match self.rows.tab {
                                WorkspaceTab::Branches if self.rows.branches.is_empty() => "No branches yet. Make the first commit before choosing a branch.",
                                WorkspaceTab::Branches => "No branches match your search.",
                                WorkspaceTab::Worktrees if self.rows.worktrees.is_empty() => "No existing worktrees in this repository.",
                                WorkspaceTab::Worktrees => "No worktrees match your search.",
                            }).into_any_element()
                    } else {
                        uniform_list("workspace-rows", self.rows.filtered.len(), cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                            range.map(|ix| this.render_row(ix, cx)).collect::<Vec<_>>()
                        })).track_scroll(&self.scroll).size_full().into_any_element()
                    }).vertical_scrollbar(&self.scroll))
                .when(self.rows.tab == WorkspaceTab::Branches, |panel| panel.child(
                    div().px_2().py_1().border_t_1().border_color(cx.theme().border).child(
                        crate::components::button("workspace-new-branch").small().ghost().w_full()
                            .icon(IconName::Plus).label("New branch…")
                            .accessibility_label("Create and switch to a new branch")
                            .tooltip("Create a branch from HEAD (Ctrl+N)")
                            .disabled(self.disabled || self.rows.new_branch_event().is_none())
                            .on_click(cx.listener(|this, _, window, cx| this.new_branch(window, cx))))))
                .child(div().px_2().py_1().text_size(px(10.)).text_color(cx.theme().muted_foreground).child("↑↓ rows · Tab action · Ctrl+Tab tabs · Enter choose · Ctrl+L local · Esc close")))
    }
}

#[derive(IntoElement)]
pub struct WorkspaceSelect {
    state: Entity<WorkspaceSelectState>,
    trigger: Button,
}

impl WorkspaceSelect {
    pub fn new(state: &Entity<WorkspaceSelectState>, trigger: Button) -> Self {
        Self { state: state.clone(), trigger }
    }
}

impl RenderOnce for WorkspaceSelect {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let (open, disabled, bounds) = (state.open, state.disabled, state.trigger.clone());
        let entity = self.state.clone();
        let trigger = self
            .trigger
            .accessibility_label("Choose branch or worktree")
            .selected(open)
            .disabled(disabled)
            .when(!open, |button| {
                button.on_click(move |_, window, cx| entity.update(cx, |s, cx| s.open(window, cx)))
            });
        anchored_popup(("workspace-select-popup", self.state.entity_id()), trigger, &bounds, false)
            .when(open, |popup| popup.content(self.state.clone()))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/components/workspace_select.rs"]
mod tests;
