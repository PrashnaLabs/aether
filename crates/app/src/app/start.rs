use aether_core::Provider;
use aether_driver::ProviderProbe;
use aether_ui::theme::CONTENT_WIDTH;
use aether_ui::{CardHealth, ChoiceMenu, HealthCard, Hero, RecentChats, Starters, provider_icon};
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{ActiveTheme, Disableable, h_flex};
use gpui_kit::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, MouseButton, ParentElement,
    Styled, div, px,
};

use crate::app::composer::chips::JOB_TIP;
use crate::app::{ChatApp, chat_cwd, now_ms};
use crate::project_page::model::Card;
use crate::projects::relative_time;

const SHOW_PROVIDER_CARDS: bool = false;

const CODE_STARTERS: &[(&str, &str)] = &[
    (
        "Tour this codebase",
        "Look around this folder: what the project is, how to run it, how its tests run, and where to start reading.",
    ),
    (
        "Run the tests",
        "Run this project's tests. Explain any failures and fix them, then run the tests again.",
    ),
    (
        "Review my changes",
        "Review the uncommitted changes here (git diff) for bugs, risky edits and missing tests.",
    ),
    (
        "Add missing tests",
        "Find the least-tested module in this project, add focused tests for it and run them.",
    ),
];

const FOLDER_STARTERS: &[(&str, &str)] = &[
    (
        "Tour this folder",
        "Look around this folder: what's in it, what each part is for, and where to start.",
    ),
    (
        "Tidy it up",
        "Suggest a tidier layout for this folder: what to group, rename or delete. Change nothing until I confirm.",
    ),
    (
        "Start a git repo",
        "Make this folder a git repository with a fitting .gitignore, then make the first commit.",
    ),
    (
        "Build a web page",
        "Build a single-file pomodoro timer page in HTML, CSS and JS, then check that it loads without errors.",
    ),
];

const GENERAL_STARTERS: &[(&str, &str)] = &[
    (
        "Explain simply",
        "Explain how HTTPS keeps a connection private, simply, with one everyday analogy.",
    ),
    (
        "Today on Hacker News",
        "What are the top Hacker News stories right now? Give a one-line take on each.",
    ),
    (
        "Draft an email",
        "Draft a short, friendly email asking my team to move Friday's meeting to Monday morning.",
    ),
    (
        "Research with sources",
        "Compare FastAPI and Litestar today: performance, typing, ecosystem and maintenance, with sources.",
    ),
];

impl ChatApp {
    pub(crate) fn render_start(
        &self,
        composer: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let chat = &self.chats[self.active];
        let project = self.is_agent_chat(chat).then(|| self.chat_project(self.active)).flatten();
        let name = project.as_deref().map(|dir| self.project_name(dir));
        let agent = self.is_agent_chat(chat);
        let (lead, link, tail) = start_headline(agent, name.as_deref());
        let mut headline = h_flex().justify_center().items_center().text_size(px(30.)).child(lead);
        if let Some(link) = link {
            let locked = !chat.messages.is_empty() || chat.worktree_bound();
            let path = chat_cwd(chat, &self.settings.workspace_root).display().to_string();
            let job = chat.worktree_job.is_some()
                || self.switching_folder(&chat_cwd(chat, &self.settings.workspace_root));
            self.open_project_switcher_sync(cx);
            let trigger = aether_ui::button("headline-project")
                .on_mouse_down(MouseButton::Right, crate::session_workspace::block_session_menu)
                .ghost()
                .cursor_pointer()
                // The button's label container overrides inherited text sizing.
                .child(div().text_size(px(30.)).font_weight(FontWeight::NORMAL).child(link.clone()))
                .accessibility_label(format!("Switch project, current {link}"))
                .tooltip(match (locked, job) {
                    (true, _) => format!("Folder is fixed for this chat: {path}"),
                    (_, true) => JOB_TIP.to_string(),
                    _ => path.clone(),
                })
                .disabled(locked || job)
                .text_size(px(30.))
                .font_weight(FontWeight::NORMAL)
                .h(px(40.))
                .px(px(2.))
                .rounded_none()
                .border_b_1()
                .border_dashed()
                .border_color(cx.theme().muted_foreground);
            headline = headline.child(ChoiceMenu::new(&self.headline_project_menu, trigger));
        }
        let headline = headline.child(tail);
        let no_git =
            matches!(self.branches.get(&chat_cwd(chat, &self.settings.workspace_root)), Some(None));
        let view = self.session_handle(cx);
        let starters =
            Starters::new(starters(project.is_some(), no_git), move |prompt, window, cx| {
                view.update(cx, |this, cx| {
                    this.composer.update(cx, |input, cx| input.set_value(prompt, window, cx));
                    this.send(window, cx);
                })
            });
        let mut hero = Hero::new(headline).child(composer).child(starters);
        if agent && SHOW_PROVIDER_CARDS {
            hero = hero.child(self.health_cards(chat.provider, cx));
        }
        if let Some(dir) = project {
            let cards = recent_chats(self.project_cards(&dir), chat.id, 4);
            if !cards.is_empty() {
                let now = now_ms();
                let view = self.session_handle(cx);
                let all = view.clone();
                let pname = self.project_name(&dir);
                let mut recent = RecentChats::new(format!("Recent in {pname}"))
                    .view_all(format!("View all chats in {pname}"), move |_, cx| {
                        all.update(cx, |this, cx| this.open_project_page(dir.clone(), cx))
                    });
                for card in cards {
                    let view = view.clone();
                    recent = recent.row(
                        provider_icon(card.provider),
                        card.title,
                        card.place,
                        card.gone,
                        relative_time(card.activity_ms, now, &chrono::Local),
                        move |window, cx| {
                            view.update(cx, |this, cx| this.select(card.ix, window, cx))
                        },
                    );
                }
                hero = hero.child(
                    div()
                        .id("session-recent-chats")
                        .w_full()
                        .on_mouse_down(
                            MouseButton::Right,
                            crate::session_workspace::block_session_menu,
                        )
                        .child(recent),
                );
            }
        }
        hero.into_any_element()
    }
}

impl ChatApp {
    fn health_cards(&self, current: Provider, cx: &mut Context<Self>) -> AnyElement {
        let view = self.session_handle(cx);
        h_flex()
            .w_full()
            .max_w(CONTENT_WIDTH)
            .mx_auto()
            .px(px(24.))
            .flex_wrap()
            .gap(px(8.))
            .children(Provider::ALL.map(|provider| {
                let probe = self.probes.get(&provider);
                let state = match provider {
                    Provider::OpenRouter => {
                        openrouter_health(self.api_key.is_some(), self.models.len())
                    }
                    _ => health(probe),
                };
                let health = match state {
                    Health::Checking => CardHealth::Checking,
                    Health::Ready(s) => CardHealth::Ready(s.into()),
                    Health::Degraded(s) => CardHealth::Degraded(s.into()),
                    Health::Missing => CardHealth::Missing,
                };
                let view = view.clone();
                let mut card = HealthCard::new(
                    format!("health-{}", provider.key()),
                    provider_icon(provider),
                    provider.label(),
                    version_text(provider, probe),
                    health,
                    move |_, cx| view.update(cx, |this, cx| this.pick_provider(provider, cx)),
                )
                .selected(provider == current);
                for chip in probe.map_or(&[][..], |p| &p.chips) {
                    card = card.chip(chip.clone());
                }
                card
            }))
            .into_any_element()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Health {
    Checking,
    Ready(String),
    Degraded(String),
    Missing,
}

pub(crate) fn health(probe: Option<&ProviderProbe>) -> Health {
    match probe {
        None => Health::Checking,
        Some(p) if !p.installed => Health::Missing,
        Some(ProviderProbe { degraded: Some(why), .. }) => Health::Degraded(why.clone()),
        Some(p) => Health::Ready(p.status.clone()),
    }
}

pub(crate) fn openrouter_health(has_key: bool, models: usize) -> Health {
    match has_key {
        true => Health::Ready(format!("Key set · {models} models")),
        false => Health::Degraded("Add an API key in Settings".into()),
    }
}

pub(crate) fn version_text(provider: Provider, probe: Option<&ProviderProbe>) -> String {
    match provider {
        Provider::OpenRouter => "built-in".into(),
        _ => probe.and_then(|p| p.version.clone()).unwrap_or_default(),
    }
}

pub(crate) fn version_chip(label: &str, version: &str) -> String {
    format!("{label} {version}").trim().into()
}

pub(crate) fn start_headline(
    agent: bool,
    project: Option<&str>,
) -> (String, Option<String>, String) {
    match (agent, project) {
        (true, Some(name)) => ("What should we build in ".into(), Some(name.into()), "?".into()),
        (true, None) => ("What should we build?".into(), None, String::new()),
        (false, _) => ("What can I help with?".into(), None, String::new()),
    }
}

/// Code prompts in a repo, folder ones once a check finds no git, general ones without a project.
pub(crate) fn starters(project: bool, no_git: bool) -> &'static [(&'static str, &'static str)] {
    match (project, no_git) {
        (false, _) => GENERAL_STARTERS,
        (true, true) => FOLDER_STARTERS,
        (true, false) => CODE_STARTERS,
    }
}

pub(crate) fn recent_chats(mut cards: Vec<Card>, exclude: i64, limit: usize) -> Vec<Card> {
    cards.retain(|c| c.id != exclude);
    cards.sort_by_key(|c| std::cmp::Reverse(c.activity_ms));
    cards.truncate(limit);
    cards
}

#[cfg(test)]
#[path = "../../tests/unit/app/start.rs"]
mod tests;
