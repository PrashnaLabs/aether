use aether_ui::theme::Corners;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use aether_git::{GitOp, Status};
use aether_ui::{field_fill, theme};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{ButtonVariants, DropdownButton};
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, IconName, Sizable, h_flex, v_flex};
use gpui_kit::{Anchor, AnyElement, Context, Entity, Window, div, prelude::*, px, rems};

use super::rows::{changed_files, file_key};
use super::{Feedback, Job, Loaded};
use crate::actions::{COMMIT_CONTEXT, CommitChanges};
use crate::app::{ChatApp, format_full};

/// The commit button's label, or why it's off.
fn commit_state(staged: usize, changed: usize, message: &str) -> Result<String, &'static str> {
    if changed == 0 {
        return Err("Nothing to commit");
    }
    if message.trim().is_empty() {
        return Err("Write a commit message to commit");
    }
    let (files, suffix) = if staged > 0 {
        (staged, if staged == 1 { "staged file" } else { "staged files" })
    } else {
        (changed, if changed == 1 { "file" } else { "files" })
    };
    Ok(format!("Commit {} {suffix}", format_full(files as u64)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CommitAction {
    Commit,
    All,
    Amend,
    Push,
    Sync,
    Undo,
}

impl CommitAction {
    pub(super) const ALL: [Self; 6] =
        [Self::Commit, Self::All, Self::Amend, Self::Push, Self::Sync, Self::Undo];

    fn label(self) -> &'static str {
        match self {
            Self::Commit => "Commit",
            Self::All => "Commit All",
            Self::Amend => "Commit (Amend)",
            Self::Push => "Commit & Push",
            Self::Sync => "Commit & Sync",
            Self::Undo => "Undo Last Commit",
        }
    }
}

/// Whether `action` can run on this repo and message, or why not.
pub(super) fn commit_allowed(
    status: &Status,
    message: &str,
    action: CommitAction,
) -> Result<(), &'static str> {
    let staged = status.changes.iter().filter(|c| c.staged).count();
    let changed = changed_files(&status.changes);
    // amend and undo replace HEAD, so only while its upstream doesn't have it
    let rewritable = || match (&status.commit, &status.upstream, status.ahead_behind) {
        (None, ..) => Err("No commit to amend or undo yet"),
        (_, Some(_), Some((0, _))) => Err("Last commit is pushed: can't amend or undo"),
        _ => Ok(()),
    };
    match action {
        CommitAction::Commit => commit_state(staged, changed, message).map(drop),
        CommitAction::All => commit_state(0, changed, message).map(drop),
        CommitAction::Push => {
            commit_state(staged, changed, message)?;
            match (&status.branch, &status.upstream, &status.remote) {
                (None, ..) => Err("Check out a branch to push"),
                (_, None, None) => Err("No remote to push to"),
                _ => Ok(()),
            }
        }
        CommitAction::Sync => {
            commit_state(staged, changed, message)?;
            match (&status.branch, &status.upstream) {
                (Some(_), Some(_)) => Ok(()),
                _ => Err("Sync needs a branch with an upstream"),
            }
        }
        CommitAction::Amend => rewritable(),
        CommitAction::Undo => {
            rewritable()?;
            if status.parent { Ok(()) } else { Err("The first commit can't be undone") }
        }
    }
}

/// The commit actions as menu items; a line under them says why amend or
/// undo is off, since menu items can't carry tooltips.
pub(super) fn commit_menu(
    menu: PopupMenu,
    view: &Entity<ChatApp>,
    allowed: &[(CommitAction, Result<(), &'static str>)],
    busy: bool,
) -> PopupMenu {
    let mut menu = menu;
    let mut reasons = Vec::new();
    for &(action, ok) in allowed {
        if matches!(action, CommitAction::Push | CommitAction::Undo) {
            menu = menu.separator();
        }
        if let (CommitAction::Amend | CommitAction::Undo, Err(reason)) = (action, ok)
            && !reasons.contains(&reason)
        {
            reasons.push(reason);
        }
        let view = view.clone();
        menu =
            menu.item(PopupMenuItem::new(action.label()).disabled(busy || ok.is_err()).on_click(
                move |_, window, cx| {
                    view.update(cx, |this, cx| this.commit_with(action, window, cx))
                },
            ));
    }
    reasons.into_iter().fold(menu, |menu, reason| menu.label(reason))
}

/// "Committed a1b2c3d · Fix login", the summary cut to fit one line.
fn commit_note(note: &str, message: &str) -> String {
    let summary = message.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    let short: String = summary.chars().take(60).collect();
    let ellipsis = if short.len() < summary.len() { "…" } else { "" };
    format!("{note} · {short}{ellipsis}")
}

/// What to do about a failed fetch, pull, push or sync; `None` when the
/// panel already shows the way (conflicts land in Merge Changes).
fn remote_hint(error: &str) -> Option<&'static str> {
    let has = |words: &[&str]| words.iter().any(|word| error.contains(word));
    if has(&["CONFLICT"]) {
        None
    } else if has(&["rejected", "non-fast-forward", "fetch first"]) {
        Some("The remote has commits this branch doesn't. Sync to pull them in, then push.")
    } else if has(&["divergent", "fast-forward"]) {
        Some(
            "The branch and its upstream have diverged. Reconcile them in the terminal as git suggests.",
        )
    } else {
        Some(
            "If git needs a password, passphrase or host key, run it from the terminal so it can ask.",
        )
    }
}

/// Whether `folder` still reads as the repo at `top`, so an op captured
/// there (a confirm, a chained push) can still run in it.
pub(super) fn still_at(statuses: &HashMap<PathBuf, Loaded>, folder: &Path, top: &Path) -> bool {
    statuses.get(folder).and_then(|l| l.status.as_ref().ok()).is_some_and(|s| s.top == top)
}

fn failed_text(op: &GitOp) -> &'static str {
    match op {
        GitOp::Stage(_) | GitOp::StageAll => "Couldn't stage",
        GitOp::Unstage(_) | GitOp::UnstageAll => "Couldn't unstage",
        GitOp::Discard(_) | GitOp::DiscardAll(_) => "Couldn't discard",
        GitOp::Commit { .. } => "Commit failed",
        GitOp::Amend { .. } => "Amend failed",
        GitOp::UndoCommit => "Couldn't undo the commit",
        GitOp::Push => "Push failed",
        GitOp::Fetch => "Fetch failed",
        GitOp::Pull => "Pull failed",
        GitOp::Sync => "Sync failed",
        GitOp::Stash { .. } => "Couldn't stash",
        GitOp::StashApply { .. } => "Couldn't apply the stash",
        GitOp::StashPop { .. } => "Couldn't pop the stash",
        GitOp::StashDrop { .. } => "Couldn't drop the stash",
        GitOp::StashClear(_) => "Couldn't drop the stashes",
        GitOp::Checkout(_) | GitOp::Track(_) => "Couldn't switch branches",
        GitOp::CreateBranch { .. } => "Couldn't create the branch",
        GitOp::Detach(_) => "Couldn't check out the commit",
    }
}

impl ChatApp {
    /// The active folder and its repo's top, to run an op in later.
    pub(super) fn git_target(&self) -> Option<(PathBuf, PathBuf)> {
        Some((self.git_folder()?, self.git_status()?.top.clone()))
    }

    /// Whether an op captured for `folder`'s repo at `top` can run now; if
    /// that folder changed or closed meanwhile, says so under `failed`.
    pub(super) fn can_run_in(
        &mut self,
        folder: &Path,
        top: &Path,
        failed: &'static str,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.git.job.is_some() || self.switching_folder(folder) {
            return false;
        }
        if still_at(&self.git.statuses, folder, top) {
            return true;
        }
        self.git.feedback = Some(Feedback {
            folder: self.git_folder(),
            error: true,
            text: failed.into(),
            detail: Some("Its folder changed or closed before it ran, so nothing was done.".into()),
            remote: false,
        });
        cx.notify();
        false
    }

    /// Runs one change to the active repo at a time, then re-reads it.
    pub(super) fn run_git(&mut self, op: GitOp, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((folder, top)) = self.git_target() {
            self.run_git_in(folder, top, op, None, window, cx)
        }
    }

    /// [`Self::run_git`] in `folder`'s repo at `top`, as captured when it was
    /// asked for, never whichever chat is active by now; then `then` there if
    /// `op` worked.
    pub(crate) fn run_git_in(
        &mut self,
        folder: PathBuf,
        top: PathBuf,
        op: GitOp,
        then: Option<GitOp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_run_in(&folder, &top, failed_text(&op), cx) {
            return;
        }
        self.git.feedback = None;
        self.git.job = Some(Job { op: op.clone(), done: false });
        let task = self.agent.git_apply(top.clone(), op.clone());
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.git.generation += 1;
                if this.git_folder().as_ref() != Some(&folder) {
                    this.git.invalidate(&folder);
                }
                let remote = matches!(op, GitOp::Fetch | GitOp::Pull | GitOp::Push | GitOp::Sync);
                match result {
                    Ok(note) => {
                        if let Some(job) = &mut this.git.job {
                            job.done = true;
                        }
                        match &op {
                            GitOp::Commit { .. } | GitOp::Amend { .. } | GitOp::Stash { .. } => {
                                this.set_commit_draft(Some(&folder), None, window, cx)
                            }
                            GitOp::UndoCommit => {
                                this.set_commit_draft(Some(&folder), Some(&note), window, cx)
                            }
                            GitOp::Checkout(_)
                            | GitOp::Track(_)
                            | GitOp::CreateBranch { .. }
                            | GitOp::Detach(_) => this.branch_moved(folder.clone(), cx),
                            _ => {}
                        }
                        let note = match &op {
                            GitOp::Commit { message, .. } => commit_note(&note, message),
                            GitOp::Amend { message } | GitOp::Stash { message, .. }
                                if !message.trim().is_empty() =>
                            {
                                commit_note(&note, message)
                            }
                            GitOp::UndoCommit => commit_note("Undid last commit", &note),
                            _ => note,
                        };
                        this.git.feedback = (!note.is_empty()).then(|| Feedback {
                            folder: Some(folder.clone()),
                            error: false,
                            text: note.into(),
                            detail: None,
                            remote,
                        });
                        if let Some(then) = then {
                            this.git.job = None;
                            this.run_git_in(folder, top, then, None, window, cx);
                        }
                    }
                    Err(e) => {
                        this.git.job = None;
                        this.pop_git_failed(
                            &folder,
                            failed_text(&op),
                            &e,
                            this.host_of(window),
                            cx,
                        );
                        this.git.feedback = Some(Feedback {
                            folder: Some(folder),
                            error: true,
                            text: failed_text(&op).into(),
                            detail: Some(e.into()),
                            remote,
                        });
                    }
                }
                this.refresh_git(false, cx);
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Stages a work-tree change, or unstages a staged one (a rename with its old path).
    pub(super) fn toggle_stage(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(change) =
            self.git_status().and_then(|s| s.changes.iter().find(|c| file_key(c) == key))
        else {
            return;
        };
        let op = if change.staged {
            GitOp::Unstage(change.from.iter().cloned().chain([change.path.clone()]).collect())
        } else {
            GitOp::Stage(vec![change.path.clone()])
        };
        self.run_git(op, window, cx);
    }

    pub(super) fn commit_with(
        &mut self,
        action: CommitAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(status) = self.git_status() else {
            return;
        };
        let message = self.git.message.read(cx).value().to_string();
        if commit_allowed(status, &message, action).is_err() {
            return;
        }
        let stage_all = action == CommitAction::All || !status.changes.iter().any(|c| c.staged);
        let op = match action {
            CommitAction::Amend => GitOp::Amend { message },
            CommitAction::Undo => GitOp::UndoCommit,
            _ => GitOp::Commit { message, stage_all },
        };
        let then = match action {
            CommitAction::Push => Some(GitOp::Push),
            CommitAction::Sync => Some(GitOp::Sync),
            _ => None,
        };
        if let Some((folder, top)) = self.git_target() {
            self.run_git_in(folder, top, op, then, window, cx);
        }
    }

    /// After a commit, clears `folder`'s message; after an undo, puts the
    /// undone message back unless something new was typed.
    fn set_commit_draft(
        &mut self,
        folder: Option<&PathBuf>,
        undone: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git.draft_for.as_ref() == folder {
            let typed = !self.git.message.read(cx).value().trim().is_empty();
            match undone {
                None => self.git.message.update(cx, |s, cx| s.clean(window, cx)),
                Some(message) if !typed => {
                    self.git.message.update(cx, |s, cx| s.set_value(message, window, cx))
                }
                Some(_) => {}
            }
        } else if let Some(folder) = folder {
            match undone {
                None => {
                    self.git.drafts.remove(folder);
                }
                Some(message) => {
                    let draft = self.git.drafts.entry(folder.clone()).or_default();
                    if draft.trim().is_empty() {
                        *draft = message.to_string();
                    }
                }
            }
        }
    }

    /// Each repo folder keeps its own unsent commit message, so one written
    /// for another repo is never committed here.
    pub(super) fn swap_commit_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let folder = self.git_folder();
        if folder == self.git.draft_for {
            return;
        }
        let current = self.git.message.read(cx).value().to_string();
        if let Some(old) = self.git.draft_for.take() {
            self.git.drafts.insert(old, current);
        }
        let next = folder.as_ref().and_then(|f| self.git.drafts.remove(f)).unwrap_or_default();
        self.git.message.update(cx, |s, cx| s.set_value(next, window, cx));
        self.git.draft_for = folder;
    }

    /// Typing a new message clears the last success line; clearing the
    /// field after a commit doesn't.
    pub(crate) fn git_message_changed(&mut self, cx: &mut Context<Self>) {
        let typing = !self.git.message.read(cx).value().is_empty();
        if typing && self.git.feedback.as_ref().is_some_and(|f| !f.error) {
            self.git.feedback = None;
        }
        cx.notify();
    }

    pub(super) fn feedback_here(&self) -> Option<&Feedback> {
        self.git.feedback.as_ref().filter(|f| f.folder == self.git_folder())
    }

    /// Top-loaded commit composer: message field, full-width Commit button, auto-staging.
    pub(super) fn git_commit_area(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let status = self.git_status()?;
        let theme = cx.theme();
        let border = theme.sidebar_border;
        let staged = status.changes.iter().filter(|c| c.staged).count();
        let changed = changed_files(&status.changes);
        let message = self.git.message.read(cx).value().to_string();
        let commit = commit_state(staged, changed, &message);
        let job = self.git.job.as_ref().map(|job| &job.op);
        let committing =
            matches!(job, Some(GitOp::Commit { .. } | GitOp::Amend { .. } | GitOp::UndoCommit));
        let feedback = self.feedback_here().is_some();
        if changed == 0 && !feedback {
            return None;
        }
        let commit_button = (changed > 0).then(|| {
            let (label, reason) = match &commit {
                Ok(label) => (label.clone(), None),
                Err(reason) => ("Commit".to_string(), Some(*reason)),
            };
            let blocked = reason.is_some() || (job.is_some() && !committing);
            let button = aether_ui::button("git-commit")
                .flex_1()
                .icon(Lucide::GitCommitHorizontal)
                .label(label)
                .loading(committing)
                .disabled(blocked)
                .map(|b| match reason {
                    Some(reason) => b.tooltip(reason),
                    None => b.tooltip_with_action("Commit", &CommitChanges, Some(COMMIT_CONTEXT)),
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    this.commit_with(CommitAction::Commit, window, cx)
                }));
            let allowed = CommitAction::ALL.map(|a| (a, commit_allowed(status, &message, a)));
            let (view, busy) = (cx.entity(), job.is_some());
            // both halves share one variant; a bright caret beside a disabled commit reads wrong
            DropdownButton::new("git-commit-split")
                .button(button)
                .when(!blocked, |split| split.primary())
                .w_full()
                .menu_accessibility_label("More commit actions")
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                    commit_menu(menu, &view, &allowed, busy)
                })
        });
        Some(IntoElement::into_any_element(
            v_flex()
                .flex_none()
                .p_3()
                .gap_2p5()
                .border_b_1()
                .border_color(border)
                .when(changed > 0, |area| {
                    area.child(
                        div()
                            .key_context(COMMIT_CONTEXT)
                            .on_action(cx.listener(|this, _: &CommitChanges, window, cx| {
                                this.commit_with(CommitAction::Commit, window, cx)
                            }))
                            .child(
                                div()
                                    .w_full()
                                    .bg(field_fill(cx))
                                    .corner(px(6.))
                                    .border_1()
                                    .border_color(border)
                                    .text_sm()
                                    .px(px(0.))
                                    .py(px(0.))
                                    .child(Textarea::new(&self.git.message).appearance(false)),
                            ),
                    )
                })
                .children(commit_button)
                .children(self.git_feedback(cx)),
        ))
    }

    pub(super) fn git_feedback(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let feedback = self.feedback_here()?;
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let (icon, color) = if feedback.error {
            (Lucide::CircleAlert, theme::danger_text(cx))
        } else {
            (Lucide::CircleCheck, theme::success_text(cx))
        };
        let dismiss = aether_ui::button("git-feedback-dismiss")
            .ghost()
            .xsmall()
            .icon(IconName::Close)
            .accessibility_label("Dismiss")
            .tooltip("Dismiss")
            .on_click(cx.listener(|this, _, _, cx| {
                this.git.feedback = None;
                cx.notify();
            }));
        let detail = feedback.detail.clone().map(|detail| {
            div()
                .relative()
                .child(
                    div()
                        .id("git-feedback-detail")
                        .max_h(px(140.))
                        .overflow_y_scrollbar()
                        .p_2()
                        .pr_8()
                        .corner(px(6.))
                        .bg(field_fill(cx))
                        .font_family(theme.mono_font_family.clone())
                        .text_xs()
                        .child(detail.clone()),
                )
                .child(div().absolute().top_1().right_1().child(
                    Clipboard::new("git-feedback-copy").value(detail).tooltip("Copy").xsmall(),
                ))
        });
        let hint = (feedback.error && feedback.remote)
            .then(|| remote_hint(feedback.detail.as_deref().unwrap_or_default()))
            .flatten()
            .map(|hint| {
                h_flex()
                    .gap_2()
                    .items_start()
                    .text_xs()
                    .text_color(muted)
                    .child(div().flex_1().min_w_0().child(hint))
                    .child(
                        aether_ui::button("git-open-terminal")
                            .outline()
                            .xsmall()
                            .icon(IconName::SquareTerminal)
                            .label("Open terminal")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_terminal(window, cx)),
                            ),
                    )
            });
        Some(
            v_flex()
                .gap_1p5()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .text_xs()
                        .child(Icon::new(icon).size(rems(0.875)).flex_none().text_color(color))
                        .child(div().flex_1().min_w_0().truncate().child(feedback.text.clone()))
                        .child(dismiss),
                )
                .children(detail)
                .children(hint)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
#[path = "../../tests/unit/git_panel/commit.rs"]
mod tests;
