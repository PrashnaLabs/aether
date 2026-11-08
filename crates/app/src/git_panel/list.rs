use aether_ui::theme::Corners;
use std::collections::HashSet;
use std::path::PathBuf;

use aether_git::{Change, ChangeKind, GitOp, StatusError};
use aether_ui::{field_fill, row_style, splice_range, tabular, theme};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, StyledExt, h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, ClipboardItem, Context, ElementId, Entity, FocusHandle,
    KeyDownEvent, KeyboardButton, ListOffset, Role, SharedString, Window, div, list, prelude::*,
    px, rems,
};

use super::rows::{
    Dir, Fold, Row, Section, describe, dir_label, discard_deletes, discard_dir_prompt,
    discard_prompt, file_key, files_under, fold, header_label, key_path, section_of, section_title,
    split_path, stage_op, stat_words, step,
};
use super::sync::{Push, push_state};
use super::widgets::{hover_actions, letter_chip, note, stat_element, tone};
use super::worktrees::TreeJob;
use crate::app::{ChatApp, format_full};

pub(super) const ROW_HEIGHT: f32 = 1.75;

/// A tree row's left padding in px, as in the explorer's file tree.
fn indent(depth: usize) -> f32 {
    depth as f32 * 14. + 8.
}

/// A changed file's color in the explorer's file tree.
fn explorer_tone(kind: ChangeKind, cx: &App) -> gpui_kit::Hsla {
    match kind {
        ChangeKind::Modified => theme::warning_text(cx),
        ChangeKind::Added | ChangeKind::Untracked => theme::success_text(cx),
        ChangeKind::Deleted | ChangeKind::Conflicted => theme::danger_text(cx),
        ChangeKind::Renamed | ChangeKind::Copied => theme::info_text(cx),
        ChangeKind::TypeChanged => gpui_kit::component::Theme::global(cx).muted_foreground,
    }
}

impl ChatApp {
    fn toggle_section(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>) {
        if !self.git.collapsed.remove(&section) {
            self.git.collapsed.insert(section);
        }
        self.sync_history(cx);
        let (rows, keys) = self.git_listing();
        self.sync_git_rows(rows, keys, window, cx);
        cx.notify();
    }

    fn git_dir(&self, key: &str) -> Option<&Dir> {
        let ix = self.git.keys.iter().position(|k| k == key)?;
        match self.git.rows.get(ix)? {
            Row::Dir(dir) => Some(dir),
            _ => None,
        }
    }

    fn toggle_dir(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.git_dir(key).map(|dir| (dir.section, dir.path.clone())) else {
            return;
        };
        if !self.git.collapsed_dirs.remove(&dir) {
            self.git.collapsed_dirs.insert(dir);
        }
        let (rows, keys) = self.git_listing();
        self.sync_git_rows(rows, keys, window, cx);
        cx.notify();
    }

    fn stage_dir(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((dir, status)) = self.git_dir(key).zip(self.git_status()) else {
            return;
        };
        let op = stage_op(dir.section, &files_under(&status.changes, dir.section, &dir.path));
        self.run_git(op, window, cx);
    }

    /// Asks before discarding the work-tree changes under a folder row.
    fn confirm_discard_dir(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((dir, status)) = self.git_dir(key).zip(self.git_status()) else {
            return;
        };
        if dir.section != Section::Changes {
            return;
        }
        let changes: Vec<Change> =
            files_under(&status.changes, dir.section, &dir.path).into_iter().cloned().collect();
        let op = GitOp::Discard(changes.iter().map(|c| c.path.clone()).collect());
        let path = dir.path.clone();
        self.ask_discard(&changes, Some(&path), op, window, cx);
    }

    /// Icon and label of what switches the changes between list and tree.
    pub(crate) fn git_view_toggle(&self) -> (Lucide, &'static str) {
        match self.settings.git_tree_view {
            true => (Lucide::List, "View as List"),
            false => (Lucide::ListTree, "View as Tree"),
        }
    }

    pub(crate) fn toggle_git_tree_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.git_tree_view = !self.settings.git_tree_view;
        crate::settings::save(&self.settings);
        let (rows, keys) = self.git_listing();
        self.sync_git_rows(rows, keys, window, cx);
        cx.notify();
    }

    fn git_row_key(
        &mut self,
        key: &SharedString,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (name, mods) = (ev.keystroke.key.as_str(), &ev.keystroke.modifiers);
        if name == "enter" && mods.alt && !mods.platform && !mods.control && !mods.shift {
            cx.stop_propagation();
            return self.open_git_file(key, window, cx);
        }
        if mods.modified() {
            return;
        }
        if name == "escape" {
            cx.stop_propagation();
            return self.close_changes(window, cx);
        }
        if matches!(name, "delete" | "backspace") {
            cx.stop_propagation();
            if key.starts_with("stash-") {
                return self.confirm_drop_stash(key, window, cx);
            }
            if key.starts_with("worktree-") {
                return self.remove_tree(key, window, cx);
            }
            return self.confirm_discard(key, window, cx);
        }
        let Some(from) = self.git.keys.iter().position(|k| k == key) else {
            return;
        };
        let to = match fold(&self.git.rows, from, name) {
            Some(Fold::Toggle) => {
                cx.stop_propagation();
                return self.toggle_dir(key, window, cx);
            }
            Some(Fold::Focus(to)) => Some(to),
            None => step(from, self.git.keys.len(), name),
        };
        let Some(to) = to else {
            return;
        };
        cx.stop_propagation();
        self.git.list.scroll_to_reveal_item(to);
        let target = self.git.keys[to].clone();
        if let Some(handle) = self.git.focus.get(&target).cloned() {
            window.focus(&handle, cx);
        }
        self.git.cursor = Some(target);
        cx.notify();
    }

    /// Lines the list's rows and focus handles up with the current changes,
    /// keeping the scroll position and the focused file.
    pub(super) fn sync_git_rows(
        &mut self,
        rows: Vec<Row>,
        keys: Vec<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let git = &mut self.git;
        git.rows = rows;
        if keys != git.keys {
            let lost = git.keys.iter().position(|k| {
                !keys.contains(k) && git.focus.get(k).is_some_and(|h| h.is_focused(window))
            });
            let live: HashSet<&SharedString> = keys.iter().collect();
            git.focus.retain(|key, _| live.contains(key));
            for key in &keys {
                git.focus.entry(key.clone()).or_insert_with(|| cx.focus_handle().tab_stop(false));
            }
            let top = git.list.logical_scroll_top();
            let anchor = (top.item_ix > 0 || top.offset_in_item > px(0.))
                .then(|| git.keys.get(top.item_ix).cloned())
                .flatten();
            let (old, count) = splice_range(&git.keys, &keys);
            let handles: Vec<Option<FocusHandle>> = keys[old.start..old.start + count]
                .iter()
                .map(|key| git.focus.get(key).cloned())
                .collect();
            git.list.splice_focusable(old, handles);
            if let Some(ix) = anchor.and_then(|a| keys.iter().position(|key| *key == a)) {
                git.list.scroll_to(ListOffset { item_ix: ix, offset_in_item: top.offset_in_item });
            }
            // a staged or unstaged file keeps focus in its new section; else the row taking its place gets it
            if let Some(from) = lost {
                let path = key_path(&git.keys[from]);
                let to = keys
                    .iter()
                    .position(|k| path.is_some() && key_path(k) == path)
                    .or_else(|| keys.len().checked_sub(1).map(|last| from.min(last)));
                git.cursor = to.map(|to| keys[to].clone());
                if let Some(to) = to {
                    git.list.scroll_to_reveal_item(to);
                }
                git.refocus = true;
            }
            git.keys = keys;
        }
        let stop = git
            .cursor
            .clone()
            .filter(|c| git.keys.contains(c))
            .or_else(|| git.keys.first().cloned());
        if stop != git.tab_stop {
            for (key, on) in [(&git.tab_stop, false), (&stop, true)] {
                if let Some(handle) = key.as_ref().and_then(|k| git.focus.get(k)) {
                    let _ = handle.clone().tab_stop(on);
                }
            }
            git.tab_stop = stop;
        }
        let folder = self.git_folder();
        let diff_shown = self.git.diff.as_ref().is_some_and(|d| Some(&d.folder) == folder.as_ref());
        // focus left on a diff that closed by itself goes back to the list
        if !diff_shown && self.git.diff_focus.is_focused(window) {
            self.git.refocus = true;
        }
        if std::mem::take(&mut self.git.refocus) {
            let target = match diff_shown {
                true => Some(self.git.diff_focus.clone()),
                false => self.git.tab_stop.as_ref().and_then(|k| self.git.focus.get(k)).cloned(),
            }
            .unwrap_or_else(|| self.git.root.clone());
            window.defer(cx, move |window, cx| window.focus(&target, cx));
        }
    }

    /// The list, or what stands in for it: placeholders, a clean tree, an error.
    pub(super) fn git_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let Some(loaded) = self.git_loaded() else {
            return self.git_placeholder_rows(cx);
        };
        let status = match &loaded.status {
            Ok(status) => status,
            Err(error) => {
                let (title, text) = match error {
                    StatusError::Locked => (
                        "Git is busy",
                        "Another git command is using this repository. If none is running, delete .git/index.lock, then retry."
                            .to_string(),
                    ),
                    StatusError::Failed(e) => ("Couldn't read changes", e.clone()),
                    StatusError::NotRepo => ("Not a git repository", "This folder isn't in a git repository.".into()),
                };
                return note(Lucide::CircleAlert, theme::danger_text(cx), title, text, cx)
                    .child(
                        aether_ui::button("git-retry")
                            .outline()
                            .small()
                            .mt_2()
                            .label("Retry")
                            .loading(self.git.loading.is_some())
                            .on_click(cx.listener(|this, _, _, cx| this.refresh_git(true, cx))),
                    )
                    .into_any_element();
            }
        };
        let clean = status.changes.is_empty().then(|| match push_state(status) {
            Push::Ready { op: GitOp::Push, .. } => "Everything is committed. Push to share it.",
            _ => "Files this chat's agent changes show up here.",
        });
        if let Some(text) = clean
            && self.git.rows.is_empty()
        {
            return note(Lucide::CircleCheck, muted, "No changes", text, cx).into_any_element();
        }
        let view = cx.entity();
        let list_state = self.git.list.clone();
        let count = self.git.rows.len();
        let body = list(list_state.clone(), move |ix, window, cx| {
            let this = view.read(cx);
            div()
                .px_1()
                .when(ix == 0, |el| el.pt_1())
                .when(ix + 1 == count, |el| el.pb_2())
                .when(ix > 0 && matches!(this.git.rows.get(ix), Some(Row::Header(..))), |el| {
                    el.pt_2()
                })
                .child(this.git_row(ix, &view, window, cx))
                .into_any_element()
        })
        .size_full();
        v_flex()
            .flex_1()
            .min_h_0()
            .children(clean.map(|text| {
                h_flex()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .text_color(muted)
                    .child(Icon::new(Lucide::CircleCheck).size(rems(0.875)).flex_none())
                    .child(div().flex_1().min_w_0().child(format!("No changes. {text}")))
            }))
            .when(status.truncated, |el| {
                el.child(
                    h_flex()
                        .px_3()
                        .py_1p5()
                        .gap_2()
                        .text_xs()
                        .text_color(muted)
                        .child(Icon::new(Lucide::Info).size(rems(0.875)).flex_none())
                        .child("Too many changes to list them all; showing the first ones."),
                )
            })
            .child(
                div().id("git-list").flex_1().min_h_0().child(body).vertical_scrollbar(&list_state),
            )
            .into_any_element()
    }

    /// Rows in the final layout while the first read runs.
    pub(super) fn git_placeholder_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        let bar = cx.theme().foreground.opacity(0.07);
        v_flex()
            .flex_1()
            .min_h_0()
            .px_4()
            .pt_2()
            .children([0.55, 0.7, 0.4, 0.62].map(|w| {
                h_flex()
                    .h(rems(ROW_HEIGHT))
                    .gap_2()
                    .items_center()
                    .child(div().size(rems(1.)).corner(px(6.)).bg(bar))
                    .child(div().h(rems(0.625)).w(gpui_kit::relative(w)).corner(px(6.)).bg(bar))
            }))
            .into_any_element()
    }

    fn git_row(&self, ix: usize, view: &Entity<Self>, window: &Window, cx: &App) -> AnyElement {
        let (Some(row), Some(key), Some(status)) =
            (self.git.rows.get(ix), self.git.keys.get(ix), self.git_status())
        else {
            return div().into_any_element();
        };
        let focus = self.git.focus.get(key).cloned();
        let focused = focus.as_ref().is_some_and(|f| f.is_focused(window));
        let busy = self.git.job.is_some();
        let theme = cx.theme();
        let (muted, ring, radius) = (theme.muted_foreground, theme.ring, theme.radius);
        match *row {
            Row::Header(section, files) => {
                let title = section_title(section);
                let collapsed = self.git.collapsed.contains(&section);
                let chevron = if collapsed { Lucide::ChevronRight } else { Lucide::ChevronDown };
                let job = self.git.job.as_ref().map(|job| &job.op);
                let aria = header_label(section, files, collapsed);
                let click = {
                    let view = view.clone();
                    move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        view.update(cx, |this, cx| this.toggle_section(section, window, cx));
                    }
                };
                let run = |op: GitOp| {
                    move |this: &mut ChatApp, window: &mut Window, cx: &mut Context<ChatApp>| {
                        this.run_git(op.clone(), window, cx)
                    }
                };
                let buttons = match section {
                    Section::Merge | Section::Stashes | Section::History => vec![],
                    Section::Worktrees => vec![section_button(
                        "git-create-worktree",
                        IconName::Plus,
                        "Create worktree…",
                        self.git.tree_job == Some(TreeJob::Create),
                        busy || self.git.tree_job.is_some(),
                        view,
                        |this, window, cx| this.open_tree_picker(window, cx),
                    )],
                    Section::Staged => vec![section_button(
                        "git-unstage-all",
                        IconName::Minus,
                        "Unstage all",
                        job == Some(&GitOp::UnstageAll),
                        busy,
                        view,
                        run(GitOp::UnstageAll),
                    )],
                    Section::Changes => vec![
                        section_button(
                            "git-discard-all",
                            Lucide::Undo2,
                            "Discard all changes",
                            matches!(job, Some(GitOp::DiscardAll(_))),
                            busy,
                            view,
                            |this, window, cx| this.confirm_discard_all(window, cx),
                        ),
                        section_button(
                            "git-stage-all",
                            IconName::Plus,
                            "Stage all",
                            job == Some(&GitOp::StageAll),
                            busy,
                            view,
                            run(GitOp::StageAll),
                        ),
                    ],
                };
                h_flex()
                    .id(SharedString::from(format!("git-row-{key}")))
                    .when_some(focus, |el, focus| el.track_focus(&focus))
                    .role(Role::Button)
                    .aria_label(aria)
                    .cursor_pointer()
                    .border_1()
                    .border_color(gpui_kit::transparent_black())
                    .focus_visible(move |s| s.border_color(ring))
                    .rounded(radius)
                    .w_full()
                    .h(rems(ROW_HEIGHT))
                    .px_2()
                    .gap_1p5()
                    .items_center()
                    .text_xs()
                    .text_color(muted)
                    .hover(|s| s.bg(theme.muted.opacity(0.4)))
                    .child(Icon::new(chevron).size(rems(0.75)).flex_none().text_color(muted))
                    .child(div().font_semibold().text_color(theme.foreground).child(title))
                    .when(files > 0 || section != Section::History, |el| {
                        el.child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .corner(px(10.))
                                .bg(field_fill(cx))
                                .text_xs()
                                .map(tabular)
                                .child(format_full(files as u64)),
                        )
                    })
                    .child(div().flex_1())
                    .children(buttons)
                    .on_click(click)
                    .on_key_down(self.row_keys(key, view))
                    .into_any_element()
            }
            Row::Dir(ref dir) => {
                let files = files_under(&status.changes, dir.section, &dir.path);
                let job = self.git.job.as_ref().map(|job| &job.op);
                let stage = stage_op(dir.section, &files);
                let discard = GitOp::Discard(files.iter().map(|c| c.path.clone()).collect());
                let (staging, discarding) = (job == Some(&stage), job == Some(&discard));
                let show_action =
                    focused || staging || discarding || self.git.cursor.as_ref() == Some(key);
                let group = SharedString::from(format!("git-group-{key}"));
                let (chevron, icon) = match dir.collapsed {
                    true => (Lucide::ChevronRight, Lucide::Folder),
                    false => (Lucide::ChevronDown, Lucide::FolderOpen),
                };
                let (tip, icon_name) = match dir.section {
                    Section::Staged => ("Unstage folder", IconName::Minus),
                    Section::Merge => {
                        ("Stage folder, marking its conflicts resolved", IconName::Plus)
                    }
                    _ => ("Stage folder", IconName::Plus),
                };
                let stage_button = {
                    let key = key.clone();
                    section_button(
                        SharedString::from(format!("git-dir-stage-{key}")),
                        icon_name,
                        tip,
                        staging,
                        busy,
                        view,
                        move |this, window, cx| this.stage_dir(&key, window, cx),
                    )
                };
                let discard_button = (dir.section == Section::Changes).then(|| {
                    let key = key.clone();
                    section_button(
                        SharedString::from(format!("git-dir-discard-{key}")),
                        Lucide::Undo2,
                        "Discard folder…",
                        discarding,
                        busy,
                        view,
                        move |this, window, cx| this.confirm_discard_dir(&key, window, cx),
                    )
                });
                let click = {
                    let (view, key) = (view.clone(), key.clone());
                    move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        view.update(cx, |this, cx| this.toggle_dir(&key, window, cx));
                    }
                };
                let path = SharedString::from(dir.path.clone());
                let folder_color = theme::mango_text(cx);
                row_style(
                    h_flex()
                        .id(SharedString::from(format!("git-row-{key}")))
                        .when_some(focus, |el, focus| el.track_focus(&focus))
                        .role(Role::Button)
                        .aria_label(dir_label(dir, files.len())),
                    false,
                    cx,
                )
                .group(group.clone())
                .w_full()
                .h(px(24.))
                .px_2()
                .pl(px(indent(dir.depth)))
                .corner(rems(0.375))
                .text_xs()
                .gap_1p5()
                .items_center()
                .tooltip(move |window, cx| Tooltip::new(path.clone()).build(window, cx))
                .child(Icon::new(chevron).size(px(12.)).flex_none().text_color(muted))
                .child(Icon::new(icon).size(px(14.)).flex_none().text_color(folder_color))
                .child(
                    div()
                        .truncate()
                        .font_medium()
                        .text_color(theme::warning_text(cx))
                        .child(dir.name.clone()),
                )
                .child(div().flex_none().size(px(5.)).rounded_full().bg(folder_color))
                .child(div().flex_1())
                .child(hover_actions(
                    group,
                    show_action,
                    div(),
                    h_flex().children(discard_button).child(stage_button),
                    cx,
                ))
                .on_click(click)
                .on_key_down(self.row_keys(key, view))
                .into_any_element()
            }
            Row::File(change_ix) | Row::TreeFile(change_ix, _) => {
                let depth = match *row {
                    Row::TreeFile(_, depth) => Some(depth),
                    _ => None,
                };
                let Some(change) = status.changes.get(change_ix) else {
                    return div().into_any_element();
                };
                let running = self.git.busy_with(change);
                let discarding = running
                    && matches!(
                        self.git.job.as_ref().map(|job| &job.op),
                        Some(GitOp::Discard(_) | GitOp::DiscardAll(_))
                    );
                let (name, dir) = split_path(&change.path);
                let group = SharedString::from(format!("git-group-{key}"));
                let action = if change.staged { "Unstage" } else { "Stage" };
                let mut aria = vec![change.path.clone(), describe(change)];
                aria.extend(stat_words(change.stat));
                let tip = match &change.from {
                    Some(from) => format!("{}\n{} from {from}", change.path, change.kind.label()),
                    None => change.path.clone(),
                };
                let show_action = focused || running || self.git.cursor.as_ref() == Some(key);
                let button = {
                    let (view, key) = (view.clone(), key.clone());
                    let tooltip = if change.kind == ChangeKind::Conflicted && !change.staged {
                        "Stage, marking the conflict resolved"
                    } else {
                        action
                    };
                    aether_ui::button(SharedString::from(format!("git-action-{key}")))
                        .ghost()
                        .xsmall()
                        .icon(if change.staged { IconName::Minus } else { IconName::Plus })
                        .accessibility_label(format!("{action} {}", change.path))
                        .tooltip(tooltip)
                        .tab_stop(false)
                        .loading(running && !discarding)
                        .disabled(busy && !running)
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            view.update(cx, |this, cx| this.toggle_stage(&key, window, cx));
                        })
                };
                let open_file = {
                    let (view, key) = (view.clone(), key.clone());
                    aether_ui::button(SharedString::from(format!("git-open-{key}")))
                        .ghost()
                        .xsmall()
                        .icon(Lucide::FileOutput)
                        .accessibility_label(format!("Open {}", change.path))
                        .tooltip("Open file")
                        .tab_stop(false)
                        .disabled(change.kind == ChangeKind::Deleted)
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            view.update(cx, |this, cx| this.open_git_file(&key, window, cx));
                        })
                };
                let discard = (section_of(change) == Section::Changes).then(|| {
                    let (view, key) = (view.clone(), key.clone());
                    let (tip, verb) = if discard_deletes(change) {
                        ("Delete file", "Delete")
                    } else {
                        ("Discard changes", "Discard changes in")
                    };
                    aether_ui::button(SharedString::from(format!("git-discard-{key}")))
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Undo2)
                        .accessibility_label(format!("{verb} {}", change.path))
                        .tooltip(tip)
                        .tab_stop(false)
                        .loading(discarding)
                        .disabled(busy && !discarding)
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            view.update(cx, |this, cx| this.confirm_discard(&key, window, cx));
                        })
                });
                let open = {
                    let (view, key) = (view.clone(), key.clone());
                    move |ev: &ClickEvent, window: &mut Window, cx: &mut App| {
                        let space = matches!(ev, ClickEvent::Keyboard(k) if k.button == KeyboardButton::Space);
                        view.update(cx, |this, cx| {
                            if space {
                                this.toggle_stage(&key, window, cx)
                            } else {
                                this.open_diff(key.clone(), window, cx)
                            }
                        });
                    }
                };
                let menu = {
                    let (view, key, change) = (view.clone(), key.clone(), change.clone());
                    let path = status.top.join(&change.path);
                    move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
                        git_row_menu(menu, &view, &key, &change, path.clone(), busy)
                    }
                };
                let color = explorer_tone(change.kind, cx);
                let actions = h_flex().child(open_file).children(discard).child(button);
                let styled = row_style(
                    h_flex()
                        .id(SharedString::from(format!("git-row-{key}")))
                        .when_some(focus, |el, focus| el.track_focus(&focus))
                        .role(Role::Button)
                        .aria_label(aria.join(", ")),
                    false,
                    cx,
                )
                .group(group.clone())
                .w_full()
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx));
                match depth {
                    // Tree rows mirror the explorer's file tree rows.
                    Some(depth) => styled
                        .h(px(24.))
                        .px_2()
                        .pl(px(indent(depth)))
                        .corner(rems(0.375))
                        .text_xs()
                        .gap_1p5()
                        .items_center()
                        .child(div().w(px(12.)).flex_none())
                        .child(
                            Icon::new(aether_ui::file_icon(std::path::Path::new(&change.path)))
                                .size(px(14.))
                                .flex_none()
                                .text_color(color),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(color)
                                .when(change.kind == ChangeKind::Deleted, |el| el.line_through())
                                .child(name.to_string()),
                        )
                        .child(hover_actions(
                            group.clone(),
                            show_action,
                            h_flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_bold()
                                        .text_color(color)
                                        .child(change.kind.letter().to_string()),
                                )
                                .child(stat_element(change.stat, cx)),
                            actions,
                            cx,
                        )),
                    None => styled
                        .h(rems(ROW_HEIGHT))
                        .pl_3()
                        .pr_2()
                        .gap_2()
                        .items_center()
                        .child(
                            h_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1p5()
                                .items_baseline()
                                .overflow_hidden()
                                .child(
                                    div()
                                        .flex_none()
                                        .max_w_full()
                                        .truncate()
                                        .text_sm()
                                        .font_medium()
                                        .text_color(tone(change.kind, cx))
                                        .when(change.kind == ChangeKind::Deleted, |el| {
                                            el.line_through()
                                        })
                                        .child(name.to_string()),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis_middle()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(dir.to_string()),
                                ),
                        )
                        .child(hover_actions(
                            group.clone(),
                            show_action,
                            h_flex()
                                .gap_2()
                                .child(stat_element(change.stat, cx))
                                .child(letter_chip(change.kind, cx)),
                            actions,
                            cx,
                        )),
                }
                .on_click(open)
                .on_key_down(self.row_keys(key, view))
                .context_menu(menu)
                .into_any_element()
            }
            Row::Stash(ix) => self.stash_row(ix, key, view, window, cx),
            Row::Worktree(ix) => self.tree_row(ix, key, view, window, cx),
            Row::Commit(ix) => self.commit_row(ix, key, view, cx),
            Row::HistoryNote => self.history_note(key, view, cx),
        }
    }

    pub(super) fn row_keys(
        &self,
        key: &SharedString,
        view: &Entity<Self>,
    ) -> impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static {
        let (view, key) = (view.clone(), key.clone());
        move |ev, window, cx| view.update(cx, |this, cx| this.git_row_key(&key, ev, window, cx))
    }

    /// Opens a row's file in this chat's explorer; a deleted one has none.
    fn open_git_file(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.git_status().and_then(|s| {
            let change = s.changes.iter().find(|c| file_key(c) == key)?;
            (change.kind != ChangeKind::Deleted).then(|| s.top.join(&change.path))
        }) else {
            return;
        };
        self.open_in_explorer(path, window, cx);
    }

    /// Asks before discarding a row's work-tree change.
    fn confirm_discard(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(change) = self.git_status().and_then(|s| {
            s.changes.iter().find(|c| file_key(c) == key && section_of(c) == Section::Changes)
        }) else {
            return;
        };
        let change = change.clone();
        let op = GitOp::Discard(vec![change.path.clone()]);
        self.ask_discard(std::slice::from_ref(&change), None, op, window, cx);
    }

    /// Asks before discarding every change in the Changes section.
    pub(super) fn confirm_discard_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(status) = self.git_status() else {
            return;
        };
        let changes: Vec<Change> =
            status.changes.iter().filter(|c| section_of(c) == Section::Changes).cloned().collect();
        let op = GitOp::DiscardAll(changes.iter().map(|c| c.path.clone()).collect());
        self.ask_discard(&changes, None, op, window, cx);
    }

    fn ask_discard(
        &mut self,
        changes: &[Change],
        dir: Option<&str>,
        op: GitOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((folder, top)) = self.git_target() else {
            return;
        };
        if changes.is_empty() || self.git.job.is_some() {
            return;
        }
        let (title, text, ok) = match dir {
            Some(dir) => discard_dir_prompt(dir, changes),
            None => discard_prompt(changes),
        };
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let (view, op) = (view.clone(), op.clone());
            let (folder, top) = (folder.clone(), top.clone());
            alert
                .confirm()
                .title(SharedString::from(title.clone()))
                .description(SharedString::from(text.clone()))
                .ok_text(ok)
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let (folder, top, op) = (folder.clone(), top.clone(), op.clone());
                    view.update(cx, |this, cx| this.run_git_in(folder, top, op, None, window, cx));
                    true
                })
        });
    }
}

/// A section caption's action button; clicks stay off the caption's own toggle.
fn section_button(
    id: impl Into<ElementId>,
    icon: impl Into<Icon>,
    tip: &'static str,
    running: bool,
    busy: bool,
    view: &Entity<ChatApp>,
    act: impl Fn(&mut ChatApp, &mut Window, &mut Context<ChatApp>) + 'static,
) -> Button {
    let view = view.clone();
    aether_ui::button(id)
        .ghost()
        .xsmall()
        .icon(icon)
        .accessibility_label(tip)
        .tooltip(tip)
        .tab_stop(false)
        .loading(running)
        .disabled(busy && !running)
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            view.update(cx, |this, cx| act(this, window, cx));
        })
}

/// Right-click menu of a file row: what its row does, plus Open file,
/// Discard, Reveal and Copy.
fn git_row_menu(
    menu: PopupMenu,
    view: &Entity<ChatApp>,
    key: &SharedString,
    change: &Change,
    path: PathBuf,
    busy: bool,
) -> PopupMenu {
    let (open, stage) = ((view.clone(), key.clone()), (view.clone(), key.clone()));
    let (file, discard) = ((view.clone(), key.clone()), (view.clone(), key.clone()));
    let reveal = path.clone();
    let reveal_label =
        if cfg!(target_os = "macos") { "Reveal in Finder" } else { "Show in folder" };
    let discard_label =
        if discard_deletes(change) { "Delete file…" } else { "Discard changes…" };
    menu.item(PopupMenuItem::new("Open diff").icon(IconName::FileText).on_click(
        move |_, window, cx| {
            open.0.update(cx, |this, cx| this.open_diff(open.1.clone(), window, cx))
        },
    ))
    .item(
        PopupMenuItem::new("Open file")
            .icon(Lucide::FileOutput)
            .disabled(change.kind == ChangeKind::Deleted)
            .on_click(move |_, window, cx| {
                file.0.update(cx, |this, cx| this.open_git_file(&file.1, window, cx))
            }),
    )
    .separator()
    .item(
        PopupMenuItem::new(if change.staged { "Unstage" } else { "Stage" })
            .icon(if change.staged { IconName::Minus } else { IconName::Plus })
            .disabled(busy)
            .on_click(move |_, window, cx| {
                stage.0.update(cx, |this, cx| this.toggle_stage(&stage.1, window, cx))
            }),
    )
    .when(section_of(change) == Section::Changes, |menu| {
        menu.item(PopupMenuItem::new(discard_label).icon(Lucide::Undo2).disabled(busy).on_click(
            move |_, window, cx| {
                discard.0.update(cx, |this, cx| this.confirm_discard(&discard.1, window, cx))
            },
        ))
    })
    .separator()
    .item(
        PopupMenuItem::new(reveal_label)
            .icon(IconName::FolderOpen)
            .disabled(change.kind == ChangeKind::Deleted)
            .on_click(move |_, _, cx| cx.reveal_path(&reveal)),
    )
    .item(PopupMenuItem::new("Copy path").icon(IconName::Copy).on_click(move |_, _, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(path.display().to_string()))
    }))
}
