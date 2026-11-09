use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::font_picker::FontItem;
use crate::git_panel::GitPanel;
use crate::hitl::RequestInput;
use crate::open_in::OpenApp;
use crate::palette::PaletteCommand;
use crate::settings_view::SettingsTab;
use crate::skills_view::SkillEditor;
use crate::suggest::Suggestions;
use crate::usage_view::{UsageBreakdown, UsageMetric, UsageViewMode};
use crate::workspace_popover::{GhState, ScriptRuns, WorkspacePopover};
use aether_agent::usage::PlanUsage;
use aether_agent::{Agent, Cancel, ExternalAgent, SessionHealth, Store};
use aether_core::{
    AccessMode, Ask, Attachment, Message, NoticeKind, Provider, Settings, Skill, SkillSource,
    ToolCall, Watch, home_dir,
};
use aether_driver::ProviderProbe;
use aether_git::{Bases, BranchInfo, Listed, Remote};
use aether_ui::{
    AgentSelectorState, BranchSelectState, ChatScrollerState, ChatStatus, ChoiceMenuState,
    ModelItem, SILENT_MS, TranscriptRow, TriggerBounds, WorkspaceSelectState, background_rows,
    dead_agent_shown, running_notice, schemes,
};
use gpui_kit::component::{
    Theme, ThemeMode,
    command::CommandState,
    input::{EditorState, InputState, TextareaState},
    select::{SearchableVec, SelectState},
    slider::SliderState,
};
use gpui_kit::{App, Context, Entity, FocusHandle, SharedString, Subscription, Window, px};

mod chat;
mod composer;
mod control;
mod draft_editor;
mod format;
mod links;
mod new;
pub(crate) mod orchestrate;
mod providers;
mod render;
mod session_ui;
mod sidebar;
pub(crate) use session_ui::{SessionHandle, SessionState};
mod start;
pub(crate) mod status_bar;
#[cfg(any(target_os = "macos", test))]
pub(crate) mod touch_bar;
mod watch;
pub(crate) use watch::{WATCH_STRIP, watch_item};
mod worktree;

pub(crate) use chat::Bulk;
pub(crate) use format::{chat_markdown, display_path, format_full, format_time, now_ms};
pub(crate) use providers::picker_value;
use sidebar::folder_label;
use worktree::SetupJob;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    Chat,
    Settings,
    Skills,
    Agents,
    Usage,
    Terminal,
    Explorer,
    Project,
    HackerNews,
    Notes,
    Board,
}

pub(crate) const CLIS: [Provider; 5] =
    [Provider::Claude, Provider::Codex, Provider::Pi, Provider::OpenCode, Provider::Antigravity];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum ChatViewMode {
    #[default]
    Chat,
    Explorer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum SidebarView {
    #[default]
    Projects,
    DirectChats,
    Outside,
    Archived,
}

impl SidebarView {
    /// View listing a just-selected chat; Outside keeps chats an outside agent holds.
    fn after_select(self, agent: bool, locked: bool) -> Self {
        match self {
            Self::Outside if locked => Self::Outside,
            _ if agent => Self::Projects,
            _ => Self::DirectChats,
        }
    }
}

pub(crate) struct Chat {
    /// Key in the store; creation time in ms, so it also orders chats.
    pub id: i64,
    pub session: String,
    pub title: SharedString,
    pub messages: Vec<Message>,
    pub pinned: bool,
    pub archived: bool,
    pub provider: Provider,
    pub provider_model: String,
    pub cancel: Option<Cancel>,
    pub open_steps: HashMap<usize, bool>,
    /// Bumped per send; updates from an older run are ignored.
    run: u64,
    /// Bumped when the chat is rewound; older runs leave its messages alone.
    pub(crate) epoch: u64,
    /// How the last run ended, until the user opens the chat.
    unseen: ChatStatus,
    /// Folder the CLI runs in; empty until the first send fixes it.
    cwd: String,
    pub(crate) view: ChatViewMode,
    pub(crate) access: AccessMode,
    pub(crate) effort: String,
    /// Provider's own resume id for this chat's CLI session.
    pub(crate) cli_session: String,
    pub queue: Vec<Queued>,
    pub stopped: bool,
    /// Outside agent this chat was opened from; read-only while it runs.
    pub external_pid: Option<u32>,
    /// When the latest turn was sent, in ms; 0 until one is sent this run.
    pub(crate) turn_at: i64,
    /// Folder the chat started in, once it runs in a worktree; empty otherwise.
    project: String,
    /// Worktree the chat runs in and the branch made for it; empty for none.
    worktree_path: String,
    worktree_branch: String,
    /// Lead chat that spawned this one.
    pub(crate) parent: Option<i64>,
    pub(crate) lead: bool,
    /// Newest reply of this child its lead has read with read_agent.
    pub(crate) lead_seen: Option<usize>,
    /// Chat this one was handed off from.
    pub(crate) handoff_from: Option<i64>,
    /// Draft only: base branch for a worktree made at first send; `None` works locally.
    pub(crate) new_worktree: Option<String>,
    /// Draft only: live worktree to share at first send; `None` unless picked. Disarms `new_worktree`.
    pub(crate) new_worktree_existing: Option<PathBuf>,
    worktree_job: Option<WorktreeJob>,
    /// Why making the worktree failed; shown above the composer until dismissed.
    worktree_error: Option<SharedString>,
    /// Something to know about the new worktree, shown until dismissed.
    worktree_info: Option<SharedString>,
    setup: Option<SetupJob>,
    /// Worktree aether just made for this chat; bound to it, the first prompt waits for setup.
    pub(crate) fresh_worktree: Option<PathBuf>,
    /// Composer text while the chat is not the active one.
    pub(crate) draft: String,
    pub(crate) pending: Vec<Attachment>,
    /// Draft set aside while editing a sent message.
    pub(crate) edit_stash: Option<(String, Vec<Attachment>)>,
    /// Unix ms the running turn's last update was applied; 0 when idle.
    pub(crate) last_event_ms: i64,
    /// Unix ms of the last mid-turn save.
    saved_ms: i64,
    /// Access mode the CLI reports it runs in.
    pub(crate) mode_in_effect: Option<AccessMode>,
    /// The last health check of the chat's CLI and when it ran, in ms.
    pub(crate) health: Option<(SessionHealth, i64)>,
    /// The `last_event_ms` at which the user chose to keep waiting.
    pub(crate) stall_kept_ms: i64,
    /// Unix ms the user last restarted the chat's CLI.
    pub(crate) restarted_ms: i64,
    /// Cached [`Chat::has_live_background`] verdict with the message/steps shape it was computed from.
    bg_live: Cell<Option<((usize, usize), bool)>>,
}

/// What a chat's sidebar status is decided from.
#[derive(Clone, Copy)]
pub(crate) struct StatusFacts {
    pub busy: bool,
    pub ask: bool,
    pub started: bool,
    pub retrying: bool,
    pub last_event_ms: i64,
    pub turn_at: i64,
    pub background: usize,
    pub unseen: ChatStatus,
}

pub(crate) fn chat_status(f: &StatusFacts, now_ms: i64) -> ChatStatus {
    if f.busy {
        let anchor = f.last_event_ms.max(f.turn_at);
        let quiet = now_ms - anchor >= SILENT_MS as i64;
        return if f.ask {
            ChatStatus::Waiting
        } else if !f.started && !quiet {
            ChatStatus::Starting
        } else if f.retrying {
            ChatStatus::Retrying
        } else if quiet {
            ChatStatus::Quiet { since_ms: anchor }
        } else {
            ChatStatus::Working { since_ms: f.turn_at }
        };
    }
    if f.unseen == ChatStatus::Failed {
        ChatStatus::Failed
    } else if f.background > 0 {
        ChatStatus::Background(f.background)
    } else if f.unseen == ChatStatus::Done {
        ChatStatus::Done
    } else {
        ChatStatus::Idle
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Queued {
    pub text: String,
    pub attachments: Vec<Attachment>,
    /// Handed to the CLI as a steer, waiting for `Steered`.
    pub delivering: bool,
    /// Child agents' results for a lead; later ones merge into it.
    pub from_children: bool,
    pub deliveries: Vec<Delivery>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Delivery {
    pub child: i64,
    /// `None` for setup failures, which are never dropped as read.
    pub reply_ix: Option<usize>,
    pub text: String,
}

#[derive(Clone, Debug)]
enum WorktreeJob {
    /// Making one from this base branch before the first send; set the flag to stop.
    Creating(String, Arc<AtomicBool>),
    Checking,
    Removing,
    Switching,
}

/// Sidebar group of a CLI chat: the folder it started in, even once it runs in a worktree.
fn project_dir(chat: &Chat, workspace_root: &str) -> PathBuf {
    if chat.project.is_empty() {
        chat_cwd(chat, workspace_root)
    } else {
        chat.project.clone().into()
    }
}

fn chat_cwd(chat: &Chat, workspace_root: &str) -> std::path::PathBuf {
    if !chat.cwd.is_empty() {
        chat.cwd.clone().into()
    } else if !workspace_root.is_empty() {
        workspace_root.into()
    } else {
        home_dir()
    }
}

impl Chat {
    fn workspace_draft_ready(&self) -> bool {
        self.messages.is_empty()
            && self.cancel.is_none()
            && self.worktree_job.is_none()
            && !self.worktree_bound()
    }

    /// Model this chat sends with. Legacy OpenRouter chats saved none and
    /// use the global one.
    pub(crate) fn model(&self, settings: &Settings) -> String {
        match self.provider {
            Provider::OpenRouter if self.provider_model.is_empty() => settings.model.clone(),
            Provider::OpenCode if self.provider_model == "openrouter/meta/muse-spark-open" => {
                String::new()
            }
            _ => self.provider_model.clone(),
        }
    }

    fn context_used(&self) -> u64 {
        self.messages
            .iter()
            .rev()
            .find_map(|m| {
                m.context_tokens.filter(|&t| t > 0).or_else(|| {
                    (m.prompt_tokens + m.completion_tokens > 0)
                        .then_some(m.prompt_tokens + m.completion_tokens)
                })
            })
            .unwrap_or(0)
    }

    /// A reply streams, or a steer may still start its own turn.
    pub(crate) fn busy(&self) -> bool {
        self.cancel.is_some() || self.queue.iter().any(|q| q.delivering)
    }

    /// Kind of reply the pending request needs, for the project page.
    pub(crate) fn wait_label(&self) -> &'static str {
        match self.messages.last().and_then(Message::pending_request).map(|r| &r.ask) {
            Some(Ask::Question { .. } | Ask::Questions(_) | Ask::Form(_)) => "Answer",
            Some(Ask::Plan(_)) => "Review",
            _ => "Approve",
        }
    }

    pub(crate) fn status(&self, now_ms: i64) -> ChatStatus {
        let last = self.messages.last();
        let busy = self.cancel.is_some();
        let retrying = last.is_some_and(|m| running_notice(m, &[NoticeKind::Retry]));
        // chat_status ignores background while busy, so don't scan for it then
        let background = if busy { 0 } else { self.background_count() };
        let facts = StatusFacts {
            busy,
            ask: last
                .and_then(Message::pending_request)
                .is_some_and(|r| !matches!(r.ask, Ask::Unknown)),
            started: self.last_event_ms > 0,
            retrying,
            last_event_ms: self.last_event_ms,
            turn_at: self.turn_at,
            background,
            unseen: self.unseen,
        };
        chat_status(&facts, now_ms)
    }

    /// Message count + total steps: changes whenever a step is added or a message lands.
    fn shape(&self) -> (usize, usize) {
        (self.messages.len(), self.messages.iter().map(|m| m.steps.len()).sum())
    }

    /// Whether any live background row exists, without rescanning steps on every tick/render.
    /// `Some(false)` is trusted only for the shape it was computed from; `true` always rescans
    /// since a background row may finish at any time. Turn end invalidates via `invalidate_bg`,
    /// which covers in-place Running→Background flips mid-turn.
    pub(crate) fn has_live_background(&self) -> bool {
        let shape = self.shape();
        if let Some((s, false)) = self.bg_live.get()
            && s == shape
        {
            return false;
        }
        let has = self.live_background().next().is_some();
        self.bg_live.set(Some((shape, has)));
        has
    }

    pub(crate) fn background_count(&self) -> usize {
        if !self.has_live_background() {
            return 0;
        }
        let n = self.live_background().count();
        self.bg_live.set(Some((self.shape(), n > 0)));
        n
    }

    pub(crate) fn invalidate_bg(&self) {
        self.bg_live.set(None);
    }

    /// Top-level rows still running in the background, in any reply of this run. A restored
    /// reply's count only while it is the last one, the one the driver reattaches to settle.
    pub(crate) fn live_background(&self) -> impl Iterator<Item = &ToolCall> {
        let reattached = chat::reattach_wanted(self).then(|| self.messages.len() - 1);
        self.messages
            .iter()
            .enumerate()
            .filter(move |&(ix, m)| !m.restored || Some(ix) == reattached)
            .flat_map(|(_, m)| background_rows(m))
    }

    /// The CLI died on the last reply, by the cached health check; never while a TUI holds it.
    pub(crate) fn dead_bar(&self, locked: bool) -> bool {
        dead_agent_shown(
            self.is_agent() && !self.busy() && !locked,
            self.health.is_some_and(|(h, _)| h.alive),
            self.messages.last(),
            self.restarted_ms,
        )
    }

    /// Latest of creation, the last sent turn and the last finished reply.
    pub(crate) fn activity(&self) -> i64 {
        // replies finish in order, so the last stamped one is the newest
        let reply = self.messages.iter().rev().find_map(|m| m.ended_at_ms);
        self.id.max(self.turn_at).max(reply.unwrap_or(0))
    }

    /// Folder a CLI chat runs in; `None` for OpenRouter chats.
    pub(crate) fn folder(&self, workspace_root: &str) -> Option<PathBuf> {
        (self.provider != Provider::OpenRouter).then(|| chat_cwd(self, workspace_root))
    }

    pub(crate) fn is_agent(&self) -> bool {
        self.provider != Provider::OpenRouter
    }

    /// Project name of a CLI chat, so search finds chats by project.
    pub(crate) fn folder_name(&self, workspace_root: &str) -> Option<String> {
        self.folder(workspace_root)
            .map(|_| folder_label(&project_dir(self, workspace_root).display().to_string()))
    }

    /// Bound to a worktree, now or before it was removed; its folder is fixed.
    fn worktree_bound(&self) -> bool {
        !self.worktree_path.is_empty() || !self.worktree_branch.is_empty()
    }

    /// Its worktree was removed; the branch name stays for the record.
    pub(crate) fn worktree_removed(&self) -> bool {
        self.worktree_path.is_empty() && !self.worktree_branch.is_empty()
    }

    /// Bound to a worktree that still exists, as far as the last check knows.
    pub(crate) fn has_worktree(&self, missing: &HashSet<PathBuf>, workspace_root: &str) -> bool {
        !self.worktree_path.is_empty() && !missing.contains(&chat_cwd(self, workspace_root))
    }

    /// Draft toggle: the first send makes a worktree from `base`.
    fn arm_new_worktree(&mut self, base: String) {
        self.new_worktree = Some(base);
        self.new_worktree_existing = None;
    }

    /// Draft toggle: the first send shares the live worktree at `path`.
    fn arm_existing_worktree(&mut self, path: PathBuf) {
        self.new_worktree = None;
        self.new_worktree_existing = Some(path);
    }

    /// Draft toggle: the first send works locally.
    fn arm_local_worktree(&mut self) {
        self.new_worktree = None;
        self.new_worktree_existing = None;
    }

    pub(crate) fn access_mode(&self) -> AccessMode {
        self.access
    }

    pub(crate) fn effort_level(&self) -> &str {
        &self.effort
    }

    pub(crate) fn cwd_str(&self) -> &str {
        &self.cwd
    }

    pub(crate) fn project_str(&self) -> &str {
        &self.project
    }

    pub(crate) fn worktree_path_str(&self) -> &str {
        &self.worktree_path
    }

    pub(crate) fn worktree_branch_str(&self) -> &str {
        &self.worktree_branch
    }
}

pub(crate) struct ChatApp {
    pub(crate) image_viewers:
        HashMap<gpui_kit::WindowId, (Entity<crate::image_viewer::ImageViewer>, Subscription)>,
    chat_id_highwater: i64,
    stopped_by_lead: HashSet<i64>,
    spawn_nonces: HashMap<(i64, String), serde_json::Value>,
    /// (lead id, spawn_agent args) waiting for a free agent slot, oldest first.
    // ponytail: in memory only; a restart drops queued spawns
    pub(crate) spawn_queue: Vec<(i64, serde_json::Value)>,
    /// Highest card id ever handed out; new cards count up from it.
    pub(crate) card_seq: i64,
    pub(crate) session_views: HashMap<i64, SessionState>,
    pub(crate) session_subscriptions: HashMap<i64, Vec<Subscription>>,
    pub(crate) session_windows: HashMap<i64, gpui_kit::WindowId>,
    pub(crate) ui_owner: Option<i64>,
    pub(crate) workspace: crate::session_workspace::Workspace,
    pub(crate) agent: Agent,
    pub(crate) store: Option<Store>,
    pub(crate) cards: Vec<crate::board::model::BoardCard>,
    pub(crate) board_selected: Option<i64>,
    pub(crate) board_title: Entity<InputState>,
    pub(crate) board_body: Entity<TextareaState>,
    pub(crate) chats: Vec<Chat>,
    pub(crate) active: usize,
    /// Chat whose text the composer holds; by id, as indices shift.
    pub(crate) draft_owner: Option<i64>,
    pub(crate) composer: Entity<TextareaState>,
    pub(crate) draft_editor: Option<Entity<EditorState>>,
    pub(crate) scroller: Entity<ChatScrollerState>,
    /// The active chat's transcript, one virtual-list row per block.
    pub(crate) rows: Vec<TranscriptRow>,
    folded: bool,
    selector: Entity<AgentSelectorState>,
    access_menu: Entity<ChoiceMenuState>,
    pub(crate) project_menu: Entity<ChoiceMenuState>,
    pub(crate) headline_project_menu: Entity<ChoiceMenuState>,
    pub(crate) switcher_folders: Rc<RefCell<Vec<PathBuf>>>,
    worktree_menu: Entity<WorkspaceSelectState>,
    pub(crate) open_apps: Vec<OpenApp>,
    pub(crate) open_error: Option<SharedString>,
    pub(crate) default_model: Entity<AgentSelectorState>,
    pub(crate) binary_inputs: HashMap<Provider, Entity<InputState>>,
    pub(crate) model_inputs: HashMap<Provider, Entity<InputState>>,
    context_lengths: HashMap<String, u64>,
    claude_plan_usage: Option<PlanUsage>,
    provider_plan_usages: HashMap<Provider, PlanUsage>,
    /// Unix ms when each provider's usage last arrived.
    plan_usage_at: HashMap<Provider, i64>,
    usage_attempt_at: i64,
    /// Subscribed provider found at startup; used while `settings.provider` is unset.
    detected_provider: Option<Provider>,
    pub(crate) sys_sample: Option<crate::sysinfo::Sample>,
    pub(crate) usage_pop: status_bar::UsagePop,
    pub(crate) crew_pop: bool,
    pub(crate) crew_pop_trigger: TriggerBounds,
    pub(crate) show_usage_limits: bool,
    /// Dollars per token per model, from OpenRouter's model list.
    model_prices: HashMap<String, (f64, f64)>,
    pub(crate) settings: Settings,
    api_key: Option<String>,
    pub(crate) key_status: SharedString,
    pub(crate) page: Page,
    pub(crate) instructions: Entity<TextareaState>,
    pub(crate) key_input: Entity<InputState>,
    pub(crate) terminal_font_input: Entity<InputState>,
    pub(crate) renaming: Option<usize>,
    /// The rename field sits in the chat's tab instead of its sidebar row.
    pub(crate) rename_in_tab: bool,
    pub(crate) rename_input: Entity<InputState>,
    pub(crate) worktree_name_input: Entity<InputState>,
    pub(crate) worktree_base_select: Option<Entity<BranchSelectState>>,
    pub(crate) pending_worktree_dialog: bool,
    pub(crate) pending_worktree_dialog_base: Option<String>,
    pub(crate) models: Vec<ModelItem>,
    pub(crate) probes: HashMap<Provider, ProviderProbe>,
    probe_runs: HashMap<Provider, u64>,
    /// Git branch per agent folder, filled in the background.
    pub(crate) branches: HashMap<PathBuf, Option<BranchInfo>>,
    pub(crate) missing_folders: HashSet<PathBuf>,
    pub(crate) missing_attachments: HashSet<PathBuf>,
    refreshing_branches: bool,
    /// A refresh was asked for while one ran; run a full pass after it.
    branches_stale: bool,
    /// Base branches per draft folder; `None` outside a git repo.
    bases: HashMap<PathBuf, Option<Bases>>,
    loading_bases: HashSet<PathBuf>,
    /// Chats whose Background panel the user hid; cleared once their tasks end.
    pub(crate) background_hidden: HashSet<i64>,
    /// (chat id, tool id) the stall line asked the Background panel to show.
    pub(crate) inspect: Option<(i64, String)>,
    /// (chat id, text) of the queued message shown whole in the Background panel.
    pub(crate) inspect_queued: Option<(i64, String)>,
    /// (chat, text) of the steer message the side panel shows in full.
    pub(crate) inspect_steer: Option<(i64, String)>,
    /// A Background panel edge drag is under way; its end saves the width.
    pub(crate) background_resizing: bool,
    /// Worktree paths being checked or removed.
    pub(crate) removing: HashSet<PathBuf>,
    /// The Settings cleanup list; `None` until first loaded.
    pub(crate) worktrees: Option<Vec<Listed>>,
    pub(crate) listing_worktrees: bool,
    pub(crate) external: Vec<ExternalAgent>,
    scan_run: u64,
    pub(crate) scanning: bool,
    pub(crate) agents_status: Option<String>,
    pub(crate) palette: Entity<CommandState>,
    pub(crate) palette_query: Entity<InputState>,
    pub(crate) palette_open: bool,
    pub(crate) watch_open: bool,
    pub(crate) watch_peek: Option<Watch>,
    /// Peek bounds while a drag or resize is under way; its end saves them.
    pub(crate) watch_peek_live: Option<[f32; 4]>,
    pub(crate) palette_restore: Option<FocusHandle>,
    pub(crate) palette_rows: Vec<Vec<Option<PaletteCommand>>>,
    pub(crate) skills: Vec<Skill>,
    pub(crate) skill_tab: SkillSource,
    pub(crate) settings_tab: SettingsTab,
    pub(crate) settings_provider: Provider,
    pub(crate) notes_advanced_open: bool,
    pub(crate) settings_search_input: Entity<InputState>,
    pub(crate) font_select: Entity<SelectState<SearchableVec<FontItem>>>,
    pub(crate) opacity_slider: Entity<SliderState>,
    pub(crate) settings_sidebar_focus: FocusHandle,
    pub(crate) skill_search: Entity<InputState>,
    pub(crate) skill_editor: Option<SkillEditor>,
    pub(crate) suggestions: Option<Suggestions>,
    pub(crate) suggestions_dismissed: Option<(usize, char)>,
    pub(crate) request_inputs: HashMap<u64, RequestInput>,
    pub(crate) request_subscriptions: HashMap<u64, Subscription>,
    /// Option indices picked per question or form field.
    pub(crate) request_picks: HashMap<u64, Vec<BTreeSet<usize>>>,
    /// Why a form's last submit was refused.
    pub(crate) request_errors: HashMap<u64, String>,
    /// Plan requests switched from the list view to the editor.
    pub(crate) editing_plans: HashSet<u64>,
    /// Chat id and index of the user message being edited.
    editing: Option<(i64, usize)>,
    pub(crate) composer_history_index: Option<usize>,
    pub(crate) composer_draft: Option<String>,
    pub(crate) attach_error: Option<SharedString>,
    /// Attachment copies still in flight, per chat id; Send waits for them, and pasted
    /// images count them so their numbers stay distinct.
    pub(crate) attaching: HashMap<i64, usize>,
    /// Reply text each chat's next message quotes, by chat id.
    quotes: HashMap<i64, SharedString>,
    /// In-memory `/btw` thread per chat id; one thread, many follow-ups; never saved.
    pub(crate) btw: HashMap<i64, chat::BtwThread>,
    /// Whether the BTW panel shows (right sidebar in Tabbed, a tab otherwise).
    pub(crate) btw_open: bool,
    /// Window controls ride the main window's top-right tab bar instead of a title bar row.
    pub(crate) tab_bar_controls: bool,
    pub(crate) btw_input: Entity<TextareaState>,
    /// Chat, window point and text of the reply selection the toolbar acts on.
    pub(crate) file_selection_bar: Option<(gpui_kit::Point<gpui_kit::Pixels>, std::path::PathBuf)>,
    selection_bar: Option<(i64, gpui_kit::Point<gpui_kit::Pixels>, SharedString)>,
    /// (chat, message) whose Copy button reads "Copied" until the timer clears it.
    copied: Option<(usize, usize)>,
    sessions_made: usize,
    focus: FocusHandle,
    sidebar_resizing: bool,
    /// Width (stacked: height) of the explorer's file tree, set by dragging its divider.
    pub(crate) explorer_tree_size: f32,
    pub(crate) explorer_tree_collapsed: bool,
    pub(crate) sidebar_view: SidebarView,
    /// Terminal drawer per chat id, made on first open.
    pub(crate) terminals: HashMap<i64, crate::terminal::Drawer>,
    pub(crate) global_terminal: Option<crate::terminal::Drawer>,
    pub(crate) active_terminal_tab: aether_terminal::TerminalTab,
    pub(crate) git: GitPanel,
    pub(crate) browser: Entity<crate::browser::Browser>,
    pub(crate) browser_open: bool,
    pub(crate) diff_open: bool,
    pub(crate) explorer_view: crate::explorer::ExplorerView,
    pub(crate) side_tab: crate::side_panel::SideTab,
    pub(crate) right_surface: Option<crate::side_panel::Surface>,
    pub(crate) right_review_open: bool,
    pub(crate) right_browser_open: bool,
    pub(crate) board_tab: bool,
    pub(crate) board_all: bool,
    pub(crate) board_search: Entity<InputState>,
    /// Author filter: `None` all, `Some(true)` agent-added, `Some(false)` user-added.
    pub(crate) board_by: Option<bool>,
    /// Cards picked for a bulk delete.
    pub(crate) board_picks: HashSet<i64>,
    pub(crate) archive_picks: HashSet<i64>,
    pub(crate) project_picks: HashSet<i64>,
    pub(crate) right_panel_resizing: bool,
    /// Detached session windows' tabs; main keeps `side_tab`.
    pub(crate) side_tabs: HashMap<crate::session_workspace::WindowId, crate::side_panel::SideTab>,
    /// The tab the Agent tab was picked from, shown when Agent goes away.
    pub(crate) side_back: crate::side_panel::SideTab,
    /// The open side panel fills the main area.
    pub(crate) side_full: bool,
    /// The chat's terminal was moved from the drawer into a side panel tab.
    pub(crate) terminal_tab: bool,
    /// The reply each chat's Diff tab shows.
    pub(crate) diff_targets: HashMap<i64, crate::side_panel::DiffTarget>,
    pub(crate) diff_focus: FocusHandle,
    pub(crate) project_explorers: HashMap<PathBuf, Entity<aether_explorer::ExplorerState>>,
    pub(crate) explorer_prev_page: Page,
    pub(crate) project_page: Option<PathBuf>,
    pub(crate) project_view: crate::project_page::ProjectPageState,
    pub(crate) project_open_menu: Entity<ChoiceMenuState>,
    pub(crate) explorer_quick_open_input: Entity<InputState>,
    pub(crate) quick_open_restore: Option<FocusHandle>,
    pub(crate) quick_open_scroll: gpui_kit::ScrollHandle,
    pub(crate) explorer_grep_input: Entity<InputState>,
    /// Each open explorer tab's editor and its change subscription, by tab id.
    pub(crate) explorer_markdown_source: HashSet<u64>,
    pub(crate) explorer_editors: HashMap<u64, (Entity<EditorState>, Subscription)>,
    /// What `explorer_diff` was read for: the file, its git change and the status read time.
    pub(crate) explorer_diff_key: Option<(PathBuf, aether_git::Change, i64)>,
    /// The active explorer file's parsed diff; `None` while the first read for it runs.
    pub(crate) explorer_diff: Option<Result<Rc<Vec<aether_ui::DiffRow>>, SharedString>>,
    pub(crate) usage_view: UsageViewMode,
    pub(crate) usage_window: aether_agent::usage_history::UsageWindow,
    pub(crate) usage_metric: UsageMetric,
    pub(crate) usage_breakdown: UsageBreakdown,
    pub(crate) usage_history: Option<aether_agent::usage_history::UsageHistory>,
    pub(crate) usage_history_scanned_at: Option<std::time::Instant>,
    pub(crate) usage_history_pending_for: Option<aether_agent::usage_history::UsageWindow>,
    pub(crate) usage_history_generation: u64,
    pub(crate) usage_chart_bounds: Rc<Cell<Option<gpui_kit::Bounds<gpui_kit::Pixels>>>>,
    pub(crate) usage_chart_hover: Option<usize>,
    pub(crate) usage_project_filter: Entity<InputState>,
    pub(crate) usage_window_menu: Entity<ChoiceMenuState>,
    pub(crate) usage_expanded_models: Option<SharedString>,
    pub(crate) workspace_pop: WorkspacePopover,
    /// Remotes per git folder, read when the workspace popover opens; `None`: the read failed.
    pub(crate) remotes: HashMap<PathBuf, Option<Vec<Remote>>>,
    pub(crate) gh: Option<GhState>,
    pub(crate) script_runs: ScriptRuns,
    pub(crate) voice: composer::voice::VoiceState,
    pub(crate) voice_note: Option<SharedString>,
    pub(crate) notes: crate::notes::NotesModel,
    pub(crate) notes_setting_inputs: std::collections::BTreeMap<&'static str, Entity<InputState>>,
    pub(crate) hackernews: crate::hackernews::HackerNewsModel,
    pub(crate) hn_browser: Entity<crate::browser::Browser>,
    pub(crate) hn_reader_open: bool,
    pub(crate) hn_search_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl ChatApp {
    pub(crate) fn open_project_page(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        self.project_page = Some(folder.clone());
        self.project_picks.clear();
        self.open_page(Page::Project, cx);
        self.load_project_data(folder, cx);
    }

    pub(crate) fn open_page(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page == Page::Board && page != Page::Board {
            self.board_commit(cx);
        }
        self.open_error = None;
        if page == Page::Agents {
            self.scan_agents(cx);
        }
        if page == Page::Skills {
            self.skills = aether_agent::skills::load_all();
            self.skill_editor = None;
        }
        if page == Page::Usage {
            self.ensure_usage_history(false, cx);
        }
        if page != Page::Chat
            && let Some(window) =
                cx.active_window().or_else(|| self.workspace.windows.get(&0).copied())
        {
            let this = cx.entity();
            cx.defer(move |cx| {
                let _ = window.update(cx, |_, window, cx| {
                    this.update(cx, |app, cx| {
                        app.close_draft_editor(true, window, cx);
                        app.end_hidden_voice(window, cx);
                    })
                });
            });
        }
        self.page = page;
        if page == Page::Settings {
            let focus = self.settings_sidebar_focus.clone();
            let search = self.settings_search_input.clone();
            let window = cx.active_window().or_else(|| self.workspace.windows.get(&0).copied());
            cx.defer(move |cx| {
                if let Some(window) = window {
                    let _ = window.update(cx, |_, window, cx| {
                        search.update(cx, |input, cx| input.set_value("", window, cx));
                        focus.focus(window, cx);
                    });
                }
            });
            if self.settings_tab == SettingsTab::Worktrees {
                self.load_worktrees(cx);
            }
        }
        self.workspace_pop.dismiss();
        for selector in [&self.selector, &self.default_model] {
            selector.update(cx, |s, cx| s.dismiss(cx));
        }
        self.worktree_menu.update(cx, |s, cx| s.dismiss(cx));
        for menu in [
            &self.access_menu,
            &self.project_menu,
            &self.headline_project_menu,
            &self.usage_window_menu,
            &self.project_open_menu,
        ] {
            menu.update(cx, |s, cx| s.dismiss(cx));
        }
        cx.notify();
    }
}

pub(crate) fn apply_look(settings: &Settings, window: &mut Window, cx: &mut App) {
    let a = settings.appearance;
    schemes::install_scheme(a, cx);
    cx.set_window_appearance(schemes::forced_appearance(a));
    match a.is_dark() {
        None => Theme::sync_system_appearance(Some(window), cx),
        Some(true) => Theme::change(ThemeMode::Dark, Some(window), cx),
        Some(false) => Theme::change(ThemeMode::Light, Some(window), cx),
    }
    let family = if settings.ui_font_family.trim().is_empty() {
        ".SystemUIFont".into()
    } else {
        aether_terminal::view::resolve_terminal_family(
            &settings.ui_font_family,
            aether_terminal::view::installed_font_names_cached(cx),
            ".SystemUIFont",
        )
    };
    let scale = settings.corner_style.scale();
    aether_ui::theme::set_corner_scale(scale);
    Theme::update(cx, |t| {
        t.font_family = family;
        // radius 0 makes kit radius_full() 0, squares pills. tiny floor still renders square
        t.radius = px((6. * scale).max(0.01));
        t.radius_lg = px(8. * scale);
    });
    let op = settings.clamped_window_opacity();
    Theme::update(cx, |t| aether_ui::theme::apply_surface_opacity(&mut t.colors, op));
    let background = aether_ui::theme::window_background(op, settings.window_blur);
    window.set_background_appearance(background);
    for handle in cx.windows() {
        handle.update(cx, |_, window, _| window.set_background_appearance(background)).ok();
    }
}

pub(crate) fn apply_font_size(size: f32, window: &mut Window) {
    window.set_rem_size(px(size.clamp(12.0, 24.0)));
}

#[cfg(test)]
use aether_core::Update;

#[cfg(test)]
pub(super) fn chat_with(title: &str, texts: &[&str]) -> Chat {
    let mut messages = Vec::new();
    for (i, text) in texts.iter().enumerate() {
        let mut m = if i % 2 == 0 { Message::user(*text) } else { Message::assistant() };
        if i % 2 == 1 {
            m.apply(Update::Final((*text).into()));
        }
        messages.push(m);
    }
    Chat {
        id: 1,
        session: "s".into(),
        title: title.into(),
        messages,
        pinned: false,
        archived: false,
        provider: Provider::OpenRouter,
        provider_model: String::new(),
        cancel: None,
        open_steps: HashMap::new(),
        run: 0,
        epoch: 0,
        unseen: ChatStatus::Idle,
        cwd: String::new(),
        view: ChatViewMode::Chat,
        access: AccessMode::Ask,
        effort: String::new(),
        cli_session: String::new(),
        queue: Vec::new(),
        stopped: false,
        external_pid: None,
        turn_at: 0,
        project: String::new(),
        worktree_path: String::new(),
        worktree_branch: String::new(),
        new_worktree: None,
        new_worktree_existing: None,
        worktree_job: None,
        worktree_error: None,
        worktree_info: None,
        setup: None,
        fresh_worktree: None,
        draft: String::new(),
        pending: Vec::new(),
        edit_stash: None,
        last_event_ms: 0,
        saved_ms: 0,
        mode_in_effect: None,
        health: None,
        stall_kept_ms: 0,
        restarted_ms: 0,
        bg_live: Cell::new(None),
        parent: None,
        lead: false,
        lead_seen: None,
        handoff_from: None,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/mod.rs"]
mod tests;
