use std::path::{Path, PathBuf};

use aether_git::BranchInfo;
use gpui_kit::{Context, Window};

use crate::app::{ChatApp, WorktreeJob, chat_cwd};

impl ChatApp {
    /// An agent, or a CLI outside aether, is working in `folder`.
    pub(crate) fn agent_in(&self, folder: &Path) -> bool {
        self.chats.iter().enumerate().any(|(ix, chat)| {
            chat_cwd(chat, &self.settings.workspace_root) == folder
                && (chat.cancel.is_some() || self.locked(ix))
        })
    }

    /// HEAD moved in `folder`: re-reads its branch for the sidebar and composer.
    pub(crate) fn branch_moved(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        self.bases.remove(&folder);
        self.refresh_branches(Some(folder), cx);
        self.load_bases(cx);
    }

    pub(in crate::app) fn confirm_switch_workspace_branch(
        &mut self,
        branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.chats[self.active].id;
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let (view, branch) = (view.clone(), branch.clone());
            alert
                .confirm()
                .title(format!("Switch to branch {branch}?"))
                .description("Switching branches changes files for other chats and tools using the same checkout. Uncommitted changes will not be discarded.")
                .ok_text("Switch branch")
                .on_ok(move |_, _, cx| {
                    view.update(cx, |this, cx| this.switch_workspace_branch(id, branch.clone(), cx));
                    true
                })
        });
    }

    fn switch_workspace_branch(&mut self, id: i64, branch: String, cx: &mut Context<Self>) {
        let Some(ix) = self.chat_ix(id) else {
            return;
        };
        if !self.workspace_can_change(ix) {
            return;
        }
        let folder = chat_cwd(&self.chats[ix], &self.settings.workspace_root);
        if self.chats.iter().enumerate().any(|(other, chat)| {
            chat_cwd(chat, &self.settings.workspace_root) == folder
                && (chat.cancel.is_some() || self.locked(other))
        }) {
            self.chats[ix].worktree_error =
                Some("Stop the agent using this folder before switching branches.".into());
            return cx.notify();
        }
        self.chats[ix].worktree_job = Some(WorktreeJob::Switching);
        self.chats[ix].worktree_error = None;
        self.dismiss_suggestions(cx);
        let task = self.agent.switch_branch(folder.clone(), branch.clone());
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                if let Some(ix) = this.chat_ix(id) {
                    let chat = &mut this.chats[ix];
                    chat.worktree_job = None;
                    match result {
                        Ok(()) => {
                            chat.arm_local_worktree();
                            let worktree = this
                                .branches
                                .get(&folder)
                                .and_then(|b| b.as_ref())
                                .is_some_and(|b| b.worktree);
                            this.branches.insert(
                                folder.clone(),
                                Some(BranchInfo { branch, detached: false, worktree }),
                            );
                        }
                        Err(error) => chat.worktree_error = Some(error.into()),
                    }
                }
                this.branch_moved(folder.clone(), cx);
                this.git.invalidate(&folder);
                this.refresh_git(false, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
}
