use aether_ui::theme::Corners;
use std::ops::Range;
use std::path::PathBuf;

use aether_git::worktree::Start;
use aether_git::{Branches, GitOp};
use aether_ui::{field, theme};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::{
    AnyElement, Context, Entity, FocusHandle, Focusable, KeyBinding, Role, ScrollStrategy,
    SharedString, UniformListScrollHandle, Window, actions, div, prelude::*, px, rems,
    uniform_list,
};

use super::Feedback;
use super::worktrees::tree_pick_title;
use crate::app::ChatApp;

actions!(git_branch_picker, [PickUp, PickDown, PickFirst, PickLast, PickConfirm, PickDismiss]);

const PICKER_CONTEXT: &str = "GitBranchPicker";
const PICK_ROW: f32 = 28.;
/// Rows shown before the list scrolls.
const PICK_ROWS: usize = 10;

pub(super) struct Picker {
    folder: PathBuf,
    mode: PickMode,
    branches: Option<Result<Branches, SharedString>>,
    highlight: Option<usize>,
    scroll: UniformListScrollHandle,
    /// Focus before it opened, given back when it closes.
    restore: Option<FocusHandle>,
    /// Picking what a new worktree checks out, instead of switching.
    worktree: bool,
    /// Held by the popup while choosing where a worktree goes, which has no field.
    focus: FocusHandle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PickMode {
    Switch,
    /// Choosing where a new branch starts.
    Base,
    /// Naming a new branch that starts at this full ref, else at HEAD.
    Name(Option<String>),
    /// Choosing where the worktree checking this out goes.
    Place(Start),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PickRow {
    Create,
    CreateFrom,
    Caption(&'static str),
    Local(String),
    Remote(String),
    AetherFolder,
    ChooseFolder,
}

/// The picker's rows for what's typed: the create actions while switching,
/// then matching local and remote branches under captions. Naming lists none.
pub(crate) fn pick_rows(branches: &Branches, mode: &PickMode, query: &str) -> Vec<PickRow> {
    let query = query.trim().to_lowercase();
    let mut rows = match mode {
        PickMode::Switch => vec![PickRow::Create, PickRow::CreateFrom],
        PickMode::Base => Vec::new(),
        PickMode::Name(_) => return Vec::new(),
        PickMode::Place(_) => return vec![PickRow::AetherFolder, PickRow::ChooseFolder],
    };
    let hits = |names: &[String], row: fn(String) -> PickRow| -> Vec<PickRow> {
        names.iter().filter(|n| n.to_lowercase().contains(&query)).cloned().map(row).collect()
    };
    for (caption, found) in [
        ("Branches", hits(&branches.local, PickRow::Local)),
        ("Remote branches", hits(&branches.remote, PickRow::Remote)),
    ] {
        if !found.is_empty() {
            rows.push(PickRow::Caption(caption));
            rows.extend(found);
        }
    }
    rows
}

/// Where the highlight starts: on the first branch matching what's typed,
/// else on the first row.
pub(crate) fn first_pick(rows: &[PickRow], query: &str) -> Option<usize> {
    let branch = rows.iter().position(|row| matches!(row, PickRow::Local(_) | PickRow::Remote(_)));
    let typed = !query.trim().is_empty();
    typed
        .then_some(branch)
        .flatten()
        .or_else(|| rows.iter().position(|row| !matches!(row, PickRow::Caption(_))))
}

/// The next row up (`delta` < 0) or down that isn't a caption, wrapping around.
pub(super) fn pick_step(rows: &[PickRow], from: Option<usize>, delta: isize) -> Option<usize> {
    let len = rows.len() as isize;
    let mut at = from.map_or(if delta < 0 { len } else { -1 }, |ix| ix as isize);
    for _ in 0..len {
        at = (at + delta).rem_euclid(len);
        if !matches!(rows[at as usize], PickRow::Caption(_)) {
            return Some(at as usize);
        }
    }
    None
}

/// The line over the field: what Enter does in `mode`.
pub(super) fn pick_title(mode: &PickMode) -> String {
    match mode {
        PickMode::Switch => "Switch branch".into(),
        PickMode::Base => "Create branch from…".into(),
        PickMode::Name(None) => "Create branch from HEAD".into(),
        PickMode::Name(Some(base)) => {
            let name = base
                .strip_prefix("refs/heads/")
                .or_else(|| base.strip_prefix("refs/remotes/"))
                .unwrap_or(base);
            format!("Create branch from {name}")
        }
        PickMode::Place(
            Start::Local(branch) | Start::Remote(branch) | Start::New { name: branch, .. },
        ) => {
            format!("Create worktree on {branch} in…")
        }
    }
}

impl ChatApp {
    fn picker_field(&self) -> Option<Entity<InputState>> {
        self.git.query.as_ref().map(|(query, _)| query.clone())
    }

    /// The picker's field, made the first time it opens.
    fn branch_query(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(query) = self.picker_field() {
            return query;
        }
        // bound after the field's own keys, so these win in the picker
        for context in [format!("{PICKER_CONTEXT} > Input"), PICKER_CONTEXT.into()] {
            let context = Some(context.as_str());
            cx.bind_keys([
                KeyBinding::new("up", PickUp, context),
                KeyBinding::new("down", PickDown, context),
                KeyBinding::new("home", PickFirst, context),
                KeyBinding::new("end", PickLast, context),
                KeyBinding::new("enter", PickConfirm, context),
                KeyBinding::new("escape", PickDismiss, context),
            ]);
        }
        let query = cx.new(|cx| InputState::new(window, cx));
        let changes = cx.subscribe(&query, |this, _, ev: &InputEvent, cx| {
            if let InputEvent::Change = ev {
                this.reset_pick(cx);
            }
        });
        self.git.query = Some((query.clone(), changes));
        query
    }

    /// The footer's branch button: lists branches off the UI thread and
    /// puts focus in the search field.
    pub(super) fn open_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_picker_as(false, window, cx)
    }

    /// Composer shortcut into the existing naming flow, with its result visible
    /// in Changes. Keep focus in the name field when the drawer opens.
    pub(crate) fn open_create_branch_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git.busy() || self.git_repo().is_none() {
            return;
        }
        self.open_explorer_view(crate::explorer::ExplorerView::Changes, window, cx);
        self.git.diff = None;
        self.git.refocus = false;
        self.open_picker_as(false, window, cx);
        self.pick(PickRow::Create, window, cx);
    }

    /// [`Self::open_picker`], or picking a new worktree's branch when `worktree`.
    pub(super) fn open_picker_as(
        &mut self,
        worktree: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(folder) = self.git_repo() else {
            return;
        };
        let top = self.git_status().map(|s| s.top.clone()).unwrap_or_else(|| folder.clone());
        let restore = window.focused(cx);
        let query = self.branch_query(window, cx);
        query.update(cx, |query, cx| {
            query.set_value("", window, cx);
            query.set_placeholder("Search branches", window, cx);
            query.focus(window, cx);
        });
        self.git.picker = Some(Picker {
            folder: folder.clone(),
            mode: PickMode::Switch,
            branches: None,
            highlight: Some(0),
            scroll: UniformListScrollHandle::new(),
            restore,
            worktree,
            focus: cx.focus_handle(),
        });
        let task = cx.background_executor().spawn(async move { aether_git::branches(&top) });
        cx.spawn(async move |this, cx| {
            let read = task.await;
            this.update(cx, |this, cx| {
                let Some(picker) = this.git.picker.as_mut().filter(|p| p.folder == folder) else {
                    return;
                };
                picker.branches = Some(read.map_err(SharedString::from));
                this.reset_pick(cx);
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn close_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.git.picker.take() else {
            return;
        };
        if picker.focus.is_focused(window)
            || self.picker_field().is_some_and(|query| query.focus_handle(cx).is_focused(window))
        {
            let restore = picker.restore.unwrap_or_else(|| self.git.root.clone());
            window.focus(&restore, cx);
        }
        cx.notify();
    }

    /// Rows for the picker as it stands; the create actions show before branches load.
    fn pick_rows_now(&self, cx: &gpui_kit::App) -> Vec<PickRow> {
        let (Some(picker), Some(query)) = (&self.git.picker, self.picker_field()) else {
            return Vec::new();
        };
        let none = Branches::default();
        let branches = match &picker.branches {
            Some(Ok(branches)) => branches,
            _ => &none,
        };
        pick_rows(branches, &picker.mode, &query.read(cx).value())
    }

    fn reset_pick(&mut self, cx: &mut Context<Self>) {
        let rows = self.pick_rows_now(cx);
        let query = self.picker_field().map(|q| q.read(cx).value()).unwrap_or_default();
        if let Some(picker) = &mut self.git.picker {
            picker.highlight = first_pick(&rows, &query);
            picker.scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
        cx.notify();
    }

    fn pick_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        let rows = self.pick_rows_now(cx);
        if let Some(picker) = &mut self.git.picker {
            picker.highlight = pick_step(&rows, picker.highlight, delta);
            if let Some(ix) = picker.highlight {
                picker.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
            }
        }
        cx.notify();
    }

    /// Home or End: the first or last row; the name field keeps its own.
    fn pick_edge(&mut self, delta: isize, cx: &mut Context<Self>) {
        match &mut self.git.picker {
            Some(picker) if !matches!(picker.mode, PickMode::Name(_)) => picker.highlight = None,
            _ => return cx.propagate(),
        }
        self.pick_move(delta, cx);
    }

    fn pick_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = &self.git.picker else {
            return;
        };
        if let PickMode::Name(base) = &picker.mode {
            // The composer can open this before the drawer's first status read.
            // Keep the prompt open until run_git has a verified target.
            if !picker.worktree && self.git_status().is_none() {
                return;
            }
            let (base, worktree) = (base.clone(), picker.worktree);
            let name = self.picker_field().map(|q| q.read(cx).value().trim().to_string());
            if let Some(name) = name.filter(|name| !name.is_empty()) {
                if worktree {
                    self.place_tree(Start::New { name, base }, window, cx);
                } else {
                    self.close_picker(window, cx);
                    self.switch_branch_op(GitOp::CreateBranch { name, base }, window, cx);
                }
            }
            return;
        }
        let highlight = picker.highlight;
        if let Some(row) = highlight.and_then(|ix| self.pick_rows_now(cx).get(ix).cloned()) {
            self.pick(row, window, cx);
        }
    }

    fn pick(&mut self, row: PickRow, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = &self.git.picker else {
            return;
        };
        let current = match &picker.branches {
            Some(Ok(branches)) => branches.current.clone(),
            _ => None,
        };
        let worktree = picker.worktree;
        let mode = match (&picker.mode, row) {
            (_, PickRow::Caption(_)) => return,
            (PickMode::Place(start), PickRow::AetherFolder) => {
                let start = start.clone();
                self.close_picker(window, cx);
                return self.create_tree(start, None, window, cx);
            }
            (PickMode::Place(start), PickRow::ChooseFolder) => {
                let start = start.clone();
                self.close_picker(window, cx);
                return self.choose_tree_folder(start, window, cx);
            }
            (_, PickRow::AetherFolder | PickRow::ChooseFolder) => return,
            (_, PickRow::Create) => PickMode::Name(None),
            (_, PickRow::CreateFrom) => PickMode::Base,
            (PickMode::Base, PickRow::Local(base)) => {
                PickMode::Name(Some(format!("refs/heads/{base}")))
            }
            (PickMode::Base, PickRow::Remote(base)) => {
                PickMode::Name(Some(format!("refs/remotes/{base}")))
            }
            (_, PickRow::Local(branch)) if worktree => {
                if self.branch_tree(&branch).is_none() {
                    self.place_tree(Start::Local(branch), window, cx);
                }
                return;
            }
            (_, PickRow::Remote(branch)) if worktree => {
                return self.place_tree(Start::Remote(branch), window, cx);
            }
            (_, PickRow::Local(branch)) => {
                self.close_picker(window, cx);
                if current.as_ref() != Some(&branch) {
                    self.switch_branch_op(GitOp::Checkout(branch), window, cx);
                }
                return;
            }
            (_, PickRow::Remote(branch)) => {
                self.close_picker(window, cx);
                return self.switch_branch_op(GitOp::Track(branch), window, cx);
            }
        };
        // "Create new branch…" takes what was typed as the name
        let keep = mode == PickMode::Name(None);
        if let Some(picker) = &mut self.git.picker {
            picker.mode = mode.clone();
        }
        if let Some(query) = self.picker_field() {
            query.update(cx, |query, cx| {
                if !keep {
                    query.set_value("", window, cx);
                }
                let hint = if mode == PickMode::Base { "Search branches" } else { "Branch name" };
                query.set_placeholder(hint, window, cx);
            });
        }
        self.reset_pick(cx);
    }

    /// The worktree picker's last step: where the worktree checking `start` out goes.
    fn place_tree(&mut self, start: Start, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(picker) = &mut self.git.picker {
            picker.mode = PickMode::Place(start);
            window.focus(&picker.focus, cx);
        }
        self.reset_pick(cx);
    }

    /// Runs a checkout or new branch as the panel's job, unless an agent is
    /// working in the folder: switching would move files under it.
    fn switch_branch_op(&mut self, op: GitOp, window: &mut Window, cx: &mut Context<Self>) {
        let Some(folder) = self.git_folder() else {
            return;
        };
        if self.agent_in(&folder) {
            self.git.feedback = Some(Feedback {
                folder: Some(folder),
                error: true,
                text: "Couldn't switch branches".into(),
                detail: Some("Stop the agent using this folder before switching branches.".into()),
                remote: false,
            });
            return cx.notify();
        }
        self.run_git(op, window, cx);
    }

    fn pick_row(
        &self,
        row: &PickRow,
        ix: usize,
        current: Option<&str>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (muted, accent, primary) = (theme.muted_foreground, theme.accent, theme.primary);
        let highlighted = self.git.picker.as_ref().is_some_and(|p| p.highlight == Some(ix));
        let checkout =
            self.git.picker.as_ref().is_some_and(|p| p.worktree && p.mode == PickMode::Switch);
        let taken = match row {
            PickRow::Local(branch) if checkout => self.branch_tree(branch),
            _ => None,
        };
        let is_current =
            !checkout && matches!(row, PickRow::Local(b) if Some(b.as_str()) == current);
        let slot = matches!(row, PickRow::AetherFolder).then(|| self.tree_slot()).flatten();
        let (icon, label, aria) = match row {
            PickRow::Caption(text) => {
                return div()
                    .h(px(PICK_ROW))
                    .px_2()
                    .pb_1()
                    .flex()
                    .items_end()
                    .text_xs()
                    .font_medium()
                    .text_color(muted)
                    .child(*text)
                    .into_any_element();
            }
            PickRow::Create => {
                (Lucide::Plus, "Create new branch…".to_string(), "Create new branch".to_string())
            }
            PickRow::CreateFrom => (
                Lucide::GitBranchPlus,
                "Create new branch from…".into(),
                "Create new branch from another branch".into(),
            ),
            PickRow::Local(branch) if is_current => {
                (Lucide::GitBranch, branch.clone(), format!("{branch}, current branch"))
            }
            PickRow::Local(branch) if taken.is_some() => (
                Lucide::GitBranch,
                branch.clone(),
                format!("{branch}, checked out {}, unavailable", taken.clone().unwrap_or_default()),
            ),
            PickRow::Local(branch) => (Lucide::GitBranch, branch.clone(), branch.clone()),
            PickRow::Remote(branch) => {
                (Lucide::Cloud, branch.clone(), format!("{branch}, remote branch"))
            }
            PickRow::AetherFolder => (
                Lucide::Folder,
                "aether's worktree folder".into(),
                match &slot {
                    Some(slot) => format!("aether's worktree folder, {slot}"),
                    None => "aether's worktree folder".into(),
                },
            ),
            PickRow::ChooseFolder => (
                Lucide::FolderOpen,
                "Choose folder…".into(),
                "Choose a folder to create the worktree in".into(),
            ),
        };
        h_flex()
            .id(("git-pick", ix))
            .w_full()
            .h(px(PICK_ROW))
            .px_2()
            .gap_2()
            .items_center()
            .corner(rems(0.375))
            .cursor_pointer()
            .role(Role::ListBoxOption)
            .aria_label(aria)
            .aria_selected(highlighted)
            .when(highlighted, |row| row.bg(accent).aria_active_descendant())
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if let Some(picker) = &mut this.git.picker
                    && *hovered
                    && picker.highlight != Some(ix)
                {
                    picker.highlight = Some(ix);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                if let Some(row) = this.pick_rows_now(cx).get(ix).cloned() {
                    this.pick(row, window, cx);
                }
            }))
            .child(Icon::new(icon).size(rems(0.875)).flex_none().text_color(muted))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .when(taken.is_some(), |el| el.text_color(muted))
                    .child(label),
            )
            .children(taken.map(|at| {
                div().flex_none().text_xs().text_color(muted).child(format!("checked out {at}"))
            }))
            .children(slot.map(|slot| {
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_middle()
                    .text_xs()
                    .text_color(muted)
                    .child(slot)
            }))
            .when(is_current, |row| {
                row.child(Icon::new(IconName::Check).small().flex_none().text_color(primary))
            })
            .into_any_element()
    }

    /// The branch picker's popup while it's open for the active folder.
    pub(super) fn branch_picker(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let picker =
            self.git.picker.as_ref().filter(|p| Some(&p.folder) == self.git_folder().as_ref())?;
        let query = self.picker_field()?;
        let rows = self.pick_rows_now(cx);
        let naming = matches!(picker.mode, PickMode::Name(_));
        let placing = matches!(picker.mode, PickMode::Place(_));
        let current = match &picker.branches {
            Some(Ok(branches)) => branches.current.clone(),
            _ => None,
        };
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let note = match &picker.branches {
            _ if naming || placing => None,
            None => Some(("Reading branches…".to_string(), muted)),
            Some(Err(e)) => Some((format!("Couldn't list branches: {e}"), theme::danger_text(cx))),
            Some(Ok(_)) if rows.is_empty() => Some(("No branches match.".into(), muted)),
            Some(Ok(_)) => None,
        };
        let hint = match (picker.worktree, naming) {
            _ if placing => "↑↓ to choose · Enter to create the worktree there · Esc to close",
            (true, true) => "Enter to choose where the worktree goes · Esc to close",
            (true, false) => {
                "↑↓ to choose · Enter to check it out in a new worktree · Esc to close"
            }
            (false, true) => "Enter to create and switch · Esc to close",
            (false, false) => "↑↓ to choose · Enter to pick · Esc to close",
        };
        let title =
            if picker.worktree { tree_pick_title(&picker.mode) } else { pick_title(&picker.mode) };
        let list = (!rows.is_empty()).then(|| {
            let height = rows.len().min(PICK_ROWS) as f32 * PICK_ROW;
            let count = rows.len();
            div()
                .id("git-branch-list")
                .role(Role::ListBox)
                .aria_label(if placing { "Worktree folder" } else { "Branches" })
                .h(px(height))
                .child(
                    uniform_list(
                        "git-branch-rows",
                        count,
                        cx.processor(move |this, range: Range<usize>, _, cx| {
                            range
                                .map(|ix| this.pick_row(&rows[ix], ix, current.as_deref(), cx))
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&picker.scroll)
                    .size_full(),
                )
                .vertical_scrollbar(&picker.scroll)
        });
        let width = (self.settings.git_panel_width - 20.).clamp(240., 420.);
        Some(
            v_flex()
                .id("git-branch-picker")
                .key_context(PICKER_CONTEXT)
                .when(placing, |el| el.track_focus(&picker.focus))
                .role(Role::Dialog)
                .aria_label(if picker.worktree {
                    "Create a worktree"
                } else {
                    "Switch or create a branch"
                })
                .w(px(width))
                .p_2()
                .gap_1p5()
                .corner(rems(0.5))
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .shadow_lg()
                .on_action(cx.listener(|this, _: &PickUp, _, cx| this.pick_move(-1, cx)))
                .on_action(cx.listener(|this, _: &PickDown, _, cx| this.pick_move(1, cx)))
                .on_action(cx.listener(|this, _: &PickFirst, _, cx| this.pick_edge(1, cx)))
                .on_action(cx.listener(|this, _: &PickLast, _, cx| this.pick_edge(-1, cx)))
                .on_action(
                    cx.listener(|this, _: &PickConfirm, window, cx| this.pick_confirm(window, cx)),
                )
                .on_action(
                    cx.listener(|this, _: &PickDismiss, window, cx| this.close_picker(window, cx)),
                )
                .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close_picker(window, cx)))
                .child(div().px_1().text_xs().font_semibold().child(title))
                .children((!placing).then(|| {
                    field(&query)
                        .small()
                        .icon(if naming { IconName::Plus } else { IconName::Search })
                        .aria_label(if naming { "New branch name" } else { "Search branches" })
                }))
                .children(list)
                .children(
                    note.map(|(text, color)| div().px_1().text_xs().text_color(color).child(text)),
                )
                .child(div().px_1().text_size(px(10.)).text_color(muted).child(hint))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
#[path = "../../tests/unit/git_panel/picker.rs"]
mod tests;
