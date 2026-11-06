use aether_core::{AccessMode, Appearance, Reasoning};
use aether_ui::SuggestionMenu;
use gpui_kit::{Context, EntityInputHandler, SharedString, Window};

use crate::app::{ChatApp, Page};

mod slash;
pub(crate) use slash::prompt_commands;
mod sources;

const MAX_SUGGESTIONS: usize = 8;

/// What a `/` command does once picked.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SlashCommand {
    NewChat,
    NewDirectChat,
    ClearChat,
    WebSearch,
    PlanMode,
    Stop,
    Reasoning(Reasoning),
    Theme(Appearance),
    Model(String),
    Access(AccessMode),
    Open(Page),
    Doctor,
    Help,
    Usage,
    Diff,
    Terminal,
    AgentPrompt(String),
    Prefill(String),
}

#[derive(Clone)]
pub(crate) enum Pick {
    Command(SlashCommand),
    Insert(String),
}

/// The open suggestion menu. `at` is where the trigger character sits.
pub(crate) struct Suggestions {
    pub(crate) at: usize,
    pub(crate) trigger: char,
    pub(crate) title: SharedString,
    pub(crate) items: Vec<(SharedString, SharedString, Pick)>,
    pub(crate) selected: usize,
}

/// Finds trigger ('/', '@', '$', '#') right before cursor in the composer text.
pub(crate) fn find_trigger(text: &str, cursor: usize) -> Option<(usize, char, String)> {
    let before = text.get(..cursor)?;
    // If the whole input begins with '/' or '$', handle as primary command trigger
    if before.starts_with('/') && !before.contains('\n') {
        return Some((0, '/', before[1..].to_string()));
    }
    if before.starts_with('$') && !before.contains('\n') {
        return Some((0, '$', before[1..].trim_start().to_string()));
    }
    // Check for trigger at the start of the current word before cursor
    let word_start = before.rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0);
    let word = &before[word_start..];
    if let Some(partial) = word.strip_prefix('@') {
        return Some((word_start, '@', partial.to_string()));
    }
    if let Some(partial) = word.strip_prefix('#') {
        return Some((word_start, '#', partial.to_string()));
    }
    if let Some(partial) = word.strip_prefix('$') {
        return Some((word_start, '$', partial.to_string()));
    }
    None
}

pub(crate) fn trigger_fits(text: &str, at: usize, cursor: usize) -> bool {
    text.is_char_boundary(at) && cursor >= at
}

impl ChatApp {
    /// Re-reads the composer and opens, refreshes or closes the menu.
    pub(crate) fn update_suggestions(&mut self, cx: &mut Context<Self>) {
        let input = self.composer.read(cx);
        let (text, cursor) = (input.value().to_string(), input.cursor());
        let trigger = find_trigger(&text, cursor).filter(|_| !self.shell_mode(cx));
        let Some((at, trigger_char, query)) = trigger else {
            self.suggestions = None;
            self.suggestions_dismissed = None;
            return cx.notify();
        };
        if self.suggestions_dismissed == Some((at, trigger_char)) {
            return;
        }

        let chat = &self.chats[self.active];
        let (title, items) = match trigger_char {
            '/' => {
                let title = if chat.is_agent() {
                    format!("Commands ({})", chat.provider.label())
                } else {
                    "Commands (OpenRouter)".to_string()
                };
                (title.into(), self.slash_commands(&query))
            }
            '@' => {
                let title = if chat.is_agent() {
                    format!("Installed Skills & Mentions ({})", chat.provider.label())
                } else {
                    "Installed Skills & Mentions".to_string()
                };
                (title.into(), self.mentions(&query))
            }
            '$' => ("Terminal Commands".into(), self.terminal_commands(&query)),
            '#' => ("Workspace Files".into(), self.workspace_file_suggestions(&query)),
            _ => ("Suggestions".into(), Vec::new()),
        };

        let selected = match &self.suggestions {
            Some(open) if open.at == at && open.trigger == trigger_char => {
                open.selected.min(items.len().saturating_sub(1))
            }
            _ => 0,
        };
        self.suggestions = (!items.is_empty()).then_some(Suggestions {
            at,
            trigger: trigger_char,
            title,
            items,
            selected,
        });
        cx.notify();
    }

    pub(crate) fn move_suggestion(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(open) = &mut self.suggestions {
            let len = open.items.len() as isize;
            open.selected = (open.selected as isize + delta).rem_euclid(len) as usize;
            cx.notify();
        }
    }

    pub(crate) fn dismiss_suggestions(&mut self, cx: &mut Context<Self>) {
        if let Some(open) = self.suggestions.take() {
            self.suggestions_dismissed = Some((open.at, open.trigger));
            cx.notify();
        }
    }

    pub(crate) fn accept_suggestion(
        &mut self,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self.suggestions.take() else {
            return;
        };
        let Some((_, _, pick)) = open.items.get(index.unwrap_or(open.selected)).cloned() else {
            return;
        };
        let input = self.composer.read(cx);
        if !trigger_fits(&input.value(), open.at, input.cursor()) {
            return cx.notify();
        }
        match pick {
            Pick::Insert(replacement) => {
                self.composer.update(cx, |input, cx| {
                    let text = input.value().to_string();
                    let utf16 = |byte: usize| text[..byte].encode_utf16().count();
                    let range = utf16(open.at)..utf16(input.cursor());
                    input.replace_text_in_range(Some(range), &replacement, window, cx);
                    input.focus(window, cx);
                });
            }
            Pick::Command(SlashCommand::Prefill(text)) => {
                self.composer.update(cx, |input, cx| {
                    input.set_value(text, window, cx);
                    input.focus(window, cx);
                });
            }
            Pick::Command(command) => {
                self.composer.update(cx, |input, cx| input.clean(window, cx));
                self.run_command(command, window, cx);
            }
        }
        cx.notify();
    }

    fn run_command(&mut self, command: SlashCommand, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            SlashCommand::NewChat => self.new_chat_in_current_project(window, cx),
            SlashCommand::NewDirectChat => self.new_direct_chat(window, cx),
            SlashCommand::ClearChat => self.clear_chat(cx),
            SlashCommand::WebSearch => self.update_settings(cx, |s| s.web_search = !s.web_search),
            SlashCommand::PlanMode => self.update_settings(cx, |s| s.plan_mode = !s.plan_mode),
            SlashCommand::Stop => self.stop(window, cx),
            SlashCommand::Reasoning(reasoning) => {
                self.update_settings(cx, |s| s.reasoning = reasoning)
            }
            SlashCommand::Theme(appearance) => self.set_appearance(appearance, window, cx),
            SlashCommand::Model(id) => self.pick_model(id, window, cx),
            SlashCommand::Access(mode) => self.set_access(mode, cx),
            SlashCommand::Open(page) => self.open_page(page, cx),
            SlashCommand::Doctor => self.run_doctor(cx),
            SlashCommand::Help => self.show_help(cx),
            SlashCommand::Usage => self.show_usage(cx),
            SlashCommand::Diff => {
                self.toggle_changes(true, window, cx);
            }
            SlashCommand::Terminal => {
                self.toggle_terminal(window, cx);
            }
            SlashCommand::AgentPrompt(prompt) => {
                self.submit(self.active, prompt, vec![], window, cx);
            }
            SlashCommand::Prefill(text) => {
                self.composer.update(cx, |input, cx| {
                    input.set_value(text, window, cx);
                    input.focus(window, cx);
                });
            }
        }
    }

    pub(crate) fn suggestion_pick_listener(
        &self,
        cx: &Context<Self>,
    ) -> Box<dyn Fn(&usize, &mut Window, &mut gpui_kit::App)> {
        Box::new(self.session_listener(cx, |this, ix: &usize, window, cx| {
            this.accept_suggestion(Some(*ix), window, cx)
        }))
    }

    pub(crate) fn render_suggestions(&self, cx: &mut Context<Self>) -> Option<SuggestionMenu> {
        let open = self.suggestions.as_ref()?;
        let menu = open.items.iter().fold(
            SuggestionMenu::new(open.title.clone()).selected(open.selected),
            |menu, (label, description, _)| menu.item(label.clone(), description.clone()),
        );
        Some(menu.on_pick(self.suggestion_pick_listener(cx)))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/suggest/mod.rs"]
mod tests;
