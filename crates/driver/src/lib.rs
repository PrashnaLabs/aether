use std::path::PathBuf;

use aether_core::{AccessMode, Answer, Attachment, Provider, Settings, Update};
use futures::channel::mpsc::UnboundedSender;

pub mod antigravity;
pub mod catalog;
pub mod claude;
pub mod codex;
pub mod opencode;
pub mod pi;
pub mod record;
pub mod spawn;
mod tool;
pub mod version;

pub use antigravity::AntigravityDriver;
pub use catalog::{CliModel, ProviderProbe};
pub use claude::ClaudeDriver;
pub use codex::CodexDriver;
pub use opencode::OpencodeDriver;
pub use pi::PiDriver;
pub use tool::{INPUT_CHARS, clip, head_tail};

#[derive(Clone)]
pub struct DriverStartOptions {
    pub binary: String,
    pub cwd: PathBuf,
    pub model: String,
    pub effort: String,
    pub access: AccessMode,
    pub resume: String,
    /// Opened for a reply left unfinished at quit; one that ended meanwhile is closed out.
    pub reattach: bool,
    /// This chat's attachment folder, where tool result images are saved.
    pub attachments: Option<PathBuf>,
    /// Background row ids of the reply a reattach picks up; ones whose work ended get settled.
    pub background: Vec<String>,
    /// Aether tools to attach to this chat's CLI; set only for a lead chat.
    pub lead: Option<LeadTools>,
    /// A `/btw` side run: the CLI shouldn't save its session.
    pub ephemeral: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LeadTools {
    pub exe: PathBuf,
    pub sock: PathBuf,
    pub token: String,
    pub specs: serde_json::Value,
}

pub fn mcp_config_json(lead: &LeadTools) -> String {
    serde_json::json!({"mcpServers": {"aether": {
        "command": lead.exe,
        "args": ["mcp"],
        "env": {"AETHER_ORCH_SOCK": lead.sock, "AETHER_ORCH_TOKEN": lead.token},
    }}})
    .to_string()
}

impl DriverStartOptions {
    pub fn for_provider(s: &Settings, p: Provider, cwd: impl Into<PathBuf>) -> Self {
        let (key, binary) = match p {
            Provider::Claude => ("claude", s.claude_binary.clone()),
            Provider::Codex => ("codex", s.codex_binary.clone()),
            Provider::Pi => ("pi", s.pi_binary.clone()),
            Provider::OpenCode => ("opencode", s.opencode_binary.clone()),
            Provider::Antigravity => ("antigravity", s.antigravity_binary.clone()),
            Provider::OpenRouter => {
                return Self {
                    binary: String::new(),
                    cwd: cwd.into(),
                    model: s.model.clone(),
                    effort: String::new(),
                    access: AccessMode::default(),
                    resume: String::new(),
                    reattach: false,
                    attachments: None,
                    background: Vec::new(),
                    lead: None,
                    ephemeral: false,
                };
            }
        };
        Self {
            binary,
            cwd: cwd.into(),
            model: s.provider_models.get(key).cloned().unwrap_or_default(),
            effort: String::new(),
            access: AccessMode::default(),
            resume: String::new(),
            reattach: false,
            attachments: None,
            background: Vec::new(),
            lead: None,
            ephemeral: false,
        }
    }
}

pub trait Driver: Send {
    /// Starts a turn with `text` and the `images` this provider takes inline.
    fn prompt(&mut self, text: String, images: Vec<Attachment>);
    /// Adds `text` to the running turn. False, doing nothing, when no turn runs.
    fn steer(&mut self, text: String, images: Vec<Attachment>) -> bool;
    /// Switches model and effort for later turns; false when only a respawn can.
    fn set_options(&mut self, _model: &str, _effort: &str) -> bool {
        false
    }
    /// Stops the running turn; the session stays alive.
    fn cancel(&mut self);
    /// Answers the driver's own Ask `id`.
    fn answer(&mut self, _id: u64, _answer: Answer) {}
    /// True when a non-empty `resume` id was reattached.
    fn resumed(&self) -> bool;
    /// False once the session can no longer run prompts.
    fn alive(&self) -> bool {
        true
    }
    fn pid(&self) -> Option<u32> {
        None
    }
    /// True when the Driver itself emits `Update::Steered` once the CLI consumes the steer.
    /// Only such Drivers may send `Update::SteerRefused`, with the text minus attached files.
    fn reports_steered(&self) -> bool {
        false
    }
    /// Stops one background task or terminal by tool row id; false when the CLI can't.
    fn stop_tool(&mut self, _id: &str) -> bool {
        false
    }
    /// Runs a notice action, offered by a notice whose `url` is `"aether:<action>"`.
    fn notice_action(&mut self, _action: &str) {}
    /// Asks `question` natively, off the main turn: the answer comes on `reply` as one
    /// `Update::Final` or an `Update::Error`, never on the driver's own stream. False,
    /// sending nothing, when the CLI can't.
    /// Loads earlier messages as native history before the first prompt; `false` when the CLI
    /// can't, and the caller puts them in the prompt instead.
    fn seed_history(&mut self, _context: &str, _messages: &[(aether_core::Role, String)]) -> bool {
        false
    }

    fn side_question(&mut self, _question: &str, _reply: UnboundedSender<Update>) -> bool {
        false
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
