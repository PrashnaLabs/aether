use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;

use aether_agent::{ExternalAgent, HISTORY_BUDGET, SavedChat, budget_history};
use aether_core::{
    Message, NoticeKind, NoticeState, Provider, RequestStatus, Role, Step, ToolStatus,
    attachments_dir,
};
use aether_ui::{ChatStatus, background_rows};
use gpui_kit::component::WindowExt;
use gpui_kit::component::notification::Notification;
use gpui_kit::{Context, SharedString, Window};

use super::{fit_access, history};
use crate::app::format::now_ms;
use crate::app::providers::initial_model;
use crate::app::{CLIS, Chat, ChatApp, ChatViewMode, Delivery, Queued};
use crate::attachments;

const LIMIT_PHRASES: [&str; 15] = [
    "usage limit",
    "rate limit",
    "rate_limit",
    "quota",
    "context window",
    "context length",
    "maximum context",
    "too many tokens",
    "too many requests",
    "http 429",
    "status 429",
    "error 429",
    "max_turns",
    "max_budget",
    "output limit",
];

/// The reply stopped on a usage, rate, budget or context limit another agent could get past.
pub(crate) fn limit_hit(msg: &Message) -> bool {
    let hit = |text: &str| {
        let text = text.to_lowercase();
        LIMIT_PHRASES.iter().any(|p| text.contains(p))
    };
    msg.role == Role::Assistant
        && (msg.error.as_deref().is_some_and(hit)
            || msg.steps.iter().any(|s| match s {
                Step::Notice(n) => match n.kind {
                    NoticeKind::RateLimit => n.state == NoticeState::Failed,
                    NoticeKind::Warning => hit(&n.text),
                    _ => false,
                },
                _ => false,
            }))
}

pub(super) fn new_chat_folder(
    cli: bool,
    current: Option<PathBuf>,
    recent: Option<PathBuf>,
) -> Option<PathBuf> {
    if !cli {
        return None;
    }
    current.or(recent)
}

/// Rows a relaunch left running read as interrupted; OpenCode's shared service keeps working.
pub(crate) fn restore_messages(provider: Provider, messages: &mut [Message]) {
    for m in messages {
        m.restored = true;
        if provider != Provider::OpenCode && m.role == Role::Assistant {
            m.interrupt("Interrupted: Aether closed");
        }
        for request in &mut m.requests {
            if request.status == RequestStatus::Pending {
                request.status = RequestStatus::Expired;
            }
        }
    }
}

/// An OpenCode chat whose last reply may still be running in the service, or still holds
/// background rows the driver settles once attached.
pub(crate) fn reattach_wanted(chat: &Chat) -> bool {
    chat.provider == Provider::OpenCode
        && !chat.cli_session.is_empty()
        && chat.messages.last().is_some_and(|m| {
            m.role == Role::Assistant
                && (m.ended_at_ms.is_none()
                    || m.requests.iter().any(|r| r.status == RequestStatus::Pending)
                    || background_rows(m).next().is_some())
        })
}

/// Background rows of the reply a reattach picks up, for the driver to settle.
pub(crate) fn restored_background(chat: &Chat) -> Vec<String> {
    let steps = chat.messages.last().into_iter().flat_map(|m| &m.steps);
    steps
        .filter_map(|s| match s {
            Step::Tool(c) if c.status == ToolStatus::Background => Some(c.id.clone()),
            _ => None,
        })
        .collect()
}

impl ChatApp {
    pub(in crate::app) fn duplicate_chat(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fork_chat(ix, None, window, cx);
    }

    /// Continues the chat in `provider` in place, from the error card, limit banner or /handoff.
    pub(crate) fn handoff_to(
        &mut self,
        ix: usize,
        provider: Provider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let model = initial_model(&self.settings, provider);
        self.switch_agent(ix, provider, model, window, cx);
        if ix == self.active {
            self.sync_placeholder(window, cx);
            self.focus_composer(window, cx);
        }
    }

    /// A copy of the chat on `provider`, leaving the original as it is.
    pub(crate) fn continue_in_new_chat(
        &mut self,
        ix: usize,
        provider: Provider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let model = initial_model(&self.settings, provider);
        self.handoff_chat(ix, provider, model, window, cx);
    }

    pub(in crate::app) fn handoff_chat(
        &mut self,
        ix: usize,
        provider: Provider,
        model: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fork_chat(ix, Some((provider, model)), window, cx);
    }

    /// Installed agents and, with a key, OpenRouter: where a chat can continue, minus `current`.
    pub(crate) fn handoff_targets(&self, current: Option<Provider>) -> Vec<Provider> {
        let installed = |p: &Provider| self.probes.get(p).is_some_and(|probe| probe.installed);
        CLIS.into_iter()
            .filter(installed)
            .chain(self.api_key.is_some().then_some(Provider::OpenRouter))
            .filter(|p| Some(*p) != current)
            .collect()
    }

    fn fork_chat(
        &mut self,
        ix: usize,
        handoff: Option<(Provider, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.chats.get(ix) else {
            return;
        };
        let (mut messages, title, provider, provider_model, mut cwd, access, effort) = (
            source.messages.clone(),
            source.title.to_string(),
            source.provider,
            source.provider_model.clone(),
            source.cwd.clone(),
            source.access,
            source.effort.clone(),
        );
        if let Some(m) =
            messages.last_mut().filter(|m| m.role == Role::Assistant && m.ended_at_ms.is_none())
        {
            m.interrupt(if handoff.is_some() { "Handed off" } else { "Copied" });
            m.finish();
        }
        let handoff_from = handoff.is_some().then_some(source.id);
        let (provider, provider_model, access, effort, title) = match handoff {
            Some((target, model)) => {
                if CLIS.contains(&target) && cwd.is_empty() {
                    cwd = self.settings.workspace_root.clone();
                    if cwd.is_empty() {
                        window.push_notification(
                            Notification::error(
                                "Pick a project folder or set a workspace root first.",
                            ),
                            cx,
                        );
                        return;
                    }
                }
                (
                    target,
                    model,
                    fit_access(target, access),
                    String::new(),
                    format!("{title} \u{2192} {}", target.label()),
                )
            }
            None => (provider, provider_model, access, effort, format!("{title} copy")),
        };
        let label = |p: Provider, model: &str| {
            let models = self.probes.get(&p).map_or(&[][..], |probe| &probe.models);
            crate::app::sidebar::model_label(models, model)
        };
        if let Some(last) = messages.last_mut().filter(|_| handoff_from.is_some()) {
            last.handoff = Some(aether_core::Handoff {
                from: source.provider,
                from_model: label(source.provider, &source.provider_model),
                to: provider,
                to_model: label(provider, &provider_model),
            });
        }
        let (project, worktree_path, worktree_branch) =
            (source.project.clone(), source.worktree_path.clone(), source.worktree_branch.clone());
        if let Some(r) = self.renaming
            && r > ix
        {
            self.renaming = Some(r + 1);
        }
        let id = crate::session::allocate_chat_id(
            &mut self.chat_id_highwater,
            self.chats.iter().map(|c| c.id),
            now_ms(),
        );
        let session = self.next_session_id();
        if self
            .agent
            .fork_session(&session, &budget_history(&history(&messages), HISTORY_BUDGET).0)
            .is_err()
        {
            return;
        }
        self.chats.insert(
            ix + 1,
            Chat {
                id,
                session,
                title: SharedString::from(title),
                messages,
                pinned: false,
                archived: false,
                provider,
                provider_model,
                cancel: None,
                open_steps: HashMap::new(),
                run: 0,
                epoch: 0,
                unseen: ChatStatus::Idle,
                cwd,
                view: ChatViewMode::Chat,
                access,
                effort,
                cli_session: String::new(),
                queue: Vec::new(),
                stopped: false,
                external_pid: None,
                turn_at: 0,
                project,
                worktree_path,
                worktree_branch,
                new_worktree: None,
                new_worktree_existing: None,
                worktree_job: None,
                worktree_error: None,
                worktree_info: None,
                setup: None,
                fresh_worktree: None,
                draft: String::new(),
                pending: Vec::new(),
                edit_stash: None,
                last_event_ms: 0,
                saved_ms: 0,
                mode_in_effect: None,
                health: None,
                stall_kept_ms: 0,
                restarted_ms: 0,
                bg_live: Cell::new(None),
                parent: None,
                lead: false,
                lead_seen: None,
                handoff_from,
            },
        );
        let at = ix + 1;
        self.persist(at);
        self.select(at, window, cx);
        self.copy_fork_attachments(id, cx);
    }

    fn copy_fork_attachments(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(root) = attachments_dir() else {
            return;
        };
        let Some(chat) = self.chats.iter().find(|c| c.id == id) else {
            return;
        };
        let files: Vec<_> = chat.messages.iter().flat_map(|m| m.attachments.clone()).collect();
        if files.is_empty() {
            return;
        }
        let copies = cx.background_executor().spawn(async move {
            files.iter().map(|f| attachments::copy_one(&root, id, f).ok()).collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let mut copies = copies.await.into_iter();
            this.update(cx, |this, _| {
                let Some(ix) = this.chats.iter().position(|c| c.id == id) else {
                    return;
                };
                for a in this.chats[ix].messages.iter_mut().flat_map(|m| m.attachments.iter_mut()) {
                    if let Some(Some(copy)) = copies.next()
                        && copy.name == a.name
                    {
                        *a = copy;
                    }
                }
                this.persist(ix);
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn current_project_folder(&self) -> Option<PathBuf> {
        let cli = CLIS.contains(&self.settings.provider);
        let current = self
            .chats
            .get(self.active)
            .filter(|c| self.is_agent_chat(c))
            .and_then(|_| self.chat_project(self.active));
        let recent =
            self.switcher_list().into_iter().find(|p| p.last_activity.is_some()).map(|p| p.folder);
        new_chat_folder(cli, current, recent)
    }

    pub fn new_chat_in_current_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let folder = self.current_project_folder();
        self.new_chat(window, cx);
        if let Some(f) = folder {
            self.apply_draft_folder(self.active, f, false, cx);
        }
    }

    pub fn new_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let host = self.host_of(window);
        self.set_side_tab(host, crate::side_panel::SideTab::Agent);
        // a draft bound to or still making a worktree is about to send there, so it isn't reused
        if let Some(ix) = self.chats.iter().position(|c| {
            c.workspace_draft_ready()
                && c.draft.trim().is_empty()
                && c.pending.is_empty()
                && self.workspace.model.pane_for(c.id).is_none()
        }) {
            let provider = self.settings.provider;
            self.chats[ix].provider = provider;
            self.chats[ix].provider_model = initial_model(&self.settings, provider);
            self.chats[ix].access = self.settings.default_access(provider);
            self.chats[ix].effort.clear();
            self.chats[ix].turn_at = now_ms();
            self.chats[ix].new_worktree = None;
            self.chats[ix].new_worktree_existing = None;
            self.chats[ix].worktree_error = None;
            self.chats[ix].view = ChatViewMode::Chat;
            self.chats[ix].title = "New chat".into();
            return self.select(ix, window, cx);
        }
        let id = crate::session::allocate_chat_id(
            &mut self.chat_id_highwater,
            self.chats.iter().map(|c| c.id),
            now_ms(),
        );
        let session = self.next_session_id();
        if let Err(e) = self.agent.create_session(&session) {
            window.push_notification(
                Notification::error(format!("Couldn't create session: {e}")),
                cx,
            );
            return;
        }
        self.chats.push(Chat {
            id,
            session,
            title: "New chat".into(),
            messages: Vec::new(),
            pinned: false,
            archived: false,
            provider: self.settings.provider,
            provider_model: initial_model(&self.settings, self.settings.provider),
            cancel: None,
            open_steps: HashMap::new(),
            run: 0,
            epoch: 0,
            unseen: ChatStatus::Idle,
            cwd: String::new(),
            view: ChatViewMode::Chat,
            access: self.settings.default_access(self.settings.provider),
            effort: String::new(),
            cli_session: String::new(),
            queue: Vec::new(),
            stopped: false,
            external_pid: None,
            turn_at: 0,
            project: String::new(),
            worktree_path: String::new(),
            worktree_branch: String::new(),
            new_worktree: None,
            new_worktree_existing: None,
            worktree_job: None,
            worktree_error: None,
            worktree_info: None,
            setup: None,
            fresh_worktree: None,
            draft: String::new(),
            pending: Vec::new(),
            edit_stash: None,
            last_event_ms: 0,
            saved_ms: 0,
            mode_in_effect: None,
            health: None,
            stall_kept_ms: 0,
            restarted_ms: 0,
            bg_live: Cell::new(None),
            parent: None,
            lead: false,
            lead_seen: None,
            handoff_from: None,
        });
        self.persist(self.chats.len() - 1);
        self.select(self.chats.len() - 1, window, cx);
    }

    pub fn new_direct_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self
            .chats
            .iter()
            .position(|c| c.messages.is_empty() && c.provider == Provider::OpenRouter)
        {
            self.chats[ix].effort.clear();
            self.chats[ix].turn_at = now_ms();
            return self.select(ix, window, cx);
        }
        let id = crate::session::allocate_chat_id(
            &mut self.chat_id_highwater,
            self.chats.iter().map(|c| c.id),
            now_ms(),
        );
        let session = self.next_session_id();
        if let Err(e) = self.agent.create_session(&session) {
            window.push_notification(
                Notification::error(format!("Couldn't create session: {e}")),
                cx,
            );
            return;
        }
        self.chats.push(Chat {
            id,
            session,
            title: "New chat".into(),
            messages: Vec::new(),
            pinned: false,
            archived: false,
            provider: Provider::OpenRouter,
            provider_model: initial_model(&self.settings, Provider::OpenRouter),
            cancel: None,
            open_steps: HashMap::new(),
            run: 0,
            epoch: 0,
            unseen: ChatStatus::Idle,
            cwd: String::new(),
            view: ChatViewMode::Chat,
            access: self.settings.default_access(Provider::OpenRouter),
            effort: String::new(),
            cli_session: String::new(),
            queue: Vec::new(),
            stopped: false,
            external_pid: None,
            turn_at: 0,
            project: String::new(),
            worktree_path: String::new(),
            worktree_branch: String::new(),
            new_worktree: None,
            new_worktree_existing: None,
            worktree_job: None,
            worktree_error: None,
            worktree_info: None,
            setup: None,
            fresh_worktree: None,
            draft: String::new(),
            pending: Vec::new(),
            edit_stash: None,
            last_event_ms: 0,
            saved_ms: 0,
            mode_in_effect: None,
            health: None,
            stall_kept_ms: 0,
            restarted_ms: 0,
            bg_live: Cell::new(None),
            parent: None,
            lead: false,
            lead_seen: None,
            handoff_from: None,
        });
        self.persist(self.chats.len() - 1);
        self.select(self.chats.len() - 1, window, cx);
    }

    /// False when the store is missing or failed to load.
    pub(in crate::app) fn restore_chats(&mut self) -> bool {
        let Some(store) = &self.store else {
            return false;
        };
        let saved = match store.load() {
            Ok(saved) => saved,
            Err(e) => {
                eprintln!("aether: couldn't load chats: {e}");
                return false;
            }
        };
        for SavedChat {
            id,
            title,
            messages,
            pinned,
            archived,
            provider,
            provider_model,
            cli_session,
            access,
            effort,
            cwd,
            project,
            worktree_path,
            worktree_branch,
            parent,
            lead,
            lead_seen,
            pending_deliveries,
            handoff_from,
        } in saved
        {
            let session = self.next_session_id();
            if let Err(e) = self.agent.fork_session(&session, &history(&messages)) {
                eprintln!("aether: couldn't restore chat {id}: {e}");
                continue;
            }
            let mut messages = messages;
            restore_messages(provider, &mut messages);
            let mut provider_model = provider_model;
            if provider == Provider::OpenCode && provider_model.starts_with("openrouter/") {
                provider_model.clear();
            }
            self.chats.push(Chat {
                id,
                session,
                title: title.into(),
                messages,
                pinned,
                archived,
                provider,
                provider_model,
                cancel: None,
                open_steps: HashMap::new(),
                run: 0,
                epoch: 0,
                unseen: ChatStatus::Idle,
                cwd,
                view: ChatViewMode::Chat,
                access: fit_access(provider, access),
                effort,
                cli_session,
                stopped: false,
                external_pid: None,
                turn_at: 0,
                project,
                worktree_path,
                worktree_branch,
                new_worktree: None,
                new_worktree_existing: None,
                worktree_job: None,
                worktree_error: None,
                worktree_info: None,
                setup: None,
                fresh_worktree: None,
                draft: String::new(),
                pending: Vec::new(),
                edit_stash: None,
                last_event_ms: 0,
                saved_ms: 0,
                mode_in_effect: None,
                health: None,
                stall_kept_ms: 0,
                restarted_ms: 0,
                bg_live: Cell::new(None),
                parent,
                lead,
                lead_seen,
                handoff_from,
                queue: pending_deliveries
                    .into_iter()
                    .map(|d| Queued {
                        text: d.text.clone(),
                        attachments: vec![],
                        delivering: false,
                        from_children: true,
                        deliveries: vec![Delivery {
                            child: d.child,
                            reply_ix: d.reply_ix,
                            text: d.text,
                        }],
                    })
                    .collect(),
            });
        }
        for ix in 0..self.chats.len() {
            if reattach_wanted(&self.chats[ix]) {
                let (settings, session) = self.cli_request(ix);
                self.agent.attach(&settings, &session, restored_background(&self.chats[ix]));
            }
        }
        true
    }

    /// Chat already showing this outside agent, by process or session.
    pub(crate) fn external_chat(&self, a: &ExternalAgent) -> Option<usize> {
        self.chats.iter().position(|c| {
            (a.pid.is_some() && c.external_pid == a.pid)
                || (!a.session.is_empty() && c.cli_session == a.session)
        })
    }

    pub(crate) fn add_external_chat(
        &mut self,
        a: ExternalAgent,
        messages: Vec<Message>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(at) = self.external_chat(&a) {
            return self.select(at, window, cx);
        }
        let id = crate::session::allocate_chat_id(
            &mut self.chat_id_highwater,
            self.chats.iter().map(|c| c.id),
            now_ms(),
        );
        let session = self.next_session_id();
        if self.agent.fork_session(&session, &history(&messages)).is_err() {
            return;
        }
        let title = if !a.title.is_empty() {
            a.title
        } else {
            a.cwd
                .file_name()
                .map_or_else(|| "Outside chat".into(), |n| n.to_string_lossy().into_owned())
        };
        self.chats.push(Chat {
            id,
            session,
            title: title.into(),
            messages,
            pinned: false,
            archived: false,
            provider: a.provider,
            provider_model: initial_model(&self.settings, a.provider),
            cancel: None,
            open_steps: HashMap::new(),
            run: 0,
            epoch: 0,
            unseen: ChatStatus::Idle,
            cwd: a.cwd.display().to_string(),
            view: ChatViewMode::Chat,
            access: self.settings.default_access(a.provider),
            effort: String::new(),
            cli_session: a.session,
            queue: Vec::new(),
            stopped: false,
            external_pid: a.pid,
            turn_at: 0,
            project: String::new(),
            worktree_path: String::new(),
            worktree_branch: String::new(),
            new_worktree: None,
            new_worktree_existing: None,
            worktree_job: None,
            worktree_error: None,
            worktree_info: None,
            setup: None,
            fresh_worktree: None,
            draft: String::new(),
            pending: Vec::new(),
            edit_stash: None,
            last_event_ms: 0,
            saved_ms: 0,
            mode_in_effect: None,
            health: None,
            stall_kept_ms: 0,
            restarted_ms: 0,
            bg_live: Cell::new(None),
            parent: None,
            lead: false,
            lead_seen: None,
            handoff_from: None,
        });
        let at = self.chats.len() - 1;
        self.persist(at);
        self.select(at, window, cx);
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/chat/create.rs"]
mod tests;
