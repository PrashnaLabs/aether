use aether_agent::Reply;
use aether_core::{AccessMode, Answer, Provider, Role, Update};
use futures::StreamExt;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::{AnyElement, Context, SharedString, Window, div, prelude::*, px};

use super::handoff_history;
use crate::app::ChatApp;
use crate::side_panel::Surface;

pub(crate) struct BtwTurn {
    pub question: String,
    pub answer: String,
    pub error: Option<String>,
    pub running: bool,
}

impl BtwTurn {
    fn new(question: String) -> Self {
        Self { question, answer: String::new(), error: None, running: true }
    }
}

pub(crate) struct BtwThread {
    pub turns: Vec<BtwTurn>,
    cancel: Option<aether_agent::Cancel>,
    running: bool,
}

impl BtwThread {
    fn new() -> Self {
        Self { turns: Vec::new(), cancel: None, running: false }
    }
}

/// Text after `/btw` or `/side`; empty when no question follows.
pub(crate) fn btw_rest(text: &str) -> Option<&str> {
    let text = text.trim();
    let rest = text.strip_prefix("/btw").or_else(|| text.strip_prefix("/side"))?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

pub(crate) fn btw_question(text: &str) -> Option<&str> {
    btw_rest(text).filter(|q| !q.is_empty())
}

fn side_prompt(question: &str) -> String {
    format!(
        "Side question from the user. Answer briefly from the conversation so far. Do not use tools or change files unless reading is essential. Question: {question}"
    )
}

fn btw_prompt(prior: &[BtwTurn], question: &str) -> String {
    let prompt = side_prompt(question);
    if prior.is_empty() {
        return prompt;
    }
    let turns = prior
        .iter()
        .map(|t| format!("User: {}\nAssistant: {}", t.question, t.answer))
        .collect::<Vec<_>>()
        .join("\n");
    format!("Earlier in this side chat:\n{turns}\n\n{prompt}")
}

fn btw_key(chat: i64) -> String {
    format!("btw:{chat}")
}

fn empty_note(turn: &BtwTurn) -> Option<&'static str> {
    if !turn.answer.is_empty() || turn.error.is_some() {
        None
    } else if turn.running {
        Some("Thinking…")
    } else {
        Some("No answer")
    }
}

impl ChatApp {
    /// Asks one question in the chat's BTW thread, opening its panel.
    pub(in crate::app) fn ask_btw(&mut self, ix: usize, question: String, cx: &mut Context<Self>) {
        let chat = &self.chats[ix];
        let id = chat.id;
        self.btw_open = true;
        if self.settings.chat_layout != aether_core::ChatLayout::Tabbed {
            self.right_surface = Some(Surface::Btw);
        }
        let thread = self.btw.entry(id).or_insert_with(BtwThread::new);
        thread.turns.push(BtwTurn::new(question.clone()));
        let turn = thread.turns.len() - 1;
        thread.running = true;
        if chat.provider == Provider::OpenRouter && self.api_key.is_none() {
            let thread = self.btw.get_mut(&id).expect("thread just inserted");
            thread.turns[turn].error = Some("No OpenRouter API key. Add one in Settings.".into());
            thread.turns[turn].running = false;
            thread.running = false;
            return cx.notify();
        }
        let prompt = btw_prompt(&thread.turns[..turn], &question);
        let native = self.agent.side_question(&chat.id.to_string(), &prompt);
        match native {
            Some(Reply { mut updates, .. }) => {
                cx.spawn(async move |this, cx| {
                    while let Some(update) = updates.next().await {
                        if matches!(update, Update::Error(_)) {
                            this.update(cx, |this, cx| this.run_emulated(id, turn, cx)).ok();
                            return;
                        }
                        let last = matches!(update, Update::Final(_));
                        if this.update(cx, |this, cx| this.apply_btw(id, turn, update, cx)).is_err()
                        {
                            return;
                        }
                        if last {
                            break;
                        }
                    }
                    this.update(cx, |this, cx| this.finish_btw(id, turn, cx)).ok();
                })
                .detach();
                cx.notify();
            }
            None => self.run_emulated(id, turn, cx),
        }
    }

    fn run_emulated(&mut self, id: i64, turn: usize, cx: &mut Context<Self>) {
        let Some(ix) = self.chats.iter().position(|c| c.id == id) else { return };
        let chat = &self.chats[ix];
        let key = btw_key(id);
        let context = aether_agent::handoff::make_side_chat_context(
            chat.id,
            &handoff_history(&chat.messages, chat.id),
        );
        let mut hist = vec![(Role::User, context.clone())];
        if let Some(thread) = self.btw.get(&id) {
            for prior in thread.turns.iter().take(turn) {
                hist.push((Role::User, prior.question.clone()));
                if !prior.answer.is_empty() {
                    hist.push((Role::Assistant, prior.answer.clone()));
                }
            }
        }
        let (settings, mut session) = self.cli_request(ix);
        session.key = key.clone();
        session.access = AccessMode::ReadOnly;
        session.resume.clear();
        session.fork = false;
        session.lead = false;
        session.ephemeral = true;
        if chat.provider == Provider::OpenRouter
            && let Err(e) = self.agent.fork_session(&key, &hist)
        {
            if let Some(thread) = self.btw.get_mut(&id)
                && let Some(t) = thread.turns.get_mut(turn)
            {
                t.error = Some(e.to_string());
                t.running = false;
                thread.running = false;
            }
            return cx.notify();
        }
        let question = self
            .btw
            .get(&id)
            .and_then(|t| t.turns.get(turn))
            .map(|t| t.question.clone())
            .unwrap_or_default();
        let api_key = self.api_key.clone().unwrap_or_default();
        let Reply { mut updates, cancel } = self.agent.send(
            &key,
            if chat.provider == Provider::OpenRouter {
                side_prompt(&question)
            } else {
                aether_agent::handoff::with_inline_context(
                    &context,
                    &btw_prompt(&self.btw[&id].turns[..turn], &question),
                )
            },
            Vec::new(),
            &settings,
            &api_key,
            Vec::new(),
            &[],
            &session,
        );
        match self.btw.get_mut(&id) {
            Some(thread) => thread.cancel = Some(cancel),
            None => return cancel.cancel(),
        }
        cx.spawn(async move |this, cx| {
            while let Some(update) = updates.next().await {
                if this.update(cx, |this, cx| this.apply_btw(id, turn, update, cx)).is_err() {
                    return;
                }
            }
            this.update(cx, |this, cx| this.finish_btw(id, turn, cx)).ok();
        })
        .detach();
        cx.notify();
    }

    fn apply_btw(&mut self, id: i64, turn: usize, update: Update, cx: &mut Context<Self>) {
        let Some(t) = self.btw.get_mut(&id).and_then(|thread| thread.turns.get_mut(turn)) else {
            return;
        };
        match update {
            Update::Delta(text) => t.answer.push_str(&text),
            Update::Final(text) if !text.is_empty() => t.answer = text,
            Update::Error(error) => t.error = Some(error),
            Update::Ask { id: ask, .. } => {
                self.agent.answer(ask, Answer::Deny);
                return;
            }
            _ => return,
        }
        cx.notify();
    }

    fn finish_btw(&mut self, id: i64, turn: usize, cx: &mut Context<Self>) {
        if let Some(thread) = self.btw.get_mut(&id) {
            if let Some(t) = thread.turns.get_mut(turn) {
                t.running = false;
            }
            thread.running = thread.turns.iter().any(|t| t.running);
            if !thread.running {
                thread.cancel = None;
            }
        }
        let key = btw_key(id);
        self.agent.close_session(&key);
        self.agent.delete_session(&key);
        cx.notify();
    }

    fn drop_thread(&self, id: i64, thread: BtwThread) {
        if let Some(cancel) = thread.cancel {
            cancel.cancel();
        }
        let key = btw_key(id);
        self.agent.close_session(&key);
        self.agent.delete_session(&key);
    }

    /// Drops the chat's BTW thread and its side session.
    pub(crate) fn drop_btw(&mut self, id: i64) {
        if let Some(thread) = self.btw.remove(&id) {
            self.drop_thread(id, thread);
        }
    }

    /// Sends the BTW panel's input as the next follow-up.
    pub(in crate::app) fn submit_btw(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.chats[self.active].id;
        if self.btw.get(&id).is_some_and(|t| t.running) {
            return;
        }
        let question = self.btw_input.read(cx).value().trim().to_string();
        if question.is_empty() {
            return;
        }
        self.btw_input.update(cx, |s, cx| s.clean(window, cx));
        self.ask_btw(self.active, question, cx);
        self.btw_input.update(cx, |s, cx| s.focus(window, cx));
    }

    /// The BTW panel: thread turns above a follow-up input.
    pub(crate) fn render_btw_panel(&self, controls: bool, cx: &mut Context<Self>) -> AnyElement {
        let id = self.chats[self.active].id;
        let thread = self.btw.get(&id);
        let turns = thread.map_or(&[][..], |t| t.turns.as_slice());
        let running = thread.is_some_and(|t| t.running);
        let body = v_flex().w_full().min_w_0().gap_1().children(turns.iter().enumerate().map(
            |(ix, turn)| {
                let answer = v_flex()
                    .min_w_0()
                    .gap_1()
                    .when(!turn.answer.is_empty(), |d| {
                        d.child(
                            aether_ui::Markdown::new(
                                format!("btw-{id}-{ix}"),
                                SharedString::from(turn.answer.clone()),
                            )
                            .live(turn.running),
                        )
                    })
                    .when_some(empty_note(turn), |d, note| {
                        d.child(div().text_color(cx.theme().muted_foreground).child(note))
                    })
                    .when_some(turn.error.clone(), |d, error| {
                        d.child(div().text_color(aether_ui::theme::danger_text(cx)).child(error))
                    });
                v_flex().w_full().min_w_0().gap_1().children([
                    aether_ui::UserMessage::new(
                        format!("btw-user-{id}-{ix}"),
                        turn.question.clone(),
                    )
                    .into_any_element(),
                    answer.into_any_element(),
                ])
            },
        ));
        let empty = self.btw_input.read(cx).value().trim().is_empty();
        let input = aether_ui::Composer::new(&self.btw_input).full_width(true).action(
            aether_ui::SendButton::new()
                .disabled(empty || running)
                .on_send(cx.listener(|this, _, window, cx| this.submit_btw(window, cx))),
        );
        v_flex()
            .flex_1()
            .min_h_0()
            .h_full()
            .when(self.settings.chat_layout == aether_core::ChatLayout::Tabbed, |panel| {
                let bar = h_flex()
                    .id("btw-title-bar")
                    .debug_selector(|| "btw-title-bar".into())
                    .flex_none()
                    .h(px(38.))
                    .px_2()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .when(!controls, |bar| bar.border_b_1().border_color(cx.theme().sidebar_border))
                    .text_sm()
                    .child(
                        aether_ui::button("btw-menu")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::MessageCircle)
                            .label("By the way")
                            .dropdown_caret(true)
                            .accessibility_label("By the way options")
                            .tooltip("Thread options")
                            .dropdown_menu({
                                let app = cx.entity();
                                move |menu, _, _| {
                                    let app = app.clone();
                                    menu.item(
                                        PopupMenuItem::new("Clear thread")
                                            .icon(Lucide::Trash)
                                            .on_click(move |_, _, cx| {
                                                app.update(cx, |this, cx| {
                                                    this.drop_btw(id);
                                                    cx.notify();
                                                });
                                            }),
                                    )
                                }
                            }),
                    );
                panel.child(if controls {
                    self.window_bar(bar.into_any_element(), 38., false, true, cx)
                } else {
                    bar.into_any_element()
                })
            })
            .child(
                div()
                    .id("btw-scroll")
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_3()
                    .py_2()
                    .child(body),
            )
            .child(
                div()
                    .id("btw-composer")
                    .debug_selector(|| "btw-composer".into())
                    .w_full()
                    .flex_none()
                    .child(input),
            )
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/chat/side.rs"]
mod tests;
