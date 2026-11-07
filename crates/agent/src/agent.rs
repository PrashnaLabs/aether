use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use adk_rust::prelude::*;
use adk_rust::session::{CreateRequest, DeleteRequest, SessionService};
use aether_core::{
    AccessMode, Answer, Attachment, MutexExt, NoticeKind, Provider, Role, Settings, Skill,
    ToolStatus, Update, attached_files_block, attachments_dir, find_binary, mentioned_skills,
    native_image_split,
};
use aether_driver::{
    AntigravityDriver, ClaudeDriver, CodexDriver, Driver, DriverStartOptions, LeadTools,
    OpencodeDriver, PiDriver, ProviderProbe, catalog, opencode,
};
use aether_git::worktree::{self, Bases, Listed, RemoveCheck, RemoveError, Start, Worktree};
use aether_git::{self as git, BranchInfo};
use futures::StreamExt;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::runtime::Runtime;
use tokio::task::AbortHandle;

use crate::external::{self, ExternalAgent};
use crate::gate::{AlwaysAllowed, Gate, Pending};
use crate::models::{self, ModelInfo};
use crate::orchestrator::server::{Leads, listen};
use crate::orchestrator::{OrchCall, OrchPending, socket_path};
use crate::store::Store;
use crate::tools::Tools;
use crate::turn::Turn;

/// `is_dir` that gives up after `timeout`, since a dead network mount can block it.
fn dir_exists(dir: &Path, timeout: Duration) -> Option<bool> {
    let (tx, rx) = std::sync::mpsc::channel();
    let dir = dir.to_path_buf();
    std::thread::spawn(move || tx.send(dir.is_dir()));
    rx.recv_timeout(timeout).ok()
}

pub(crate) const APP: &str = "aether";
pub(crate) const USER: &str = "user";
pub(crate) const AGENT: &str = "assistant";

pub(crate) type Factory = Arc<
    dyn Fn(Provider, DriverStartOptions, UnboundedSender<Update>) -> Box<dyn Driver> + Send + Sync,
>;

/// Per-chat inputs of a CLI session; the app owns them.
pub struct ChatSession {
    pub key: String,
    pub cwd: PathBuf,
    pub access: AccessMode,
    pub effort: String,
    pub resume: String,
    pub lead: bool,
    pub ephemeral: bool,
}

/// What a live CLI session can't change without a restart; model and effort ride on each prompt.
#[derive(Clone, Debug, PartialEq)]
struct SessionKey {
    provider: Provider,
    binary: String,
    cwd: PathBuf,
    access: AccessMode,
    lead: bool,
}

enum Command {
    /// `history` seeds a fresh session; `resume` restarts a dead or respawned one.
    Prompt {
        text: String,
        attachments: Vec<Attachment>,
        history: Vec<(Role, String)>,
        resume: String,
        model: String,
        effort: String,
    },
    /// Text and files for the running turn, the reply a prompt streams into if the
    /// turn already ended, and the resume id.
    Steer(String, Vec<Attachment>, UnboundedSender<Update>, String),
    Cancel,
    Answer(u64, Answer),
    /// Stops a background task or terminal by tool row id.
    StopTool(String),
    NoticeAction(String),
    /// A native side question and the reply its answer goes to.
    Side(String, UnboundedSender<Update>),
}

pub const SIDE_UNSUPPORTED: &str = "native side questions are unsupported";

type TurnTx = Arc<Mutex<Option<UnboundedSender<Update>>>>;

struct Live {
    key: SessionKey,
    commands: std::sync::mpsc::Sender<Command>,
    turn: TurnTx,
    last_active: Arc<Mutex<Instant>>,
    holds: Arc<Mutex<HashMap<String, bool>>>,
    health: Arc<Mutex<SessionHealth>>,
}

#[derive(Clone, Copy)]
pub struct SessionHealth {
    pub pid: Option<u32>,
    pub alive: bool,
}

/// Owns the async runtime and chat sessions so the UI never touches tokio.
pub struct Agent {
    rt: Runtime,
    sessions: Arc<InMemorySessionService>,
    tools: Tools,
    pending: Pending,
    always_allowed: Arc<Mutex<HashMap<String, AlwaysAllowed>>>,
    next_request: Arc<AtomicU64>,
    live: Arc<Mutex<HashMap<String, Live>>>,
    cli_asks: Arc<Mutex<HashMap<u64, (String, u64)>>>,
    factory: Factory,
    side: UnboundedSender<SideUpdate>,
    side_rx: Mutex<Option<UnboundedReceiver<SideUpdate>>>,
    leads: Leads,
    orch: UnboundedSender<OrchCall>,
    orch_rx: Mutex<Option<UnboundedReceiver<OrchCall>>>,
    orch_pending: OrchPending,
    #[cfg(test)]
    idle: Duration,
}

/// What a CLI sends while no turn of its chat runs, keyed by chat.
pub enum SideUpdate {
    /// A title, resume id, late tool, notice, resolved ask, usage or error.
    Meta(String, Box<Update>),
    /// A turn the CLI started on its own; stream it like a reply.
    Turn(String, Reply),
}

/// A reply being generated: its update stream and a way to stop it.
pub struct Reply {
    pub updates: UnboundedReceiver<Update>,
    pub cancel: Cancel,
}

pub struct Cancel(CancelInner);

enum CancelInner {
    Task(AbortHandle),
    Cli(std::sync::mpsc::Sender<Command>),
}

impl Cancel {
    /// A handle that stops nothing, for tests that only need a busy chat.
    #[doc(hidden)]
    pub fn inert() -> Self {
        Self(CancelInner::Cli(std::sync::mpsc::channel().0))
    }

    pub fn cancel(&self) {
        match &self.0 {
            CancelInner::Task(handle) => handle.abort(),
            CancelInner::Cli(tx) => {
                let _ = tx.send(Command::Cancel);
            }
        }
    }
}

impl Agent {
    pub fn new() -> std::io::Result<Self> {
        Self::with_factory(Arc::new(real_driver), Duration::from_secs(600))
    }

    pub(crate) fn with_factory(factory: Factory, idle: Duration) -> std::io::Result<Self> {
        let rt = Runtime::new()?;
        let live: Arc<Mutex<HashMap<String, Live>>> = Arc::default();
        let weak = Arc::downgrade(&live);
        std::thread::spawn(move || {
            while let Some(live) = {
                std::thread::sleep(Duration::from_secs(30));
                weak.upgrade()
            } {
                reap(&live, idle);
            }
        });
        let (side, side_rx) = mpsc::unbounded();
        let (orch, orch_rx) = mpsc::unbounded();
        Ok(Self {
            rt,
            sessions: Arc::new(InMemorySessionService::new()),
            tools: Tools::new(),
            pending: Pending::default(),
            always_allowed: Arc::default(),
            next_request: Arc::new(AtomicU64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(1),
            )),
            live,
            cli_asks: Arc::default(),
            factory,
            side,
            side_rx: Mutex::new(Some(side_rx)),
            leads: Leads::default(),
            orch,
            orch_rx: Mutex::new(Some(orch_rx)),
            orch_pending: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            #[cfg(test)]
            idle,
        })
    }

    /// Opens the chat database at `path`, creating it if needed.
    pub fn open_store(&self, path: &Path) -> sqlx::Result<Store> {
        Store::open(path, self.rt.handle().clone())
    }

    /// Updates that arrive while no turn of their chat runs; only the first call gets them.
    pub fn side_updates(&self) -> Option<UnboundedReceiver<SideUpdate>> {
        self.side_rx.guard().take()
    }

    /// Tool calls from lead chats; only the first call gets them.
    pub fn orchestrator_calls(&self) -> Option<(UnboundedReceiver<OrchCall>, OrchPending)> {
        Some((self.orch_rx.guard().take()?, self.orch_pending.clone()))
    }

    pub fn serve_orchestrator(&self) {
        let started =
            socket_path().ok_or_else(|| std::io::Error::other("no config dir")).and_then(|p| {
                listen(&p, self.leads.clone(), self.orch.clone(), self.orch_pending.clone())
            });
        if let Err(e) = started {
            eprintln!("aether: orchestration off: {e}");
        }
    }

    /// The chat's token; minted on first use, stable for the app's lifetime.
    pub fn lead_token(&self, chat_key: &str) -> String {
        let mut leads = self.leads.guard();
        if let Some((token, _)) = leads.iter().find(|(_, key)| *key == chat_key) {
            return token.clone();
        }
        let mut bytes = [0u8; 16];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut bytes))
            .expect("read /dev/urandom");
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        leads.insert(token.clone(), chat_key.to_string());
        token
    }

    fn lead_tools(&self, chat: &ChatSession) -> Option<LeadTools> {
        if !chat.lead {
            return None;
        }
        Some(LeadTools {
            exe: std::env::current_exe().ok()?,
            sock: socket_path()?,
            token: self.lead_token(&chat.key),
            specs: crate::orchestrator::tool_specs(),
        })
    }

    /// Delivers the user's answer to a paused run. Ignored if that run ended.
    pub fn answer(&self, id: u64, answer: Answer) {
        let routed = self.cli_asks.guard().remove(&id);
        if let Some((key, local)) = routed {
            if let Some(session) = self.live.guard().get(&key) {
                session.holds.guard().remove(&format!("ask:{id}"));
                let _ = session.commands.send(Command::Answer(local, answer));
            }
            return;
        }
        if let Some(tx) = self.pending.guard().remove(&id) {
            let _ = tx.send(answer);
        }
    }

    /// Starts session `id` already holding `history`, so a chat can continue
    /// from an edited or retried point.
    pub fn fork_session(&self, id: &str, history: &[(Role, String)]) -> Result<()> {
        self.create_session(id)?;
        self.rt.block_on(async {
            for (role, text) in history {
                let mut event = Event::new("history");
                let (author, content_role) = match role {
                    Role::User => (USER, "user"),
                    Role::Assistant => (AGENT, "model"),
                };
                event.author = author.into();
                event.set_content(Content::new(content_role).with_text(text.clone()));
                self.sessions.append_event(id, event).await?;
            }
            Ok(())
        })
    }

    pub fn create_session(&self, id: &str) -> Result<()> {
        self.rt.block_on(self.sessions.create(CreateRequest {
            app_name: APP.into(),
            user_id: USER.into(),
            session_id: Some(id.into()),
            state: Default::default(),
        }))?;
        Ok(())
    }

    /// Models that can call tools. Empty on failure.
    pub fn list_models(&self, api_key: &str) -> impl Future<Output = Vec<ModelInfo>> + 'static {
        let client = OpenRouterClient::new(OpenRouterConfig::new(api_key, "openrouter/auto"));
        let task = self.rt.spawn(async move { models::list(client?).await });
        async move { task.await.ok().and_then(Result::ok).unwrap_or_default() }
    }

    /// Whether `provider`'s CLI is usable and which models it offers.
    pub fn probe_provider(
        &self,
        provider: Provider,
        settings: &Settings,
    ) -> impl Future<Output = ProviderProbe> + 'static {
        let settings = settings.clone();
        let task = self.rt.spawn_blocking(move || catalog::probe(provider, &settings));
        async move { task.await.unwrap_or_default() }
    }

    /// Per folder: whether it still exists, and its git branch. A folder
    /// whose check times out counts as present, with no branch.
    pub fn branch_infos(
        &self,
        folders: Vec<PathBuf>,
    ) -> impl Future<Output = Vec<(PathBuf, bool, Option<BranchInfo>)>> + 'static {
        let task = self.rt.spawn_blocking(move || {
            folders
                .into_iter()
                .map(|folder| match dir_exists(&folder, Duration::from_secs(3)) {
                    Some(true) => {
                        let info = git::branch_info(&folder);
                        (folder, true, info)
                    }
                    Some(false) => (folder, false, None),
                    None => (folder, true, None),
                })
                .collect()
        });
        async move { task.await.unwrap_or_default() }
    }

    /// The changes in the repo `folder` is in, for the git panel.
    pub fn git_status(
        &self,
        folder: PathBuf,
    ) -> impl Future<Output = std::result::Result<git::Status, git::StatusError>> + 'static {
        let task = self.rt.spawn_blocking(move || git::status(&folder));
        async move { task.await.unwrap_or_else(|e| Err(git::StatusError::Failed(e.to_string()))) }
    }

    /// One file's diff in the work tree whose top folder is `top`.
    pub fn git_diff(
        &self,
        top: PathBuf,
        change: git::Change,
    ) -> impl Future<Output = std::result::Result<String, String>> + 'static {
        let task = self.rt.spawn_blocking(move || git::diff(&top, &change));
        async move { task.await.map_err(|e| e.to_string())? }
    }

    /// Runs a git panel operation (stage, discard, commit, sync…) in the work tree at `top`.
    pub fn git_apply(
        &self,
        top: PathBuf,
        op: git::GitOp,
    ) -> impl Future<Output = std::result::Result<String, String>> + 'static {
        let task = self.rt.spawn_blocking(move || git::apply(&top, &op));
        async move { task.await.map_err(|e| e.to_string())? }
    }

    pub fn git_remotes(
        &self,
        top: PathBuf,
    ) -> impl Future<Output = std::result::Result<Vec<git::Remote>, String>> + 'static {
        let task = self.rt.spawn_blocking(move || git::remotes(&top));
        async move { task.await.map_err(|e| e.to_string())? }
    }

    pub fn git_init(
        &self,
        folder: PathBuf,
    ) -> impl Future<Output = std::result::Result<(), String>> + 'static {
        let task = self.rt.spawn_blocking(move || git::init(&folder));
        async move { task.await.map_err(|e| e.to_string())? }
    }

    pub fn switch_branch(
        &self,
        folder: PathBuf,
        branch: String,
    ) -> impl Future<Output = std::result::Result<(), String>> + 'static {
        let task = self.rt.spawn_blocking(move || git::switch_branch(&folder, &branch));
        async move { task.await.map_err(|e| e.to_string())? }
    }

    /// Branches a new worktree in `folder` can start from; `None` outside a repo.
    pub fn worktree_bases(&self, folder: PathBuf) -> impl Future<Output = Option<Bases>> + 'static {
        let task = self.rt.spawn_blocking(move || worktree::bases(&folder));
        async move { task.await.ok().flatten() }
    }

    /// Makes a worktree for a chat in `folder`, named from its first `prompt`;
    /// setting `cancel` stops it.
    pub fn create_worktree(
        &self,
        folder: PathBuf,
        prompt: String,
        base: String,
        cancel: Arc<std::sync::atomic::AtomicBool>,
    ) -> impl Future<Output = std::result::Result<Worktree, String>> + 'static {
        let task = self.rt.spawn_blocking(move || {
            let root =
                worktree::root().ok_or("Couldn't find your home folder to keep worktrees in.")?;
            worktree::create(&root, &folder, &prompt, &base, &cancel)
        });
        async move { task.await.map_err(|e| e.to_string())? }
    }

    /// What removing the worktree at `path`, of the repo at `project`, would lose.
    pub fn check_worktree(
        &self,
        path: PathBuf,
        project: PathBuf,
    ) -> impl Future<Output = std::result::Result<RemoveCheck, String>> + 'static {
        let task = self
            .rt
            .spawn_blocking(move || worktree::check_any(&worktree::roots(), &project, &path));
        async move { task.await.map_err(|e| e.to_string())? }
    }

    /// Removes the worktree at `path`; its branch is kept unless `delete_branch`.
    /// `force` drops its changes (and uses `branch -D`).
    pub fn remove_worktree(
        &self,
        path: PathBuf,
        project: PathBuf,
        force: bool,
        delete_branch: bool,
    ) -> impl Future<Output = std::result::Result<(), RemoveError>> + 'static {
        let task = self.rt.spawn_blocking(move || {
            worktree::remove_worktree_with_branch(
                &worktree::roots(),
                &path,
                &project,
                force,
                delete_branch,
            )
        });
        async move { task.await.map_err(|e| RemoveError::Failed(e.to_string()))? }
    }

    /// Every worktree of the repo `folder` is in, the main one first.
    pub fn repo_worktrees(
        &self,
        folder: PathBuf,
    ) -> impl Future<Output = std::result::Result<Vec<Listed>, String>> + 'static {
        let task = self.rt.spawn_blocking(move || worktree::list_repo(&worktree::roots(), &folder));
        async move { task.await.map_err(|e| e.to_string())? }
    }

    /// Checks `start` out in a new worktree for `folder`'s repo, in aether's
    /// folder or a new subfolder of `parent`.
    pub fn add_worktree(
        &self,
        folder: PathBuf,
        start: Start,
        parent: Option<PathBuf>,
    ) -> impl Future<Output = std::result::Result<PathBuf, String>> + 'static {
        let task = self.rt.spawn_blocking(move || {
            let root =
                worktree::root().ok_or("Couldn't find your home folder to keep worktrees in.")?;
            worktree::add(&root, &folder, &start, parent.as_deref())
        });
        async move { task.await.map_err(|e| e.to_string())? }
    }

    /// Drops git's records of `folder`'s repo's worktrees whose folders are gone; `Ok` names them.
    pub fn prune_worktrees(
        &self,
        folder: PathBuf,
    ) -> impl Future<Output = std::result::Result<Vec<String>, String>> + 'static {
        let task = self.rt.spawn_blocking(move || worktree::prune(&folder));
        async move { task.await.map_err(|e| e.to_string())? }
    }

    /// aether's worktrees on disk and in git, also asking the repos in `repos`.
    pub fn list_worktrees(
        &self,
        repos: Vec<PathBuf>,
    ) -> impl Future<Output = Vec<Listed>> + 'static {
        let task = self.rt.spawn_blocking(move || worktree::list(&worktree::roots(), &repos));
        async move { task.await.unwrap_or_default() }
    }

    /// Agents running outside aether (`None`: the scan failed);
    /// `own_sessions` (aether chats' CLI session ids) are never matched to
    /// Pi or OpenCode agents.
    pub fn scan_external(
        &self,
        own_sessions: std::collections::HashSet<String>,
    ) -> impl Future<Output = Option<Vec<ExternalAgent>>> + 'static {
        let task =
            self.rt.spawn_blocking(move || external::scan(std::process::id(), &own_sessions));
        async move { task.await.ok().flatten() }
    }

    pub fn stop_external(
        &self,
        agent: ExternalAgent,
        force: bool,
    ) -> impl Future<Output = std::result::Result<(), String>> + 'static {
        let task = self.rt.spawn_blocking(move || external::stop(&agent, force));
        async move { task.await.map_err(|e| e.to_string())? }
    }

    pub fn transcript(
        &self,
        agent: ExternalAgent,
    ) -> impl Future<Output = Vec<(Role, String)>> + 'static {
        let task = self.rt.spawn_blocking(move || external::transcript(&agent));
        async move { task.await.unwrap_or_default() }
    }

    /// Answers `text` in `session`. `skills` are the enabled ones; any the
    /// text mentions as `@name` are applied directly. `history` is the
    /// visible transcript before this turn; a CLI session that did not
    /// resume gets it joined into its first prompt as context.
    pub fn send(
        &self,
        session: &str,
        text: String,
        attachments: Vec<Attachment>,
        settings: &Settings,
        api_key: &str,
        skills: Vec<Skill>,
        history: &[(Role, String)],
        chat: &ChatSession,
    ) -> Reply {
        let (tx, updates) = mpsc::unbounded();
        let invoked: Vec<Skill> = mentioned_skills(&text, &skills).into_iter().cloned().collect();
        for skill in &invoked {
            let _ = tx.unbounded_send(Update::Skill(skill.name.clone()));
        }
        if settings.provider != Provider::OpenRouter {
            return self.send_cli(text, attachments, settings, chat, history, tx, updates);
        }
        let always_allowed = self
            .always_allowed
            .guard()
            .entry(chat.key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(Default::default())))
            .clone();
        let gate = Arc::new(Gate::new(
            tx.clone(),
            self.pending.clone(),
            self.next_request.clone(),
            settings.clone(),
            always_allowed,
        ));
        let turn = Turn {
            api_key: api_key.into(),
            settings: settings.clone(),
            sessions: self.sessions.clone(),
            session: session.into(),
            tools: self.tools.enabled(
                settings,
                &skills,
                gate,
                chat.lead.then(|| (chat.key.clone(), self.orch.clone(), self.orch_pending.clone())),
            ),
            skills,
            invoked,
        };
        let task = self.rt.spawn(async move {
            if let Err(e) = turn.run(text, attachments, &tx).await {
                let _ = tx.unbounded_send(Update::Error(e.to_string()));
            }
        });
        Reply { updates, cancel: Cancel(CancelInner::Task(task.abort_handle())) }
    }

    fn send_cli(
        &self,
        text: String,
        attachments: Vec<Attachment>,
        settings: &Settings,
        chat: &ChatSession,
        history: &[(Role, String)],
        tx: UnboundedSender<Update>,
        updates: UnboundedReceiver<Update>,
    ) -> Reply {
        let provider = settings.provider;
        if let Some(error) = missing_binary_error(provider, settings) {
            let _ = tx.unbounded_send(Update::Error(error));
            drop(tx);
            let (done, _) = std::sync::mpsc::channel();
            return Reply { updates, cancel: Cancel(CancelInner::Cli(done)) };
        }
        let (opts, key) = session_options(settings, chat, self.lead_tools(chat));
        let mut live = self.live.guard();
        let mut command = Command::Prompt {
            text,
            attachments,
            history: history.to_vec(),
            resume: chat.resume.clone(),
            model: opts.model.clone(),
            effort: opts.effort.clone(),
        };
        if let Some(session) = live.get(&chat.key).filter(|s| s.key == key) {
            let mut turn = session.turn.guard();
            match session.commands.send(command) {
                Ok(()) => {
                    *turn = Some(tx);
                    *session.last_active.guard() = Instant::now();
                    return Reply {
                        updates,
                        cancel: Cancel(CancelInner::Cli(session.commands.clone())),
                    };
                }
                Err(dead) => command = dead.0,
            }
        }
        self.cli_asks.guard().retain(|_, (k, _)| *k != chat.key);
        let session = self.spawn_live(&chat.key, key, opts, Some(tx));
        let _ = session.commands.send(command);
        let cancel = session.commands.clone();
        live.insert(chat.key.clone(), session);
        Reply { updates, cancel: Cancel(CancelInner::Cli(cancel)) }
    }

    /// Opens the chat's CLI session without a prompt, so a turn still running in it
    /// streams in as a side turn; `background` rows of the last reply settle if their work
    /// ended. Does nothing while a session is open.
    pub fn attach(&self, settings: &Settings, chat: &ChatSession, background: Vec<String>) {
        let mut live = self.live.guard();
        if live.contains_key(&chat.key) {
            return;
        }
        let (mut opts, key) = session_options(settings, chat, self.lead_tools(chat));
        opts.reattach = true;
        opts.background = background;
        let session = self.spawn_live(&chat.key, key, opts, None);
        live.insert(chat.key.clone(), session);
    }

    fn spawn_live(
        &self,
        chat_key: &str,
        key: SessionKey,
        opts: DriverStartOptions,
        tx: Option<UnboundedSender<Update>>,
    ) -> Live {
        let (commands, rx) = std::sync::mpsc::channel();
        let (driver_tx, mut driver_rx) = mpsc::unbounded();
        let factory = self.factory.clone();
        let provider = key.provider;
        let turn: TurnTx = Arc::new(Mutex::new(tx));
        // a late steer's reply; the pump installs it on the Wake queued behind the old turn's Final
        let fallback: TurnTx = Arc::default();
        let next_reply = fallback.clone();
        let health = Arc::new(Mutex::new(SessionHealth { pid: None, alive: true }));
        let driver_health = health.clone();
        // a side turn opened by a stray event: the idle driver's cancel won't end it
        let orphan = Arc::new(AtomicBool::new(false));
        let pump_orphan = orphan.clone();
        std::thread::spawn(move || {
            let report = |driver: &dyn Driver| {
                *driver_health.guard() = SessionHealth { pid: driver.pid(), alive: driver.alive() };
            };
            let mut opts = opts;
            let mut driver = factory(provider, opts.clone(), driver_tx.clone());
            // a respawn serves a prompt; closing out the old reply would end that prompt's turn
            opts.reattach = false;
            opts.background.clear();
            report(&*driver);
            let mut seeded = driver.resumed();
            for command in rx {
                match command {
                    Command::Prompt { text, attachments, history, resume, model, effort } => {
                        let switch = model != opts.model || effort != opts.effort;
                        (opts.model, opts.effort) = (model, effort);
                        if !driver.alive()
                            || switch && !driver.set_options(&opts.model, &opts.effort)
                        {
                            let opts = DriverStartOptions { resume, ..opts.clone() };
                            driver = factory(provider, opts, driver_tx.clone());
                            report(&*driver);
                            seeded = driver.resumed();
                        }
                        let (native, by_path) = native_image_split(provider, &attachments);
                        let text = if seeded || history.is_empty() {
                            text
                        } else {
                            context_prompt(&history, &text)
                        } + &attached_files_block(&by_path);
                        seeded = true;
                        orphan.store(false, Ordering::SeqCst);
                        driver.prompt(text, native);
                    }
                    Command::Steer(text, attachments, reply, resume) => {
                        seeded = true;
                        let (native, by_path) = native_image_split(provider, &attachments);
                        let full = text.clone() + &attached_files_block(&by_path);
                        if driver.steer(full.clone(), native.clone()) {
                            if !driver.reports_steered() {
                                let _ = driver_tx.unbounded_send(Update::Steered(text));
                            }
                        } else {
                            if !driver.alive() {
                                let opts = DriverStartOptions { resume, ..opts.clone() };
                                driver = factory(provider, opts, driver_tx.clone());
                                report(&*driver);
                            }
                            *next_reply.guard() = Some(reply);
                            let _ = driver_tx.unbounded_send(Update::Wake);
                            orphan.store(false, Ordering::SeqCst);
                            driver.prompt(full, native);
                        }
                    }
                    Command::Cancel => {
                        driver.cancel();
                        if orphan.swap(false, Ordering::SeqCst) {
                            let _ = driver_tx.unbounded_send(Update::Final(String::new()));
                        }
                    }
                    Command::Answer(id, answer) => driver.answer(id, answer),
                    Command::StopTool(id) => {
                        if !driver.stop_tool(&id) {
                            let _ = driver_tx.unbounded_send(Update::Warning(
                                "This agent can't stop background work from Aether".into(),
                            ));
                        }
                    }
                    Command::NoticeAction(action) => driver.notice_action(&action),
                    Command::Side(question, reply) => {
                        if !driver.side_question(&question, reply.clone()) {
                            let _ = reply.unbounded_send(Update::Error(SIDE_UNSUPPORTED.into()));
                        }
                    }
                }
                report(&*driver);
            }
        });
        let last_active = Arc::new(Mutex::new(Instant::now()));
        let holds: Arc<Mutex<HashMap<String, bool>>> = Arc::default();
        let pump_turn = turn.clone();
        let pump_active = last_active.clone();
        let pump_holds = holds.clone();
        let cli_asks = self.cli_asks.clone();
        let next_request = self.next_request.clone();
        let side = self.side.clone();
        // weak: a strong map here would keep its own command sender, and so this thread, alive
        let live = Arc::downgrade(&self.live);
        let chat_key = chat_key.to_string();
        std::thread::spawn(move || {
            // a prompt beat the attach's first driver: its settle notice (1), Final (2) are stale
            let mut stale_settle = 0;
            while let Some(update) = futures::executor::block_on(driver_rx.next()) {
                *pump_active.guard() = Instant::now();
                match (std::mem::take(&mut stale_settle), &update) {
                    (1, Update::Notice(n)) if n.id == "reattach" && n.kind == NoticeKind::Info => {
                        stale_settle = 2;
                        continue;
                    }
                    (2, Update::Final(text)) if text.is_empty() => continue,
                    _ => {}
                }
                let late = match update {
                    Update::Wake => fallback.guard().take(),
                    _ => None,
                };
                let opens = late.is_none()
                    && match &update {
                        Update::Wake
                        | Update::Ask { .. }
                        | Update::Delta(_)
                        | Update::Thinking(_)
                        | Update::Steered(_) => true,
                        Update::Notice(notice) => notice.kind == NoticeKind::Wake,
                        _ => false,
                    };
                // take the command sender before the turn lock: send_cli locks live, then turn
                let commands = if opens && pump_turn.guard().is_none() {
                    live.upgrade()
                        .and_then(|live| live.guard().get(&chat_key).map(|s| s.commands.clone()))
                } else {
                    None
                };
                let update = match update {
                    Update::Ask { id: local, ask } => {
                        let id = next_request.fetch_add(1, Ordering::SeqCst);
                        cli_asks.guard().insert(id, (chat_key.clone(), local));
                        pump_holds.guard().insert(format!("ask:{id}"), false);
                        Update::Ask { id, ask }
                    }
                    Update::AskResolved(local) => {
                        let mut asks = cli_asks.guard();
                        let Some(id) = asks.iter().find_map(|(id, (key, l))| {
                            (*key == chat_key && *l == local).then_some(*id)
                        }) else {
                            continue;
                        };
                        asks.remove(&id);
                        drop(asks);
                        pump_holds.guard().remove(&format!("ask:{id}"));
                        Update::AskResolved(id)
                    }
                    Update::Tool(call) => {
                        let mut holds = pump_holds.guard();
                        if matches!(
                            call.status,
                            ToolStatus::Running | ToolStatus::Waiting | ToolStatus::Background
                        ) {
                            holds.insert(call.id.clone(), call.status == ToolStatus::Background);
                        } else {
                            holds.remove(&call.id);
                        }
                        Update::Tool(call)
                    }
                    update => update,
                };
                let terminal = matches!(update, Update::Final(_));
                // before the turn closes: whoever sees the reply end may reap at once
                if terminal {
                    pump_holds.guard().retain(|_, background| *background);
                    pump_orphan.store(false, Ordering::SeqCst);
                }
                let mut turn = pump_turn.guard();
                let ignored = matches!(update, Update::Wake) && late.is_none() && turn.is_some();
                if let Some(reply) = late {
                    *turn = Some(reply);
                } else if let Some(commands) = commands
                    && turn.is_none()
                {
                    let (tx, updates) = mpsc::unbounded();
                    *turn = Some(tx);
                    if !matches!(update, Update::Wake) {
                        pump_orphan.store(true, Ordering::SeqCst);
                    }
                    let cancel = Cancel(CancelInner::Cli(commands));
                    let reply = Reply { updates, cancel };
                    let _ = side.unbounded_send(SideUpdate::Turn(chat_key.clone(), reply));
                }
                if matches!(update, Update::Wake) {
                    stale_settle = u8::from(ignored);
                    continue;
                }
                match turn.as_ref() {
                    Some(tx) => {
                        let _ = tx.unbounded_send(update);
                    }
                    None if matches!(
                        update,
                        Update::Tool(_)
                            | Update::Notice(_)
                            | Update::AskResolved(_)
                            | Update::Mode(_)
                            | Update::Model(_)
                            | Update::Cost(_)
                            | Update::Usage(_)
                            | Update::Count { .. }
                            | Update::Window(_)
                            | Update::Title(_)
                            | Update::Session(_)
                            | Update::Error(_)
                            | Update::Warning(_)
                            | Update::SteerRefused(_)
                    ) =>
                    {
                        // no reply to fail: an error out of turn would fail the last finished one
                        let update = match update {
                            Update::Error(error) => Update::Warning(error),
                            update => update,
                        };
                        let _ = side
                            .unbounded_send(SideUpdate::Meta(chat_key.clone(), Box::new(update)));
                    }
                    None => {}
                }
                if terminal {
                    *turn = None;
                }
            }
            pump_turn.guard().take();
        });
        Live { key, commands, turn, last_active, holds, health }
    }

    pub fn steer(
        &self,
        text: String,
        attachments: Vec<Attachment>,
        settings: &Settings,
        history: &[(Role, String)],
        chat: &ChatSession,
    ) -> Reply {
        let (tx, updates) = mpsc::unbounded();
        let (_, key) = session_options(settings, chat, self.lead_tools(chat));
        if let Some(session) = self.live.guard().get(&chat.key).filter(|s| s.key == key) {
            let turn = session.turn.guard();
            let steer =
                Command::Steer(text.clone(), attachments.clone(), tx.clone(), chat.resume.clone());
            if turn.is_some() && session.commands.send(steer).is_ok() {
                *session.last_active.guard() = Instant::now();
                return Reply {
                    updates,
                    cancel: Cancel(CancelInner::Cli(session.commands.clone())),
                };
            }
        }
        self.send_cli(text, attachments, settings, chat, history, tx, updates)
    }

    /// Stops a background task or terminal of the chat's live session, if one is open.
    pub fn stop_tool(&self, chat_key: &str, tool_id: &str) {
        if let Some(session) = self.live.guard().get(chat_key) {
            let _ = session.commands.send(Command::StopTool(tool_id.to_string()));
        }
    }

    /// Runs a notice's `aether:<action>` in the chat's live session, if one is open.
    pub fn notice_action(&self, chat_key: &str, action: &str) {
        if let Some(session) = self.live.guard().get(chat_key) {
            let _ = session.commands.send(Command::NoticeAction(action.to_string()));
        }
    }

    /// Asks `question` through the chat's live CLI without touching its turn. None when no
    /// session is live. A CLI that can't take it answers with `Update::Error(SIDE_UNSUPPORTED)`.
    pub fn side_question(&self, chat_key: &str, question: &str) -> Option<Reply> {
        let commands = self.live.guard().get(chat_key)?.commands.clone();
        let (tx, updates) = mpsc::unbounded();
        commands.send(Command::Side(question.to_string(), tx)).ok()?;
        Some(Reply { updates, cancel: Cancel::inert() })
    }

    pub fn health(&self, chat_key: &str) -> Option<SessionHealth> {
        let health = *self.live.guard().get(chat_key)?.health.guard();
        Some(SessionHealth {
            alive: health.alive && health.pid.is_none_or(process_alive),
            ..health
        })
    }

    pub fn live_sessions(&self) -> std::collections::HashSet<String> {
        self.live.guard().keys().cloned().collect()
    }

    /// Drops the in-memory conversation behind `key` (OpenRouter only).
    pub fn delete_session(&self, key: &str) {
        let sessions = self.sessions.clone();
        let req =
            DeleteRequest { app_name: APP.into(), user_id: USER.into(), session_id: key.into() };
        self.rt.spawn(async move {
            let _ = sessions.delete(req).await;
        });
    }

    pub fn close_session(&self, key: &str) {
        self.live.guard().remove(key);
        self.cli_asks.guard().retain(|_, (k, _)| k != key);
    }

    #[cfg(test)]
    fn reap(&self) {
        reap(&self.live, self.idle)
    }
}

fn session_options(
    settings: &Settings,
    chat: &ChatSession,
    lead: Option<LeadTools>,
) -> (DriverStartOptions, SessionKey) {
    let opts = DriverStartOptions {
        effort: chat.effort.clone(),
        access: chat.access,
        resume: chat.resume.clone(),
        attachments: attachments_dir().map(|d| d.join(&chat.key)),
        lead,
        ephemeral: chat.ephemeral,
        ..DriverStartOptions::for_provider(settings, settings.provider, chat.cwd.clone())
    };
    let key = SessionKey {
        provider: settings.provider,
        binary: opts.binary.clone(),
        cwd: opts.cwd.clone(),
        access: opts.access,
        lead: chat.lead,
    };
    (opts, key)
}

fn real_driver(
    p: Provider,
    opts: DriverStartOptions,
    tx: UnboundedSender<Update>,
) -> Box<dyn Driver> {
    match p {
        Provider::Claude => Box::new(ClaudeDriver::spawn(opts, tx)),
        Provider::Codex => Box::new(CodexDriver::spawn(opts, tx)),
        Provider::Pi => Box::new(PiDriver::spawn(opts, tx)),
        Provider::OpenCode => Box::new(OpencodeDriver::spawn(opts, tx)),
        Provider::Antigravity => Box::new(AntigravityDriver::spawn(opts, tx)),
        Provider::OpenRouter => unreachable!("OpenRouter has no driver"),
    }
}

/// Drops sessions with no running turn and no held work that sat idle for `idle`.
fn reap(live: &Mutex<HashMap<String, Live>>, idle: Duration) {
    live.guard().retain(|_, s| {
        s.turn.guard().is_some()
            || !s.holds.guard().is_empty()
            || s.last_active.guard().elapsed() < idle
    });
}

fn process_alive(pid: u32) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Whether an answer lets the driver continue.
#[allow(dead_code)]
fn answer_allows(answer: &Answer) -> bool {
    match answer {
        Answer::Approve { .. }
        | Answer::Reply(_)
        | Answer::Pick(_)
        | Answer::Answers(_)
        | Answer::Form(_) => true,
        Answer::Deny | Answer::DenyWith(_) => false,
        Answer::Plan { approved, .. } => *approved,
    }
}

pub const HISTORY_BUDGET: usize = 64_000;

/// The first user message plus the newest whole messages that fit `limit`
/// characters, and how many were left out.
pub fn budget_history(history: &[(Role, String)], limit: usize) -> (Vec<(Role, String)>, usize) {
    if history.is_empty() {
        return (Vec::new(), 0);
    }
    let first_ix = history.iter().position(|(r, _)| *r == Role::User).unwrap_or(0);
    let first = &history[first_ix];
    let mut used = first.1.chars().count();
    let mut from = history.len();
    for (i, (_, text)) in history.iter().enumerate().skip(first_ix + 1).rev() {
        used += text.chars().count();
        if used > limit {
            break;
        }
        from = i;
    }
    let mut kept = vec![first.clone()];
    kept.extend_from_slice(&history[from.max(first_ix + 1)..]);
    let omitted = history.len() - kept.len();
    (kept, omitted)
}

/// The visible transcript joined with the new message, so a stateless
/// CLI prompt still sees the conversation.
fn context_prompt(history: &[(Role, String)], text: &str) -> String {
    if history.is_empty() {
        return text.to_string();
    }
    let (history, omitted) = budget_history(history, HISTORY_BUDGET);
    let mut out = String::from("<conversation_so_far>\n");
    for (i, (role, line)) in history.iter().enumerate() {
        out.push_str(match role {
            Role::User => "User: ",
            Role::Assistant => "Assistant: ",
        });
        out.push_str(line);
        out.push('\n');
        if i == 0 && omitted > 0 {
            out.push_str(&format!("({omitted} earlier messages omitted)\n"));
        }
    }
    out.push_str(
        "</conversation_so_far>\n\nTreat the conversation above as context only. Your task now:\n",
    );
    out.push_str(text);
    out
}

/// Pre-flight check so a missing CLI binary surfaces as `Update::Error`
/// without depending on the driver constructors. OpenCode is exempt: a
/// running service answers without any binary on disk. Runs on the UI
/// thread; the service check (file read, 1 s HTTP cap) only runs without a binary.
fn missing_binary_error(provider: Provider, settings: &Settings) -> Option<String> {
    let (name, override_path) = match provider {
        Provider::Claude => ("claude", settings.claude_binary.as_str()),
        Provider::Codex => ("codex", settings.codex_binary.as_str()),
        Provider::Pi => ("pi", settings.pi_binary.as_str()),
        Provider::OpenCode => {
            if find_binary("opencode", settings.opencode_binary.as_str()).is_some()
                || opencode::service_reachable()
            {
                return None;
            }
            return Some(
                "opencode is not installed; install it or set an explicit binary path".into(),
            );
        }
        Provider::Antigravity => {
            if find_binary("agy", settings.antigravity_binary.as_str()).is_some() {
                return None;
            }
            return Some("agy is not installed; install it or set an explicit binary path".into());
        }
        Provider::OpenRouter => return None,
    };
    match find_binary(name, override_path) {
        Some(_) => None,
        None => Some(format!("{name} is not installed; install it or set an explicit binary path")),
    }
}

#[cfg(test)]
#[path = "../tests/unit/agent.rs"]
mod tests;
