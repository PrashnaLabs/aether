mod attachment;
mod lock;
mod paths;
mod request;
mod settings;
mod skill;
mod sys;

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub use attachment::*;
pub use lock::MutexExt;
pub use paths::{
    attachments_dir, canonicalize, config_dir, create_private_dir, find_binary, home_dir,
    repaired_path, simplify,
};
pub use request::{
    Answer, Approval, ApprovalKind, ApprovalOption, Ask, Choice, Field, FieldKind, Form, Plan,
    Question, Request, RequestStatus, When, field_label, field_visible, form_answer,
};
pub use settings::{
    AccessMode, Appearance, ChatDetail, ChatLayout, CornerStyle, DateFormat, FooterChips, IconPack,
    NotesEditor, NotesSettings, NotifyDelivery, NotifySound, OrgHire, OrgLayout, ProjectScript,
    Provider, Reasoning, Settings, SidebarGrouping, SidebarOrdering, SidebarStyle, THEME_FAMILIES,
    TerminalCursor, TerminalTheme, ThemeFamily, TileLayout, TimeFormat, ToolPolicy, Watch,
    WatchCorner, effort_label, effort_levels, family_index, toggle_watch,
};
pub use skill::{
    Skill, SkillSource, complete_skill, mention_at, mentioned_skills, valid_skill_name,
};
pub use sys::{command, random_bytes, signal_process};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolStatus {
    Running,
    Done,
    Failed,
    Stopped,
    /// The user declined it in an approval.
    Denied,
    /// Blocked on the user or another agent.
    Waiting,
    /// Runs on after the reply ends.
    Background,
    #[serde(other)]
    Unknown,
}

/// Replaces tool output dropped to keep a saved reply small.
pub const NOT_KEPT: &str = "… Not kept: this reply's tool output was too large to save";

/// One tool call of a reply, upserted by `id` as the CLI reports progress.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub title: String,
    #[serde(default, deserialize_with = "legacy_detail")]
    pub detail: ToolDetail,
    pub status: ToolStatus,
    /// First line of the failure, shown without expanding the row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Wall time once finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
    /// Id of the subagent call this one ran under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<Attachment>,
    /// Unix ms when the row first appeared; drives the live timer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_ms: Option<i64>,
}

fn now_ms() -> Option<i64> {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).ok()
}

impl ToolCall {
    pub fn new(id: impl Into<String>, title: impl Into<String>, detail: ToolDetail) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            detail,
            status: ToolStatus::Running,
            error: None,
            ms: None,
            parent: None,
            images: Vec::new(),
            started_ms: None,
        }
    }
}

/// What a tool call did, normalized once by the driver. Text is pre-truncated
/// there, so views never parse or cap it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolDetail {
    Command {
        command: String,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        output: String,
        #[serde(default)]
        exit_code: Option<i32>,
    },
    /// `diff` is unified: `@@` hunk headers (bare `@@` when the position is
    /// unknown), `+`/`-`/` ` lines, and a trailing `… N more lines` note.
    Edit {
        path: String,
        #[serde(default)]
        diff: String,
        #[serde(default)]
        added: u32,
        #[serde(default)]
        removed: u32,
    },
    Read {
        path: String,
        #[serde(default)]
        range: Option<String>,
    },
    Search {
        pattern: String,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        matches: Option<u32>,
        #[serde(default)]
        output: String,
    },
    Web {
        target: String,
        #[serde(default)]
        output: String,
    },
    Task {
        description: String,
        #[serde(default)]
        output: String,
        #[serde(default)]
        agent: String,
        #[serde(default)]
        prompt: String,
        #[serde(default)]
        tools: u32,
        #[serde(default)]
        tokens: u64,
        #[serde(default)]
        activity: String,
    },
    Plan {
        items: Vec<PlanItem>,
    },
    /// `input` is pretty JSON.
    Other {
        name: String,
        #[serde(default)]
        input: String,
        #[serde(default)]
        output: String,
    },
    /// Chats saved before typed details, and details a newer build wrote.
    Plain(String),
}

impl Default for ToolDetail {
    fn default() -> Self {
        Self::Plain(String::new())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanItem {
    pub text: String,
    pub status: PlanStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanStatus {
    Pending,
    Active,
    Done,
    Cancelled,
    #[serde(other)]
    Unknown,
}

impl ToolDetail {
    /// One line for the collapsed row: command, path, pattern, target or description.
    pub fn summary(&self) -> String {
        let line = match self {
            Self::Command { command, .. } => command,
            Self::Edit { path, .. } | Self::Read { path, .. } => path,
            Self::Search { pattern, path, .. } if pattern.is_empty() => {
                path.as_deref().unwrap_or_default()
            }
            Self::Search { pattern, .. } => pattern,
            Self::Web { target, .. } => target,
            Self::Task { description, .. } => description,
            Self::Plan { items } => {
                return items
                    .iter()
                    .find(|i| i.status == PlanStatus::Active)
                    .map(|i| i.text.clone())
                    .unwrap_or_default();
            }
            Self::Other { input, .. } => {
                return input.split_whitespace().collect::<Vec<_>>().join(" ");
            }
            Self::Plain(text) => text,
        };
        let line = line.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim();
        match line.char_indices().nth(SUMMARY_CHARS) {
            Some((at, _)) => format!("{}…", &line[..at]),
            None => line.to_string(),
        }
    }

    /// The bulky text a saved reply may drop: outputs, diffs, tool input.
    fn payload(&mut self) -> Vec<&mut String> {
        match self {
            Self::Command { output, .. }
            | Self::Search { output, .. }
            | Self::Web { output, .. }
            | Self::Task { output, .. } => vec![output],
            Self::Edit { diff, .. } => vec![diff],
            Self::Other { input, output, .. } => vec![input, output],
            Self::Read { .. } | Self::Plan { .. } | Self::Plain(_) => vec![],
        }
    }
}

/// `@@ -3,2 +3,4 @@` as (old start, old count, new start, new count).
pub fn hunk_header(line: &str) -> Option<(u32, u32, u32, u32)> {
    let mut parts = line.strip_prefix("@@ ")?.split_whitespace();
    let side = |part: Option<&str>, sign: char| -> Option<(u32, u32)> {
        let part = part?.strip_prefix(sign)?;
        let (start, count) = part.split_once(',').unwrap_or((part, "1"));
        Some((start.parse().ok()?, count.parse().ok()?))
    };
    let (old, old_n) = side(parts.next(), '-')?;
    let (new, new_n) = side(parts.next(), '+')?;
    Some((old, old_n, new, new_n))
}

const SUMMARY_CHARS: usize = 200;
/// Tool text a finished reply keeps; chats are saved whole on every change.
const SAVED_TOOL_BYTES: usize = 256 * 1024;

// Unknown shapes become Plain instead of failing the whole chat: the store
// drops any chat whose messages don't parse.
fn legacy_detail<'de, D: serde::Deserializer<'de>>(d: D) -> Result<ToolDetail, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Wire {
        Legacy(String),
        Typed(ToolDetail),
        Unknown(serde::de::IgnoredAny),
    }
    Ok(match Wire::deserialize(d)? {
        Wire::Legacy(text) => ToolDetail::Plain(text),
        Wire::Typed(detail) => detail,
        Wire::Unknown(_) => ToolDetail::default(),
    })
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Lenient<T> {
    Typed(T),
    Unknown(serde::de::IgnoredAny),
}

/// Enum variants from a newer build become `fallback` instead of failing the chat.
pub(crate) fn lenient<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
    fallback: T,
) -> Result<T, D::Error> {
    Ok(match Lenient::<T>::deserialize(d)? {
        Lenient::Typed(v) => v,
        Lenient::Unknown(_) => fallback,
    })
}

fn lenient_steps<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Step>, D::Error> {
    Ok(Vec::<Lenient<Step>>::deserialize(d)?
        .into_iter()
        .map(|s| match s {
            Lenient::Typed(step) => step,
            Lenient::Unknown(_) => Step::Unknown,
        })
        .collect())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    /// The CLI took the prompt into its native session.
    Accepted,
    Delta(String),
    /// Full text of the current model turn; replaces that turn's deltas.
    Final(String),
    /// Swaps the current turn's text without ending the turn.
    Replace(String),
    Thinking(String),
    Search {
        id: String,
        query: String,
    },
    Skill(String),
    SearchDone {
        id: String,
        outcome: Result<usize, String>,
    },
    /// Tokens the conversation occupies after the latest model call.
    Usage(u64),
    /// Context window of the model serving the reply, in tokens.
    Window(u64),
    /// Prompt, completion and cache-read tokens of one model call;
    /// accumulates per reply.
    Count {
        prompt: u64,
        completion: u64,
        cached: u64,
    },
    /// The run is paused until the user answers request `id`.
    Ask {
        id: u64,
        ask: Ask,
    },
    Error(String),
    Title(String),
    /// The CLI session's resume id; UI-only.
    Session(String),
    /// Native id of the current turn, used to roll back.
    NativeTurn(String),
    /// The CLI started a turn on its own, e.g. woken by finished background work.
    Wake,
    /// Upserts one tool step by `id`.
    Tool(ToolCall),
    /// Upserts one notice step by `id`.
    Notice(Notice),
    /// The CLI took a mid-run message from the user.
    Steered(String),
    /// The CLI refused a steer, sent only by Drivers whose `reports_steered()` is true. The
    /// text is the user's message without its attached-files block; it goes back to the queue.
    SteerRefused(String),
    /// Recoverable problem; the run continues.
    Warning(String),
    /// Cost of the reply so far in micro-USD, added to the total.
    Cost(u64),
    /// Model the CLI says actually served the reply.
    Model(String),
    Mode(AccessMode),
    /// Request `id` was answered, cancelled or timed out on the CLI side.
    AskResolved(u64),
}

/// Index of the message an update belongs to: background tool results and
/// resolved asks go to the row's owner, the rest to the live reply.
pub fn route(messages: &[Message], reply_ix: usize, update: &Update) -> usize {
    let owner = match update {
        // drivers reuse ids (compaction, mcp-{name}): only a background row is still waiting
        Update::Tool(call) => {
            let holds = |m: &Message, background: bool| {
                m.steps.iter().any(|s| {
                    matches!(s, Step::Tool(c) if c.id == call.id
                        && (!background || c.status == ToolStatus::Background
                            || c.parent.as_deref().is_some_and(|p| m.under_background(p))))
                })
            };
            match messages.get(reply_ix) {
                Some(reply) if holds(reply, false) => None,
                _ => messages.iter().position(|m| holds(m, true)).or_else(|| {
                    let parent = call.parent.as_deref()?;
                    messages.iter().position(|m| m.under_background(parent))
                }),
            }
        }
        Update::AskResolved(id) => {
            messages.iter().position(|m| m.requests.iter().any(|r| r.id == *id))
        }
        _ => None,
    };
    owner.unwrap_or(reply_ix)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub id: String,
    pub kind: NoticeKind,
    pub text: String,
    #[serde(default)]
    pub state: NoticeState,
    /// Unix ms a countdown ends at (retry backoff).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until_ms: Option<i64>,
    /// Link the notice offers (MCP sign-in).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Longer text behind "View summary" / "Details".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Notice {
    pub fn new(id: impl Into<String>, kind: NoticeKind, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind,
            text: text.into(),
            state: NoticeState::Done,
            until_ms: None,
            url: None,
            detail: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoticeKind {
    Startup,
    Retry,
    RateLimit,
    Compaction,
    Hook,
    ModelChange,
    ModeChange,
    Warning,
    McpAuth,
    Wake,
    Info,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoticeState {
    Running,
    #[default]
    Done,
    Failed,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Step {
    Thought(String),
    Skill(String),
    Search {
        #[serde(default)]
        id: String,
        query: String,
        results: Option<Result<usize, String>>,
    },
    Tool(ToolCall),
    Notice(Notice),
    /// Where the user steered the run.
    Steer(String),
    Thinking {
        text: String,
        ms: Option<u64>,
    },
    /// A step from a newer build; renders nothing.
    Unknown,
}

/// The chat continued in another agent after this message; labels are the models' display names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Handoff {
    pub from: Provider,
    pub from_model: String,
    pub to: Provider,
    pub to_model: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentWith {
    pub model: String,
    pub effort: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    #[serde(default)]
    pub native_turn: Option<String>,
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff: Option<Handoff>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent_with: Option<SentWith>,
    pub text: String,
    #[serde(deserialize_with = "lenient_steps")]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub step_positions: Vec<usize>,
    pub thought_for: Option<Duration>,
    pub context_tokens: Option<u64>,
    /// Context window the provider reported for this reply's model.
    #[serde(default)]
    pub context_window: Option<u64>,
    /// Prompt and completion tokens summed over the reply's model calls.
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    /// Cache-read tokens, a subset of the prompt count.
    #[serde(default)]
    pub cached_tokens: u64,
    /// Wall time of the finished run.
    pub elapsed_ms: Option<u64>,
    /// When the run finished, millis since the epoch.
    pub ended_at_ms: Option<i64>,
    /// When the user sent it, millis since the epoch.
    #[serde(default)]
    pub sent_at_ms: Option<i64>,
    /// Model that wrote the reply; prices the cost line.
    pub model: Option<String>,
    pub requests: Vec<Request>,
    /// The run ended in an error.
    #[serde(default)]
    pub failed: bool,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub cost_micros: u64,
    /// Model the CLI reported serving the reply.
    #[serde(default)]
    pub served_model: Option<String>,
    /// Fatal error that ended the run; first line is the message, the rest details.
    #[serde(default)]
    pub error: Option<String>,
    /// Loaded from disk: when its run started is unknown.
    #[serde(skip)]
    pub restored: bool,
    #[serde(skip, default = "Instant::now")]
    started: Instant,
    #[serde(skip)]
    thinking_since: Option<Instant>,
    #[serde(skip)]
    turn_start: usize,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            native_turn: None,
            role: Role::User,
            handoff: None,
            sent_with: None,
            turn_start: text.len(),
            text,
            steps: Vec::new(),
            step_positions: Vec::new(),
            thought_for: None,
            context_tokens: None,
            context_window: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            cached_tokens: 0,
            elapsed_ms: None,
            ended_at_ms: None,
            sent_at_ms: None,
            model: None,
            requests: Vec::new(),
            failed: false,
            attachments: Vec::new(),
            cost_micros: 0,
            served_model: None,
            error: None,
            restored: false,
            started: Instant::now(),
            thinking_since: None,
        }
    }

    pub fn assistant() -> Self {
        Self {
            native_turn: None,
            role: Role::Assistant,
            handoff: None,
            sent_with: None,
            text: String::new(),
            steps: Vec::new(),
            step_positions: Vec::new(),
            thought_for: None,
            context_tokens: None,
            context_window: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            cached_tokens: 0,
            elapsed_ms: None,
            ended_at_ms: None,
            sent_at_ms: None,
            model: None,
            requests: Vec::new(),
            failed: false,
            attachments: Vec::new(),
            cost_micros: 0,
            served_model: None,
            error: None,
            restored: false,
            started: Instant::now(),
            thinking_since: None,
            turn_start: 0,
        }
    }

    pub fn searches(&self) -> usize {
        self.steps.iter().filter(|s| matches!(s, Step::Search { .. })).count()
    }

    pub fn step_at(&self, index: usize) -> usize {
        self.step_positions.get(index).copied().unwrap_or(0).min(self.text.len())
    }

    pub fn apply(&mut self, update: Update) {
        if matches!(
            update,
            Update::Delta(_)
                | Update::Final(_)
                | Update::Replace(_)
                | Update::Skill(_)
                | Update::Search { .. }
                | Update::Tool(_)
                | Update::Notice(_)
                | Update::Warning(_)
                | Update::Steered(_)
                | Update::Error(_)
        ) {
            self.close_thinking();
        }
        match update {
            Update::NativeTurn(id) => self.native_turn = Some(id),
            Update::Delta(text) if !text.is_empty() => {
                self.begin_turn();
                self.text.push_str(&text);
                self.end_thinking();
            }
            Update::Delta(_) => {}
            Update::Final(text) => {
                if text.is_empty() {
                    // Gap-fill ending a turn (e.g. cancel): keep streamed text.
                    self.turn_start = self.text.len();
                } else {
                    self.text.truncate(self.turn_start);
                    self.begin_turn();
                    self.text.push_str(&text);
                    self.end_thinking();
                    self.turn_start = self.text.len();
                }
            }
            Update::Replace(text) => {
                self.text.truncate(self.turn_start);
                if !text.is_empty() {
                    self.begin_turn();
                    self.text.push_str(&text);
                    self.end_thinking();
                }
            }
            Update::Thinking(text) => match self.steps.last_mut() {
                Some(Step::Thinking { text: open, .. })
                    if self.step_positions.last() == Some(&self.text.len()) =>
                {
                    open.push_str(&text)
                }
                _ if text.trim().is_empty() => {}
                _ => {
                    self.close_thinking();
                    self.steps.push(Step::Thinking { text, ms: None });
                    self.step_positions.push(self.text.len());
                    self.thinking_since = Some(Instant::now());
                }
            },
            Update::Skill(name) => {
                if !self.steps.contains(&Step::Skill(name.clone())) {
                    self.steps.push(Step::Skill(name));
                    self.step_positions.push(self.text.len());
                }
            }
            Update::Search { id, query } => {
                self.steps.push(Step::Search { id, query, results: None });
                self.step_positions.push(self.text.len());
            }
            Update::SearchDone { id, outcome } => {
                if let Some(Step::Search { results, .. }) = self
                    .steps
                    .iter_mut()
                    .find(|s| matches!(s, Step::Search { id: i, results: None, .. } if *i == id))
                {
                    *results = Some(outcome);
                }
            }
            Update::Usage(tokens) => self.context_tokens = Some(tokens),
            Update::Window(tokens) => self.context_window = Some(tokens),
            Update::Count { prompt, completion, cached } => {
                self.prompt_tokens = self.prompt_tokens.saturating_add(prompt);
                self.completion_tokens = self.completion_tokens.saturating_add(completion);
                self.cached_tokens = self.cached_tokens.saturating_add(cached);
            }
            Update::Ask { id, ask } => self.requests.push(Request {
                id,
                at: self.text.len(),
                ask,
                status: RequestStatus::Pending,
                asked_ms: now_ms(),
            }),
            Update::Title(_)
            | Update::Session(_)
            | Update::Wake
            | Update::SteerRefused(_)
            | Update::Accepted => {}
            Update::Tool(call) => {
                let id = call.id.clone();
                let ends = matches!(
                    call.status,
                    ToolStatus::Done
                        | ToolStatus::Failed
                        | ToolStatus::Stopped
                        | ToolStatus::Denied
                ) && self.under_background(&id);
                match self
                    .steps
                    .iter_mut()
                    .find(|s| matches!(s, Step::Tool(existing) if existing.id == id))
                {
                    Some(Step::Tool(existing)) => {
                        let parent = existing.parent.take();
                        let started = existing.started_ms;
                        *existing = call;
                        existing.started_ms = started.or(existing.started_ms);
                        if existing.parent.is_none() {
                            existing.parent = parent;
                        }
                    }
                    Some(_) => {}
                    None => {
                        let mut call = call;
                        call.started_ms = call.started_ms.or_else(now_ms);
                        self.steps.push(Step::Tool(call));
                        self.step_positions.push(self.text.len());
                        self.turn_start = self.text.len();
                    }
                }
                // finish spares rows under background work; nothing else stops them
                let mut gone = if ends { vec![id] } else { Vec::new() };
                while let Some(parent) = gone.pop() {
                    for step in &mut self.steps {
                        if let Step::Tool(c) = step
                            && c.parent.as_deref() == Some(parent.as_str())
                            && matches!(c.status, ToolStatus::Running | ToolStatus::Waiting)
                        {
                            c.status = ToolStatus::Stopped;
                            gone.push(c.id.clone());
                        }
                    }
                }
            }
            Update::Error(error) => {
                self.failed = true;
                self.error = Some(match self.error.take() {
                    Some(first) => first + "\n" + &error,
                    None => error,
                });
                self.turn_start = self.text.len();
            }
            Update::Notice(notice) => {
                match self
                    .steps
                    .iter_mut()
                    .find(|s| matches!(s, Step::Notice(n) if n.id == notice.id))
                {
                    Some(existing) => *existing = Step::Notice(notice),
                    None => {
                        self.steps.push(Step::Notice(notice));
                        self.step_positions.push(self.text.len());
                    }
                }
            }
            Update::Warning(text) => {
                let id = format!("warning-{}", self.steps.len());
                self.apply(Update::Notice(Notice {
                    id,
                    kind: NoticeKind::Warning,
                    text,
                    state: NoticeState::Done,
                    until_ms: None,
                    url: None,
                    detail: None,
                }));
            }
            Update::Steered(text) => {
                self.steps.push(Step::Steer(text));
                self.step_positions.push(self.text.len());
                self.turn_start = self.text.len();
            }
            Update::Cost(micros) => self.cost_micros = self.cost_micros.saturating_add(micros),
            Update::Model(model) => self.served_model = Some(model),
            Update::Mode(_) => {}
            Update::AskResolved(id) => {
                if let Some(r) = self
                    .requests
                    .iter_mut()
                    .find(|r| r.id == id && r.status == RequestStatus::Pending)
                {
                    r.status = RequestStatus::Withdrawn;
                }
            }
        }
    }

    /// Records the user's answer; false if `id` isn't found or already answered here.
    pub fn answer(&mut self, id: u64, answer: Answer) -> bool {
        match self.requests.iter_mut().find(|r| r.id == id) {
            Some(request)
                if request.status == RequestStatus::Pending
                    || request.status == RequestStatus::Expired =>
            {
                request.status = RequestStatus::Answered(answer);
                true
            }
            _ => false,
        }
    }

    pub fn pending_request(&self) -> Option<&Request> {
        self.requests.iter().find(|r| r.status == RequestStatus::Pending)
    }

    pub fn finish(&mut self) {
        self.end_thinking();
        self.close_thinking();
        if !self.restored {
            self.elapsed_ms = Some(self.started.elapsed().as_millis() as u64);
        }
        self.ended_at_ms = now_ms();
        for request in &mut self.requests {
            if request.status == RequestStatus::Pending {
                request.status = RequestStatus::Expired;
            }
        }
        let live: Vec<bool> = self
            .steps
            .iter()
            .map(|s| {
                matches!(s, Step::Tool(c)
                    if c.parent.as_deref().is_some_and(|p| self.under_background(p)))
            })
            .collect();
        let failed = self.failed;
        for (step, live) in self.steps.iter_mut().zip(live) {
            match step {
                Step::Search { results: results @ None, .. } => {
                    *results = Some(Err("stopped".into()))
                }
                Step::Tool(call)
                    if !live
                        && matches!(call.status, ToolStatus::Running | ToolStatus::Waiting) =>
                {
                    call.status = ToolStatus::Stopped
                }
                Step::Notice(n)
                    if n.state == NoticeState::Running && n.kind != NoticeKind::RateLimit =>
                {
                    n.state = if failed { NoticeState::Failed } else { NoticeState::Done }
                }
                _ => {}
            }
        }
    }

    /// Row `id` is a background subagent, or runs live under one.
    fn under_background(&self, id: &str) -> bool {
        let mut id = id;
        for _ in 0..self.steps.len() {
            let Some(call) = self.steps.iter().find_map(|s| match s {
                Step::Tool(c) if c.id == id => Some(c),
                _ => None,
            }) else {
                return false;
            };
            match (call.status, call.parent.as_deref()) {
                (ToolStatus::Background, _) => return true,
                (ToolStatus::Running | ToolStatus::Waiting, Some(parent)) => id = parent,
                _ => return false,
            }
        }
        false
    }

    pub fn interrupt(&mut self, why: &str) {
        for step in &mut self.steps {
            match step {
                Step::Tool(call)
                    if matches!(
                        call.status,
                        ToolStatus::Running | ToolStatus::Waiting | ToolStatus::Background
                    ) =>
                {
                    call.status = ToolStatus::Stopped;
                    call.error.get_or_insert_with(|| why.to_string());
                }
                Step::Search { results: results @ None, .. } => *results = Some(Err(why.into())),
                _ => {}
            }
        }
        for request in &mut self.requests {
            if request.status == RequestStatus::Pending {
                request.status = RequestStatus::Expired;
            }
        }
        self.close_thinking();
    }

    /// The copy written to disk, tool text capped.
    pub fn for_save(&self) -> Self {
        let mut saved = self.clone();
        saved.cap_tool_payload();
        saved
    }

    pub fn unreadable() -> Self {
        let mut m = Self::assistant();
        m.text = "This message was saved by a newer version of Aether and can't be shown.".into();
        m.ended_at_ms = Some(0);
        m
    }

    /// Drops the oldest tool outputs and diffs until the reply's tool text
    /// fits the saved budget; titles, statuses, errors and counts stay.
    fn cap_tool_payload(&mut self) {
        let mut texts: Vec<&mut String> = self
            .steps
            .iter_mut()
            .filter_map(|s| match s {
                Step::Tool(call) => Some(call.detail.payload()),
                _ => None,
            })
            .flatten()
            .collect();
        let mut total: usize = texts.iter().map(|t| t.len()).sum();
        for text in &mut texts {
            if total <= SAVED_TOOL_BYTES {
                break;
            }
            if text.len() > NOT_KEPT.len() {
                total -= text.len() - NOT_KEPT.len();
                **text = NOT_KEPT.to_string();
            }
        }
    }

    fn end_thinking(&mut self) {
        if self.thought_for.is_none() && !self.restored {
            self.thought_for = Some(self.started.elapsed());
        }
    }

    pub fn live_thinking_ms(&self) -> Option<u64> {
        self.thinking_since.map(|t| t.elapsed().as_millis() as u64)
    }

    fn close_thinking(&mut self) {
        if let Some(since) = self.thinking_since.take()
            && let Some(Step::Thinking { ms, .. }) =
                self.steps.iter_mut().rev().find(|s| matches!(s, Step::Thinking { ms: None, .. }))
        {
            *ms = Some(since.elapsed().as_millis() as u64);
        }
    }

    fn begin_turn(&mut self) {
        if self.turn_start > 0 && self.text.len() == self.turn_start {
            self.text.push_str("\n\n");
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
