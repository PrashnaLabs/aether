use aether_core::{AccessMode, Appearance, Provider, Reasoning};
use gpui_kit::SharedString;

use super::{MAX_SUGGESTIONS, Pick, SlashCommand};
use crate::app::{ChatApp, Page, picker_value};

pub(crate) fn prompt_commands(provider: Provider) -> &'static [&'static str] {
    if provider == Provider::Antigravity { &[] } else { &["compact"] }
}

impl ChatApp {
    /// Commands matching `query`: strictly separated for normal chat vs agent drivers.
    pub(super) fn slash_commands(&self, query: &str) -> Vec<(SharedString, SharedString, Pick)> {
        let on = |yes: bool| if yes { " (current)" } else { "" };
        let chat = &self.chats[self.active];
        let is_agent = chat.is_agent();
        let provider = chat.provider;

        let mut all: Vec<(String, String, SlashCommand)> = vec![
            (
                "new".into(),
                if is_agent {
                    "Start a new agent session".into()
                } else {
                    "Start a new chat".into()
                },
                SlashCommand::NewChat,
            ),
            (
                "direct".into(),
                "Start a new direct chat (non-agentic)".into(),
                SlashCommand::NewDirectChat,
            ),
        ];

        if chat.cancel.is_some() {
            all.push(("stop".into(), "Stop generating".into(), SlashCommand::Stop));
        }

        if !is_agent {
            // ==========================================
            // NORMAL CHAT (OpenRouter / adk-rust)
            // ==========================================
            all.push(("clear".into(), "Clear chat history".into(), SlashCommand::ClearChat));
            all.push((
                "btw".into(),
                "Ask a side question".into(),
                SlashCommand::Prefill("/btw ".into()),
            ));
            all.push((
                "search".into(),
                if self.settings.web_search { "Turn web search off" } else { "Turn web search on" }
                    .into(),
                SlashCommand::WebSearch,
            ));
            all.push((
                "plan".into(),
                if self.settings.plan_mode {
                    "Turn plan mode off"
                } else {
                    "Plan first: model proposes a plan for approval"
                }
                .into(),
                SlashCommand::PlanMode,
            ));
            for (reasoning, name) in
                Reasoning::ALL.into_iter().zip(["off", "low", "medium", "high"])
            {
                let current = on(self.settings.reasoning == reasoning);
                all.push((
                    format!("reasoning {name}"),
                    format!("Set reasoning effort{current}"),
                    SlashCommand::Reasoning(reasoning),
                ));
            }
            all.push((
                "usage".into(),
                "Show token and cost usage for OpenRouter".into(),
                SlashCommand::Usage,
            ));
            all.push((
                "cost".into(),
                "Show token and cost usage for OpenRouter".into(),
                SlashCommand::Usage,
            ));
        } else {
            // ==========================================
            // AGENTS (Claude, Codex, Antigravity, OpenCode, Pi)
            // ==========================================
            all.push(("clear".into(), "Clear session history".into(), SlashCommand::ClearChat));
            all.push((
                "btw".into(),
                "Ask a side question without interrupting the agent".into(),
                SlashCommand::Prefill("/btw ".into()),
            ));
            all.push((
                "doctor".into(),
                "Check agent CLI binary, environment and diagnostics".into(),
                SlashCommand::Doctor,
            ));
            all.push((
                "diff".into(),
                "Inspect unstaged and staged git changes".into(),
                SlashCommand::Diff,
            ));
            all.push((
                "terminal".into(),
                "Toggle integrated terminal drawer".into(),
                SlashCommand::Terminal,
            ));
            all.push((
                "usage".into(),
                format!("Show token and cost usage for {}", provider.label()),
                SlashCommand::Usage,
            ));
            all.push((
                "cost".into(),
                format!("Show token and cost usage for {}", provider.label()),
                SlashCommand::Usage,
            ));

            for (mode, name, desc) in [
                (AccessMode::Ask, "ask", "Ask before each action"),
                (AccessMode::AutoEdit, "autoedit", "Auto-approve edits, ask for commands"),
                (AccessMode::Auto, "auto", "A reviewer model approves routine actions"),
                (AccessMode::Full, "full", "Auto-approve all actions and commands"),
                (AccessMode::ReadOnly, "readonly", "Read-only mode, no edits or changes"),
            ] {
                if !AccessMode::for_provider(provider).contains(&mode) {
                    continue;
                }
                let current = on(chat.access == mode);
                all.push((
                    format!("mode {name}"),
                    format!("{desc}{current}"),
                    SlashCommand::Access(mode),
                ));
            }

            if prompt_commands(provider).contains(&"compact") {
                all.push((
                    "compact".into(),
                    "Summarize and compact conversation context".into(),
                    SlashCommand::AgentPrompt("/compact".into()),
                ));
            }
            // the CLI's own commands, minus names aether already handles itself
            let taken: Vec<String> = all.iter().map(|(label, _, _)| label.clone()).collect();
            let reserved = ["help", "model", "theme", "settings", "skills", "new", "stop"];
            let cli = self.probes.get(&provider).map_or(&[][..], |p| &p.commands);
            for command in cli {
                let name = &command.name;
                if taken.contains(name) || reserved.contains(&name.as_str()) {
                    continue;
                }
                all.push((
                    name.clone(),
                    command.description.clone(),
                    SlashCommand::Prefill(format!("/{name} ")),
                ));
            }
        }

        // Shared appearance & page navigation
        for appearance in Appearance::ALL {
            let current = on(self.settings.appearance == appearance);
            all.push((
                format!("theme {}", appearance.slug()),
                format!("Change the theme{current}"),
                SlashCommand::Theme(appearance),
            ));
        }
        all.push(("settings".into(), "Open settings".into(), SlashCommand::Open(Page::Settings)));
        if is_agent {
            all.push(("skills".into(), "Manage skills".into(), SlashCommand::Open(Page::Skills)));
        }
        all.push((
            "usage dashboard".into(),
            "Open full analytics dashboard across agents".into(),
            SlashCommand::Open(Page::Usage),
        ));
        all.push(("help".into(), "Show available slash commands".into(), SlashCommand::Help));

        // Models
        let items = self.model_items(provider);
        let selected = picker_value(provider, &chat.model(&self.settings), &items);
        if chat.cancel.is_none() {
            all.extend(items.into_iter().map(|item| {
                let current = on(item.id == selected);
                (
                    format!("model {}", item.name),
                    format!("{}{current}", item.detail),
                    SlashCommand::Model(item.id.into()),
                )
            }));
        }

        let query = query.trim().to_lowercase();
        let mut found: Vec<(bool, (String, String, SlashCommand))> = all
            .into_iter()
            .filter(|(label, _, _)| {
                let label = label.to_lowercase();
                query.split_whitespace().all(|word| label.contains(word))
            })
            .map(|entry| (!entry.0.to_lowercase().starts_with(&query), entry))
            .collect();
        found.sort_by_key(|(later, _)| *later);
        found
            .into_iter()
            .take(MAX_SUGGESTIONS)
            .map(|(_, (label, description, command))| {
                (format!("/{label}").into(), description.into(), Pick::Command(command))
            })
            .collect()
    }
}
