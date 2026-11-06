use aether_core::{Ask, NoticeKind, NoticeState, Provider, Step};
use aether_ui::{
    LiveFacts, Phase, Stall, StallActions, StatusLine, Wait, countdown, last_event, phase,
    running_subagents, since, stalled, verb, wait_detail,
};
use gpui_kit::{AnyElement, SharedString, prelude::*};
use std::rc::Rc;

use crate::app::{Chat, ChatApp, Queued, now_ms};

pub(crate) const STATUS_NUDGE: &str =
    "Status check: what are you working on right now, and is anything blocking you?";

/// Only OpenCode's TUI attaches to the live session, its service is shared. Others fork it.
pub(crate) fn tui_command(provider: Provider, session: &str) -> Option<String> {
    (provider == Provider::OpenCode && !session.is_empty())
        .then(|| format!("opencode --session {session}"))
}

/// Quiet time counts from the last event, or the turn start before any.
fn anchor(last_event_ms: i64, turn_at: i64) -> i64 {
    last_event_ms.max(turn_at)
}

fn idle_since(anchor: i64, now: i64) -> u64 {
    if anchor > 0 { (now - anchor).max(0) as u64 } else { 0 }
}

fn wait_of(ask: &Ask) -> (Wait, Option<&str>) {
    match ask {
        Ask::Approval(a) => (Wait::Approval, a.source.as_deref()),
        Ask::Question { .. } | Ask::Questions(_) => (Wait::Answer, None),
        _ => (Wait::Input, None),
    }
}

/// Waiting on the user restarts the quiet clock, so answering never reads as a stall.
fn hold_clock(last_event_ms: &mut i64, pending: bool, now: i64) {
    if pending {
        *last_event_ms = now;
    }
}

/// The card shows for a stalled turn the user hasn't chosen to keep waiting on.
fn card_shown(stalled: bool, kept_ms: i64, anchor: i64) -> bool {
    stalled && kept_ms != anchor
}

/// Finished and restored messages have nothing to redraw each second; background rows
/// of any earlier reply still tick in the composer tray.
fn has_live_work(busy: bool, chat: &Chat) -> bool {
    busy || chat.has_live_background()
}

impl ChatApp {
    /// 1 Hz: redraws live timers only while a chat has live work, so finished and restored rows stay still.
    pub(in crate::app) fn tick_live(&mut self, cx: &mut gpui_kit::Context<Self>) {
        let now = now_ms();
        let mut live = false;
        for ix in 0..self.chats.len() {
            let chat = &mut self.chats[ix];
            let busy = chat.busy();
            // a running turn can (re)background rows in place; rescan once it settles
            if busy {
                chat.invalidate_bg();
            }
            live |= has_live_work(busy, chat);
            if !busy {
                continue;
            }
            let pending = chat.messages.last().is_some_and(|m| m.pending_request().is_some());
            hold_clock(&mut chat.last_event_ms, pending, now);
            let at = anchor(chat.last_event_ms, chat.turn_at);
            let shown = ix == self.active
                && card_shown(stalled(idle_since(at, now), pending), chat.stall_kept_ms, at);
            if shown && chat.is_agent() && chat.health.is_none_or(|(_, at)| now - at >= 5_000) {
                // ponytail: kill -0 spawn on the UI thread every 5 s while the card shows, move off-thread if it shows in profiles
                self.refresh_health(ix, now);
            }
        }
        let sec = now / 1000;
        let minute_tick = sec % 60 == 0;
        if live || self.settings.status_bar_show_seconds || minute_tick {
            cx.notify();
        }
    }

    pub(in crate::app) fn live_row(
        &self,
        msg: usize,
        view: &crate::app::SessionHandle,
    ) -> AnyElement {
        let ix = self.active;
        let chat = &self.chats[ix];
        let Some(m) = chat.messages.get(msg) else {
            return gpui_kit::div().into_any_element();
        };
        let now = now_ms();
        let pending = m.pending_request();
        let (wait, source) = pending.map(|r| wait_of(&r.ask)).unzip();
        let source = source.flatten();
        let subagents = running_subagents(m);
        let at = anchor(chat.last_event_ms, chat.turn_at);
        let idle_ms = idle_since(at, now);
        let stall = stalled(idle_ms, pending.is_some());
        let phase = phase(m, wait);
        let provider = chat.provider;
        let retry_in = m.steps.iter().rev().find_map(|s| match s {
            Step::Notice(n) if n.state == NoticeState::Running && n.kind == NoticeKind::Retry => {
                n.until_ms
            }
            _ => None,
        });
        let facts = LiveFacts {
            phase,
            label: match phase {
                Phase::Starting
                | Phase::Thinking
                | Phase::Writing
                | Phase::Working
                | Phase::Tools(_)
                | Phase::Subagents(_) => verb(chat.turn_at).into(),
                _ => phase.label(provider.label()),
            },
            detail: wait.and_then(|_| wait_detail(source, subagents)),
            elapsed: since((chat.turn_at > 0).then_some(chat.turn_at), now),
            tokens: m.completion_tokens,
            retry_in: retry_in
                .filter(|_| phase == Phase::Retrying)
                .and_then(|t| countdown(Some(t), now)),
            idle_ms,
            stall: card_shown(stall, chat.stall_kept_ms, at).then(|| Stall {
                last_event: last_event(m),
                quiet: since(Some(at), now).unwrap_or_default(),
                alive: chat.health.map(|(health, _)| health.alive),
            }),
        };
        let actions = self.stall_actions(view);
        StatusLine::new(format!("{}-{msg}-live", chat.session), facts)
            .on_stall(actions)
            .into_any_element()
    }
    pub(crate) fn stall_actions(&self, view: &crate::app::SessionHandle) -> StallActions {
        let ix = self.active;
        let chat = &self.chats[ix];
        let provider = chat.provider;
        let (keep, nudge, tui, stop) = (view.clone(), view.clone(), view.clone(), view.clone());
        let last_row = chat.messages.last().and_then(|m| match m.steps.last() {
            Some(Step::Tool(call)) => Some(call.id.clone()),
            _ => None,
        });
        let (inspect, chat_id, session) = (view.clone(), chat.id, chat.session.clone());
        let command = tui_command(provider, &chat.cli_session);
        StallActions {
            keep: Rc::new(move |_, cx| {
                keep.update(cx, |this, cx| {
                    let chat = &mut this.chats[this.active];
                    chat.stall_kept_ms = anchor(chat.last_event_ms, chat.turn_at);
                    cx.notify();
                })
            }),
            nudge: chat.is_agent().then(|| -> aether_ui::Act {
                Rc::new(move |window, cx| {
                    nudge.update(cx, |this, cx| {
                        let chat = &mut this.chats[this.active];
                        chat.queue.push(Queued {
                            text: STATUS_NUDGE.into(),
                            attachments: Vec::new(),
                            delivering: false,
                            from_children: false,
                            deliveries: vec![],
                        });
                        let last = chat.queue.len() - 1;
                        this.steer_queued(last, window, cx);
                    })
                })
            }),
            tui: command.map(|command| {
                let label = SharedString::from(format!("Open in {} TUI", provider.label()));
                let open: aether_ui::Act = Rc::new(move |window, cx| {
                    let command = command.clone();
                    tui.update(cx, |this, cx| {
                        let Some(drawer) = this.drawer_for(this.active, window, cx) else { return };
                        let key = SharedString::from("tui");
                        let title = SharedString::from(format!("{} TUI", provider.label()));
                        drawer.update(cx, |t, cx| {
                            t.run_command(key.clone(), title, command, window, cx)
                        });
                        this.show_command(&key, window, cx);
                    })
                });
                (label, open)
            }),
            stop: Rc::new(move |_, cx| stop.update(cx, |this, cx| this.stop_chat(this.active, cx))),
            inspect: last_row.map(|tool| -> aether_ui::Act {
                Rc::new(move |_, cx| {
                    aether_ui::open_tool(cx, aether_ui::tool_key(&session, &tool));
                    inspect.update(cx, |this, cx| {
                        this.background_hidden.remove(&chat_id);
                        this.inspect = Some((chat_id, tool.clone()));
                        cx.notify();
                    })
                })
            }),
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/chat/live.rs"]
mod tests;
