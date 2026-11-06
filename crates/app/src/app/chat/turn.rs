use std::collections::HashMap;

use aether_core::Message;
use aether_driver::CliModel;
use aether_ui::{FoldLine, FootFacts, TurnFooter, TurnOutcome, tool_counts};
use gpui_kit::{AnyElement, div, prelude::*};

use crate::app::ChatApp;
use crate::app::format::{cost_text, format_time, message_cost};
use crate::app::sidebar::model_label;

fn foot_facts(
    message: &Message,
    models: &[CliModel],
    prices: &HashMap<String, (f64, f64)>,
    stopped: bool,
) -> FootFacts {
    let (tools, subagents) = tool_counts(&message.steps);
    let model = message.served_model.clone().or(message.model.clone());
    let cost = match message.cost_micros {
        0 => message_cost(message, prices).and_then(|c| cost_text((c * 1e6) as u64, true)),
        micros => cost_text(micros, false),
    };
    FootFacts {
        outcome: TurnOutcome::of(message, stopped),
        duration_ms: message.elapsed_ms,
        tools,
        subagents,
        model: model.map(|m| model_label(models, &m)),
        tokens_in: message.prompt_tokens,
        tokens_out: message.completion_tokens,
        cached: message.cached_tokens,
        cost,
        at: message.ended_at_ms.and_then(format_time),
    }
}

impl ChatApp {
    fn stopped(&self, msg: usize) -> bool {
        let chat = &self.chats[self.active];
        chat.stopped && msg + 1 == chat.messages.len()
    }

    pub(in crate::app) fn fold_row(
        &self,
        msg: usize,
        view: &crate::app::SessionHandle,
    ) -> AnyElement {
        let chat = &self.chats[self.active];
        let Some(message) = chat.messages.get(msg) else {
            return div().into_any_element();
        };
        let open = chat.open_steps.get(&msg).copied().unwrap_or(false);
        let view = view.clone();
        FoldLine::new(
            format!("{}-{msg}-fold", chat.session),
            message,
            TurnOutcome::of(message, self.stopped(msg)),
            message.elapsed_ms,
            open,
        )
        .on_toggle(move |open, _, cx| {
            view.update(cx, |this, cx| this.set_steps_open(msg, open, cx));
        })
        .into_any_element()
    }

    pub(in crate::app) fn footer_row(
        &self,
        msg: usize,
        view: &crate::app::SessionHandle,
    ) -> Option<AnyElement> {
        let chat = &self.chats[self.active];
        let message = chat.messages.get(msg)?;
        let last = msg + 1 == chat.messages.len();
        let busy = chat.cancel.is_some();
        let (copy_it, retry_it) = (!message.text.trim().is_empty(), last && !busy);
        let (retry, copy, save_note) = (view.clone(), view.clone(), view.clone());
        let note_text = message.text.clone();
        let models = self.probes.get(&chat.provider).map_or(&[][..], |probe| &probe.models);
        let facts = foot_facts(message, models, &self.model_prices, self.stopped(msg));
        let footer = TurnFooter::new(format!("{}-{msg}-foot", chat.session), &facts)
            .when(copy_it && self.settings.notes_enabled, |footer| {
                footer.on_save_note(move |_, window, cx| {
                    save_note.update(cx, |this, cx| {
                        this.notes_form(crate::notes::Form::Capture, note_text.clone(), window, cx);
                    });
                })
            })
            .when(copy_it, |footer| {
                footer.copied(self.copied == Some((self.active, msg))).on_copy(move |_, _, cx| {
                    copy.update(cx, |this, cx| this.copy_message(msg, cx));
                })
            })
            .when(retry_it, |footer| {
                footer.on_retry(move |_, window, cx| {
                    retry.update(cx, |this, cx| this.retry(window, cx));
                })
            });
        Some(footer.into_any_element())
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/chat/turn.rs"]
mod tests;
