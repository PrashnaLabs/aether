use aether_git::{ChangeKind, GitOp, Status};
use aether_ui::{arrow_count, icon_chip};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::Popup;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Selectable, Sizable, h_flex};
use gpui_kit::{Anchor, AnyElement, Context, Role, Window, div, prelude::*, px};

use super::commit::{CommitAction, commit_allowed, commit_menu};
use super::count;
use super::rows::{Section, section_of};
use super::worktrees::{tree_menu, tree_menu_items};
use crate::app::ChatApp;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Push {
    Hidden,
    Blocked(&'static str),
    Ready { label: String, tip: String, op: GitOp },
}

/// The footer's sync button: Sync when the remote has commits this branch
/// doesn't, Push when there's only something to send (ahead, or a branch
/// that has never been pushed).
pub(crate) fn push_state(status: &Status) -> Push {
    if status.commit.is_none() {
        return Push::Hidden;
    }
    let Some(branch) = &status.branch else {
        return Push::Blocked("Check out a branch to push. HEAD is detached.");
    };
    match (&status.upstream, status.ahead_behind) {
        (None, _) => match &status.remote {
            Some(remote) => Push::Ready {
                label: "Publish branch".into(),
                tip: format!("Push {branch} to {remote} and track it"),
                op: GitOp::Push,
            },
            None => Push::Hidden,
        },
        (Some(upstream), Some((ahead, behind))) if behind > 0 => {
            let pull =
                format!("Pull {} from {upstream}", count(behind as usize, "commit", "commits"));
            match ahead {
                0 => Push::Ready { label: "Sync Changes".into(), tip: pull, op: GitOp::Pull },
                _ => Push::Ready {
                    label: "Sync Changes".into(),
                    tip: format!(
                        "{pull}, then push {}",
                        count(ahead as usize, "commit", "commits")
                    ),
                    op: GitOp::Sync,
                },
            }
        }
        (Some(upstream), Some((ahead, _))) if ahead > 0 => Push::Ready {
            label: format!("Push {}", count(ahead as usize, "commit", "commits")),
            tip: format!("Push to {upstream}"),
            op: GitOp::Push,
        },
        (Some(upstream), None) => Push::Ready {
            label: "Push".into(),
            tip: format!("{upstream} is gone from the remote; pushing makes it again"),
            op: GitOp::Push,
        },
        _ => Push::Hidden,
    }
}

/// Which of Fetch, Pull, Push and Sync can run on this branch.
pub(crate) fn remote_ops(status: &Status) -> [(GitOp, bool); 4] {
    let remote = status.upstream.is_some() || status.remote.is_some();
    let tracked = status.branch.is_some() && status.upstream.is_some();
    let push = status.branch.is_some() && status.commit.is_some() && remote;
    [(GitOp::Fetch, remote), (GitOp::Pull, tracked), (GitOp::Push, push), (GitOp::Sync, tracked)]
}

impl ChatApp {
    /// 28px branch bar atop the panel: branch, ahead/behind, Push action.
    pub(super) fn git_branch_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let status = self.git_status()?;
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let border = theme.sidebar_border;
        let push = push_state(status);
        let job = self.git.job.as_ref().map(|job| &job.op);
        let (icon, branch_name) = match (&status.branch, &status.commit) {
            (Some(branch), _) => (Lucide::GitBranch, branch.clone()),
            (None, Some(sha)) => (Lucide::GitCommitHorizontal, sha.clone()),
            _ => (Lucide::GitBranch, "HEAD".to_string()),
        };
        let counts = status.ahead_behind.unwrap_or_default();
        let push_action = match &push {
            Push::Hidden => None,
            Push::Blocked(reason) => Some(
                aether_ui::button("git-push")
                    .outline()
                    .xsmall()
                    .icon(Lucide::ArrowUpFromLine)
                    .label("Push")
                    .disabled(true)
                    .tooltip(*reason),
            ),
            Push::Ready { label, tip, op } => {
                let running = job == Some(op);
                let op = op.clone();
                let sync = op != GitOp::Push;
                Some(
                    aether_ui::button("git-push")
                        .outline()
                        .xsmall()
                        .icon(match op {
                            GitOp::Push => Lucide::ArrowUpFromLine,
                            _ => Lucide::ArrowDownUp,
                        })
                        .label(if sync { "Sync" } else { "Push" })
                        .accessibility_label(label.clone())
                        .when(sync, |button| {
                            button
                                .accessibility_label(format!("{label}. {tip}"))
                                .child(arrow_count(Lucide::ArrowDown, counts.1))
                                .when(counts.0 > 0, |b| {
                                    b.child(arrow_count(Lucide::ArrowUp, counts.0))
                                })
                        })
                        .tooltip(tip.clone())
                        .loading(running)
                        .disabled(job.is_some() && !running)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.run_git(op.clone(), window, cx)
                        })),
                )
            }
        };
        // a sync button carries the counts itself
        let counts = match &push {
            Push::Ready { op: GitOp::Push, .. } | Push::Hidden | Push::Blocked(_) => counts,
            Push::Ready { .. } => (0, 0),
        };
        let picker = self.branch_picker(cx);
        let open = picker.is_some();
        let trigger = aether_ui::button("git-branch")
            .ghost()
            .xsmall()
            .icon(Icon::new(icon).text_color(muted))
            .child(div().flex_1().min_w_0().text_left().truncate().child(branch_name.clone()))
            .dropdown_caret(true)
            .max_w_full()
            .selected(open)
            .disabled(job.is_some() && !open)
            .accessibility_label(format!("Branch {branch_name}. Switch or create a branch"))
            .tooltip("Switch or create a branch")
            .when(!open, |button| {
                button.on_click(cx.listener(|this, _, window, cx| this.open_picker(window, cx)))
            });
        let branch_info = Popup::new("git-branch-popup", trigger)
            .anchor(Anchor::TopLeft)
            .offset(px(6.))
            .min_w_0()
            .when_some(picker, |popup, picker| popup.content(picker));
        let ahead_info = (counts.0 > 0).then(|| {
            let tip = format!("{} ready to push", count(counts.0 as usize, "commit", "commits"));
            div()
                .id("git-sync-ahead")
                .role(Role::Label)
                .aria_label(tip.clone())
                .text_xs()
                .text_color(muted)
                .child(arrow_count(Lucide::ArrowUp, counts.0))
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        });
        let behind_info = (counts.1 > 0).then(|| {
            let tip =
                format!("{} on remote to pull", count(counts.1 as usize, "commit", "commits"));
            div()
                .id("git-sync-behind")
                .role(Role::Label)
                .aria_label(tip.clone())
                .text_xs()
                .text_color(muted)
                .child(arrow_count(Lucide::ArrowDown, counts.1))
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        });
        Some(
            h_flex()
                .id("git-branch-bar")
                .flex_none()
                .h(px(36.))
                .px_2p5()
                .gap_2()
                .items_center()
                .border_b_1()
                .border_color(border)
                .bg(theme.sidebar)
                .child(div().flex_1().min_w_0().overflow_hidden().child(branch_info))
                .children(ahead_info)
                .children(behind_info)
                .children(push_action)
                .into_any_element(),
        )
    }

    /// The side panel header's "…" menu: remote, commit and whole-list actions.
    pub(crate) fn git_more_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let status = self.git_status()?;
        let busy = self.git.job.is_some();
        let message = self.git.message.read(cx).value().to_string();
        let commits = CommitAction::ALL.map(|a| (a, commit_allowed(status, &message, a)));
        let remote = remote_ops(status);
        let unstaged = status.changes.iter().any(|c| !c.staged);
        let staged = status.changes.iter().any(|c| c.staged);
        let discard = status.changes.iter().any(|c| section_of(c) == Section::Changes);
        let tracked = status.changes.iter().any(|c| c.kind != ChangeKind::Untracked);
        let changed = !status.changes.is_empty();
        let stashed = !status.stashes.is_empty();
        let trees = tree_menu(status, busy, self.git.tree_job);
        let (view_icon, view_label) = self.git_view_toggle();
        let view = cx.entity();
        let menu = move |menu: PopupMenu, window: &mut Window, cx: &mut Context<PopupMenu>| {
            let mut menu = menu;
            for (op, ok) in remote.clone() {
                let (label, icon) = match op {
                    GitOp::Fetch => ("Fetch", Lucide::CloudDownload),
                    GitOp::Pull => ("Pull", Lucide::ArrowDownToLine),
                    GitOp::Push => ("Push", Lucide::ArrowUpFromLine),
                    _ => ("Sync", Lucide::ArrowDownUp),
                };
                let view = view.clone();
                menu =
                    menu.item(PopupMenuItem::new(label).icon(icon).disabled(busy || !ok).on_click(
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.run_git(op.clone(), window, cx))
                        },
                    ));
            }
            let commit_view = view.clone();
            menu = menu.separator().submenu_with_icon(
                Some(Icon::new(Lucide::GitCommitHorizontal)),
                "Commit",
                window,
                cx,
                move |menu, _, _| commit_menu(menu, &commit_view, &commits, busy),
            );
            let stash_view = view.clone();
            menu = menu.submenu_with_icon(
                Some(Icon::new(Lucide::Archive)),
                "Stash",
                window,
                cx,
                move |menu, _, _| {
                    let (plain, untracked) = (stash_view.clone(), stash_view.clone());
                    let clear = stash_view.clone();
                    menu.item(
                        PopupMenuItem::new("Stash changes…").disabled(busy || !tracked).on_click(
                            move |_, window, cx| {
                                plain.update(cx, |this, cx| this.stash_changes(false, window, cx))
                            },
                        ),
                    )
                    .item(
                        PopupMenuItem::new("Stash (include untracked)")
                            .disabled(busy || !changed)
                            .on_click(move |_, window, cx| {
                                untracked
                                    .update(cx, |this, cx| this.stash_changes(true, window, cx))
                            }),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new("Drop all stashes…")
                            .disabled(busy || !stashed)
                            .on_click(move |_, window, cx| {
                                clear.update(cx, |this, cx| this.confirm_clear_stashes(window, cx))
                            }),
                    )
                },
            );
            let (stage, unstage, discard_all) = (view.clone(), view.clone(), view.clone());
            let menu = menu
                .separator()
                .item(
                    PopupMenuItem::new("Stage All")
                        .icon(Lucide::Plus)
                        .disabled(busy || !unstaged)
                        .on_click(move |_, window, cx| {
                            stage.update(cx, |this, cx| this.run_git(GitOp::StageAll, window, cx))
                        }),
                )
                .item(
                    PopupMenuItem::new("Unstage All")
                        .icon(Lucide::Minus)
                        .disabled(busy || !staged)
                        .on_click(move |_, window, cx| {
                            unstage
                                .update(cx, |this, cx| this.run_git(GitOp::UnstageAll, window, cx))
                        }),
                )
                .item(
                    PopupMenuItem::new("Discard All…")
                        .icon(Lucide::Undo2)
                        .disabled(busy || !discard)
                        .on_click(move |_, window, cx| {
                            discard_all.update(cx, |this, cx| this.confirm_discard_all(window, cx))
                        }),
                );
            let toggle_view = view.clone();
            tree_menu_items(menu, &view, trees).separator().item(
                PopupMenuItem::new(view_label).icon(view_icon).on_click(move |_, window, cx| {
                    toggle_view.update(cx, |this, cx| this.toggle_git_tree_view(window, cx))
                }),
            )
        };
        Some(
            icon_chip("git-more", false, cx)
                .icon(Lucide::Ellipsis)
                .accessibility_label("More git actions")
                .tooltip("More actions")
                .dropdown_menu_with_anchor(Anchor::TopRight, menu)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
#[path = "../../tests/unit/git_panel/sync.rs"]
mod tests;
