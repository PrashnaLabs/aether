use std::path::PathBuf;
use std::time::Duration;

use aether_core::home_dir;
use aether_git::{Status, StatusError};
use gpui_kit::Context;

use super::Loaded;
use crate::app::{Chat, ChatApp, now_ms};

const LOCK_RETRY: Duration = Duration::from_millis(400);
const LOCK_RETRIES: u8 = 8;

impl ChatApp {
    /// The active chat's folder when the panel can read it: an agent chat's
    /// folder that exists, never the home folder (a dotfiles repo there would
    /// make git walk everything).
    pub(crate) fn git_folder(&self) -> Option<PathBuf> {
        self.git_folder_for(&self.chats[self.active])
    }

    fn git_folder_for(&self, chat: &Chat) -> Option<PathBuf> {
        let folder = chat.folder(&self.settings.workspace_root)?;
        (folder != home_dir()
            && !self.missing_folders.contains(&folder)
            && !chat.worktree_removed())
        .then_some(folder)
    }

    /// The active chat's folder when it's in a repo, as far as anything has said.
    pub(crate) fn git_repo(&self) -> Option<PathBuf> {
        self.git_repo_for(&self.chats[self.active])
    }

    pub(crate) fn git_repo_for(&self, chat: &Chat) -> Option<PathBuf> {
        let folder = self.git_folder_for(chat)?;
        let repo = match self.git.statuses.get(&folder).map(|loaded| &loaded.status) {
            Some(Ok(_)) => true,
            Some(Err(StatusError::NotRepo)) => false,
            _ => matches!(self.branches.get(&folder), Some(Some(_))),
        };
        repo.then_some(folder)
    }

    pub(crate) fn git_loaded(&self) -> Option<&Loaded> {
        self.git.statuses.get(&self.git_folder()?)
    }

    pub(crate) fn git_status(&self) -> Option<&Status> {
        self.git_loaded()?.status.as_ref().ok()
    }

    /// Re-reads the active chat's changes off the UI thread. Asked again while
    /// a read runs, it reads once more after it.
    pub(crate) fn refresh_git(&mut self, manual: bool, cx: &mut Context<Self>) {
        let Some(folder) = self.git_folder() else {
            // no read will come to clear a finished job
            if self.git.job.as_ref().is_some_and(|job| job.done) {
                self.git.job = None;
            }
            return;
        };
        if self.git.loading.is_some() {
            self.git.again = true;
            self.git.manual |= manual;
            return;
        }
        if manual {
            self.git.lock_retries = 0;
        }
        self.git.loading = Some(folder.clone());
        self.git.manual = manual;
        let generation = self.git.generation;
        let task = self.agent.git_status(folder.clone());
        cx.spawn(async move |this, cx| {
            let status = task.await;
            this.update(cx, |this, cx| this.git_read(folder, generation, status, cx)).ok();
        })
        .detach();
        cx.notify();
    }

    fn git_read(
        &mut self,
        folder: PathBuf,
        generation: u64,
        status: Result<Status, StatusError>,
        cx: &mut Context<Self>,
    ) {
        self.git.loading = None;
        let manual = std::mem::take(&mut self.git.manual);
        match status {
            Err(StatusError::Locked) if self.git.lock_retries < LOCK_RETRIES => {
                self.git.lock_retries += 1;
                self.git.again = false;
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(LOCK_RETRY).await;
                    this.update(cx, |this, cx| this.refresh_git(manual, cx)).ok();
                })
                .detach();
                return cx.notify();
            }
            // a stage or commit landed while this read ran; it may predate it
            _ if generation != self.git.generation => self.git.again = true,
            status => {
                if !matches!(status, Err(StatusError::Locked)) {
                    self.git.lock_retries = 0;
                }
                self.git.statuses.insert(folder, Loaded { status, at: now_ms() });
                if self.git.job.as_ref().is_some_and(|job| job.done) {
                    self.git.job = None;
                }
                self.follow_diff(cx);
            }
        }
        if std::mem::take(&mut self.git.again) {
            self.refresh_git(false, cx);
        }
        cx.notify();
    }
}
