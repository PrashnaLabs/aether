use std::path::PathBuf;

use aether_core::home_dir;
use aether_git::StatusError;
use gpui_kit::Context;

use super::model::ProjectData;
use crate::app::{ChatApp, Page};

impl ChatApp {
    pub(crate) fn load_project_data(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        // same as the git panel: a repo in home would make git walk everything
        if folder == home_dir() {
            return;
        }
        let status = self.agent.git_status(folder.clone());
        let worktrees = self.agent.repo_worktrees(folder.clone());
        let remotes = self.agent.git_remotes(folder.clone());
        let dir = folder.clone();
        let rest = cx.background_executor().spawn(async move {
            let exists = dir.is_dir();
            let commits =
                if exists { aether_git::history(&dir).unwrap_or_default() } else { vec![] };
            (exists, commits)
        });
        cx.spawn(async move |this, cx| {
            let status = status.await;
            let worktrees = worktrees.await.unwrap_or_default();
            let remotes = remotes.await.unwrap_or_default();
            let (exists, commits) = rest.await;
            this.update(cx, |this, cx| {
                if exists {
                    this.missing_folders.remove(&folder);
                } else {
                    this.missing_folders.insert(folder.clone());
                }
                if this.project_page.as_ref() == Some(&folder) {
                    let repo = !matches!(status, Err(StatusError::NotRepo));
                    let status = status.ok();
                    this.project_view.data =
                        Some(ProjectData { folder, status, repo, worktrees, remotes, commits });
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn refresh_project_page(&mut self, cx: &mut Context<Self>) {
        if self.page == Page::Project
            && let Some(folder) = self.project_page.clone()
        {
            self.load_project_data(folder, cx);
        }
    }
}
