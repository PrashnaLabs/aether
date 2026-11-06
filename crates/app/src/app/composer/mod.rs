use crate::actions::{
    COMPOSER_CONTEXT, COMPOSER_HISTORY_CONTEXT, ComposerHistoryNext, ComposerHistoryPrev,
    EditDraft, GENERATING_CONTEXT, SUGGESTION_CONTEXT, StopGenerating, SuggestionAccept,
    SuggestionDismiss, SuggestionNext, SuggestionPrev, ToggleVoice,
};
use crate::agents_view::externally_locked;
use aether_agent::usage::default_plan_usage;
use aether_core::{FooterChips, Role, is_image};
use aether_ui::{
    AgentSelector, AttachmentView, Composer, ComposerMode, ContextRing, DeadAgentBar, PlanQuota,
    PlanTray, QueueChip, SendButton, attachment_strip, composer_mode, control_chip, fold_bar,
    fold_bar_text, format_tokens, icon_chip, overlay_strong, plan_tray_items, provider_icon,
};
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, IconName, h_flex, v_flex};
use gpui_kit::{App, Context, Focusable, SharedString, Window, div, prelude::*, px};

use self::attach::{PasteKind, paste_kind, shell_command};
use self::chips::resting_chip;
use super::format::{format_cost, format_full, message_cost};
use super::{ChatApp, WorktreeJob, chat_cwd};

pub(in crate::app) mod attach;
pub(in crate::app) mod chips;
mod header;
mod message;
pub(in crate::app) mod quote;
mod usage;
pub(in crate::app) mod voice;

fn composer_folds(has_messages: bool, user_scrolled_up: bool, input_focused: bool) -> bool {
    has_messages && user_scrolled_up && !input_focused
}

impl ChatApp {
    pub(crate) fn expand_composer(&mut self, cx: &mut Context<Self>) {
        if self.folded {
            self.folded = false;
            cx.notify();
        }
    }

    pub(crate) fn unfocus_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.composer.focus_handle(cx).is_focused(window) {
            self.focus.focus(window, cx);
        }
    }

    pub(super) fn sync_fold(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let has_messages = self.chats.get(self.active).is_some_and(|c| !c.messages.is_empty());
        let folded = composer_folds(
            has_messages,
            self.scroller.read(cx).user_scrolled_up(),
            self.composer.focus_handle(cx).is_focused(window),
        );
        if folded == self.folded {
            return;
        }
        self.folded = folded;
        if !folded && self.focus.is_focused(window) {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    pub(in crate::app) fn edit_queued(
        &mut self,
        i: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.composer.read(cx).value().trim().is_empty() {
            return;
        }
        let chat = &mut self.chats[self.active];
        if chat.queue.get(i).is_none_or(|q| q.delivering) {
            return;
        }
        let queued = chat.queue.remove(i);
        chat.pending.extend(queued.attachments);
        self.composer.update(cx, |input, cx| input.set_value(queued.text, window, cx));
        cx.notify();
    }

    fn unfold(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.scroller.update(cx, |s, cx| s.scroll_to_end(cx));
        self.focus_composer(window, cx);
    }

    pub(crate) fn render_composer(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let menu = self.render_suggestions(cx);
        let composer = self.composer_card(cx);
        let has_history = self.chats.get(self.active).is_some_and(|c| {
            c.messages.iter().any(|m| m.role == Role::User && !m.text.trim().is_empty())
        });
        let history_context = menu.is_none() && has_history;
        let generating = menu.is_none() && self.chats[self.active].busy();
        div()
            .w_full()
            .key_context(COMPOSER_CONTEXT)
            .on_action(self.session_listener(cx, |this, _: &EditDraft, window, cx| {
                this.open_draft_editor(window, cx)
            }))
            .on_action(self.session_listener(cx, |this, _: &ToggleVoice, window, cx| {
                this.toggle_voice(window, cx)
            }))
            .when(generating, |wrap| wrap.key_context(GENERATING_CONTEXT))
            .on_action(self.session_listener(cx, |this, _: &SuggestionPrev, _, cx| {
                this.move_suggestion(-1, cx)
            }))
            .on_action(self.session_listener(cx, |this, _: &SuggestionNext, _, cx| {
                this.move_suggestion(1, cx)
            }))
            .on_action(self.session_listener(cx, |this, _: &SuggestionAccept, window, cx| {
                this.accept_suggestion(None, window, cx)
            }))
            .on_action(self.session_listener(cx, |this, _: &SuggestionDismiss, _, cx| {
                this.dismiss_suggestions(cx)
            }))
            .on_action(self.session_listener(cx, |this, _: &ComposerHistoryPrev, window, cx| {
                this.navigate_history(-1, window, cx);
            }))
            .on_action(self.session_listener(cx, |this, _: &ComposerHistoryNext, window, cx| {
                this.navigate_history(1, window, cx);
            }))
            .child(
                div()
                    .w_full()
                    .when(menu.is_some(), |wrap| wrap.key_context(SUGGESTION_CONTEXT))
                    .when(history_context, |wrap| wrap.key_context(COMPOSER_HISTORY_CONTEXT))
                    .child(composer.when_some(menu, |composer, menu| composer.popup(menu))),
            )
    }

    /// Context ring with usage telemetry, plus the used and total token counts.
    pub(super) fn context_ring(&self) -> Option<(ContextRing, u64, u64)> {
        let chat = &self.chats[self.active];
        let chat_model = chat.model(&self.settings);
        let used = chat.context_used();
        let chat_tokens: u64 =
            chat.messages.iter().map(|m| m.prompt_tokens + m.completion_tokens).sum();
        let chat_cost: f64 =
            chat.messages.iter().filter_map(|m| message_cost(m, &self.model_prices)).sum();
        let chat_usage = (chat_tokens > 0).then(|| {
            let mut line = format!("{} tokens this chat", format_tokens(chat_tokens));
            if chat_cost > 0. {
                line.push_str(&format!(" · ≈{}", format_cost(chat_cost)));
            }
            line
        });
        let total = self.context_limit(chat);
        let mut quotas = Vec::new();
        let plan = self
            .provider_plan_usages
            .get(&chat.provider)
            .cloned()
            .unwrap_or_else(|| default_plan_usage(chat.provider.label()));
        for limit in plan.limits {
            quotas.push(
                PlanQuota::new(limit.label, 100.0 - limit.percent_left)
                    .reset_label(Some(SharedString::from(limit.reset_label))),
            );
        }

        let mut breakdown: Vec<(SharedString, SharedString)> = Vec::new();
        if let Some(latest) = chat
            .messages
            .iter()
            .rev()
            .find(|m| m.role == Role::Assistant && (m.prompt_tokens + m.completion_tokens > 0))
        {
            let uncached = latest.prompt_tokens.saturating_sub(latest.cached_tokens);
            if uncached > 0 {
                breakdown
                    .push(("Input tokens".into(), format!("{} tok", format_full(uncached)).into()));
            }
            if latest.cached_tokens > 0 {
                breakdown.push((
                    "Cache read".into(),
                    format!("{} tok", format_full(latest.cached_tokens)).into(),
                ));
            }
            if latest.completion_tokens > 0 {
                breakdown.push((
                    "Output tokens".into(),
                    format!("{} tok", format_full(latest.completion_tokens)).into(),
                ));
            }
        }

        (total > 0).then(|| {
            let ring = ContextRing::new(used, total)
                .model(chat_model)
                .cost(chat_usage)
                .quotas(quotas)
                .breakdown(breakdown);
            (ring, used, total)
        })
    }

    fn composer_card(&self, cx: &mut Context<Self>) -> Composer {
        let quote = self.quote_chip(cx);
        let empty = self.composer.read(cx).value().trim().is_empty()
            && self.chats[self.active].pending.is_empty()
            && quote.is_none();
        let shell = self.shell_mode(cx);
        let no_command = shell && shell_command(self.composer.read(cx).value().trim()).is_none();
        let busy = self.chats[self.active].busy();
        let chat = &self.chats[self.active];
        let lock = externally_locked(chat.external_pid, &chat.cli_session, &self.external);
        let locked = lock.is_some();
        let banner = lock.map(|a| {
            let origin = if a.origin.is_empty() { "another terminal" } else { a.origin.as_str() };
            let outside = a.clone();
            h_flex()
                .px_2()
                .gap_2()
                .justify_between()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("Running in {origin}. Stop it or wait for it to finish to continue here."))
                .child(
                    div().debug_selector(|| "composer-stop-external".into()).child(
                        aether_ui::button("composer-stop-external")
                            .outline()
                            .xsmall()
                            .label("Stop to continue here")
                            .on_click(self.session_listener(cx, move |this, _, window, cx| {
                                this.stop_external(outside.clone(), window, cx)
                            })),
                    ),
                )
        });
        let editing = self.editing.is_some().then(|| {
            h_flex()
                .px_2()
                .justify_between()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("Editing your message. Sending replaces it and everything after.")
                .child(aether_ui::button("cancel-edit").ghost().xsmall().label("Cancel").on_click(
                    self.session_listener(cx, |this, _, window, cx| this.cancel_edit(window, cx)),
                ))
        });
        let voice = self.voice_banner(cx);
        let recording = self.recording_voice();
        let voice_busy = self.voice_active();
        let cockpit = self.usage_cockpit(cx);
        let note = self.worktree_note(cx);
        let setup = self.setup_note(cx);
        let blocked = self.worktree_blocks_send(self.active);
        let attaching = self.attaching(chat.id);
        let creating = matches!(chat.worktree_job, Some(WorktreeJob::Creating(..)));
        let mode = composer_mode(!chat.messages.is_empty(), empty, self.folded);
        let resting = mode == ComposerMode::Resting;
        let live = chat
            .cancel
            .is_some()
            .then(|| chat.messages.len().checked_sub(1))
            .flatten()
            .map(|msg| self.live_row(msg, &self.session_handle(cx)));
        let switching = self.switching_folder(&chat_cwd(chat, &self.settings.workspace_root));
        let selector = AgentSelector::new(&self.selector)
            .locked(chat.provider_locked())
            .resting(resting)
            .disabled(busy || chat.worktree_job.is_some() || chat.setup.is_some() || switching);
        let footer = FooterChips::for_provider(chat.provider);
        let (lead_chip, lead, chat_id) =
            (crate::app::orchestrate::shows_lead_chip(chat), chat.lead, chat.id);
        let lead_tip = if lead {
            "Lead: orchestration tools on".to_string()
        } else if chat.messages.is_empty() {
            "Lead: orchestration tools join on first send".to_string()
        } else {
            "Lead: turning on restarts the driver with orchestration tools".to_string()
        };
        let app = self.session_handle(cx).downgrade();
        let provider = chat.provider.label();
        let chat_key = chat.id.to_string();
        let plan = plan_tray_items(&chat.messages)
            .map(|items| PlanTray::new(chat_key.clone(), items.to_vec()));
        let dead = chat.dead_bar(locked).then(|| {
            DeadAgentBar::new(
                provider,
                Box::new(self.session_listener(cx, |this, _, window, cx| {
                    let ix = this.active;
                    this.restart_agent(ix, cx);
                    this.focus_composer(window, cx);
                })),
            )
        });
        let sends = busy && !chat.stopped && blocked.is_none();
        let edit_blocked = !self.composer.read(cx).value().trim().is_empty();
        let rows: Vec<_> = chat
            .queue
            .iter()
            .enumerate()
            .map(|(i, queued)| {
                let (send, edit, remove) = (app.clone(), app.clone(), app.clone());
                QueueChip::new(
                    i,
                    queued.text.clone(),
                    queued.attachments.len(),
                    queued.delivering,
                    sends,
                    provider,
                    edit_blocked,
                    move |i, window, cx| {
                        send.update(cx, |this, cx| {
                            this.steer_queued(i, window, cx);
                            this.focus_composer(window, cx);
                        })
                        .ok();
                    },
                    move |i, window, cx| {
                        edit.update(cx, |this, cx| {
                            this.edit_queued(i, window, cx);
                            this.focus_composer(window, cx);
                        })
                        .ok();
                    },
                    move |i, window, cx| {
                        remove
                            .update(cx, |this, cx| {
                                this.remove_queued(i, cx);
                                this.focus_composer(window, cx);
                            })
                            .ok();
                    },
                )
                .on_open({
                    let (open, chat_id, text) = (app.clone(), chat.id, queued.text.clone());
                    move |_, _, cx| {
                        open.update(cx, |this, cx| {
                            this.background_hidden.remove(&chat_id);
                            this.inspect_queued = Some((chat_id, text.clone()));
                            cx.notify();
                        })
                        .ok();
                    }
                })
            })
            .collect();
        let attention = chat.messages.last().is_some_and(|m| m.pending_request().is_some());
        let strip = (!chat.pending.is_empty()).then(|| {
            let views: Vec<AttachmentView> = chat
                .pending
                .iter()
                .map(|a| AttachmentView {
                    name: a.name.clone().into(),
                    path: a.path.clone(),
                    image: is_image(&a.mime),
                    size: a.size,
                    missing: false,
                })
                .collect();
            let app = app.clone();
            let opener = app.clone();
            let open = Rc::new(move |path: &std::path::Path, window: &mut Window, cx: &mut App| {
                let opened = opener
                    .update(cx, |app, cx| app.preview_image(path, window, cx))
                    .unwrap_or(false);
                if !opened {
                    cx.open_with_system(path);
                }
            });
            let remove: Rc<dyn Fn(usize, &mut Window, &mut App)> = Rc::new(move |i, _, cx| {
                app.update(cx, |this, cx| this.remove_pending(i, cx)).ok();
            });
            move |cx: &App| {
                let remove = (!creating).then(|| remove.clone());
                attachment_strip(views.clone(), px(56.), Some(open.clone()), remove, false, cx)
            }
        });
        let attach_error = self.attach_error.clone();
        let divider = div().w(px(1.)).h(px(14.)).mx(px(2.)).flex_none().bg(overlay_strong(cx));
        let access = self.access_control(resting, cx);
        // Keep both bodies available while the composer animates in either direction.
        let bar = {
            let draft = self.composer.read(cx).value();
            let agent = self.is_agent_chat(chat).then(|| chat.provider.short_label()).flatten();
            let (text, is_draft) = fold_bar_text(&draft, agent);
            fold_bar(
                if shell {
                    Icon::new(Lucide::SquareTerminal)
                } else {
                    provider_icon(chat.provider)
                },
                text.into(),
                is_draft,
                self.selector.read(cx).model_summary(),
                Box::new(self.session_listener(cx, |this, _, window, cx| this.unfold(window, cx))),
                Box::new(self.session_listener(cx, |this, _, _, cx| {
                    this.scroller.update(cx, |s, cx| s.scroll_to_end(cx))
                })),
                cx,
            )
        };
        Composer::new(&self.composer)
            .shell(shell)
            .recording(recording)
            .mode(mode)
            .bar(bar)
            .when_some(live, |composer, live| composer.status(live))
            .attention(attention)
            .tray(self.tray(cx))
            .attach_scope(chat.id as u64)
            .when_some(strip, |composer, strip| composer.attachments(strip))
            .on_paste({
                let app = app.clone();
                move |item, _, cx| match paste_kind(item) {
                    PasteKind::Text => false,
                    PasteKind::Image(image) => {
                        app.update(cx, |this, cx| this.add_pasted_image(image, cx)).ok();
                        true
                    }
                    PasteKind::Files(paths) => {
                        app.update(cx, |this, cx| this.add_attachment_paths(paths, cx)).ok();
                        true
                    }
                }
            })
            .on_drop_paths({
                let app = app.clone();
                move |paths, _, cx| {
                    app.update(cx, |this, cx| this.add_attachment_paths(paths, cx)).ok();
                }
            })
            .disabled(switching)
            .leading(selector)
            .leading(divider)
            .when_some(access, |composer, access| composer.leading(access))
            .when(footer.toggles, |composer| {
                composer
                    .leading(
                        resting_chip(
                            control_chip("web-search", self.settings.web_search, cx),
                            resting,
                        )
                        .icon(IconName::Globe)
                        .label("Search")
                        .on_click(self.session_listener(
                            cx,
                            |this, _, _, cx| {
                                this.update_settings(cx, |s| s.web_search = !s.web_search);
                            },
                        )),
                    )
                    .leading(
                        resting_chip(
                            control_chip("plan-mode", self.settings.plan_mode, cx),
                            resting,
                        )
                        .icon(IconName::Check)
                        .label("Plan")
                        .on_click(self.session_listener(
                            cx,
                            |this, _, _, cx| {
                                this.update_settings(cx, |s| s.plan_mode = !s.plan_mode);
                            },
                        )),
                    )
            })
            .when(lead_chip, |composer| {
                composer.leading(
                    resting_chip(control_chip("lead-chip", lead, cx), resting)
                        .icon(gpui_kit::assets::IconName::Users)
                        .label("Lead")
                        .tooltip(lead_tip)
                        .debug_selector(|| "lead-chip".into())
                        .on_click(self.session_listener(cx, move |this, _, window, cx| {
                            if let Some(ix) = this.chat_ix(chat_id) {
                                this.toggle_lead(ix, window, cx);
                            }
                        })),
                )
            })
            .when(
                plan.is_some()
                    || dead.is_some()
                    || !rows.is_empty()
                    || quote.is_some()
                    || editing.is_some()
                    || voice.is_some()
                    || locked
                    || note.is_some()
                    || setup.is_some()
                    || attach_error.is_some()
                    || cockpit.is_some(),
                |composer| {
                    composer.header(
                        v_flex()
                            .gap_2()
                            .children(plan)
                            .children(dead)
                            .children(rows)
                            .children(quote)
                            .children(cockpit)
                            .children(banner)
                            .children(note)
                            .children(setup)
                            .children(editing)
                            .children(voice)
                            .children(attach_error.map(|error| {
                                h_flex()
                                    .px_2()
                                    .text_xs()
                                    .text_color(aether_ui::theme::danger_text(cx))
                                    .child(error)
                            })),
                    )
                },
            )
            .when(self.settings.voice_enabled, |composer| {
                composer.trailing(
                    icon_chip("voice", recording, cx)
                        .icon(if recording { Lucide::Square } else { Lucide::Mic })
                        .tooltip_with_action(
                            if recording { "Stop dictating" } else { "Dictate" },
                            &ToggleVoice,
                            Some(COMPOSER_CONTEXT),
                        )
                        .disabled(switching)
                        .on_click(self.session_listener(cx, |this, _, window, cx| {
                            this.toggle_voice(window, cx)
                        })),
                )
            })
            .trailing(
                icon_chip("attach", false, cx)
                    .icon(gpui_kit::assets::IconName::Paperclip)
                    .tooltip("Attach files")
                    .disabled(switching)
                    .on_click(self.session_listener(cx, |this, _, window, cx| {
                        this.pick_attachments(window, cx)
                    })),
            )
            .action(
                SendButton::new()
                    .shell(shell)
                    .streaming(busy && empty)
                    .loading(creating)
                    .disabled(
                        empty
                            || no_command
                            || locked
                            || attaching
                            || voice_busy
                            || blocked.is_some(),
                    )
                    .disabled_reason(if locked {
                        Some("Running in another app (read-only)")
                    } else if attaching {
                        Some("Attaching…")
                    } else if voice_busy {
                        Some("Finish dictating first")
                    } else {
                        blocked
                    })
                    .on_send(self.session_listener(cx, |this, _, window, cx| this.send(window, cx)))
                    .on_stop(self.session_listener(cx, |this, _, window, cx| this.stop(window, cx)))
                    .stop_action(StopGenerating),
            )
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/composer/mod.rs"]
mod tests;
