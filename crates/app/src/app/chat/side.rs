use aether_agent::Reply;
use aether_core::{AccessMode, Answer, Provider, Role, Update};
use aether_ui::theme::Corners;
use futures::StreamExt;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::{Context, SharedString, div, prelude::*, px};

use super::history;
use crate::app::ChatApp;

const MAX_CARDS: usize = 3;
const HISTORY_CAP: usize = 40;

pub(crate) struct SideCard {
    pub n: u64,
    pub question: String,
    pub answer: String,
    pub error: Option<String>,
    pub running: bool,
    cancel: Option<aether_agent::Cancel>,
}

impl SideCard {
    pub fn new(n: u64, question: String) -> Self {
        Self { n, question, answer: String::new(), error: None, running: false, cancel: None }
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

fn side_history(mut history: Vec<(Role, String)>) -> Vec<(Role, String)> {
    history.drain(..history.len().saturating_sub(HISTORY_CAP));
    history
}

/// Adds `card`; the oldest beyond the cap is returned.
fn push_card(cards: &mut Vec<SideCard>, card: SideCard) -> Option<SideCard> {
    cards.push(card);
    (cards.len() > MAX_CARDS).then(|| cards.remove(0))
}

fn empty_note(card: &SideCard) -> Option<&'static str> {
    if !card.answer.is_empty() || card.error.is_some() {
        None
    } else if card.running {
        Some("Thinking…")
    } else {
        Some("No answer")
    }
}

fn side_key(chat: i64, n: u64) -> String {
    format!("btw:{chat}:{n}")
}

impl ChatApp {
    pub(in crate::app) fn ask_side(&mut self, ix: usize, question: String, cx: &mut Context<Self>) {
        let chat = &self.chats[ix];
        let id = chat.id;
        self.side_runs += 1;
        let n = self.side_runs;
        let key = side_key(id, n);
        let mut card = SideCard::new(n, question.clone());
        if chat.provider == Provider::OpenRouter && self.api_key.is_none() {
            card.error = Some("No OpenRouter API key. Add one in Settings.".into());
            self.add_side(id, card);
            return cx.notify();
        }
        let hist = side_history(history(&chat.messages));
        let (settings, mut session) = self.cli_request(ix);
        session.key = key.clone();
        session.access = AccessMode::ReadOnly;
        session.resume.clear();
        session.lead = false;
        if chat.provider == Provider::OpenRouter
            && let Err(e) = self.agent.fork_session(&key, &hist)
        {
            card.error = Some(e.to_string());
            self.add_side(id, card);
            return cx.notify();
        }
        let api_key = self.api_key.clone().unwrap_or_default();
        let Reply { mut updates, cancel } = self.agent.send(
            &key,
            side_prompt(&question),
            Vec::new(),
            &settings,
            &api_key,
            Vec::new(),
            &hist,
            &session,
        );
        card.cancel = Some(cancel);
        card.running = true;
        self.add_side(id, card);
        cx.spawn(async move |this, cx| {
            while let Some(update) = updates.next().await {
                if this.update(cx, |this, cx| this.apply_side(id, n, update, cx)).is_err() {
                    return;
                }
            }
            this.update(cx, |this, cx| this.finish_side(id, n, cx)).ok();
        })
        .detach();
        cx.notify();
    }

    fn add_side(&mut self, id: i64, card: SideCard) {
        if let Some(old) = push_card(self.side.entry(id).or_default(), card) {
            self.drop_side(id, old);
        }
    }

    fn drop_side(&self, id: i64, card: SideCard) {
        if let Some(cancel) = card.cancel {
            cancel.cancel();
        }
        let key = side_key(id, card.n);
        self.agent.close_session(&key);
        self.agent.delete_session(&key);
    }

    pub(in crate::app) fn drop_side_cards(&mut self, id: i64) {
        for card in self.side.remove(&id).unwrap_or_default() {
            self.drop_side(id, card);
        }
    }

    fn side_card(&mut self, id: i64, n: u64) -> Option<&mut SideCard> {
        self.side.get_mut(&id)?.iter_mut().find(|c| c.n == n)
    }

    fn apply_side(&mut self, id: i64, n: u64, update: Update, cx: &mut Context<Self>) {
        let Some(card) = self.side_card(id, n) else { return };
        match update {
            Update::Delta(text) => card.answer.push_str(&text),
            Update::Final(text) if !text.is_empty() => card.answer = text,
            Update::Error(error) => card.error = Some(error),
            Update::Ask { id: ask, .. } => {
                self.agent.answer(ask, Answer::Deny);
                return;
            }
            _ => return,
        }
        cx.notify();
    }

    fn finish_side(&mut self, id: i64, n: u64, cx: &mut Context<Self>) {
        if let Some(card) = self.side_card(id, n) {
            card.running = false;
            card.cancel = None;
        }
        let key = side_key(id, n);
        self.agent.close_session(&key);
        self.agent.delete_session(&key);
        cx.notify();
    }

    pub(in crate::app) fn close_side(&mut self, id: i64, n: u64, cx: &mut Context<Self>) {
        let Some(cards) = self.side.get_mut(&id) else { return };
        if let Some(at) = cards.iter().position(|c| c.n == n) {
            let card = cards.remove(at);
            self.drop_side(id, card);
        }
        cx.notify();
    }

    pub(in crate::app) fn side_cards(&self, id: i64, cx: &mut Context<Self>) -> Vec<gpui_kit::Div> {
        let cards = self.side.get(&id).map_or(&[][..], Vec::as_slice);
        cards
            .iter()
            .map(|card| {
                let n = card.n;
                let question: String = card.question.lines().next().unwrap_or_default().into();
                let body = v_flex()
                    .gap_1()
                    .when(!card.answer.is_empty(), |d| {
                        d.child(
                            aether_ui::Markdown::new(
                                format!("btw-{id}-{n}"),
                                SharedString::from(card.answer.clone()),
                            )
                            .live(card.running),
                        )
                    })
                    .when_some(empty_note(card), |d, note| {
                        d.child(div().text_color(cx.theme().muted_foreground).child(note))
                    })
                    .when_some(card.error.clone(), |d, error| {
                        d.child(div().text_color(aether_ui::theme::danger_text(cx)).child(error))
                    });
                v_flex()
                    .gap_1()
                    .p_2()
                    .corner(px(9.))
                    .border_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .child(
                        h_flex()
                            .justify_between()
                            .gap_2()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(format!("By the way · {question}")),
                            )
                            .child(
                                aether_ui::button(SharedString::from(format!("btw-close-{n}")))
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::X)
                                    .accessibility_label("Close side answer")
                                    .tooltip(if card.running { "Stop and close" } else { "Close" })
                                    .on_click(self.session_listener(cx, move |this, _, _, cx| {
                                        this.close_side(id, n, cx)
                                    })),
                            ),
                    )
                    .child(body)
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/chat/side.rs"]
mod tests;
