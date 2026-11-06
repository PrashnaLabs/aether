use std::path::PathBuf;

use aether_core::{
    Attachment, FooterChips, MAX_ATTACHMENTS, attachments_dir, check_attachment, mime_for,
};
use gpui_kit::{
    ClipboardEntry, ClipboardItem, Context, Image, PathPromptOptions, SharedString, Window,
};

use super::ChatApp;
use crate::app::WorktreeJob;
use crate::attachments::{pasted_name, store_bytes, store_file};

pub(crate) enum PasteKind {
    Text,
    Image(Image),
    Files(Vec<PathBuf>),
}

pub(crate) fn paste_kind(item: &ClipboardItem) -> PasteKind {
    let entries = item.entries();
    let files = entries.iter().find_map(|e| match e {
        ClipboardEntry::ExternalPaths(p) => Some(p.paths().to_vec()),
        _ => None,
    });
    let image = entries.iter().find_map(|e| match e {
        ClipboardEntry::Image(i) => Some(i.clone()),
        _ => None,
    });
    match (files, image) {
        (Some(files), _) => PasteKind::Files(files),
        (None, Some(image)) => PasteKind::Image(image),
        _ => PasteKind::Text,
    }
}

/// Copies `paths` into the chat's folder; stops at the first refusal, which comes back as the error.
pub(crate) fn store_batch(
    root: &std::path::Path,
    chat: i64,
    pending: usize,
    direct: bool,
    paths: Vec<PathBuf>,
) -> (Vec<Attachment>, Option<String>) {
    let mut stored = Vec::new();
    let mut error = None;
    for path in paths {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            continue;
        }
        let check =
            check_attachment(pending + stored.len(), &name, meta.len(), mime_for(&path), direct);
        match check {
            Ok(()) => match store_file(root, chat, &path) {
                Ok(file) => stored.push(file),
                Err(_) => {
                    error.get_or_insert(format!("Couldn't attach {name}"));
                }
            },
            Err(e) => {
                error.get_or_insert(e);
            }
        }
    }
    (stored, error)
}

/// Re-checks files at send time: a draft can switch provider while they wait.
pub(crate) fn first_refusal(files: &[Attachment], direct: bool) -> Option<String> {
    files
        .iter()
        .enumerate()
        .find_map(|(i, f)| check_attachment(i, &f.name, f.size, &f.mime, direct).err())
}

pub(crate) fn is_cli_command<'a>(text: &str, mut names: impl Iterator<Item = &'a str>) -> bool {
    let token = text.split_whitespace().next().unwrap_or("");
    token.strip_prefix('/').is_some_and(|name| names.any(|n| n == name))
}

/// The shell command a leading `!` asks for.
pub(crate) fn shell_command(text: &str) -> Option<&str> {
    text.strip_prefix('!').map(str::trim).filter(|command| !command.is_empty())
}

pub(crate) fn delete_later<T>(files: Vec<Attachment>, cx: &mut Context<T>) {
    cx.background_executor()
        .spawn(async move {
            for file in files {
                let _ = std::fs::remove_file(file.path);
            }
        })
        .detach();
}

pub(crate) fn split_overflow(stored: &mut Vec<Attachment>, have: usize) -> Vec<Attachment> {
    stored.split_off(MAX_ATTACHMENTS.saturating_sub(have).min(stored.len()))
}

impl ChatApp {
    pub(in crate::app) fn pending_direct(&self) -> bool {
        !FooterChips::for_provider(self.chats[self.active].provider).folder
    }

    pub(in crate::app) fn attaching(&self, chat: i64) -> bool {
        self.attaching.get(&chat).is_some_and(|n| *n > 0)
    }

    pub(in crate::app) fn attach_started(&mut self, chat: i64) {
        *self.attaching.entry(chat).or_default() += 1;
    }

    pub(in crate::app) fn attach_settled(&mut self, chat: i64) {
        if let Some(n) = self.attaching.get_mut(&chat) {
            *n = n.saturating_sub(1);
        }
    }

    pub(in crate::app) fn finish_attach(
        &mut self,
        chat: i64,
        mut stored: Vec<Attachment>,
        mut error: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let dropped = match self.chats.iter_mut().find(|c| c.id == chat) {
            Some(c) => {
                let over = split_overflow(&mut stored, c.pending.len());
                if !over.is_empty() {
                    error.get_or_insert(format!("Up to {MAX_ATTACHMENTS} attachments"));
                }
                c.pending.extend(stored);
                over
            }
            None => stored,
        };
        delete_later(dropped, cx);
        if error.is_some() && self.chats.get(self.active).is_some_and(|c| c.id == chat) {
            self.attach_error = error.map(SharedString::from);
        }
        cx.notify();
    }

    pub(crate) fn add_attachment_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.attach_error = None;
        let Some(root) = attachments_dir() else {
            return;
        };
        let (chat, pending, direct) = (
            self.chats[self.active].id,
            self.chats[self.active].pending.len(),
            self.pending_direct(),
        );
        self.attach_started(chat);
        cx.spawn(async move |this, cx| {
            let (stored, error) = cx
                .background_executor()
                .spawn(async move { store_batch(&root, chat, pending, direct, paths) })
                .await;
            this.update(cx, |this, cx| {
                this.attach_settled(chat);
                this.finish_attach(chat, stored, error, cx)
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn add_pasted_image(&mut self, image: Image, cx: &mut Context<Self>) {
        self.attach_error = None;
        let Some(root) = attachments_dir() else {
            return;
        };
        let active = &self.chats[self.active];
        let (chat, pending, direct) = (active.id, active.pending.len(), self.pending_direct());
        let n = active.pending.iter().filter(|a| a.name.starts_with("Pasted image")).count()
            + self.attaching.get(&chat).copied().unwrap_or(0)
            + 1;
        let name = pasted_name(n, image.format().extension());
        let bytes = image.bytes().to_vec();
        if let Err(e) =
            check_attachment(pending, &name, bytes.len() as u64, image.format().mime_type(), direct)
        {
            self.attach_error = Some(e.into());
            cx.notify();
            return;
        }
        self.attach_started(chat);
        cx.spawn(async move |this, cx| {
            let stored = cx
                .background_executor()
                .spawn(async move { store_bytes(&root, chat, &name, &bytes) })
                .await;
            this.update(cx, |this, cx| {
                this.attach_settled(chat);
                match stored {
                    Ok(file) => this.finish_attach(chat, vec![file], None, cx),
                    Err(_) => {
                        this.finish_attach(chat, vec![], Some("Couldn't attach image".into()), cx)
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn remove_pending(&mut self, i: usize, cx: &mut Context<Self>) {
        let chat = &mut self.chats[self.active];
        if i >= chat.pending.len() || matches!(chat.worktree_job, Some(WorktreeJob::Creating(..))) {
            return;
        }
        let file = chat.pending.remove(i);
        self.attach_error = None;
        delete_later(vec![file], cx);
        cx.notify();
    }

    pub(in crate::app) fn clear_pending(&mut self, cx: &mut Context<Self>) {
        let files = std::mem::take(&mut self.chats[self.active].pending);
        self.attach_error = None;
        delete_later(files, cx);
    }

    pub(crate) fn pick_attachments(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let view = self.session_handle(cx).downgrade();
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach".into()),
        });
        cx.spawn(async move |_this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            view.update(cx, |this, cx| this.add_attachment_paths(paths, cx)).ok();
        })
        .detach();
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/composer/attach.rs"]
mod tests;
