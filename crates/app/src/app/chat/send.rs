use aether_agent::{ChatSession, Reply};
use aether_core::{
    Ask, Attachment, Message, Provider, Role, Settings, ToolStatus, Update, route,
    without_attached_files,
};
use aether_ui::ChatStatus;
use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::{App, Context, SharedString, Window};

use super::{Changed, history};
use crate::app::composer::attach::{delete_later, first_refusal, is_cli_command, shell_command};
use crate::app::composer::quote::quoted;
use crate::app::format::now_ms;
use crate::app::orchestrate::{clip, stranded_texts};
use crate::app::providers::set_default_model;
use crate::app::worktree::{WorktreeTarget, worktree_name_hint, worktree_target};
use crate::app::{Chat, ChatApp, Queued, chat_cwd};
use crate::suggest::prompt_commands;

/// Title a chat takes from its first message until renamed or titled by its CLI.
fn auto_title(text: &str) -> String {
    text.lines().next().unwrap_or_default().chars().take(40).collect()
}

/// A files-only message takes its title from its first file.
fn title_text(m: &Message) -> &str {
    match m.attachments.first() {
        Some(file) if m.text.is_empty() => &file.name,
        _ => &m.text,
    }
}

const SAVE_EVERY_MS: i64 = 2_000;

/// The trailing reply, if it never finished, so a turn the CLI picks back up continues it.
fn reusable_reply(messages: &[Message]) -> Option<usize> {
    let last = messages.len().checked_sub(1)?;
    (messages[last].role == Role::Assistant && messages[last].ended_at_ms.is_none()).then_some(last)
}

/// That reply when no run streams into it; a run in flight finishes its own reply.
fn left_unfinished(chat: &Chat) -> Option<usize> {
    chat.cancel.is_none().then(|| reusable_reply(&chat.messages)).flatten()
}

/// Next queued prompt; none while a steer may still start its own turn.
fn next_queued(chat: &mut Chat) -> Option<Queued> {
    let delivering = chat.queue.iter().any(|q| q.delivering);
    (!chat.stopped && !delivering && !chat.queue.is_empty()).then(|| chat.queue.remove(0))
}

/// A failed or stopped run reports no more steers: their chips go back to the queue.
fn release_steers(chat: &mut Chat, failed: bool) {
    if chat.stopped || failed {
        for queued in &mut chat.queue {
            queued.delivering = false;
        }
    }
}

/// Chip of the steer the CLI took: same text first, else the oldest delivering one.
pub(crate) fn take_delivered(queue: &mut Vec<Queued>, text: &str) -> Option<Queued> {
    let i = queue
        .iter()
        .position(|q| q.delivering && q.text == text)
        .or_else(|| queue.iter().position(|q| q.delivering))?;
    Some(queue.remove(i))
}

/// Returns a refused steer's chip to the queue: same text first, else the oldest delivering one.
pub(crate) fn refuse_delivered(queue: &mut [Queued], text: &str) -> bool {
    let text = without_attached_files(text);
    let i = queue
        .iter()
        .position(|q| q.delivering && q.text == text)
        .or_else(|| queue.iter().position(|q| q.delivering));
    if let Some(i) = i {
        queue[i].delivering = false;
    }
    i.is_some()
}

/// Clears a refused steer's chip; with no run left to end, the queue moves on now.
fn after_refusal(chat: &mut Chat, text: &str) -> Option<Queued> {
    refuse_delivered(&mut chat.queue, text);
    chat.cancel.is_none().then(|| next_queued(chat)).flatten()
}

/// A late steer's reply is still wanted: no rewind since, and its chip still waits.
fn still_owns(chat_epoch: u64, epoch: u64, queue: &[Queued], text: &str) -> bool {
    chat_epoch == epoch && queue.iter().any(|q| q.delivering && q.text == text)
}

pub(crate) fn save_due(now_ms: i64, saved_ms: i64) -> bool {
    now_ms - saved_ms >= SAVE_EVERY_MS
}

impl ChatApp {
    /// Sends the composer's text. While a reply streams it joins the chat's
    /// queue, unless it replaces an edited message.
    pub fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.locked(self.active)
            || self.worktree_blocks_send(self.active).is_some()
            || self.attaching(self.chats[self.active].id)
            || self.voice_active()
        {
            return;
        }
        let id = self.chats[self.active].id;
        let typed = self.composer.read(cx).value().trim().to_string();
        if self.shell_mode(cx) {
            let Some(command) = shell_command(&typed) else { return };
            self.composer.update(cx, |s, cx| s.clean(window, cx));
            self.composer_history_index = None;
            self.composer_draft = None;
            return self.run_in_new_tab(command, window, cx);
        }
        let text = quoted(self.quotes.get(&id).map(SharedString::as_str), &typed);
        let attachments = self.chats[self.active].pending.clone();
        if text.is_empty() && attachments.is_empty() {
            return;
        }
        if let Some(error) = first_refusal(&attachments, self.pending_direct()) {
            self.attach_error = Some(error.into());
            return cx.notify();
        }
        if !attachments.is_empty() && self.cli_command(&text) {
            self.attach_error = Some("Commands can't take attachments".into());
            return cx.notify();
        }
        self.composer_history_index = None;
        self.composer_draft = None;
        let chat = &self.chats[self.active];
        if self.editing.is_none()
            && chat.messages.is_empty()
            && chat.provider != Provider::OpenRouter
        {
            match worktree_target(chat) {
                WorktreeTarget::Create(base) => {
                    let custom = self.worktree_name_input.read(cx).value();
                    let prompt = worktree_name_hint(&custom, &text);
                    return self.send_in_new_worktree(
                        self.active,
                        base.to_string(),
                        prompt,
                        text,
                        attachments,
                        window,
                        cx,
                    );
                }
                WorktreeTarget::Share(path) => {
                    return self.send_in_existing_worktree(
                        self.active,
                        path.to_path_buf(),
                        text,
                        attachments,
                        window,
                        cx,
                    );
                }
                WorktreeTarget::Local => {}
            }
        }
        self.composer.update(cx, |s, cx| s.clean(window, cx));
        self.chats[self.active].pending.clear();
        self.quotes.remove(&id);
        if let Some((_, user_ix)) = self.editing.take() {
            self.restore_stash(self.active, window, cx);
            self.rewind(self.active, user_ix, cx);
        } else if self.chats[self.active].busy() {
            self.chats[self.active].queue.push(Queued {
                text,
                attachments,
                delivering: false,
                from_children: false,
                deliveries: vec![],
            });
            return cx.notify();
        }
        self.begin_setup_or_submit(self.active, text, attachments, window, cx);
    }

    /// The draft is a `!command` for the terminal, not a message for the agent.
    pub(crate) fn shell_mode(&self, cx: &App) -> bool {
        self.composer.read(cx).value().trim_start().starts_with('!')
            && self.editing.is_none()
            && self.terminal_folder().is_some()
    }

    fn cli_command(&self, text: &str) -> bool {
        let chat = &self.chats[self.active];
        if !chat.is_agent() {
            return false;
        }
        let cli = self.probes.get(&chat.provider).map_or(&[][..], |p| &p.commands);
        let built_in = prompt_commands(chat.provider).iter().copied();
        is_cli_command(text, cli.iter().map(|c| c.name.as_str()).chain(built_in))
    }

    pub(in crate::app) fn steer_queued(
        &mut self,
        i: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ix = self.active;
        if self.locked(ix)
            || self.worktree_blocks_send(ix).is_some()
            || self.chats[ix].queue.get(i).is_none_or(|q| q.delivering)
        {
            return;
        }
        let chat = &mut self.chats[ix];
        if !chat.busy() || !chat.is_agent() {
            let Queued { text, attachments, .. } = chat.queue.remove(i);
            return self.submit(ix, text, attachments, window, cx);
        }
        let queued = &mut chat.queue[i];
        queued.delivering = true;
        let (text, attachments) = (queued.text.clone(), queued.attachments.clone());
        let (id, epoch, hist) = (chat.id, chat.epoch, history(&chat.messages));
        let (settings, session) = self.cli_request(ix);
        let reply = self.agent.steer(text.clone(), attachments.clone(), &settings, &hist, &session);
        cx.spawn_in(window, async move |this, cx| {
            let Reply { mut updates, cancel } = reply;
            // streams only when the turn had already ended and the steer runs as its own prompt
            let Some(first) = updates.next().await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                let owner = this.chats.iter().position(|c| c.id == id);
                let Some(ix) = owner.filter(|&ix| {
                    let chat = &this.chats[ix];
                    still_owns(chat.epoch, epoch, &chat.queue, &text)
                }) else {
                    // a run started since shares the CLI session, and a cancel would stop it
                    if owner.is_none_or(|ix| this.chats[ix].cancel.is_none()) {
                        cancel.cancel();
                    }
                    return;
                };
                let chat = &mut this.chats[ix];
                take_delivered(&mut chat.queue, &text);
                let mut sent = Message::user(text);
                sent.attachments = attachments;
                sent.sent_at_ms = Some(now_ms());
                chat.messages.push(sent);
                this.take_unprompted(ix, Reply { updates, cancel }, window, cx);
                let chat = &this.chats[ix];
                this.apply(ix, chat.messages.len() - 1, chat.run, first, window, cx);
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(in crate::app) fn remove_queued(&mut self, i: usize, cx: &mut Context<Self>) {
        let queue = &mut self.chats[self.active].queue;
        if queue.get(i).is_some_and(|q| !q.delivering) {
            let removed = queue.remove(i);
            delete_later(removed.attachments, cx);
        }
        cx.notify();
    }

    pub(crate) fn submit(
        &mut self,
        ix: usize,
        text: String,
        attachments: Vec<Attachment>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_as(ix, text, attachments, true, window, cx);
    }

    /// `submit`; without `follow` a reader scrolled up stays where they are.
    pub(in crate::app) fn submit_as(
        &mut self,
        ix: usize,
        text: String,
        attachments: Vec<Attachment>,
        follow: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.worktree_blocks_send(ix).is_some() {
            return;
        }
        let provider = self.chats[ix].provider;
        let skills =
            if self.chats[ix].is_agent() { self.agent_skills(provider) } else { Vec::new() };
        // Transcript before this turn; live CLI sessions keep their own
        // memory, so it only seeds a fresh one.
        let hist = history(&self.chats[ix].messages);
        let chat = &mut self.chats[ix];
        let model = chat.model(&self.settings);
        let prev = chat.messages.len().checked_sub(1);
        if let Some(at) = left_unfinished(chat) {
            chat.messages[at].finish();
        }
        chat.stopped = false;
        if let Some(cancel) = chat.cancel.take() {
            cancel.cancel();
        }
        if chat.cwd.is_empty() && provider != Provider::OpenRouter {
            chat.cwd = chat_cwd(chat, &self.settings.workspace_root).display().to_string();
        }
        chat.turn_at = now_ms();
        let mut sent = Message::user(text.clone());
        sent.attachments = attachments.clone();
        sent.sent_at_ms = Some(chat.turn_at);
        if chat.messages.is_empty() && chat.title == "New chat" {
            chat.title = auto_title(title_text(&sent)).into();
        }
        chat.messages.push(sent);
        let mut reply = Message::assistant();
        reply.model = (!model.is_empty()).then(|| model.clone());
        chat.messages.push(reply);
        let reply_ix = chat.messages.len() - 1;
        chat.run += 1;
        let (run, epoch) = (chat.run, chat.epoch);
        self.persist(ix);
        self.unhide_project(ix);

        // Only OpenRouter needs the API key; CLI providers use their own logins.
        if provider == Provider::OpenRouter && self.api_key.is_none() {
            let reply = &mut self.chats[ix].messages[reply_ix];
            reply.apply(Update::Error("No OpenRouter API key. Add one in Settings.".into()));
            reply.finish();
            self.persist(ix);
            self.list_turn(ix, prev, follow, cx);
            return cx.notify();
        }
        let (effective, session) = self.cli_request(ix);
        let key = self.api_key.clone().unwrap_or_default();
        let chat = &mut self.chats[ix];
        let Reply { updates, cancel } = self.agent.send(
            &chat.session,
            text,
            attachments,
            &effective,
            &key,
            skills,
            &hist,
            &session,
        );
        chat.cancel = Some(cancel);
        self.list_turn(ix, prev, follow, cx);
        if ix == self.active {
            self.worktree_menu.update(cx, |s, cx| s.dismiss(cx));
            self.access_menu.update(cx, |m, cx| m.close(window, cx));
        }
        self.stream(ix, reply_ix, run, epoch, updates, window, cx);
        cx.notify();
    }

    fn list_turn(&mut self, ix: usize, prev: Option<usize>, follow: bool, cx: &mut Context<Self>) {
        match follow {
            true => self.append_rows(ix, prev, cx),
            false if ix == self.active => self.sync_rows(prev, cx),
            false => {}
        }
    }

    /// Settings and session chat `ix`'s requests run with.
    pub(in crate::app) fn cli_request(&self, ix: usize) -> (Settings, ChatSession) {
        let chat = &self.chats[ix];
        // Chat's own provider and model win; an empty CLI model omits `--model`.
        let mut effective = self.settings.clone();
        effective.provider = chat.provider;
        set_default_model(&mut effective, chat.provider, chat.model(&self.settings));
        let session = ChatSession {
            key: chat.id.to_string(),
            cwd: chat_cwd(chat, &self.settings.workspace_root),
            access: chat.access,
            effort: chat.effort.clone(),
            resume: chat.cli_session.clone(),
            lead: chat.lead,
        };
        (effective, session)
    }

    /// A turn chat `ix`'s CLI started on its own, as a new reply. A prompted run
    /// already settled when this arrives, so it only takes over the stop handle.
    pub(in crate::app) fn take_unprompted(
        &mut self,
        ix: usize,
        reply: Reply,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let model = self.chats[ix].model(&self.settings);
        let chat = &mut self.chats[ix];
        let reply_ix = match left_unfinished(chat) {
            Some(at) => {
                // turn start isn't saved; a later Final would overwrite the restored text
                chat.messages[at].apply(Update::Final(String::new()));
                at
            }
            None => {
                let mut message = Message::assistant();
                message.model = (!model.is_empty()).then_some(model);
                chat.messages.push(message);
                chat.messages.len() - 1
            }
        };
        chat.run += 1;
        chat.stopped = false;
        chat.turn_at = now_ms();
        chat.cancel = Some(reply.cancel);
        let (run, epoch) = (chat.run, chat.epoch);
        self.persist(ix);
        if ix == self.active {
            // the CLI started this turn; only follow it if the reader is at the end
            let follow = !self.scroller.read(cx).is_scrolled_up();
            // the reply the list ended on lost its "last" state, so remeasure it too
            let prev = self.rows.last().map_or(reply_ix, |r| r.message);
            self.sync_rows(Some(prev), cx);
            if follow {
                self.scroller.update(cx, |s, cx| s.scroll_to_end(cx));
            }
        }
        self.stream(ix, reply_ix, run, epoch, reply.updates, window, cx);
        cx.notify();
    }

    pub(crate) fn acknowledge_session(&mut self, id: i64, cx: &mut Context<Self>) {
        if let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == id)
            && chat.unseen != ChatStatus::Idle
        {
            chat.unseen = ChatStatus::Idle;
            cx.notify();
        }
    }

    pub(crate) fn session_is_watched(&self, id: i64, window: &Window, cx: &App) -> bool {
        if self.peek_in_front(id, cx) {
            return true;
        }
        if !self.session_visible(id) {
            return false;
        }
        let Some(pane) = self.workspace.model.pane_for(id) else { return false };
        let Some(host) = self.workspace.model.window_of(pane) else { return false };
        let Some(handle) = self.workspace.windows.get(&host) else { return false };
        cx.active_window() == Some(*handle)
            || (handle.window_id() == window.window_handle().window_id()
                && window.is_window_active())
    }

    /// Tells the user about chat `ix` unless they're looking at it.
    fn notify_chat(&self, ix: usize, what: &str, window: &Window, cx: &App) {
        let watching = self.session_is_watched(self.chats[ix].id, window, cx);
        if !self.settings.notifications || watching {
            return;
        }
        let chat = &self.chats[ix];
        let models = self.probes.get(&chat.provider).map_or(&[][..], |probe| &probe.models);
        let model = crate::app::sidebar::model_label(models, &chat.model(&self.settings));
        let reply = chat.messages.iter().rev().find(|m| m.role == aether_core::Role::Assistant);
        let body = reply
            .map(|m| {
                if m.text.trim().is_empty() {
                    m.error.clone().unwrap_or_default()
                } else {
                    m.text.clone()
                }
            })
            .unwrap_or_default();
        // the generated title can land after the reply, so fall back to the prompt
        let title = match chat.messages.iter().find(|m| m.role == aether_core::Role::User) {
            Some(prompt) if chat.title.is_empty() || chat.title == "New chat" => {
                crate::notify::preview(&prompt.text, 60)
            }
            _ => chat.title.to_string(),
        };
        crate::notify::post(
            crate::notify::Note {
                tag: chat.id.to_string(),
                title,
                subtitle: format!("{} {what} · {model}", chat.provider.label()),
                body: crate::notify::preview(&body, 180),
                provider: chat.provider,
            },
            cx,
        );
    }

    fn stream(
        &mut self,
        ix: usize,
        reply_ix: usize,
        run: u64,
        epoch: u64,
        mut updates: UnboundedReceiver<Update>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = crate::session::CommandTarget {
            chat: crate::session::ChatId(self.chats[ix].id),
            epoch,
        };
        cx.spawn_in(window, async move |this, cx| {
            while let Some(update) = updates.next().await {
                let applied = this.update_in(cx, |this, window, cx| {
                    if let Some(ix) = target.resolve(&this.chats) {
                        this.apply(ix, reply_ix, run, update, window, cx);
                    }
                });
                if applied.is_err() {
                    return;
                }
            }
            this.update_in(cx, |this, window, cx| {
                if let Some(ix) = target.resolve(&this.chats) {
                    this.finish_run(ix, reply_ix, run, epoch, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn finish_run(
        &mut self,
        ix: usize,
        reply_ix: usize,
        run: u64,
        epoch: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let watched = self.session_is_watched(self.chats[ix].id, window, cx);
        let chat = &mut self.chats[ix];
        if chat.run == run {
            chat.cancel = None;
            chat.last_event_ms = 0;
            chat.invalidate_bg();
        }
        let mut failed = false;
        if chat.epoch == epoch
            && let Some(reply) = chat.messages.get_mut(reply_ix)
        {
            // the stream is gone, so a still-pending request can never be answered
            reply.finish();
            failed = reply.failed;
            let expired: Vec<u64> = reply.requests.iter().map(|r| r.id).collect();
            if !watched {
                chat.unseen = if failed { ChatStatus::Failed } else { ChatStatus::Done };
            }
            for id in expired {
                self.drop_request(id, window, cx);
            }
            if failed {
                self.refresh_health(ix, now_ms());
            }
            self.persist(ix);
            if ix == self.active {
                self.sync_rows(Some(reply_ix), cx);
            }
            let chat = &self.chats[ix];
            if chat.run == run && !chat.stopped {
                self.notify_chat(ix, if failed { "failed" } else { "finished" }, window, cx);
            }
            if self.chats[ix].run == run {
                self.deliver_to_lead(ix, reply_ix, failed, window, cx);
            }
        }
        let blocked = self.worktree_blocks_send(ix).is_some();
        let follow = ix != self.active || !self.scroller.read(cx).is_scrolled_up();
        let chat = &mut self.chats[ix];
        if chat.run == run {
            // after Final: a Steered ahead of it on this stream has landed by now
            release_steers(chat, failed);
        }
        self.drop_read_deliveries(ix);
        if failed && self.chats[ix].run == run {
            let dropped = stranded_texts(&mut self.chats[ix]);
            if !dropped.is_empty() {
                let child = &self.chats[ix];
                let body = format!(
                    "Dropped {} queued message(s) that never reached agent {} \"{}\": {}",
                    dropped.len(),
                    child.id,
                    child.title,
                    clip(&dropped.join(" | "))
                );
                self.deliver_failure_to_lead(ix, &body, window, cx);
            }
        }
        let chat = &mut self.chats[ix];
        if !blocked
            && chat.run == run
            && !failed
            && let Some(Queued { text, attachments, .. }) = next_queued(chat)
        {
            self.submit_as(ix, text, attachments, follow, window, cx);
        }
        self.refresh_branches(None, cx);
        self.refresh_git(false, cx);
        self.refresh_project_page(cx);
        cx.notify();
    }

    pub fn stop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.chats[self.active].cancel.is_none() {
            return self.stop_chat(self.active, cx);
        }
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let view = view.clone();
            alert
                .confirm()
                .title(SharedString::from("Stop generating?"))
                .description(SharedString::from(
                    "This stops the agent's current reply. It can't be undone.",
                ))
                .ok_text("Stop")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _window, cx| {
                    view.update(cx, |this, cx| this.stop_chat(this.active, cx));
                    true
                })
        });
    }

    pub(crate) fn stop_chat(&mut self, ix: usize, cx: &mut Context<Self>) {
        let chat = &mut self.chats[ix];
        if let Some(cancel) = &chat.cancel {
            chat.stopped = true;
            cancel.cancel();
        } else {
            for queued in &mut chat.queue {
                queued.delivering = false;
            }
        }
        cx.notify();
    }

    pub(in crate::app) fn set_steps_open(&mut self, ix: usize, open: bool, cx: &mut Context<Self>) {
        self.chats[self.active].open_steps.insert(ix, open);
        self.sync_rows(Some(ix), cx);
        cx.notify();
    }

    fn refuse_steer(&mut self, ix: usize, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.worktree_blocks_send(ix).is_some() {
            refuse_delivered(&mut self.chats[ix].queue, text);
        } else if let Some(Queued { text, attachments, .. }) =
            after_refusal(&mut self.chats[ix], text)
        {
            return self.submit(ix, text, attachments, window, cx);
        }
        cx.notify();
    }

    /// An update for chat `ix` from outside a turn: resume id, title, mode, or a late
    /// row or count for its last reply. A title only replaces the auto title.
    pub(in crate::app) fn apply_cli_meta(
        &mut self,
        ix: usize,
        update: Update,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Update::AskResolved(id) = update {
            self.drop_request(id, window, cx);
        }
        let chat = &mut self.chats[ix];
        match update {
            Update::Session(id) => chat.cli_session = id,
            Update::Title(title) => {
                let first = chat.messages.first().map_or("", title_text);
                if title.trim().is_empty() || chat.title.as_ref() != auto_title(first) {
                    return;
                }
                chat.title = title.trim().to_string().into();
                cx.notify();
            }
            Update::Mode(mode) => chat.mode_in_effect = Some(mode),
            Update::SteerRefused(text) => return self.refuse_steer(ix, &text, window, cx),
            Update::Tool(_)
            | Update::Notice(_)
            | Update::AskResolved(_)
            | Update::Cost(_)
            | Update::Model(_)
            | Update::Usage(_)
            | Update::Count { .. }
            | Update::Window(_)
            | Update::Warning(_)
            | Update::Error(_) => {
                let Some(last) = chat.messages.iter().rposition(|m| m.role == Role::Assistant)
                else {
                    return;
                };
                let target = route(&chat.messages, last, &update);
                let changed = Changed::of(&update);
                chat.messages[target].apply(update);
                if ix == self.active {
                    self.sync(Some((target, changed)), cx);
                }
                cx.notify();
            }
            _ => return,
        }
        self.persist(ix);
    }

    fn apply(
        &mut self,
        ix: usize,
        reply_ix: usize,
        run: u64,
        update: Update,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.chats[ix].run != run {
            return;
        }
        if let Update::Ask { id, ask } = &update {
            let agent = self.chats[ix].is_agent();
            self.add_request_input(*id, ask, agent, ix == self.active, window, cx);
            let what = match ask {
                Ask::Approval(_) => "needs approval",
                Ask::Question { .. } => "has a question",
                Ask::Questions(q) if q.len() > 1 => "has questions",
                Ask::Questions(_) => "has a question",
                Ask::Form(_) => "needs input",
                Ask::Plan(_) => "proposed a plan",
                Ask::Unknown => "needs you",
            };
            self.notify_chat(ix, what, window, cx);
        }
        if let Update::AskResolved(id) = update {
            self.drop_request(id, window, cx);
        }
        if matches!(update, Update::Session(_) | Update::Title(_)) {
            return self.apply_cli_meta(ix, update, window, cx);
        }
        let now = now_ms();
        let chat = &mut self.chats[ix];
        chat.last_event_ms = now;
        match &update {
            Update::Mode(mode) => {
                chat.mode_in_effect = Some(*mode);
                return;
            }
            Update::Steered(text) => {
                take_delivered(&mut chat.queue, text);
            }
            Update::SteerRefused(text) => return self.refuse_steer(ix, text, window, cx),
            _ => {}
        }
        let step_done = matches!(&update, Update::Tool(call) if call.status != ToolStatus::Running);
        let target = route(&chat.messages, reply_ix, &update);
        let changed = Changed::of(&update);
        chat.messages[target].apply(update);
        if save_due(now, chat.saved_ms) {
            chat.saved_ms = now;
            self.persist(ix);
        }
        if ix == self.active {
            self.sync(Some((target, changed)), cx);
        }
        if step_done {
            let folder = chat_cwd(&self.chats[ix], &self.settings.workspace_root);
            if let Some(explorer) = self.project_explorers.get(&folder) {
                explorer.update(cx, |exp, _| exp.refresh_tree());
            }
            self.check_explorer_disk(&folder, window, cx);
            if ix == self.active {
                self.refresh_git(false, cx);
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/chat/send.rs"]
mod tests;
