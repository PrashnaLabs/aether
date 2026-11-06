use std::rc::Rc;

use aether_agent::usage::default_plan_usage;
use aether_ui::{UsageCockpit, UsageLimitRow, format_tokens};
use gpui_kit::Context;

use crate::app::ChatApp;

impl ChatApp {
    /// Toggles the native in-composer Usage Limits cockpit card.
    pub(crate) fn show_usage(&mut self, cx: &mut Context<Self>) {
        self.show_usage_limits = !self.show_usage_limits;
        if self.show_usage_limits {
            let provider = self.chats[self.active].provider;
            self.refresh_provider_usage(provider, cx);
        }
        cx.notify();
    }

    pub(super) fn usage_cockpit(&self, cx: &mut Context<Self>) -> Option<UsageCockpit> {
        if !self.show_usage_limits {
            return None;
        }
        let chat = &self.chats[self.active];
        let provider = chat.provider;
        let used = chat.context_used();
        let total = self.context_limit(chat);

        let chat_tokens: u64 =
            chat.messages.iter().map(|m| m.prompt_tokens + m.completion_tokens).sum();

        let plan = self
            .provider_plan_usages
            .get(&provider)
            .cloned()
            .unwrap_or_else(|| default_plan_usage(provider.label()));

        let rows: Vec<UsageLimitRow> = plan
            .limits
            .into_iter()
            .map(|l| {
                UsageLimitRow::new(l.label, l.percent_left, l.reset_label)
                    .threshold(l.threshold)
                    .trend_down(l.trend_down)
            })
            .collect();

        let footer_text = plan.footer_text.or_else(|| {
            (chat_tokens > 0).then(|| {
                format!(
                    "{} tokens this chat · Context {} / {}",
                    format_tokens(chat_tokens),
                    format_tokens(used),
                    format_tokens(total)
                )
            })
        });

        Some(
            UsageCockpit::new(provider.label(), plan.plan_name)
                .rows(rows)
                .footer_text(footer_text)
                .footer_action(plan.footer_action)
                .on_close(Some(Rc::new(self.session_listener(cx, |this, _, _, cx| {
                    this.show_usage_limits = false;
                    cx.notify();
                }))))
                .on_action(Some(Rc::new(self.session_listener(cx, |this, _, _, cx| {
                    this.toggle_usage_page(cx);
                })))),
        )
    }
}
