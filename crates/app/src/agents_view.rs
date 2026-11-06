use std::time::Duration;

use aether_agent::{ExternalAgent, ExternalState};
use aether_core::{Message, Provider, Role, Update};
use aether_ui::{SettingsPage, SettingsRow, SettingsSection};
use gpui_kit::Context;
use gpui_kit::component::WindowExt;
use gpui_kit::component::{
    Disableable, IconName, Sizable,
    button::{ButtonVariant, ButtonVariants},
    h_flex,
};
use gpui_kit::prelude::*;

use crate::app::{ChatApp, Page};
use gpui_kit::gpui::{SharedString, Window};

pub(crate) fn poll_interval(page_visible: bool) -> Duration {
    Duration::from_secs(if page_visible { 2 } else { 10 })
}

pub(crate) fn external_state_text(state: &ExternalState, exact: bool) -> String {
    let text = match state {
        ExternalState::Working => "Working".to_string(),
        ExternalState::Waiting(what) => {
            let what = what.trim_end();
            let cut = what
                .len()
                .checked_sub(" needed".len())
                .filter(|&i| what.is_char_boundary(i) && what[i..].eq_ignore_ascii_case(" needed"));
            format!("Needs {}", cut.map_or(what, |i| &what[..i]))
        }
        ExternalState::Idle => "Idle".to_string(),
    };
    if exact { text } else { format!("{text} (estimated)") }
}

/// Why an outside agent can't open read-only, if it can't.
pub(crate) fn open_blocker(a: &ExternalAgent) -> Option<&'static str> {
    if a.session.is_empty() {
        Some("No session to open")
    } else if a.pid.is_none()
        && !(a.provider == Provider::OpenCode
            && a.session.starts_with("ses_")
            && a.session.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
    {
        Some("Busy in another OpenCode client")
    } else {
        None
    }
}

pub(crate) fn transcript_messages(turns: Vec<(Role, String)>) -> Vec<Message> {
    turns
        .into_iter()
        .map(|(role, text)| {
            if role == Role::User {
                return Message::user(text);
            }
            let mut m = Message::assistant();
            m.apply(Update::Final(text));
            m.finish();
            m
        })
        .collect()
}

/// The running outside process holding a chat read-only: the one it was
/// opened from, or any that has the chat's CLI session open.
pub(crate) fn externally_locked<'a>(
    pid: Option<u32>,
    session: &str,
    external: &'a [ExternalAgent],
) -> Option<&'a ExternalAgent> {
    external.iter().find(|a| {
        (a.pid.is_some() && (a.pid == pid || (!session.is_empty() && a.session == session)))
            || (a.pid.is_none()
                && a.provider == Provider::OpenCode
                && !session.is_empty()
                && a.session == session
                && matches!(a.state, ExternalState::Working | ExternalState::Waiting(_)))
    })
}

fn display_title(a: &ExternalAgent) -> String {
    if a.title.is_empty() { "Untitled".to_string() } else { a.title.clone() }
}

impl ChatApp {
    pub(crate) fn open_external(
        &mut self,
        a: ExternalAgent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(why) = open_blocker(&a) {
            let hint = if a.pid.is_none() {
                "Open it in that OpenCode client, or stop it there first."
            } else {
                "Aether hasn't found its session yet. Wait for its first reply, then try again."
            };
            let text = SharedString::from(format!("{why}. {hint}"));
            aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
                alert.title("Can't open this agent here").description(text.clone())
            });
            return;
        }
        if let Some(at) = self.external_chat(&a) {
            return self.select(at, window, cx);
        }
        let turns = self.agent.transcript(a.clone());
        cx.spawn_in(window, async move |this, cx| {
            let messages = transcript_messages(turns.await);
            this.update_in(cx, |this, window, cx| this.add_external_chat(a, messages, window, cx))
                .ok();
        })
        .detach();
    }

    pub(crate) fn reveal_external(&mut self, a: &ExternalAgent, cx: &mut Context<Self>) {
        if !a.cwd.as_os_str().is_empty() {
            cx.reveal_path(&a.cwd);
        }
    }

    pub(crate) fn stop_external(
        &mut self,
        a: ExternalAgent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folder = a
            .cwd
            .file_name()
            .map_or_else(|| a.cwd.display().to_string(), |n| n.to_string_lossy().into_owned());
        let who = format!("“{}” (pid {})", display_title(&a), a.pid.unwrap_or_default());
        let view = cx.entity();
        match a.provider {
            Provider::Claude => aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
                let button = |id: &'static str, label: &'static str, force: Option<bool>| {
                    let (view, a) = (view.clone(), a.clone());
                    aether_ui::button(id).label(label).on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        if let Some(force) = force {
                            view.update(cx, |this, cx| this.run_stop(a.clone(), force, cx));
                        }
                    })
                };
                alert
                    .title(SharedString::from(format!("Interrupt Claude in {folder}?")))
                    .description(SharedString::from(format!(
                        "Sends an interrupt signal (like Ctrl-C) to {who} in {folder}; it may end that session."
                    )))
                    .footer(
                        h_flex()
                            .gap_2()
                            .justify_end()
                            .child(button("stop-cancel", "Cancel", None).ghost())
                            .child(button("stop-quit", "Quit", Some(true)).danger())
                            .child(button("stop-interrupt", "Interrupt", Some(false)).danger()),
                    )
            }),
            Provider::Pi => aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
                let (view, a) = (view.clone(), a.clone());
                alert
                    .confirm()
                    .title(SharedString::from(format!("Quit Pi in {folder}?")))
                    .description(SharedString::from(format!(
                        "Pi has no remote interrupt; this ends {who} (its history is kept)."
                    )))
                    .ok_text("Quit")
                    .ok_variant(ButtonVariant::Danger)
                    .on_ok(move |_, _, cx| {
                        view.update(cx, |this, cx| this.run_stop(a.clone(), true, cx));
                        true
                    })
            }),
            _ => self.run_stop(a, false, cx),
        }
    }

    fn run_stop(&mut self, a: ExternalAgent, force: bool, cx: &mut Context<Self>) {
        let stop = self.agent.stop_external(a, force);
        cx.spawn(async move |this, cx| {
            let result = stop.await;
            this.update(cx, |this, cx| {
                this.agents_status = result.err();
                this.scan_agents(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn render_agents(&self, cx: &mut Context<Self>) -> SettingsPage {
        let mut inside = SettingsSection::new("In Aether");
        let rows = self.in_aether_agents();
        if rows.is_empty() {
            inside = inside.child(SettingsRow::new("No agents running in aether."));
        }
        for (ix, title, provider, detail, state) in rows {
            let id = self.chats[ix].id;
            let stop = state.busy().then(|| {
                aether_ui::button(("agent-stop", id as u64))
                    .ghost()
                    .small()
                    .icon(IconName::Square)
                    .label("Stop")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(ix) = this.chats.iter().position(|c| c.id == id) {
                            this.stop_chat(ix, cx);
                        }
                    }))
            });
            inside = inside.child(
                SettingsRow::new(title)
                    .icon(aether_ui::provider_icon(provider))
                    .description(format!("{detail} · {}", state.label()))
                    .control(
                        h_flex()
                            .gap_2()
                            .child(
                                aether_ui::button(("agent-open", id as u64))
                                    .ghost()
                                    .small()
                                    .label("Open")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if let Some(ix) = this.chats.iter().position(|c| c.id == id)
                                        {
                                            this.select(ix, window, cx);
                                        }
                                    })),
                            )
                            .children(stop),
                    ),
            );
        }

        let mut outside = SettingsSection::new("Outside Aether");
        if let Some(err) = &self.agents_status {
            outside = outside.child(SettingsRow::new(format!("Couldn't stop: {err}")));
        }
        if self.external.is_empty() {
            outside = outside.child(SettingsRow::new("No agents running outside aether."));
        }
        for a in &self.external {
            let folder = a
                .cwd
                .file_name()
                .map_or_else(|| a.cwd.display().to_string(), |n| n.to_string_lossy().into_owned());
            let mut detail = format!(
                "{} · {folder} · {}",
                a.provider.label(),
                external_state_text(&a.state, a.exact)
            );
            if !a.elapsed.is_empty() {
                detail = format!("{detail} · Running {}", a.elapsed);
            }
            if !a.origin.is_empty() {
                detail = format!("{detail} · {}", a.origin);
            }
            // keyed by identity: focus and clicks must not follow a row index to another agent
            let key = format!("{}-{}-{}", a.pid.unwrap_or(0), a.session, a.cwd.display());
            let no_open = open_blocker(a);
            let no_cwd = a.cwd.as_os_str().is_empty();
            let (open, reveal, stop) = (a.clone(), a.clone(), a.clone());
            outside = outside.child(
                SettingsRow::new(display_title(a))
                    .icon(aether_ui::provider_icon(a.provider))
                    .description(detail)
                    .control(
                        h_flex()
                            .gap_2()
                            .child(
                                aether_ui::button(format!("ext-open-{key}"))
                                    .ghost()
                                    .small()
                                    .label("Open")
                                    .disabled(no_open.is_some())
                                    .when_some(no_open, |b, why| b.tooltip(why))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_external(open.clone(), window, cx)
                                    })),
                            )
                            .child(
                                aether_ui::button(format!("ext-reveal-{key}"))
                                    .ghost()
                                    .small()
                                    .label("Reveal")
                                    .disabled(no_cwd)
                                    .when(!no_cwd, |b| b.tooltip(a.cwd.display().to_string()))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.reveal_external(&reveal, cx)
                                    })),
                            )
                            .child(
                                aether_ui::button(format!("ext-stop-{key}"))
                                    .ghost()
                                    .small()
                                    .icon(IconName::Square)
                                    .label("Stop")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.stop_external(stop.clone(), window, cx)
                                    })),
                            ),
                    ),
            );
        }

        SettingsPage::new("Agents")
            .on_back(cx.listener(|this, _, _, cx| this.open_page(Page::Chat, cx)))
            .action(
                aether_ui::button("refresh-agents")
                    .ghost()
                    .small()
                    .icon(IconName::RefreshCw)
                    .label("Refresh")
                    .on_click(cx.listener(|this, _, _, cx| this.scan_agents(cx))),
            )
            .child(inside)
            .child(outside)
    }
}

#[cfg(test)]
#[path = "../tests/unit/agents_view.rs"]
mod tests;
