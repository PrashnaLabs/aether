use aether_core::{Attachment, Role, attachments_dir};
use gpui_kit::{Context, Window};

use super::history;
use crate::app::composer::attach::delete_later;
use crate::app::worktree::switch_blocks_send;
use crate::app::{Chat, ChatApp};
use crate::attachments::copy_all;

fn retry_request(
    chats: &[Chat],
    ix: usize,
    root: &str,
) -> Option<(usize, String, Vec<Attachment>)> {
    if switch_blocks_send(chats, ix, root) {
        return None;
    }
    let messages = &chats[ix].messages;
    let user_ix = messages.iter().rposition(|m| m.role == Role::User)?;
    let sent = &messages[user_ix];
    Some((user_ix, sent.text.clone(), sent.attachments.clone()))
}

fn stash_draft_for_edit(chat: &mut Chat, text: String) {
    chat.edit_stash = Some((text, std::mem::take(&mut chat.pending)));
}

pub(super) fn edit_chat_ix(chats: &[Chat], editing: Option<(i64, usize)>) -> Option<usize> {
    let (id, _) = editing?;
    chats.iter().position(|c| c.id == id)
}

#[cfg(test)]
pub(super) fn leaves_edit(editing: Option<(i64, usize)>, target: i64) -> bool {
    editing.is_some_and(|(id, _)| id != target)
}

fn unstash(chat: &mut Chat) -> String {
    let (text, pending) = chat.edit_stash.take().unwrap_or_default();
    chat.pending = pending;
    text
}

impl ChatApp {
    /// Regenerates the last answer from the same question.
    pub(crate) fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ix = self.active;
        if self.locked(ix) || self.worktree_blocks_send(ix).is_some() {
            return;
        }
        let root = &self.settings.workspace_root;
        let Some((user_ix, text, attachments)) = retry_request(&self.chats, ix, root) else {
            return;
        };
        self.rewind(ix, user_ix, cx);
        self.submit(ix, text, attachments, window, cx);
    }

    /// Loads a sent message into the composer; sending replaces it and
    /// everything after.
    pub(crate) fn edit_message(
        &mut self,
        user_ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chat = &self.chats[self.active];
        let (id, sent) = (chat.id, &chat.messages[user_ix]);
        let (text, files) = (sent.text.clone(), sent.attachments.clone());
        if self.editing.is_some() {
            self.clear_pending(cx);
        } else {
            let draft = self.composer.read(cx).value().to_string();
            stash_draft_for_edit(&mut self.chats[self.active], draft);
        }
        self.editing = Some((id, user_ix));
        self.composer.update(cx, |s, cx| {
            s.set_value(text, window, cx);
            s.focus(window, cx);
        });
        // copies, so removing one from the draft leaves the sent message's file alone
        if let Some(root) = attachments_dir().filter(|_| !files.is_empty()) {
            self.attach_started(id);
            cx.spawn(async move |this, cx| {
                let (copies, error) = cx
                    .background_executor()
                    .spawn(async move {
                        let copies = copy_all(&root, id, &files);
                        let lost = files.iter().find(|f| !copies.iter().any(|c| c.name == f.name));
                        (copies, lost.map(|f| format!("Couldn't attach {}", f.name)))
                    })
                    .await;
                this.update(cx, |this, cx| {
                    this.attach_settled(id);
                    let editing = if this.chats.get(this.active).is_some_and(|c| c.id == id) {
                        this.editing == Some((id, user_ix))
                    } else {
                        this.session_views.get(&id).is_some_and(|ui| ui.is_editing(id, user_ix))
                    };
                    if editing {
                        this.finish_attach(id, copies, error, cx);
                    } else {
                        delete_later(copies, cx);
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    pub(crate) fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = edit_chat_ix(&self.chats, self.editing) else {
            self.editing = None;
            return;
        };
        self.editing = None;
        self.attach_error = None;
        delete_later(std::mem::take(&mut self.chats[ix].pending), cx);
        self.restore_stash(ix, window, cx);
        cx.notify();
    }

    pub(super) fn restore_stash(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let text = unstash(&mut self.chats[ix]);
        self.composer_draft = None;
        if self.draft_owner == Some(self.chats[ix].id) {
            self.composer.update(cx, |s, cx| s.set_value(text, window, cx));
        } else {
            self.chats[ix].draft = text;
        }
    }

    pub(in crate::app) fn navigate_history(
        &mut self,
        delta: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.chats.get(self.active) else {
            cx.propagate();
            return;
        };
        let user_messages: Vec<String> = chat
            .messages
            .iter()
            .filter(|m| m.role == Role::User && !m.text.trim().is_empty())
            .map(|m| m.text.clone())
            .collect();
        if user_messages.is_empty() {
            cx.propagate();
            return;
        }

        let cursor_line = self.composer.read(cx).cursor_position().line;
        if delta < 0 {
            // Up arrow:
            if self.composer_history_index.is_none() && cursor_line > 0 {
                cx.propagate();
                return;
            }

            let next_index = match self.composer_history_index {
                None => {
                    self.composer_draft = Some(self.composer.read(cx).value().to_string());
                    user_messages.len().saturating_sub(1)
                }
                Some(current) => {
                    if current == 0 {
                        return;
                    }
                    current - 1
                }
            };
            self.composer_history_index = Some(next_index);
            let text = user_messages[next_index].clone();
            self.composer.update(cx, |s, cx| {
                s.set_value(text, window, cx);
                s.focus(window, cx);
            });
            cx.notify();
        } else if delta > 0 {
            // Down arrow:
            let Some(current) = self.composer_history_index else {
                cx.propagate();
                return;
            };

            let next_index = current + 1;
            if next_index >= user_messages.len() {
                self.composer_history_index = None;
                let draft = self.composer_draft.take().unwrap_or_default();
                self.composer.update(cx, |s, cx| {
                    s.set_value(draft, window, cx);
                    s.focus(window, cx);
                });
            } else {
                self.composer_history_index = Some(next_index);
                let text = user_messages[next_index].clone();
                self.composer.update(cx, |s, cx| {
                    s.set_value(text, window, cx);
                    s.focus(window, cx);
                });
            }
            cx.notify();
        }
    }

    /// Drops messages from `from` on and continues in a new session that
    /// holds only what's left.
    pub(super) fn rewind(&mut self, ix: usize, from: usize, cx: &mut Context<Self>) {
        let session = self.next_session_id();
        let history = history(&self.chats[ix].messages[..from]);
        if let Err(e) = self.agent.fork_session(&session, &history) {
            eprintln!("aether: couldn't fork session: {e}");
            return;
        }
        let chat = &mut self.chats[ix];
        if let Some(cancel) = chat.cancel.take() {
            cancel.cancel();
        }
        chat.session = session;
        chat.cli_session.clear();
        chat.agent_sessions.clear();
        chat.handoff_seen = None;
        self.agent.close_session(&chat.id.to_string());
        chat.messages.truncate(from);
        chat.lead_seen = chat.lead_seen.filter(|&seen| seen < from);
        // nothing left to title it, so the resent first message names it again
        if from == 0 {
            chat.title = "New chat".into();
        }
        chat.open_steps.retain(|&message, _| message < from);
        for queued in &mut chat.queue {
            queued.delivering = false;
        }
        chat.last_event_ms = 0;
        chat.epoch += 1;
        chat.run += 1;
        if ix == self.active {
            self.reset_rows(cx);
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/chat/edit.rs"]
mod tests;
