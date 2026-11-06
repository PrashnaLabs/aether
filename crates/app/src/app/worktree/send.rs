use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use aether_core::Attachment;
use aether_git::CANCELED;
use gpui_kit::{Context, Window};

use crate::app::chat::{SentCopy, sent_copy};
use crate::app::{ChatApp, WorktreeJob, chat_cwd, project_dir};

impl ChatApp {
    /// Makes the worktree, then binds the chat to it and sends `text`. On failure or
    /// cancel the chat keeps its folder; a composer send keeps `text` and `attachments`
    /// in the composer, a lead's child reports the failure to its lead instead.
    pub(in crate::app) fn send_in_new_worktree(
        &mut self,
        ix: usize,
        base: String,
        prompt: String,
        text: String,
        attachments: Vec<Attachment>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.worktree_blocks_send(ix).is_some() {
            return;
        }
        let chat = &mut self.chats[ix];
        let folder = chat_cwd(chat, &self.settings.workspace_root);
        let project = project_dir(chat, &self.settings.workspace_root);
        let id = chat.id;
        let cancel = Arc::new(AtomicBool::new(false));
        chat.worktree_job = Some(WorktreeJob::Creating(base.clone(), cancel.clone()));
        chat.worktree_error = None;
        let owns_composer = ix == self.active;
        if owns_composer {
            self.worktree_menu.update(cx, |m, cx| m.close(window, cx));
        }
        let canceled = cancel.clone();
        let task = self.agent.create_worktree(folder.clone(), prompt, base.clone(), cancel);
        cx.spawn_in(window, async move |this, cx| {
            let made = task.await;
            let created = |e: &str| format!("Couldn't create its worktree: {e}");
            this.update_in(cx, |this, window, cx| {
                let Some(ix) = this.chat_ix(id) else {
                    return;
                };
                let chat = &mut this.chats[ix];
                chat.worktree_job = None;
                match made {
                    // git finished before the cancel landed: the worktree stays unused, listed in Settings
                    Ok(worktree) if canceled.load(Ordering::Relaxed) => {
                        chat.worktree_info = Some(
                            format!(
                                "Canceled after git had made worktree {}, so it isn't used. \
                                 Remove it in Settings › Worktrees if you don't need it.",
                                worktree.branch
                            )
                            .into(),
                        );
                        if chat.parent.is_some() {
                            chat.worktree_error = Some(CANCELED.into());
                        }
                        this.deliver_failure_to_lead(ix, &created(CANCELED), window, cx);
                    }
                    Ok(worktree) => {
                        chat.worktree_info = worktree.missing_subdir.map(|sub| {
                            format!("{sub} isn't on {base}, so this chat works from the top of the worktree.").into()
                        });
                        chat.project = project.display().to_string();
                        chat.cwd = worktree.cwd.display().to_string();
                        chat.worktree_path = worktree.path.display().to_string();
                        chat.fresh_worktree = Some(worktree.path);
                        chat.worktree_branch = worktree.branch;
                        chat.new_worktree = None;
                        chat.pending.retain(|a| !attachments.contains(a));
                        if owns_composer {
                            this.worktree_name_input.update(cx, |s, cx| s.clean(window, cx));
                        }
                        this.clear_sent(ix, &text, window, cx);
                        this.begin_setup_or_submit(ix, text, attachments, window, cx);
                        this.refresh_git(false, cx);
                    }
                    Err(e) => {
                        if e != CANCELED || chat.parent.is_some() {
                            chat.worktree_error = Some(e.clone().into());
                        }
                        this.deliver_failure_to_lead(ix, &created(&e), window, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Sharing never makes a branch; a gone folder errors above the composer.
    pub(in crate::app) fn send_in_existing_worktree(
        &mut self,
        ix: usize,
        path: PathBuf,
        text: String,
        attachments: Vec<Attachment>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.worktree_blocks_send(ix).is_some() || self.switching_folder(&path) {
            return;
        }
        let key = path.display().to_string();
        let branch =
            self.shareable_worktrees(ix).into_iter().find(|(p, _)| p == &path).map(|(_, b)| b);
        if self.missing_folders.contains(&path) || branch.is_none() {
            self.chats[ix].worktree_error = Some(
                format!("{} is missing. Pick another worktree or work locally.", path.display())
                    .into(),
            );
            return cx.notify();
        }
        let chat = &mut self.chats[ix];
        chat.project = chat_cwd(chat, &self.settings.workspace_root).display().to_string();
        chat.cwd = key.clone();
        chat.worktree_path = key;
        chat.worktree_branch = branch.unwrap_or_default();
        chat.new_worktree_existing = None;
        chat.pending.retain(|a| !attachments.contains(a));
        self.clear_sent(ix, &text, window, cx);
        self.begin_setup_or_submit(ix, text, attachments, window, cx);
        self.refresh_git(false, cx);
    }

    fn clear_sent(&mut self, ix: usize, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let owns = self.draft_owner == Some(self.chats[ix].id);
        let composer = self.composer.read(cx).value().to_string();
        match sent_copy(owns, &composer, &self.chats[ix].draft, text) {
            Some(SentCopy::Composer) => self.composer.update(cx, |s, cx| s.clean(window, cx)),
            Some(SentCopy::Draft) => self.chats[ix].draft.clear(),
            None => {}
        }
    }

    /// Stops the active chat's worktree creation; git gets to clean up first.
    pub(super) fn cancel_worktree(&mut self, cx: &mut Context<Self>) {
        if let Some(WorktreeJob::Creating(_, cancel)) = &self.chats[self.active].worktree_job {
            cancel.store(true, Ordering::Relaxed);
        }
        cx.notify();
    }
}
