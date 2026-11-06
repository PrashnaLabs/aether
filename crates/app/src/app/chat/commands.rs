use aether_core::{AccessMode, Message, Provider, Update, find_binary};
use gpui_kit::Context;

use crate::app::ChatApp;

impl ChatApp {
    pub(crate) fn clear_chat(&mut self, cx: &mut Context<Self>) {
        let ix = self.active;
        if self.chats[ix].cancel.is_some() {
            return;
        }
        let chat = &mut self.chats[ix];
        chat.messages.clear();
        chat.lead_seen = None;
        chat.title = "New chat".into();
        chat.epoch += 1;
        chat.cli_session.clear();
        self.agent.close_session(&chat.id.to_string());
        self.persist(ix);
        self.reset_rows(cx);
        cx.notify();
    }

    pub(crate) fn run_doctor(&mut self, cx: &mut Context<Self>) {
        let find = |name, override_path: &str| {
            find_binary(name, override_path).map_or("not found".into(), |p| p.display().to_string())
        };
        let report = format!(
            "Diagnostics:\n\
             • Claude CLI: {}\n\
             • Codex CLI: {}\n\
             • Pi CLI: {}\n\
             • OpenCode CLI: {}\n\
             • Antigravity (AGY): {}\n\
             • Workspace: {}",
            find("claude", &self.settings.claude_binary),
            find("codex", &self.settings.codex_binary),
            find("pi", &self.settings.pi_binary),
            find("opencode", &self.settings.opencode_binary),
            find_binary("agy", &self.settings.antigravity_binary)
                .or_else(|| find_binary("antigravity", &self.settings.antigravity_binary))
                .map_or("not found".into(), |p| p.display().to_string()),
            self.settings.workspace_root
        );
        let chat = &mut self.chats[self.active];
        let mut msg = Message::assistant();
        msg.apply(Update::Delta(report));
        msg.finish();
        chat.messages.push(msg);
        self.persist(self.active);
        self.append_rows(self.active, None, cx);
        cx.notify();
    }

    pub(crate) fn show_help(&mut self, cx: &mut Context<Self>) {
        let chat = &self.chats[self.active];
        let help = if !chat.is_agent() {
            "Available Slash Commands (OpenRouter):\n\
             • /new — Start a new chat\n\
             • /clear — Clear chat history\n\
             • /search — Toggle web search\n\
             • /plan — Toggle plan mode\n\
             • /model <name> — Switch model\n\
             • /reasoning <off|low|medium|high> — Set reasoning effort\n\
             • /theme <system|light|dark> — Change appearance\n\
             • /settings — Open settings\n\
             • /usage, /cost — Show token and cost usage for OpenRouter\n\
             • /usage dashboard — Open full analytics dashboard\n\
             • /help — Show available commands\n\n\
             Triggers:\n\
             • / — Slash commands"
                .to_string()
        } else {
            let cli: Vec<String> = self
                .probes
                .get(&chat.provider)
                .map(|p| p.commands.iter().take(40).map(|c| format!("/{}", c.name)).collect())
                .unwrap_or_default();
            let specific = if cli.is_empty() {
                String::new()
            } else {
                format!(" • {} commands: {}\n", chat.provider.label(), cli.join(", "))
            };
            let modes: Vec<&str> = AccessMode::for_provider(chat.provider)
                .iter()
                .map(|mode| match mode {
                    AccessMode::Ask => "ask",
                    AccessMode::AutoEdit => "autoedit",
                    AccessMode::Auto => "auto",
                    AccessMode::ReadOnly => "readonly",
                    AccessMode::Full => "full",
                })
                .collect();
            let modes = modes.join("|");
            let compact = match chat.provider {
                Provider::Antigravity => "",
                _ => " • /compact — Compact conversation context\n",
            };
            format!(
                "Available Slash Commands ({}):\n\
             • /mode <{modes}> — Set agent access mode\n\
             • /clear — Clear conversation history\n\
             {compact}\
             • /doctor — System and binary diagnostics\n\
             • /diff — Show working tree changes\n\
             • /terminal — Toggle terminal drawer\n\
             • /skills — Manage skills\n\
             • /model <name> — Switch model\n\
             • /usage, /cost — Show token and cost usage for {}\n\
             • /usage dashboard — Open full analytics dashboard\n\
             • /help — Show available commands\n\
             {specific}\n\
             Triggers:\n\
             • / — Agent commands\n\
             • @ — Skills, workspace files, and subagent mentions\n\
             • $ — Dynamic terminal commands (e.g. $cargo test, $git status, $npm dev)\n\
             • ! — Run a command in a new terminal tab (e.g. !cargo test)\n\
             • # — Workspace project files",
                chat.provider.label(),
                chat.provider.label()
            )
        };
        let chat = &mut self.chats[self.active];
        let mut msg = Message::assistant();
        msg.apply(Update::Delta(help));
        msg.finish();
        chat.messages.push(msg);
        self.persist(self.active);
        self.append_rows(self.active, None, cx);
        cx.notify();
    }
}
