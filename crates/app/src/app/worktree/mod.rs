use std::path::{Path, PathBuf};

use crate::settings_view::SettingsTab;
use aether_core::Provider;
use aether_git::{Bases, Listed};
use aether_ui::WorkspaceSelection;
use gpui_kit::{App, Context, SharedString, Window};

use super::sidebar::folder_label;
use super::{Chat, ChatApp, Page, WorktreeJob, chat_cwd, project_dir};

mod branch;
mod note;
mod remove;
mod send;
mod setup;

pub(super) use setup::SetupJob;
#[cfg(test)]
pub(super) use setup::SetupFailure;

/// What a draft's first send does with its composer worktree toggle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum WorktreeTarget<'a> {
    /// Make `aether/<slug>` from this base branch, then send.
    Create(&'a str),
    /// Bind to this live worktree, then send.
    Share(&'a Path),
    /// Send in the project folder.
    Local,
}

/// Reads the toggle. Arming one side clears the other, so both are never set.
pub(super) fn worktree_target(chat: &Chat) -> WorktreeTarget<'_> {
    if let Some(path) = chat.new_worktree_existing.as_deref() {
        WorktreeTarget::Share(path)
    } else if let Some(base) = chat.new_worktree.as_deref() {
        WorktreeTarget::Create(base)
    } else {
        WorktreeTarget::Local
    }
}

/// Branch name source for a new worktree: custom text wins, else the message.
pub(super) fn worktree_name_hint(custom: &str, text: &str) -> String {
    if custom.trim().is_empty() { text.to_string() } else { custom.to_string() }
}

/// Preview branch for the composer: always `aether/<slug>`.
pub(super) fn worktree_preview_branch(custom: &str, text: &str) -> String {
    format!("aether/{}", aether_git::worktree::slug(&worktree_name_hint(custom, text)))
}

pub(super) fn workspace_label(selection: &WorkspaceSelection, current: Option<&str>) -> String {
    match selection {
        WorkspaceSelection::NewWorktree(base) => base.clone(),
        WorkspaceSelection::Worktree(path) => folder_label(&path.display().to_string()),
        WorkspaceSelection::Branch(branch) => branch.clone(),
        WorkspaceSelection::Local => current.unwrap_or("Local").into(),
    }
}

fn selectable_worktrees(repo: &Path, listed: &[Listed]) -> Vec<(PathBuf, String)> {
    listed
        .iter()
        .filter(|row| !row.missing && row.repo == repo)
        .map(|row| (row.path.clone(), row.branch.clone().unwrap_or_else(|| "Detached HEAD".into())))
        .collect()
}

pub(super) fn switch_blocks_send(chats: &[Chat], ix: usize, root: &str) -> bool {
    let folder = chat_cwd(&chats[ix], root);
    let target = chats[ix].new_worktree_existing.as_deref().unwrap_or(&folder);
    chats.iter().any(|chat| {
        matches!(chat.worktree_job, Some(WorktreeJob::Switching))
            && [folder.as_path(), target].contains(&chat_cwd(chat, root).as_path())
    })
}

/// Why a chat in a removed or missing worktree can't send; `None` runs.
fn worktree_send_block(removed: bool, missing: bool) -> Option<&'static str> {
    if removed {
        Some("Worktree removed. Continue in the project folder to keep going.")
    } else if missing {
        Some("The worktree folder is missing. Clean it up, then continue in the project folder.")
    } else {
        None
    }
}

/// Worktree chats: the draft's Local / worktree choice, making the worktree at
/// first send, removing it later, and the cleanup list in Settings.
impl ChatApp {
    pub(in crate::app) fn chat_ix(&self, id: i64) -> Option<usize> {
        self.chats.iter().position(|c| c.id == id)
    }

    pub(crate) fn switching_folder(&self, folder: &Path) -> bool {
        self.chats.iter().any(|chat| {
            matches!(chat.worktree_job, Some(WorktreeJob::Switching))
                && chat_cwd(chat, &self.settings.workspace_root) == folder
        })
    }

    pub(super) fn workspace_can_change(&self, ix: usize) -> bool {
        let chat = &self.chats[ix];
        chat.workspace_draft_ready()
            && !self.locked(ix)
            && !self.git.busy()
            && !self.switching_folder(&chat_cwd(chat, &self.settings.workspace_root))
    }

    /// Why chat `ix` can't send right now because of its worktree.
    pub(super) fn worktree_blocks_send(&self, ix: usize) -> Option<&'static str> {
        let chat = &self.chats[ix];
        if switch_blocks_send(&self.chats, ix, &self.settings.workspace_root) {
            return Some("Switching branches…");
        }
        if let Some(job) = &chat.setup {
            return Some(if job.failed.is_some() {
                "Worktree setup failed"
            } else {
                "Setting up the worktree…"
            });
        }
        match &chat.worktree_job {
            Some(WorktreeJob::Creating(..)) => Some("Creating the worktree…"),
            Some(WorktreeJob::Checking) => Some("Checking the worktree…"),
            Some(WorktreeJob::Removing) => Some("Removing the worktree…"),
            Some(WorktreeJob::Switching) => Some("Switching branches…"),
            None if chat.provider == Provider::OpenRouter => None,
            None => worktree_send_block(chat.worktree_removed(), self.worktree_missing(ix)),
        }
    }

    /// Bound to a worktree whose folder vanished without aether removing it.
    pub(crate) fn worktree_missing(&self, ix: usize) -> bool {
        let chat = &self.chats[ix];
        !chat.worktree_path.is_empty()
            && self.missing_folders.contains(&chat_cwd(chat, &self.settings.workspace_root))
    }

    /// The active draft's folder's base branches, once loaded; `None` outside a repo.
    pub(crate) fn active_bases(&self) -> Option<&Bases> {
        let folder = self.chats[self.active].folder(&self.settings.workspace_root)?;
        self.bases.get(&folder)?.as_ref()
    }

    /// Reloads base branches for the active draft's folder off the UI thread.
    pub(super) fn load_bases(&mut self, cx: &mut Context<Self>) {
        self.load_worktrees(cx);
        let chat = &self.chats[self.active];
        if !chat.messages.is_empty() {
            return;
        }
        let Some(folder) = chat.folder(&self.settings.workspace_root) else {
            return;
        };
        if !self.loading_bases.insert(folder.clone()) {
            return;
        }
        let task = self.agent.worktree_bases(folder.clone());
        cx.spawn(async move |this, cx| {
            let bases = task.await;
            this.update(cx, |this, cx| {
                this.loading_bases.remove(&folder);
                this.bases.insert(folder, bases);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `Some(base)`: the first send makes a worktree from `base`; `None`: work locally.
    pub(crate) fn choose_worktree(&mut self, base: Option<String>, cx: &mut Context<Self>) {
        if !self.workspace_can_change(self.active) {
            return;
        }
        let chat = &mut self.chats[self.active];
        match base {
            Some(base) => chat.arm_new_worktree(base),
            None => chat.arm_local_worktree(),
        }
        chat.worktree_error = None;
        cx.notify();
    }

    /// Draft toggle: the first send shares the live worktree at `path`.
    pub(crate) fn choose_existing_worktree(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.workspace_can_change(self.active)
            || !self.shareable_worktrees(self.active).iter().any(|(p, _)| p == &path)
        {
            return;
        }
        let chat = &mut self.chats[self.active];
        chat.arm_existing_worktree(path);
        chat.worktree_error = None;
        cx.notify();
    }

    /// Live Git worktrees in the draft's repository, including those without chats.
    pub(crate) fn shareable_worktrees(&self, ix: usize) -> Vec<(PathBuf, String)> {
        let root = &self.settings.workspace_root;
        let folder = chat_cwd(&self.chats[ix], root);
        let Some(bases) = self.bases.get(&folder).and_then(|bases| bases.as_ref()) else {
            return Vec::new();
        };
        selectable_worktrees(&bases.repo, self.worktrees.as_deref().unwrap_or_default())
    }

    /// Reloads the Settings worktree list off the UI thread.
    pub(crate) fn load_worktrees(&mut self, cx: &mut Context<Self>) {
        if self.listing_worktrees {
            return;
        }
        self.listing_worktrees = true;
        let root = &self.settings.workspace_root;
        let mut repos: Vec<PathBuf> = self
            .chats
            .iter()
            .filter(|c| c.provider != Provider::OpenRouter)
            .map(|c| project_dir(c, root))
            .collect();
        repos.sort();
        repos.dedup();
        let task = self.agent.list_worktrees(repos.clone());
        cx.spawn(async move |this, cx| {
            let found = task.await;
            this.update(cx, |this, cx| {
                this.listing_worktrees = false;
                this.worktrees = Some(found);
                let active = &this.chats[this.active];
                if active.provider != Provider::OpenRouter
                    && !repos.contains(&project_dir(active, &this.settings.workspace_root))
                {
                    this.load_worktrees(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// A worktree was made, removed or pruned: lists them again everywhere and
    /// re-reads the sidebar's branches.
    pub(crate) fn worktrees_changed(&mut self, cx: &mut Context<Self>) {
        self.relist_trees();
        self.refresh_branches(None, cx);
        if self.worktrees.is_some() {
            self.load_worktrees(cx);
        }
    }

    /// An agent works in the worktree at `path`.
    pub(crate) fn worktree_running(&self, path: &Path) -> bool {
        self.worktree_users(path)
            .into_iter()
            .any(|ix| self.chats[ix].cancel.is_some() || self.locked(ix))
    }

    /// "Open in new chat": a new chat in the active chat's project that shares
    /// the worktree at `path` from its first send. The main checkout, or one
    /// the project is in, just gets a new chat there.
    pub(crate) fn open_worktree_chat(
        &mut self,
        path: PathBuf,
        main: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let project = project_dir(&self.chats[self.active], &self.settings.workspace_root);
        if project.starts_with(&path) {
            return self.new_chat_in(project, window, cx);
        }
        if main {
            return self.new_chat_in(path, window, cx);
        }
        self.new_chat_in(project, window, cx);
        self.chats[self.active].arm_existing_worktree(path);
        cx.notify();
    }

    /// Palette "Clean up worktrees…": Settings, on the worktree list.
    pub(crate) fn open_worktrees(&mut self, cx: &mut Context<Self>) {
        self.settings_tab = SettingsTab::Worktrees;
        self.open_page(Page::Settings, cx);
        self.load_worktrees(cx);
    }

    /// A cleanup-list row's state: folder missing, locked, and who uses it.
    pub(crate) fn worktree_status(&self, listed: &Listed) -> String {
        let mut parts = Vec::new();
        if listed.missing {
            parts.push("Folder missing".to_string());
        }
        match listed.locked.as_deref() {
            Some("") => parts.push("Locked. Unlock it in a terminal to remove it".into()),
            Some(why) => parts.push(format!("Locked: {why}. Unlock it in a terminal to remove it")),
            None => {}
        }
        let users = self.worktree_users(&listed.path);
        parts.push(match users.first() {
            None => "No chat".into(),
            Some(&ix) => {
                let chat = &self.chats[ix];
                let archived = if chat.archived { " (archived)" } else { "" };
                match users.len() {
                    1 => format!("Used by “{}”{archived}", chat.title),
                    n => format!("Used by “{}”{archived} and {} more", chat.title, n - 1),
                }
            }
        });
        parts.join(" · ")
    }
}

/// One-button alert for something the user can't do right now, and why.
pub(super) fn notice(
    title: impl Into<SharedString>,
    text: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
) {
    let (title, text) = (title.into(), text.into());
    aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
        alert.title(title.clone()).description(text.clone())
    });
}

#[cfg(test)]
#[path = "../../../tests/unit/app/worktree/mod.rs"]
mod tests;
