use crate::agents_view::externally_locked;
use aether_agent::SavedChat;
use aether_core::{AccessMode, ChatDetail, Message, Provider, Role, Step, Update};
use aether_ui::{
    Block, ChatStatus, Reflow, ReplyView, TranscriptRow, opened_tools, row_splice,
    set_verbose_rows, transcript_rows,
};
use gpui_kit::{App, Context, SharedString, Window};
use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use super::{Chat, ChatApp, ChatViewMode, Page, chat_cwd};

mod commands;
mod create;
pub(super) use create::reattach_wanted;
mod edit;
mod live;
mod manage;
pub(crate) use manage::Bulk;
mod send;
mod side;
pub(crate) use side::{SideCard, btw_question, btw_rest};
mod turn;

fn default_access(provider: Provider) -> AccessMode {
    AccessMode::for_provider(provider).first().copied().unwrap_or_default()
}

pub(super) fn fit_access(provider: Provider, current: AccessMode) -> AccessMode {
    if AccessMode::for_provider(provider).contains(&current) {
        current
    } else {
        default_access(provider)
    }
}

/// The visible transcript, which seeds a fresh model session.
fn history(messages: &[Message]) -> Vec<(Role, String)> {
    messages
        .iter()
        .map(|m| {
            let mut text = m.text.clone();
            if !m.attachments.is_empty() {
                let names: Vec<&str> = m.attachments.iter().map(|a| a.name.as_str()).collect();
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&format!("[attached: {}]", names.join(", ")));
            }
            (m.role, text)
        })
        .filter(|(_, text)| !text.is_empty())
        .collect()
}

/// Which rows of the message an update touched it may have resized; rows whose
/// block changed are remeasured regardless.
pub(in crate::app) enum Changed {
    Blocks,
    Thought,
    Tool(String),
    All,
}

impl Changed {
    pub(in crate::app) fn of(update: &Update) -> Self {
        match update {
            Update::Delta(_)
            | Update::Usage(_)
            | Update::Window(_)
            | Update::Count { .. }
            | Update::Cost(_)
            | Update::Model(_) => Self::Blocks,
            Update::Thinking(_) => Self::Thought,
            Update::Tool(call) => Self::Tool(call.id.clone()),
            _ => Self::All,
        }
    }
}

/// Whether `block` shows the tool row `id`.
fn shows_tool(steps: &[Step], block: &Block, id: &str) -> bool {
    let is = |i: &usize| matches!(steps.get(*i), Some(Step::Tool(c)) if c.id == id);
    match block {
        Block::Tool(i) => is(i),
        Block::Subagent { step, children } => is(step) || children.iter().any(is),
        Block::Reads(reads) => reads.iter().any(is),
        Block::Activity { steps: children, .. } | Block::Worked(children) => {
            children.iter().any(|b| shows_tool(steps, b, id))
        }
        _ => false,
    }
}

/// Remeasures transcript row `row` when its content's height may have changed.
pub(in crate::app) fn row_reflow(view: &crate::app::SessionHandle, row: usize) -> Option<Reflow> {
    let view = view.downgrade();
    Some(Rc::new(move |cx: &mut App| {
        view.update(cx, |this, cx| this.remeasure_row(row, cx)).ok();
    }))
}

pub(super) fn merge_missing(missing: &mut HashSet<PathBuf>, checked: Vec<(bool, PathBuf)>) {
    for (exists, path) in checked {
        if exists {
            missing.remove(&path);
        } else {
            missing.insert(path);
        }
    }
}

#[cfg(test)]
pub(super) fn stash_draft(chats: &mut [Chat], owner: Option<i64>, text: String) -> bool {
    match chats.iter_mut().find(|c| Some(c.id) == owner) {
        Some(chat) => {
            chat.draft = text;
            true
        }
        None => false,
    }
}

#[derive(Debug, PartialEq)]
pub(super) enum SentCopy {
    Composer,
    Draft,
}

pub(super) fn sent_copy(
    owns_composer: bool,
    composer: &str,
    draft: &str,
    text: &str,
) -> Option<SentCopy> {
    match owns_composer {
        true if composer.trim() == text => Some(SentCopy::Composer),
        false if draft.trim() == text => Some(SentCopy::Draft),
        _ => None,
    }
}

impl ChatApp {
    pub(crate) fn is_agent_chat(&self, chat: &Chat) -> bool {
        chat.is_agent() && self.folder_of(chat).is_some()
    }

    /// Stats attachment paths off-thread; all chats after restore, else the active one.
    pub(in crate::app) fn refresh_missing_attachments(
        &mut self,
        all: bool,
        cx: &mut Context<Self>,
    ) {
        let chats = if all { &self.chats[..] } else { &self.chats[self.active..=self.active] };
        let paths: Vec<PathBuf> = chats
            .iter()
            .flat_map(|c| &c.messages)
            .flat_map(|m| &m.attachments)
            .map(|a| a.path.clone())
            .collect();
        if paths.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let checked = cx
                .background_executor()
                .spawn(async move { paths.into_iter().map(|p| (p.exists(), p)).collect() })
                .await;
            this.update(cx, |this, cx| {
                merge_missing(&mut this.missing_attachments, checked);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn select(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.chats[ix].id;
        self.chats[ix].unseen = ChatStatus::Idle;
        cx.notify();
        if !self.workspace.splitting {
            if let Some(pane) = self.workspace.model.pane_for(id) {
                self.workspace.model.focus(pane);
                if let Some(host) = self.workspace.model.window_of(pane).filter(|host| *host != 0) {
                    if let Some(handle) = self.workspace.windows.get(&host).copied() {
                        let _ = handle.update(cx, |_, window, _| window.activate_window());
                    }
                    return;
                }
            } else {
                let focused = self.workspace.model.windows.get(&0).and_then(|w| w.focused);
                if let Some(pane) = focused {
                    self.workspace.model.panes.insert(pane, id);
                    self.workspace.views.remove(&pane);
                } else {
                    self.workspace.model.open(id, 0);
                }
            }
        }
        self.active = ix;
        self.activate_session_ui(id, window, cx);
        self.draft_owner = Some(id);
        self.sidebar_view =
            self.sidebar_view.after_select(self.chats[ix].is_agent(), self.locked(ix));

        self.copied = None;
        self.selection_bar = None;
        self.open_error = None;
        self.attach_error = None;
        self.chats[ix].unseen = ChatStatus::Idle;
        // a TUI-owned session must not get our rules or turns; restore runs before any scan
        if create::reattach_wanted(&self.chats[ix]) && !self.locked(ix) {
            let (settings, session) = self.cli_request(ix);
            self.agent.attach(&settings, &session, create::restored_background(&self.chats[ix]));
        }
        if !self.chats[ix].messages.is_empty() {
            self.unhide_project(ix);
        }
        if let Some(folder) = self.chats[ix].folder(&self.settings.workspace_root) {
            self.refresh_branches(Some(folder), cx);
        }
        // Opening an archived chat restores it.
        if std::mem::replace(&mut self.chats[ix].archived, false) {
            self.persist(ix);
        }
        self.page = Page::Chat;
        let host = self.host_of(window);
        self.set_side_tab(host, crate::side_panel::SideTab::Agent);
        self.chats[ix].view = ChatViewMode::Chat;
        if self.folder_of(&self.chats[ix]).is_none() {
            self.active_terminal_tab = aether_terminal::TerminalTab::Global;
        } else if matches!(self.active_terminal_tab, aether_terminal::TerminalTab::Project(_)) {
            self.active_terminal_tab = aether_terminal::TerminalTab::Project(self.chats[ix].id);
        }
        self.workspace_pop.dismiss();
        for selector in [&self.selector, &self.default_model] {
            selector.update(cx, |s, cx| s.dismiss(cx));
        }
        self.worktree_menu.update(cx, |s, cx| s.dismiss(cx));
        for menu in [
            &self.access_menu,
            &self.project_menu,
            &self.headline_project_menu,
            &self.project_open_menu,
        ] {
            menu.update(cx, |s, cx| s.dismiss(cx));
        }
        self.sync_rows(None, cx);
        self.composer.update(cx, |s, cx| s.focus(window, cx));
        self.sync_selector(cx);
        self.sync_placeholder(window, cx);
        self.load_bases(cx);
        self.refresh_git(false, cx);
        self.refresh_missing_attachments(false, cx);
        if self.chats[ix].view == ChatViewMode::Explorer && self.is_agent_chat(&self.chats[ix]) {
            self.check_explorer_disk(&self.active_project_folder(), window, cx);
        }
        cx.notify();
    }

    /// Opens the chat and scrolls to the matching message.
    pub(crate) fn jump_to_message(
        &mut self,
        ix: usize,
        message: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select(ix, window, cx);
        if let Some(row) = self.rows.iter().position(|r| r.message == message) {
            self.scroller.update(cx, |s, cx| {
                s.scroll_to_item(row, cx);
            });
        }
    }

    /// The active chat's rows for messages `from..`, as they should be now.
    fn build_rows(&self, from: usize, cx: &mut App) -> Vec<TranscriptRow> {
        let chat = &self.chats[self.active];
        let opened = opened_tools(cx, &chat.session);
        let kept: Vec<_> = (from..chat.messages.len())
            .map(|message| {
                let opened = &opened;
                move |id: &str| opened.contains(id) || opened.contains(&format!("{message}-{id}"))
            })
            .collect();
        let last = chat.messages.len().checked_sub(1);
        let (busy, agent) = (chat.cancel.is_some(), self.is_agent_chat(chat));
        let detail = self.settings.chat_detail;
        set_verbose_rows(cx, detail == ChatDetail::Verbose);
        let view = |ix: usize| ReplyView {
            live: busy && Some(from + ix) == last,
            open: chat
                .open_steps
                .get(&(from + ix))
                .copied()
                .unwrap_or(detail == ChatDetail::Verbose),
            stopped: chat.stopped && Some(from + ix) == last,
            agent,
            show_hooks: self.settings.show_hooks,
            show_thinking: detail != ChatDetail::Compact,
            kept: &kept[ix],
        };
        let mut rows = transcript_rows(chat.messages.get(from..).unwrap_or_default(), &view);
        // Live status is attached to the composer, outside the scrolling transcript.
        rows.retain(|row| row.block != Block::Live);
        for row in &mut rows {
            row.message += from;
        }
        rows
    }

    /// Rebuilds the active chat's rows from message `touched` on (all of them for
    /// `None`), splices the list to match, and remeasures every row of `touched`.
    pub(crate) fn sync_rows(&mut self, touched: Option<usize>, cx: &mut Context<Self>) {
        self.sync(touched.map(|m| (m, Changed::All)), cx);
    }

    /// `sync_rows`, remeasuring only the rows of `touched` that `changed` may resize.
    /// Rows keep their place while they grow, so a reader scrolled into one stays put.
    pub(in crate::app) fn sync(
        &mut self,
        touched: Option<(usize, Changed)>,
        cx: &mut Context<Self>,
    ) {
        // from the previous last message on, which appended messages change
        let last = self.rows.last().map_or(0, |r| r.message);
        let from = touched.as_ref().map_or(0, |(m, _)| (*m).min(last));
        let at = self.rows.partition_point(|r| r.message < from);
        let rows = self.build_rows(from, cx);
        let (splices, changed) = row_splice(&self.rows[at..], &rows);
        self.rows.truncate(at);
        self.rows.extend(rows);
        let mut resized: Vec<Range<usize>> =
            changed.into_iter().map(|r| at + r..at + r + 1).collect();
        if let Some((m, changed)) = touched {
            let start = self.rows.partition_point(|r| r.message < m);
            let end = self.rows.partition_point(|r| r.message <= m);
            let steps = self.chats[self.active].messages.get(m).map_or(&[][..], |m| &m.steps);
            let one = |r: usize| r..r + 1;
            match changed {
                Changed::Blocks => {}
                Changed::Thought => resized.extend(
                    (start..end)
                        .rev()
                        .find(|&r| {
                            matches!(
                                &self.rows[r].block,
                                Block::Thought(_)
                                    | Block::Activity {
                                        kind: aether_ui::ActivityKind::Thinking,
                                        ..
                                    }
                            )
                        })
                        .map(one),
                ),
                Changed::Tool(id) => resized.extend(
                    (start..end).filter(|&r| shows_tool(steps, &self.rows[r].block, &id)).map(one),
                ),
                Changed::All => resized.push(start..end),
            }
        }
        self.scroller.update(cx, |s, cx| {
            // last first, so earlier ranges still index the old list
            for (range, count) in splices.into_iter().rev() {
                s.splice(at + range.start..at + range.end, count, cx);
            }
            for rows in resized {
                s.remeasure_items(rows, cx);
            }
        });
    }

    /// Rebuilds the active chat's rows and starts the list over.
    pub(in crate::app) fn reset_rows(&mut self, cx: &mut Context<Self>) {
        self.rows = self.build_rows(0, cx);
        let count = self.rows.len();
        self.scroller.update(cx, |s, cx| s.reset(count, cx));
    }

    /// New messages at the end of chat `ix`: list them and follow the tail.
    pub(in crate::app) fn append_rows(
        &mut self,
        ix: usize,
        touched: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        if ix == self.active {
            self.sync_rows(touched, cx);
            self.scroller.update(cx, |s, cx| s.scroll_to_end(cx));
        }
    }

    pub(crate) fn remeasure_row(&mut self, row: usize, cx: &mut Context<Self>) {
        self.scroller.update(cx, |s, cx| {
            s.remeasure_items(row..row + 1, cx);
        });
    }

    /// Folder an agent chat's paths are shown relative to and its inline code links into.
    pub(crate) fn transcript_cwd(&self) -> Option<SharedString> {
        let chat = &self.chats[self.active];
        chat.is_agent().then(|| {
            chat_cwd(chat, &self.settings.workspace_root).to_string_lossy().into_owned().into()
        })
    }

    pub(super) fn persist(&self, ix: usize) {
        if let Some(store) = &self.store {
            let chat = &self.chats[ix];
            store.save(SavedChat {
                id: chat.id,
                title: chat.title.to_string(),
                messages: chat.messages.clone(),
                pinned: chat.pinned,
                archived: chat.archived,
                provider: chat.provider,
                provider_model: chat.provider_model.clone(),
                cli_session: chat.cli_session.clone(),
                access: chat.access,
                effort: chat.effort.clone(),
                cwd: chat.cwd.clone(),
                project: chat.project.clone(),
                worktree_path: chat.worktree_path.clone(),
                worktree_branch: chat.worktree_branch.clone(),
                parent: chat.parent,
                lead: chat.lead,
                lead_seen: chat.lead_seen,
                pending_deliveries: chat
                    .queue
                    .iter()
                    .flat_map(|q| {
                        q.deliveries.iter().map(|d| aether_agent::SavedDelivery {
                            child: d.child,
                            reply_ix: d.reply_ix,
                            text: d.text.clone(),
                        })
                    })
                    .collect(),
            });
        }
    }

    pub(super) fn locked(&self, ix: usize) -> bool {
        let chat = &self.chats[ix];
        externally_locked(chat.external_pid, &chat.cli_session, &self.external).is_some()
    }

    pub(super) fn next_session_id(&mut self) -> String {
        self.sessions_made += 1;
        format!("chat-{}", self.sessions_made)
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/chat/mod.rs"]
mod tests;
