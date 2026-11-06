use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use aether_core::{Provider, home_dir};
use aether_git::{RemoveCheck, RemoveError};
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::{ActiveTheme, checkbox::Checkbox, v_flex};
use gpui_kit::{Context, Window, div, prelude::*};

use super::notice;
use crate::app::format::display_path;
use crate::app::{ChatApp, WorktreeJob, chat_cwd, project_dir};

impl ChatApp {
    /// Chats running in the worktree at `path`: bound to it, or with their folder inside it.
    pub(super) fn worktree_users(&self, path: &Path) -> Vec<usize> {
        let root = &self.settings.workspace_root;
        (0..self.chats.len())
            .filter(|&ix| {
                let chat = &self.chats[ix];
                !path.as_os_str().is_empty()
                    && chat.provider != Provider::OpenRouter
                    && !chat.worktree_removed()
                    && (Path::new(&chat.worktree_path) == path
                        || chat_cwd(chat, root).starts_with(path))
            })
            .collect()
    }

    /// "Remove worktree…" on a chat, or "Clean up" on its missing folder.
    pub(crate) fn remove_worktree(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.chats.get(ix).filter(|c| !c.worktree_path.is_empty()) else {
            return;
        };
        let (path, project) =
            (PathBuf::from(&chat.worktree_path), project_dir(chat, &self.settings.workspace_root));
        self.start_remove(path, project, window, cx);
    }

    /// Refuses while a chat using it runs or another chat shares it; else checks
    /// what removing it would lose and confirms, a missing folder too: one on an
    /// unplugged drive looks the same.
    pub(crate) fn start_remove(
        &mut self,
        path: PathBuf,
        project: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.removing.contains(&path) {
            return;
        }
        let users = self.worktree_users(&path);
        let busy = users.iter().find(|&&ix| {
            self.chats[ix].cancel.is_some()
                || self.chats[ix].worktree_job.is_some()
                || self.chats[ix].setup.is_some()
                || self.locked(ix)
        });
        if let Some(&ix) = busy {
            let chat = &self.chats[ix];
            let (title, text) = busy_notice(&chat.title, chat.setup.is_some());
            return notice(title, text, window, cx);
        }
        let owner =
            users.iter().copied().find(|&ix| Path::new(&self.chats[ix].worktree_path) == path);
        if let Some(&ix) = users.iter().find(|&&ix| Some(ix) != owner) {
            let text = format!(
                "“{}” also works in this worktree. Delete that chat first, then remove the worktree.",
                self.chats[ix].title
            );
            return notice("Another chat uses this worktree", text, window, cx);
        }
        let target = RemoveTarget { path, project, chat: owner.map(|ix| self.chats[ix].id) };
        self.set_removing(&target, Some(WorktreeJob::Checking));
        let check = self.agent.check_worktree(target.path.clone(), target.project.clone());
        cx.spawn_in(window, async move |this, cx| {
            let check = check.await;
            this.update_in(cx, |this, window, cx| {
                this.set_removing(&target, None);
                match check {
                    Ok(check) => this.confirm_remove(target, check, window, cx),
                    Err(e) => notice("Can't remove this worktree", e, window, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn set_removing(&mut self, target: &RemoveTarget, job: Option<WorktreeJob>) {
        if job.is_some() {
            self.removing.insert(target.path.clone());
        } else {
            self.removing.remove(&target.path);
        }
        if let Some(ix) = target.chat.and_then(|id| self.chat_ix(id)) {
            self.chats[ix].worktree_job = job;
        }
    }

    fn confirm_remove(
        &mut self,
        target: RemoveTarget,
        check: RemoveCheck,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (title, ok) = match (check.missing, target.chat.is_some()) {
            (true, _) => ("Prune this worktree?", "Prune worktree"),
            (false, true) => ("Remove this chat's worktree?", "Remove worktree"),
            (false, false) => ("Remove this worktree?", "Remove worktree"),
        };
        let path = display_path(&target.path.display().to_string(), &home_dir());
        let terminal = self
            .worktree_users(&target.path)
            .iter()
            .any(|&ix| self.terminal_running(self.chats[ix].id, cx));
        // unchecked: the branch stays. The dialog layer rebuilds on refresh, so
        // the box repaints from this flag after a click.
        let delete = Rc::new(Cell::new(false));
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let (view, target, check, delete) =
                (view.clone(), target.clone(), check.clone(), delete.clone());
            let mut lines = remove_summary(&check, &path, delete.get());
            if terminal {
                lines.push("Its terminal will be closed.".into());
            }
            let toggle = delete.clone();
            alert
                .confirm()
                .title(title)
                .description(
                    v_flex()
                        .gap_2()
                        .children(lines.iter().map(|line| div().child(line.clone())))
                        .when(
                            check.managed
                                && check
                                    .branch
                                    .as_deref()
                                    .is_some_and(|b| b.starts_with("aether/")),
                            |column| {
                                let branch = check.branch.clone().unwrap_or_default();
                                column.child(
                                    Checkbox::new("remove-delete-branch")
                                        .label(format!("Also delete branch {branch}"))
                                        .checked(delete.get())
                                        .on_click(move |checked: &bool, window, _| {
                                            toggle.set(*checked);
                                            window.refresh();
                                        }),
                                )
                            },
                        ),
                )
                .ok_text(ok)
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let (target, check) = (target.clone(), check.clone());
                    let delete_branch = delete.get();
                    view.update(cx, |this, cx| {
                        this.run_remove(target, false, check, delete_branch, window, cx)
                    });
                    true
                })
        });
    }

    /// Git kept a dirty worktree; removing it anyway takes a second, explicit yes.
    fn confirm_force(
        &mut self,
        target: RemoveTarget,
        why: String,
        check: RemoveCheck,
        delete_branch: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lead = match check.changes.len() {
            0 => "Git kept the worktree because it has changes.".to_string(),
            _ => format!("Git kept the worktree because of {}.", changes_phrase(&check.changes)),
        };
        let mut text = format!("{lead} Removing it anyway deletes them for good.");
        if let Some(branch) = &check.branch {
            text.push_str(&format!(
                " Branch {branch} is {}.",
                if delete_branch { "deleted too" } else { "kept" }
            ));
        }
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, cx| {
            let (view, target, check) = (view.clone(), target.clone(), check.clone());
            alert
                .confirm()
                .title("Delete uncommitted changes?")
                .description(v_flex().gap_2().child(text.clone()).child(
                    div().text_xs().text_color(cx.theme().muted_foreground).child(why.clone()),
                ))
                .ok_text("Delete changes and remove")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let (target, check) = (target.clone(), check.clone());
                    view.update(cx, |this, cx| {
                        this.run_remove(target, true, check, delete_branch, window, cx)
                    });
                    true
                })
        });
    }

    fn run_remove(
        &mut self,
        target: RemoveTarget,
        force: bool,
        check: RemoveCheck,
        delete_branch: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_removing(&target, Some(WorktreeJob::Removing));
        if let Some(id) = target.chat {
            // its idle CLI session runs in the folder that's going away
            self.agent.close_session(&id.to_string());
        }
        let task = self.agent.remove_worktree(
            target.path.clone(),
            target.project.clone(),
            force,
            delete_branch,
        );
        cx.spawn_in(window, async move |this, cx| {
            let removed = task.await;
            this.update_in(cx, |this, window, cx| {
                this.set_removing(&target, None);
                match removed {
                    Ok(()) => {
                        for ix in this.worktree_users(&target.path) {
                            let id = this.chats[ix].id;
                            this.close_terminal(id, cx);
                        }
                        this.forget_worktree(&target.path)
                    }
                    Err(RemoveError::NeedsForce(why)) if !force => {
                        this.confirm_force(target, why, check, delete_branch, window, cx)
                    }
                    Err(RemoveError::BranchDeleteFailed(why)) => {
                        // the folder is gone, so chats still detach; only the branch stays
                        for ix in this.worktree_users(&target.path) {
                            let id = this.chats[ix].id;
                            this.close_terminal(id, cx);
                        }
                        this.forget_worktree(&target.path);
                        notice(
                            "Worktree removed, but the branch couldn't be deleted",
                            why,
                            window,
                            cx,
                        )
                    }
                    Err(RemoveError::NeedsForce(why) | RemoveError::Failed(why)) => {
                        notice("Couldn't remove the worktree", why, window, cx)
                    }
                }
                this.worktrees_changed(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Chats bound to the removed worktree at `path` keep its branch and stay removed across restarts.
    fn forget_worktree(&mut self, path: &Path) {
        for ix in 0..self.chats.len() {
            if Path::new(&self.chats[ix].worktree_path) == path {
                self.chats[ix].worktree_path.clear();
                self.persist(ix);
            }
        }
    }

    /// Copies a chat whose worktree is gone into its project folder, to keep going there.
    pub(super) fn continue_in_project(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = self.chats.len();
        self.duplicate_chat(ix, window, cx);
        if self.chats.len() == before {
            return;
        }
        let at = self.active;
        let chat = &mut self.chats[at];
        chat.cwd = std::mem::take(&mut chat.project);
        chat.worktree_path.clear();
        chat.worktree_branch.clear();
        self.persist(at);
        if let Some(folder) = self.chats[at].folder(&self.settings.workspace_root) {
            self.refresh_branches(Some(folder), cx);
        }
        self.refresh_git(false, cx);
        cx.notify();
    }
}

/// A worktree to remove, with the chat bound to it, if any.
#[derive(Clone)]
struct RemoveTarget {
    path: PathBuf,
    /// Repo it came from; used when its folder is already gone.
    project: PathBuf,
    chat: Option<i64>,
}

/// "3 uncommitted changes: a.rs, b.rs, c.rs and 2 more".
fn changes_phrase(changes: &[String]) -> String {
    let n = changes.len();
    let mut named = changes.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
    if n > 3 {
        named.push_str(&format!(" and {} more", n - 3));
    }
    let noun = if n == 1 { "uncommitted change" } else { "uncommitted changes" };
    format!("{n} {noun}: {named}")
}

/// What the remove confirm says happens, one sentence per line.
fn remove_summary(check: &RemoveCheck, path: &str, delete_branch: bool) -> Vec<String> {
    if check.missing {
        return vec![
            format!("The folder {path} is gone, so this only clears git's record of it."),
            "A worktree on a drive that isn't connected right now counts as gone.".into(),
        ];
    }
    let mut lines = vec![match check.changes.len() {
        0 => format!("Deletes the folder {path}. It has no uncommitted changes."),
        _ => format!(
            "Deletes the folder {path} and {}. You'll confirm once more before they're deleted.",
            changes_phrase(&check.changes)
        ),
    }];
    let commits = |n: usize| if n == 1 { "1 commit".to_string() } else { format!("{n} commits") };
    let kept = if delete_branch { "is deleted too" } else { "is kept" };
    match (&check.branch, check.unmerged) {
        (Some(branch), 0) => lines.push(format!("Branch {branch} {kept}.")),
        (Some(branch), n) => lines.push(format!(
            "Branch {branch} {kept}, with its {} that no other branch has.",
            commits(n)
        )),
        (None, 0) => {}
        (None, 1) => lines.push("1 commit that isn't on any branch will be lost.".into()),
        (None, n) => lines.push(format!("{n} commits that aren't on any branch will be lost.")),
    }
    lines.push("Ignored files, such as build output, are deleted too.".into());
    lines
}

fn busy_notice(title: &str, setup: bool) -> (&'static str, String) {
    if setup {
        let text =
            format!("“{title}” is waiting on worktree setup. Cancel it, then remove the worktree.");
        ("Cancel setup first", text)
    } else {
        let text = format!(
            "“{title}” is still working in this worktree. Stop it, then remove the worktree."
        );
        ("Stop the agent first", text)
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/worktree/remove.rs"]
mod tests;
