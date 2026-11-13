use std::time::Duration;

use std::rc::Rc;
use std::sync::atomic::Ordering;

use aether_core::{Message, Provider, Step};
use aether_driver::antigravity::{agy_aether_entry, agy_mcp_config, register_agy_aether};
use aether_ui::theme::Corners;
use aether_ui::{
    FatalCard, NoticeLine, NoticeLink, OnLink, StartupList, SteerMarker, StoppedMarker,
    stopped_lead, stopped_summary,
};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{ButtonVariant, ButtonVariants};
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, ClipboardItem, Context, FontWeight, Role, SharedString, Window, div, prelude::*, px,
};

use super::create::restored_background;
use super::row_reflow;
use crate::app::format::{chat_markdown, export_filename, format_time, now_ms};
use crate::app::orchestrate::needs_agy_registration;
use crate::app::worktree::notice;
use crate::app::{ChatApp, WorktreeJob};
use crate::attachments;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bulk {
    Delete,
    Archive,
    Unarchive,
}

const AGY_REGISTER_PROMPT: &str = "Antigravity can only load Aether's tools from its global MCP config (~/.gemini/config/mcp_config.json). Register Aether there? Outside Aether the entry stays inactive.";

/// Why a chat can't be deleted or archived yet: deleting it mid-create would orphan the worktree.
fn worktree_busy(title: &str) -> String {
    format!(
        "“{title}” is still setting up or removing its worktree. Wait for it to finish, or cancel it, then try again."
    )
}

/// Deleting the lead mid-run would orphan its running children and drop their deliveries.
fn crew_busy(title: &str, n: usize) -> String {
    format!(
        "“{title}” still has {n} starting or running agent{}. Wait for them to finish or stop them, then try again.",
        if n == 1 { "" } else { "s" }
    )
}

impl ChatApp {
    pub(crate) fn begin_rename(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.chats.len() {
            return;
        }
        self.renaming = Some(ix);
        self.rename_in_tab = false;
        let title = self.chats[ix].title.to_string();
        self.rename_input.update(cx, |s, cx| {
            s.set_value(title, window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    pub(in crate::app) fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.renaming.take() else {
            return;
        };
        let title = self.rename_input.read(cx).value().trim().to_string();
        if !title.is_empty()
            && let Some(chat) = self.chats.get_mut(ix)
        {
            chat.title = title.into();
            self.persist(ix);
        }
        cx.notify();
    }

    pub(crate) fn confirm_delete_chat(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.chats.get(ix) else {
            return;
        };
        if chat.worktree_job.is_some() {
            return notice("Wait for the worktree", worktree_busy(&chat.title), window, cx);
        }
        let running = if chat.lead { self.running_children(ix) } else { 0 };
        let (id, title) = (chat.id, chat.title.to_string());
        let finished = if chat.lead { self.finished_crew(id) } else { Vec::new() };
        if running > 0 && finished.is_empty() {
            return notice("Wait for your crew", crew_busy(&chat.title, running), window, cx);
        }
        let mut text = "This removes the chat and its history. This can't be undone.".to_string();
        if chat.has_worktree(&self.missing_folders, &self.settings.workspace_root) {
            text.push_str(
                " Its worktree folder stays on disk; use Remove worktree… first to delete it.",
            );
        }
        if let Some(job) = self.terminal_job(chat.id, cx) {
            text.push_str(&format!(" Its terminal is running {job}, which will be stopped."));
        }
        let crew = if chat.lead { self.crew_ids(id) } else { Vec::new() };
        // a lead can't go while agents run, so then only its finished agents can
        let options = crew_options(id, (running == 0).then_some(&crew[..]), &finished);
        let picked = Rc::new(std::cell::Cell::new(0));
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let (view, options, picked) = (view.clone(), options.clone(), picked.clone());
            let labels = options.iter().map(|(label, _)| label.clone()).collect();
            alert
                .confirm()
                .title(SharedString::from(format!("Delete “{title}”?")))
                .description(crew_scope(text.clone(), "delete-crew", labels, &picked))
                .ok_text("Delete")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let ids = options.get(picked.get()).map(|(_, ids)| ids.clone());
                    view.update(cx, |this, cx| {
                        for id in ids.unwrap_or_default() {
                            if let Some(ix) = this.chat_ix(id) {
                                this.delete_chat(ix, window, cx);
                            }
                        }
                    });
                    true
                })
        });
    }

    /// Every chat a lead spawned, and theirs in turn.
    pub(crate) fn crew_ids(&self, lead: i64) -> Vec<i64> {
        let mut out = Vec::new();
        let mut queue = vec![lead];
        while let Some(parent) = queue.pop() {
            for chat in self.chats.iter().filter(|c| c.parent == Some(parent)) {
                if chat.id != lead && !out.contains(&chat.id) {
                    out.push(chat.id);
                    queue.push(chat.id);
                }
            }
        }
        out
    }

    /// Archives a plain chat at once; a lead asks first and offers its crew too.
    pub(crate) fn confirm_archive_chat(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.chats.get(ix) else {
            return;
        };
        if !chat.lead {
            return self.archive_chat(ix, window, cx);
        }
        let (id, title) = (chat.id, chat.title.to_string());
        let crew: Vec<i64> = self
            .crew_ids(id)
            .into_iter()
            .filter(|id| self.chat_ix(*id).is_some_and(|ix| !self.chats[ix].archived))
            .collect();
        let finished = self.finished_crew(id);
        let options = crew_options(id, Some(&crew), &finished);
        let picked = Rc::new(std::cell::Cell::new(0));
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let (view, options, picked) = (view.clone(), options.clone(), picked.clone());
            let labels = options.iter().map(|(label, _)| label.clone()).collect();
            alert
                .confirm()
                .title(SharedString::from(format!("Archive “{title}”?")))
                .description(crew_scope(
                    "It moves to Archive; open it from there or the palette to restore it.".into(),
                    "archive-crew",
                    labels,
                    &picked,
                ))
                .ok_text("Archive")
                .on_ok(move |_, window, cx| {
                    let ids = options.get(picked.get()).map(|(_, ids)| ids.clone());
                    view.update(cx, |this, cx| {
                        for id in ids.unwrap_or_default() {
                            if let Some(ix) = this.chat_ix(id) {
                                this.archive_chat(ix, window, cx);
                            }
                        }
                    });
                    true
                })
        });
    }

    pub(crate) fn delete_chat(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.chats.len() {
            return;
        }
        if self.chats[ix].lead && self.running_children(ix) > 0 {
            let n = self.running_children(ix);
            return notice("Wait for your crew", crew_busy(&self.chats[ix].title, n), window, cx);
        }
        match self.renaming {
            Some(r) if r == ix => self.renaming = None,
            Some(r) if r > ix => self.renaming = Some(r - 1),
            _ => {}
        }
        let id = self.chats[ix].id;
        if let Some(pane) = self.workspace.model.pane_for(id) {
            self.workspace.views.remove(&pane);
        }
        self.workspace.model.forget_chat(id);
        self.session_views.remove(&id);
        self.session_subscriptions.remove(&id);
        self.session_windows.remove(&id);
        self.expire_chat_requests(ix);
        let mut chat = self.chats.remove(ix);
        if let Some(WorktreeJob::Creating(_, cancel)) = &chat.worktree_job {
            cancel.store(true, Ordering::Relaxed);
        }
        if let Some(cancel) = chat.cancel.take() {
            cancel.cancel();
        }
        self.agent.close_session(&chat.id.to_string());
        self.drop_btw(chat.id);
        self.close_terminal(chat.id, cx);
        self.unwatch_chat(chat.id, cx);
        self.cancel_voice_of(chat.id, cx);
        self.git.forget(chat.id);
        if let Some(store) = &self.store {
            store.delete(chat.id);
        }
        self.board_unlink(chat.id);
        for ix in 0..self.chats.len() {
            let c = &mut self.chats[ix];
            let parent = c.parent == Some(chat.id);
            let source = c.handoff_from == Some(chat.id);
            if parent || source {
                if parent {
                    (c.parent, c.lead_seen) = (None, None);
                }
                if source {
                    c.handoff_from = None;
                }
                self.persist(ix);
            }
        }
        if let Some(root) = aether_core::attachments_dir() {
            let id = chat.id;
            cx.background_executor()
                .spawn(async move { attachments::remove_chat(&root, id) })
                .detach();
        }
        if self.chats.is_empty() {
            self.new_chat(window, cx);
            return;
        }
        let mut active = self.active;
        if active == ix || active >= self.chats.len() {
            active = active.min(self.chats.len() - 1);
        } else if active > ix {
            active -= 1;
        }
        self.select(active, window, cx);
    }

    /// Asks before deleting, archiving or unarchiving several chats at once.
    pub(crate) fn confirm_bulk(
        &mut self,
        bulk: Bulk,
        ids: Vec<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ids: Vec<i64> = ids.into_iter().filter(|id| self.chat_ix(*id).is_some()).collect();
        let n = ids.len();
        if n == 0 {
            return;
        }
        let noun = if n == 1 { "chat" } else { "chats" };
        let (verb, text) = match bulk {
            Bulk::Delete => (
                "Delete",
                "This removes them and their history. This can't be undone. Worktree folders stay on disk.",
            ),
            Bulk::Archive => {
                ("Archive", "They move to the Archive. You can unarchive them any time.")
            }
            Bulk::Unarchive => ("Unarchive", "They move back to the sidebar."),
        };
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let (view, ids) = (view.clone(), ids.clone());
            alert
                .confirm()
                .title(SharedString::from(format!("{verb} {n} {noun}?")))
                .description(text)
                .ok_text(SharedString::from(format!("{verb} {n}")))
                .ok_variant(if bulk == Bulk::Delete {
                    ButtonVariant::Danger
                } else {
                    ButtonVariant::Primary
                })
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| this.apply_bulk(bulk, &ids, window, cx));
                    true
                })
        });
    }

    fn picks(&mut self, project: bool) -> &mut std::collections::HashSet<i64> {
        if project { &mut self.project_picks } else { &mut self.archive_picks }
    }

    /// Count, select all, the given actions and clear, for `picked` of `all`.
    pub(crate) fn bulk_bar(
        &self,
        key: &'static str,
        picked: Vec<i64>,
        all: Vec<i64>,
        actions: &[Bulk],
        project: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if picked.is_empty() {
            return None;
        }
        let theme = cx.theme();
        let n = picked.len();
        let select_all = (n < all.len()).then(|| {
            aether_ui::button((key, 0usize)).ghost().xsmall().label("Select all").on_click(
                cx.listener(move |this, _, _, cx| {
                    this.picks(project).extend(all.iter().copied());
                    cx.notify();
                }),
            )
        });
        let clear = aether_ui::button((key, 1usize))
            .ghost()
            .xsmall()
            .icon(Lucide::X)
            .accessibility_label("Clear selection")
            .tooltip("Clear selection")
            .on_click(cx.listener(move |this, _, _, cx| {
                this.picks(project).clear();
                cx.notify();
            }));
        let buttons = actions.iter().enumerate().map(|(i, &bulk)| {
            let (label, icon) = match bulk {
                Bulk::Delete => ("Delete", Lucide::Trash),
                Bulk::Archive => ("Archive", Lucide::Archive),
                Bulk::Unarchive => ("Unarchive", Lucide::ArchiveRestore),
            };
            let ids = picked.clone();
            aether_ui::button((key, 2 + i))
                .small()
                .h(px(28.))
                .icon(icon)
                .label(label)
                .map(|b| if bulk == Bulk::Delete { b.danger() } else { b.outline() })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.confirm_bulk(bulk, ids.clone(), window, cx)
                }))
        });
        Some(
            h_flex()
                .id(key)
                .role(Role::Toolbar)
                .aria_label("Selected chats")
                .w_full()
                .gap(px(6.))
                .items_center()
                .flex_wrap()
                .p(px(6.))
                .corner(px(9.))
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .child(
                    div()
                        .px(px(4.))
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .child(format!("{n} selected")),
                )
                .children(select_all)
                .child(div().flex_1())
                .children(buttons)
                .child(clear)
                .into_any_element(),
        )
    }

    /// Skips chats still creating a worktree or running a crew, then reports them.
    pub(crate) fn apply_bulk(
        &mut self,
        bulk: Bulk,
        ids: &[i64],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut skipped = Vec::new();
        for &id in ids {
            let Some(ix) = self.chat_ix(id) else { continue };
            let chat = &self.chats[ix];
            let busy = bulk != Bulk::Unarchive
                && (chat.worktree_job.is_some() || (chat.lead && self.running_children(ix) > 0));
            if busy {
                skipped.push(format!("“{}”", chat.title));
                continue;
            }
            match bulk {
                Bulk::Delete => self.delete_chat(ix, window, cx),
                Bulk::Archive => self.archive_chat(ix, window, cx),
                Bulk::Unarchive => self.unarchive_chat(ix, cx),
            }
        }
        self.archive_picks.retain(|id| !ids.contains(id));
        self.project_picks.retain(|id| !ids.contains(id));
        if !skipped.is_empty() {
            let text = format!(
                "{} {} still setting up a worktree or running agents. Wait for them, then try again.",
                skipped.join(", "),
                if skipped.len() == 1 { "is" } else { "are" }
            );
            notice("Some chats were skipped", text, window, cx);
        }
        cx.notify();
    }

    /// Flips the pin; pinned chats sort to the top of their project.
    pub(crate) fn toggle_pin(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.chat_pin_refusal(ix) {
            return notice("Can't pin chat", text, window, cx);
        }
        if let Some(chat) = self.chats.get_mut(ix) {
            chat.pinned = !chat.pinned;
        }
        self.persist(ix);
        cx.notify();
    }

    pub(crate) fn toggle_lead(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chats.get(ix) else {
            return;
        };
        if chat.lead || chat.provider != Provider::Antigravity {
            return self.set_lead(ix, !chat.lead, window, cx);
        }
        self.agy_lead_check(chat.id, true, window, cx);
    }

    /// For an Antigravity lead: asks to register Aether in agy's MCP config when it isn't,
    /// else turns lead on when `turn_on`.
    pub(crate) fn agy_lead_check(
        &mut self,
        id: i64,
        turn_on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let provider = Provider::Antigravity;
        let check = cx.background_spawn(async move {
            let exe = std::env::current_exe().ok();
            needs_agy_registration(
                provider,
                agy_aether_entry(&agy_mcp_config()).as_ref(),
                exe.as_deref(),
            )
        });
        cx.spawn_in(window, async move |this, cx| {
            let needs = check.await;
            this.update_in(cx, |this, window, cx| {
                let Some(ix) = this.chat_ix(id) else {
                    return;
                };
                if needs {
                    this.confirm_agy_registration(id, window, cx);
                } else if turn_on {
                    this.set_lead(ix, true, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn set_lead(&mut self, ix: usize, lead: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chats.get(ix) else {
            return;
        };
        if !lead && chat.lead && self.running_children(ix) > 0 {
            let n = self.running_children(ix);
            return notice("Wait for your crew", crew_busy(&chat.title, n), window, cx);
        }
        let started = !chat.messages.is_empty();
        if lead && started && chat.busy() {
            return notice(
                "Wait for this reply",
                format!(
                    "“{}” is still replying. Wait for it to finish, then make it a lead — turning lead on restarts its driver with the orchestration tools.",
                    chat.title
                ),
                window,
                cx,
            );
        }
        if let Some(chat) = self.chats.get_mut(ix) {
            chat.lead = lead;
        }
        self.persist(ix);
        if lead && started {
            self.restart_agent(ix, cx);
        }
        cx.notify();
    }

    fn confirm_agy_registration(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let view = view.clone();
            alert
                .confirm()
                .title("Register Aether with Antigravity?")
                .description(AGY_REGISTER_PROMPT)
                .ok_text("Register")
                .cancel_text("Cancel")
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| this.register_agy_lead(id, window, cx));
                    true
                })
        });
    }

    fn register_agy_lead(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let (binary, exe) = (self.settings.antigravity_binary.clone(), std::env::current_exe());
        let work = cx.background_spawn(async move { register_agy_aether(&binary, &exe?) });
        cx.spawn_in(window, async move |this, cx| {
            let outcome = work.await;
            this.update_in(cx, |this, window, cx| match (outcome, this.chat_ix(id)) {
                (Err(e), _) => notice("Couldn't register Aether", e.to_string(), window, cx),
                (Ok(()), Some(ix)) => this.set_lead(ix, true, window, cx),
                (Ok(()), None) => {}
            })
            .ok();
        })
        .detach();
    }

    /// Hides the chat from the sidebar; opening it from the palette restores it.
    pub(crate) fn archive_chat(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chats.get(ix) else {
            return;
        };
        if chat.worktree_job.is_some() {
            return notice("Wait for the worktree", worktree_busy(&chat.title), window, cx);
        }
        let chat_id = self.chats[ix].id;
        if let Some(pane) = self.workspace.model.pane_for(chat_id) {
            self.workspace.views.remove(&pane);
        }
        self.workspace.model.forget_chat(chat_id);
        self.chats[ix].archived = true;
        self.unwatch_chat(chat_id, cx);
        self.stop_chat_scripts(self.chats[ix].id, cx);
        self.cancel_voice_of(chat_id, cx);
        self.persist(ix);
        if ix == self.active {
            match self.chats.iter().rposition(|c| !c.archived) {
                Some(next) => self.select(next, window, cx),
                None => self.new_chat(window, cx),
            }
        } else {
            cx.notify();
        }
    }

    pub(crate) fn unarchive_chat(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(chat) = self.chats.get_mut(ix) {
            chat.archived = false;
            self.persist(ix);
            cx.notify();
        }
    }

    /// Copies one reply's Markdown to the clipboard.
    pub(in crate::app) fn copy_message(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(message) = self.chats[self.active].messages.get(ix) {
            cx.write_to_clipboard(ClipboardItem::new_string(message.text.clone()));
            let copied = Some((self.active, ix));
            self.copied = copied;
            cx.notify();
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                this.update(cx, |this, cx| {
                    if this.copied == copied {
                        this.copied = None;
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    /// Asks where to save the active chat, then writes it as Markdown.
    pub(crate) fn export_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chat = &self.chats[self.active];
        let md = chat_markdown(chat);
        let name = export_filename(&chat.title);
        let home = Some(aether_core::home_dir())
            .filter(|home| !home.as_os_str().is_empty())
            .unwrap_or_else(std::env::temp_dir);
        let rx = cx.prompt_for_new_path(&home, Some(&name));
        cx.spawn_in(window, async move |_, _| {
            let Ok(Ok(Some(path))) = rx.await else {
                return;
            };
            if let Err(e) = std::fs::write(&path, md) {
                eprintln!("aether: couldn't export chat: {e}");
            }
        })
        .detach();
    }

    /// Closes chat `ix`'s CLI session and opens a fresh one on the same resume id; never
    /// while a TUI holds the session.
    pub(crate) fn restart_agent(&mut self, ix: usize, cx: &mut Context<Self>) {
        if !self.chats[ix].is_agent() || self.locked(ix) {
            return;
        }
        let (settings, session) = self.cli_request(ix);
        self.agent.close_session(&session.key);
        self.expire_chat_requests(ix);
        self.sync_chat_rows(ix, None, cx);
        self.persist(ix);
        self.agent.attach(&settings, &session, restored_background(&self.chats[ix]));
        let now = now_ms();
        self.refresh_health(ix, now);
        self.chats[ix].restarted_ms = now;
        cx.notify();
    }

    /// Asks the agent whether chat `ix`'s CLI still runs; it spawns `kill -0`, so never per render.
    pub(in crate::app) fn refresh_health(&mut self, ix: usize, now: i64) {
        let chat = &mut self.chats[ix];
        chat.health = self.agent.health(&chat.id.to_string()).map(|h| (h, now));
    }

    fn message(&self, msg: usize) -> Option<&Message> {
        self.chats[self.active].messages.get(msg)
    }

    /// The reply is still streaming, so its running notices may still change.
    fn reply_live(&self, msg: usize) -> bool {
        let chat = &self.chats[self.active];
        chat.cancel.is_some() && msg + 1 == chat.messages.len()
    }

    pub(in crate::app) fn notice_row(
        &self,
        msg: usize,
        step: usize,
        ix: usize,
        view: &crate::app::SessionHandle,
    ) -> AnyElement {
        let chat = &self.chats[self.active];
        let Some(Step::Notice(notice)) = self.message(msg).and_then(|m| m.steps.get(step)) else {
            return div().into_any_element();
        };
        let chat_id = chat.id.to_string();
        let link_view = view.clone();
        let on_link: OnLink = Rc::new(move |link, _, cx| match link {
            NoticeLink::Open(url) => cx.open_url(&url),
            NoticeLink::Action(action) => {
                link_view.update(cx, |app, _| app.agent.notice_action(&chat_id, &action));
            }
        });
        NoticeLine::new(format!("{}-{msg}-n{step}", chat.session), notice.clone())
            .now_ms(now_ms())
            .live(self.reply_live(msg))
            .on_link(Some(on_link))
            .on_reflow(row_reflow(view, ix))
            .cwd(self.transcript_cwd())
            .into_any_element()
    }

    pub(in crate::app) fn startup_row(
        &self,
        msg: usize,
        steps: &[usize],
        _view: &crate::app::SessionHandle,
    ) -> AnyElement {
        let Some(m) = self.message(msg) else {
            return div().into_any_element();
        };
        let notices = steps.iter().filter_map(|&i| match m.steps.get(i) {
            Some(Step::Notice(notice)) => Some(notice.clone()),
            _ => None,
        });
        StartupList::new(notices.collect()).live(self.reply_live(msg)).into_any_element()
    }

    pub(in crate::app) fn steer_row(
        &self,
        msg: usize,
        step: usize,
        view: &crate::app::SessionHandle,
    ) -> AnyElement {
        match self.message(msg).and_then(|m| m.steps.get(step)) {
            Some(Step::Steer(text)) => {
                let (view, chat, full) = (view.clone(), self.chats[self.active].id, text.clone());
                SteerMarker::new(SharedString::from(format!("steer-{msg}-{step}")), text.clone())
                    .on_open(move |_, cx| {
                        view.update(cx, |this, cx| {
                            this.background_hidden.remove(&chat);
                            this.inspect_steer = Some((chat, full.clone()));
                            cx.notify();
                        })
                    })
                    .into_any_element()
            }
            _ => div().into_any_element(),
        }
    }

    pub(in crate::app) fn stopped_row(
        &self,
        msg: usize,
        _view: &crate::app::SessionHandle,
    ) -> AnyElement {
        let Some(m) = self.message(msg) else {
            return div().into_any_element();
        };
        StoppedMarker::new(
            stopped_lead(&m.steps, m.failed),
            m.elapsed_ms,
            stopped_summary(&m.steps),
        )
        .into_any_element()
    }

    pub(in crate::app) fn fatal_row(
        &self,
        msg: usize,
        ix: usize,
        view: &crate::app::SessionHandle,
    ) -> AnyElement {
        let Some(m) = self.message(msg) else {
            return div().into_any_element();
        };
        let Some(error) = m.error.clone() else {
            return div().into_any_element();
        };
        let chat = &self.chats[self.active];
        let targets =
            if m.handoff.is_some() { vec![] } else { self.handoff_targets(Some(chat.provider)) };
        let (restart, retry, handoff) = (view.clone(), view.clone(), view.clone());
        let copy = error.clone();
        FatalCard::new(format!("{}-{msg}-fatal", chat.session), error)
            .time(m.ended_at_ms.and_then(format_time))
            .can_restart(chat.is_agent() && !self.locked(self.active))
            .actions(msg + 1 == chat.messages.len())
            .on_restart(Some(Rc::new(move |_, cx| {
                restart.update(cx, |this, cx| this.restart_agent(this.active, cx));
            })))
            .on_retry(Some(Rc::new(move |window, cx| {
                retry.update(cx, |this, cx| {
                    this.retry(window, cx);
                    this.focus_composer(window, cx);
                });
            })))
            .on_copy(Some(Rc::new(move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
            })))
            .handoff(
                targets,
                Rc::new(move |target, window, cx| {
                    handoff.update(cx, |this, cx| this.handoff_to(this.active, target, window, cx));
                }),
            )
            .on_reflow(row_reflow(view, ix))
            .into_any_element()
    }
}

/// Archive or Delete reaches the lead with its whole crew, or only finished agents.
/// `crew` is `None` when the lead can't go. An empty crew is a standalone chat.
pub(crate) fn crew_options(
    lead: i64,
    crew: Option<&[i64]>,
    finished: &[i64],
) -> Vec<(String, Vec<i64>)> {
    let agents = |n: usize| if n == 1 { "1 agent".to_string() } else { format!("{n} agents") };
    let mut options = Vec::new();
    if let Some(crew) = crew {
        let all = crew.iter().copied().chain([lead]).collect();
        let label = if crew.is_empty() {
            "This chat".to_string()
        } else {
            format!("This chat and all its {}", agents(crew.len()))
        };
        options.push((label, all));
    }
    if !finished.is_empty() {
        let n = finished.len();
        let noun = if n == 1 { "agent" } else { "agents" };
        let label = format!("Only its {n} finished {noun} (done or failed); this chat stays");
        options.push((label, finished.to_vec()));
    }
    options
}

/// The dialog text, with a choice of scope when there's more than one.
fn crew_scope(
    text: String,
    id: &'static str,
    labels: Vec<String>,
    picked: &Rc<std::cell::Cell<usize>>,
) -> AnyElement {
    let pick = picked.clone();
    let only_lead = labels.len() == 1 && labels[0] == "This chat";
    v_flex()
        .gap_3()
        .child(div().child(text))
        .when(!labels.is_empty() && !only_lead, |column| {
            column.child(
                RadioGroup::vertical(id)
                    .children(
                        labels.into_iter().enumerate().map(|(i, l)| Radio::new((id, i)).label(l)),
                    )
                    .selected_index(Some(picked.get()))
                    .on_change(move |ix: &usize, window, _| {
                        pick.set(*ix);
                        window.refresh();
                    }),
            )
        })
        .into_any_element()
}
