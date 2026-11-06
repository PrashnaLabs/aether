use std::cell::Cell;
use std::path::PathBuf;

use aether_git::{Commit, Flow, GitOp, Status};
use aether_ui::{field, row_style, tabular, theme};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::input::InputState;
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::{
    Anchor, AnyElement, App, ClipboardItem, Context, Div, Entity, Hsla, Role, SharedString, Window,
    div, prelude::*, rems,
};

use super::Feedback;
use super::diff::back_button;
use super::list::ROW_HEIGHT;
use super::rows::{Section, commit_key, commit_label};
use crate::app::{ChatApp, now_ms};
use crate::projects::relative_time;

/// HEAD, its upstream and how far apart they are: history read at other ones is stale.
type HistoryKey = (Option<String>, Option<String>, Option<(u32, u32)>);

fn history_key(status: &Status) -> HistoryKey {
    (status.commit.clone(), status.upstream.clone(), status.ahead_behind)
}

pub(super) struct History {
    key: HistoryKey,
    /// `None` while the first read runs; a re-read keeps the last good one up.
    commits: Option<Result<Vec<Commit>, SharedString>>,
}

/// "5m", "Yesterday", "Oct 12": how long ago Unix time `secs` was.
pub(super) fn age(secs: i64) -> String {
    relative_time(secs.saturating_mul(1000), now_ms(), &chrono::Local)
}

/// Unix time `secs` as a local date and time.
pub(super) fn date(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|at| at.with_timezone(&chrono::Local).format("%b %-d, %Y, %-I:%M %p").to_string())
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CommitItem {
    CopyId,
    CopyMessage,
    Open,
    Branch,
    Detach,
}

impl CommitItem {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::CopyId => "Copy commit ID",
            Self::CopyMessage => "Copy message",
            Self::Open => "Open changes",
            Self::Branch => "Create branch here…",
            Self::Detach => "Check out (detached)…",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::CopyId | Self::CopyMessage => Icon::new(IconName::Copy),
            Self::Open => Icon::new(IconName::FileText),
            Self::Branch => Icon::new(Lucide::GitBranchPlus),
            Self::Detach => Icon::new(Lucide::GitCommitHorizontal),
        }
    }
}

/// A commit's menu items and whether each can run: Open only from its row,
/// since its header is in its own view; branching and checking out wait
/// for the running job, and checking out the commit HEAD is detached at does nothing.
pub(super) fn commit_items(busy: bool, detached_here: bool, row: bool) -> Vec<(CommitItem, bool)> {
    let mut items = vec![(CommitItem::CopyId, true), (CommitItem::CopyMessage, true)];
    if row {
        items.push((CommitItem::Open, true));
    }
    items.extend([(CommitItem::Branch, !busy), (CommitItem::Detach, !busy && !detached_here)]);
    items
}

/// Whether HEAD is detached at `commit`.
fn detached_at(status: Option<&Status>, commit: &Commit) -> bool {
    status.is_some_and(|s| {
        s.branch.is_none() && s.commit.as_ref().is_some_and(|c| commit.sha.starts_with(c.as_str()))
    })
}

fn history_menu(
    menu: PopupMenu,
    view: &Entity<ChatApp>,
    commit: &Commit,
    items: &[(CommitItem, bool)],
) -> PopupMenu {
    items.iter().fold(menu, |menu, &(item, on)| {
        let menu = if matches!(item, CommitItem::Open | CommitItem::Branch) {
            menu.separator()
        } else {
            menu
        };
        let (view, commit) = (view.clone(), commit.clone());
        menu.item(PopupMenuItem::new(item.label()).icon(item.icon()).disabled(!on).on_click(
            move |_, window, cx| {
                view.update(cx, |this, cx| this.commit_act(item, &commit, window, cx))
            },
        ))
    })
}

fn flow_icon(flow: Option<Flow>, cx: &App) -> (Lucide, Hsla) {
    match flow {
        Some(Flow::Outgoing) => (Lucide::ArrowUp, theme::info_text(cx)),
        Some(Flow::Incoming) => (Lucide::ArrowDown, theme::mango_text(cx)),
        None => (Lucide::GitCommitVertical, cx.theme().muted_foreground),
    }
}

impl ChatApp {
    /// The active folder's history as last read; empty until a read lands.
    pub(super) fn history_commits(&self) -> &[Commit] {
        self.git_folder()
            .and_then(|folder| self.git.history.get(&folder))
            .and_then(|history| history.commits.as_ref()?.as_ref().ok())
            .map_or(&[], Vec::as_slice)
    }

    /// Reads the open History section again once HEAD or its upstream moved
    /// (a commit, pull, sync, undo or switch), off the UI thread.
    pub(super) fn sync_history(&mut self, cx: &mut Context<Self>) {
        if self.git.collapsed.contains(&Section::History) {
            return;
        }
        let (Some(folder), Some(status)) = (self.git_folder(), self.git_status()) else {
            return;
        };
        let key = history_key(status);
        if status.commit.is_none() || self.git.history.get(&folder).is_some_and(|h| h.key == key) {
            return;
        }
        let top = status.top.clone();
        let commits =
            self.git.history.remove(&folder).and_then(|h| h.commits).filter(Result::is_ok);
        self.git.history.insert(folder.clone(), History { key: key.clone(), commits });
        let task = cx.background_executor().spawn(async move { aether_git::history(&top) });
        cx.spawn(async move |this, cx| {
            let read = task.await;
            this.update(cx, |this, cx| {
                if let Some(history) = this.git.history.get_mut(&folder)
                    && history.key == key
                {
                    history.commits = Some(read.map_err(SharedString::from));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn commit_act(
        &mut self,
        item: CommitItem,
        commit: &Commit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match item {
            CommitItem::CopyId => {
                cx.write_to_clipboard(ClipboardItem::new_string(commit.sha.clone()))
            }
            CommitItem::CopyMessage => self.copy_commit_message(commit.sha.clone(), cx),
            CommitItem::Open => self.open_diff(commit_key(commit), window, cx),
            CommitItem::Branch => self.ask_branch_at(commit, window, cx),
            CommitItem::Detach => self.confirm_detach(commit, window, cx),
        }
    }

    /// Copies commit `sha`'s whole message, read off the UI thread.
    fn copy_commit_message(&mut self, sha: String, cx: &mut Context<Self>) {
        let Some((folder, top)) = self.git_target() else {
            return;
        };
        let task =
            cx.background_executor().spawn(async move { aether_git::commit_message(&top, &sha) });
        cx.spawn(async move |this, cx| {
            let read = task.await;
            this.update(cx, |this, cx| match read {
                Ok(message) => cx.write_to_clipboard(ClipboardItem::new_string(message)),
                Err(e) => {
                    this.git.feedback = Some(Feedback {
                        folder: Some(folder),
                        error: true,
                        text: "Couldn't copy the message".into(),
                        detail: Some(e.into()),
                        remote: false,
                    });
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Runs a new branch or checkout captured for `folder`, unless an agent
    /// works there now: moving HEAD would change files under it.
    fn switch_in(
        &mut self,
        folder: PathBuf,
        top: PathBuf,
        op: GitOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        self.run_git_in(folder, top, op, None, window, cx);
    }

    /// Asks for a name, then makes a branch at `commit` and switches to it.
    fn ask_branch_at(&mut self, commit: &Commit, window: &mut Window, cx: &mut Context<Self>) {
        let Some((folder, top)) = self.git_target().filter(|_| self.git.job.is_none()) else {
            return;
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Branch name"));
        let (view, sha, first) = (cx.entity(), commit.sha.clone(), Cell::new(true));
        let title = SharedString::from(format!("Create branch at {}", commit.short));
        let subject = SharedString::from(commit.subject.clone());
        aether_ui::theme::open_alert(window, cx, move |alert, window, cx| {
            if first.replace(false) {
                input.update(cx, |input, cx| input.focus(window, cx));
            }
            let (view, input, sha) = (view.clone(), input.clone(), sha.clone());
            let (folder, top) = (folder.clone(), top.clone());
            alert
                .confirm()
                .title(title.clone())
                .description(subject.clone())
                .child(field(&input).small().aria_label("New branch name"))
                .ok_text("Create branch")
                .on_ok(move |_, window, cx| {
                    let name = input.read(cx).value().trim().to_string();
                    if name.is_empty() {
                        return false;
                    }
                    let op = GitOp::CreateBranch { name, base: Some(sha.clone()) };
                    let (folder, top) = (folder.clone(), top.clone());
                    view.update(cx, |this, cx| this.switch_in(folder, top, op, window, cx));
                    true
                })
        });
    }

    /// Asks before checking out `commit` with HEAD detached.
    fn confirm_detach(&mut self, commit: &Commit, window: &mut Window, cx: &mut Context<Self>) {
        let Some((folder, top)) = self.git_target().filter(|_| self.git.job.is_none()) else {
            return;
        };
        let (view, sha) = (cx.entity(), commit.sha.clone());
        let title = SharedString::from(format!("Check out {} with HEAD detached?", commit.short));
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let (view, sha) = (view.clone(), sha.clone());
            let (folder, top) = (folder.clone(), top.clone());
            alert
                .confirm()
                .title(title.clone())
                .description(
                    "You won't be on a branch: new commits belong to none until you create one. \
                     Local changes come along; git stops if they'd be overwritten.",
                )
                .ok_text("Check out")
                .on_ok(move |_, window, cx| {
                    let op = GitOp::Detach(sha.clone());
                    let (folder, top) = (folder.clone(), top.clone());
                    view.update(cx, |this, cx| this.switch_in(folder, top, op, window, cx));
                    true
                })
        });
    }

    /// What stands in for history not read yet, or that couldn't be read.
    pub(super) fn history_note(
        &self,
        key: &SharedString,
        view: &Entity<Self>,
        cx: &App,
    ) -> AnyElement {
        let error = self
            .git_folder()
            .and_then(|folder| self.git.history.get(&folder))
            .and_then(|history| history.commits.as_ref()?.as_ref().err().cloned());
        let ring = cx.theme().ring;
        let (color, text) = match error {
            Some(error) => (theme::danger_text(cx), format!("Couldn't read the history: {error}")),
            None => (cx.theme().muted_foreground, "Reading history…".into()),
        };
        div()
            .id("git-history-note")
            .when_some(self.git.focus.get(key).cloned(), |el, focus| el.track_focus(&focus))
            .role(Role::Status)
            .aria_label(text.clone())
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .focus_visible(|s| s.border_color(ring))
            .on_key_down(self.row_keys(key, view))
            .w_full()
            .min_h(rems(ROW_HEIGHT))
            .pl_3()
            .pr_2()
            .flex()
            .items_center()
            .text_xs()
            .text_color(color)
            .child(text)
            .into_any_element()
    }

    /// A history row: where the commit is, its subject, author and age.
    /// Clicking it opens the commit's changes; its menu copies, branches or checks out.
    pub(super) fn commit_row(
        &self,
        ix: usize,
        key: &SharedString,
        view: &Entity<Self>,
        cx: &App,
    ) -> AnyElement {
        let Some(commit) = self.history_commits().get(ix) else {
            return div().into_any_element();
        };
        let muted = cx.theme().muted_foreground;
        let (icon, color) = flow_icon(commit.flow, cx);
        let age = age(commit.time);
        let tip = format!(
            "{}\n{} · {} · {}",
            commit.subject,
            commit.short,
            commit.author,
            date(commit.time)
        );
        let open = {
            let (view, key) = (view.clone(), key.clone());
            move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
                view.update(cx, |this, cx| this.open_diff(key.clone(), window, cx))
            }
        };
        let items =
            commit_items(self.git.job.is_some(), detached_at(self.git_status(), commit), true);
        let menu = {
            let (view, commit) = (view.clone(), commit.clone());
            move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
                history_menu(menu, &view, &commit, &items)
            }
        };
        row_style(
            h_flex()
                .id(SharedString::from(format!("git-row-{key}")))
                .when_some(self.git.focus.get(key).cloned(), |el, focus| el.track_focus(&focus))
                .role(Role::Button)
                .aria_label(commit_label(commit, &age)),
            false,
            cx,
        )
        .w_full()
        .h(rems(ROW_HEIGHT))
        .pl_3()
        .pr_2()
        .gap_2()
        .items_center()
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .child(Icon::new(icon).size(rems(0.875)).flex_none().text_color(color))
        .child(div().flex_1().min_w_0().truncate().text_sm().child(commit.subject.clone()))
        .child(
            div()
                .flex_none()
                .max_w(rems(7.))
                .truncate()
                .text_xs()
                .text_color(muted)
                .child(commit.author.clone()),
        )
        .child(div().flex_none().text_xs().text_color(muted).map(tabular).child(age))
        .on_click(open)
        .on_key_down(self.row_keys(key, view))
        .context_menu(menu)
        .into_any_element()
    }

    /// The open commit's header: subject, short SHA, author and date, with
    /// Copy SHA and the commit's menu.
    pub(super) fn commit_header(&self, commit: &Commit, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (muted, border, mono) =
            (theme.muted_foreground, theme.sidebar_border, theme.mono_font_family.clone());
        let (icon, color) = flow_icon(commit.flow, cx);
        let flow = match commit.flow {
            Some(Flow::Outgoing) => " · outgoing",
            Some(Flow::Incoming) => " · incoming",
            None => "",
        };
        let items =
            commit_items(self.git.job.is_some(), detached_at(self.git_status(), commit), false);
        let (view, menu_commit) = (cx.entity(), commit.clone());
        let more = aether_ui::button("git-commit-more")
            .ghost()
            .small()
            .icon(Lucide::Ellipsis)
            .accessibility_label(format!("More actions for commit {}", commit.short))
            .tooltip("More actions")
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                history_menu(menu, &view, &menu_commit, &items)
            });
        v_flex()
            .flex_none()
            .px_2()
            .py_1()
            .gap_0p5()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(back_button(cx))
                    .child(Icon::new(icon).size(rems(0.875)).flex_none().text_color(color))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .font_medium()
                            .child(commit.subject.clone()),
                    )
                    .child(
                        Clipboard::new("git-commit-sha")
                            .value(commit.sha.clone())
                            .tooltip("Copy SHA")
                            .accessibility_label(format!("Copy SHA {}", commit.short))
                            .small(),
                    )
                    .child(more),
            )
            .child(
                h_flex()
                    .pl(rems(2.))
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .text_color(muted)
                    .child(div().flex_none().font_family(mono).child(commit.short.clone()))
                    .child(div().flex_1().min_w_0().truncate().child(format!(
                        "{} · {}{flow}",
                        commit.author,
                        date(commit.time)
                    ))),
            )
    }
}

#[cfg(test)]
#[path = "../../tests/unit/git_panel/history.rs"]
mod tests;
