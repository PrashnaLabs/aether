# Agent Sessions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One live CLI session per chat (resume across restarts), steer + queue, per-chat access/effort/folder, minimal tool steps, and drivers rewritten to the real Claude / Pi / OpenCode wire.

**Architecture:** `aether-agent` keeps a map of live sessions keyed by chat; each owns a driver thread (commands in) and a pump thread (updates out, routed to the current turn's reply). Drivers become long-lived with `steer` and `resumed()`. The app owns per-chat options and the queue, and persists the driver's resume id.

**Tech Stack:** Rust 2024, std threads + `std::sync::mpsc`, `futures` channels, `reqwest` (already a dep) for OpenCode, gpui-kit 0.7.3.

**Spec:** `docs/superpowers/specs/2026-10-14-agent-sessions-design.md`. Wire authority for Tasks 4-6: `docs/wayfinder/research/claude-sessions.md`, `pi-sessions.md`, `opencode-sessions.md` (and `reference/crates/waku-core/src/driver/*.rs` they cite).

**Note on code completeness:** Tasks 1-3, 7, 8 carry concrete code. Tasks 4-6 are protocol rewrites whose exact wire is in the research docs; they carry the required interface, the canned wire fixtures each test must use, the mapping table, and a live smoke procedure instead of a full transcription.

## Global Constraints

- Never block the UI thread: driver construction, handshakes, HTTP, and process spawns happen on agent threads only.
- Every driver turn ends with exactly one `Update::Final` (empty after cancel or failure). Tool errors never fail the turn.
- Dropping a driver ends its session gracefully, kills after 3 s; it never deletes the CLI's stored session. The OpenCode service is never killed.
- Empty CLI model = no `--model`; empty effort = no effort flag.
- Access per provider: Claude and OpenCode `Ask | AutoEdit | Full`; Pi `ReadOnly | Full`. New chats: Pi `ReadOnly`, else `Ask`.
- Idle timeout 10 minutes; reaper interval 30 s.
- Live smoke turns allowed (approved by the user): throwaway temp folder, Ask/ReadOnly, prompt `Reply with exactly: ok`; at most one fresh, one resume, one cancel check per CLI. Never approve a tool in a smoke run.
- No new dependencies. No commits (the user commits by hand). Comments only for non-obvious why, one line.

## Review Focus

- A resume id whose stored session is gone must start a fresh session seeded with the transcript, not error out forever. Tests in Tasks 3, 4, 5, 6.
- Cancel must stop the turn but keep the session alive for the next prompt (today Claude/Pi kill the process at 3 s). Live check in Tasks 4-6.
- Approvals answered in the UI must reach the right driver with the driver's own request id (today the renumbered id is sent). Test in Task 3.
- Changing access or effort mid-chat must respawn with resume, not lose the conversation. Test in Task 3.
- A queued message must not auto-send after the user pressed Stop. Test in Task 8.

---

### Task 1: Core types + tool step rendering

**Files:**
- Modify: `crates/core/src/settings.rs` (add `AccessMode`, provider helpers)
- Modify: `crates/core/src/lib.rs` (`ToolStatus`, `Update::{Session, Tool}`, `Step::Tool`, `Message::apply`; re-exports)
- Modify: `crates/ui/src/chat/thinking.rs` (render `Step::Tool`, summary)

**Interfaces:**
- Produces: `aether_core::{AccessMode, ToolStatus}`; `AccessMode::{ALL, label(), for_provider(Provider) -> &'static [AccessMode]}`; `aether_core::effort_levels(Provider) -> &'static [&'static str]`; `Update::Session(String)`; `Update::Tool { id: String, title: String, detail: String, status: ToolStatus }`; `Step::Tool { id, title, detail, status }`.

- [ ] **Step 1: Failing tests** — append to `crates/core/src/settings.rs` tests:

```rust
    #[test]
    fn access_modes_per_provider_and_saved_names() {
        assert_eq!(serde_json::to_string(&AccessMode::AutoEdit).unwrap(), "\"autoedit\"");
        assert_eq!(serde_json::to_string(&AccessMode::ReadOnly).unwrap(), "\"readonly\"");
        assert_eq!(AccessMode::for_provider(Provider::Pi), [AccessMode::ReadOnly, AccessMode::Full]);
        assert_eq!(
            AccessMode::for_provider(Provider::Claude),
            [AccessMode::Ask, AccessMode::AutoEdit, AccessMode::Full]
        );
        assert!(AccessMode::for_provider(Provider::OpenRouter).is_empty());
        assert_eq!(AccessMode::Full.label(), "Full access");
        assert_eq!(effort_levels(Provider::Claude), ["low", "medium", "high", "xhigh", "max"]);
        assert!(effort_levels(Provider::OpenCode).is_empty());
    }
```

and to `crates/core/src/lib.rs` tests:

```rust
    #[test]
    fn tool_updates_upsert_one_step_and_never_fail_the_reply() {
        let mut msg = Message::assistant();
        let tool = |status| Update::Tool {
            id: "t1".into(),
            title: "Run command".into(),
            detail: "ls".into(),
            status,
        };
        msg.apply(tool(ToolStatus::Running));
        msg.apply(Update::Session("abc".into()));
        msg.apply(tool(ToolStatus::Failed));
        assert_eq!(
            msg.steps,
            [Step::Tool {
                id: "t1".into(),
                title: "Run command".into(),
                detail: "ls".into(),
                status: ToolStatus::Failed,
            }]
        );
        assert!(!msg.failed);
        assert!(msg.text.is_empty());
    }
```

- [ ] **Step 2: Run** `cargo test -p aether-core` — expect compile FAIL (`AccessMode` / `ToolStatus` missing).

- [ ] **Step 3: Implement**

In `settings.rs` after `impl Provider`:

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccessMode {
    #[default]
    Ask,
    AutoEdit,
    ReadOnly,
    Full,
}

impl AccessMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ask => "Ask",
            Self::AutoEdit => "Auto-edit",
            Self::ReadOnly => "Read only",
            Self::Full => "Full access",
        }
    }

    /// Modes `provider` can enforce, safest first; the first is a new chat's default.
    pub fn for_provider(provider: Provider) -> &'static [Self] {
        match provider {
            Provider::OpenRouter => &[],
            Provider::Pi => &[Self::ReadOnly, Self::Full],
            Provider::Claude | Provider::OpenCode => &[Self::Ask, Self::AutoEdit, Self::Full],
        }
    }
}

/// Effort values a CLI accepts; empty when aether offers no picker.
pub fn effort_levels(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Claude => &["low", "medium", "high", "xhigh", "max"],
        Provider::Pi => &["off", "minimal", "low", "medium", "high", "xhigh", "max"],
        Provider::OpenRouter | Provider::OpenCode => &[],
    }
}
```

In `lib.rs`: export `AccessMode` and `effort_levels` next to `Provider` in the `pub use settings::{…}` line. Add:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolStatus {
    Running,
    Done,
    Failed,
}
```

Add to `Update`: `Session(String)` (doc: "The CLI session's resume id; UI-only.") and `Tool { id: String, title: String, detail: String, status: ToolStatus }` (doc: "Upserts one tool step by `id`."). Add to `Step`: `Tool { id: String, title: String, detail: String, status: ToolStatus }`. In `Message::apply`, before the `Update::Error` arm:

```rust
            Update::Session(_) => {}
            Update::Tool { id, title, detail, status } => {
                let step = Step::Tool { id: id.clone(), title, detail, status };
                match self
                    .steps
                    .iter_mut()
                    .find(|s| matches!(s, Step::Tool { id: existing, .. } if *existing == id))
                {
                    Some(existing) => *existing = step,
                    None => self.steps.push(step),
                }
            }
```

Fix any other exhaustive `match` on `Update`/`Step` the compiler reports (e.g. `Update::Title(_) => {}` neighbours).

In `crates/ui/src/chat/thinking.rs`: add a `Step::Tool { title, detail, status, .. }` render arm next to `Step::Skill`, one `h_flex` row: an icon (`IconName::SquareTerminal` if available, else `IconName::Wrench`/`IconName::Settings` — pick one that exists in gpui-kit `IconName`), the title, the muted detail (`truncate`), and a status word `running` / `done` / `failed` (text, so meaning is not color-only; failed uses `cx.theme().danger`). In `summary`, count `Step::Tool` and add a `Ran N tool(s)` part in the same style as the existing searches/skills parts; extend the existing summary test with one tool step.

- [ ] **Step 4: Run** `cargo test -p aether-core && cargo test -p aether-ui && cargo build` — PASS, no new warnings.
- [ ] **Step 5: Stop.** No commit.

---

### Task 2: Store columns

**Files:**
- Modify: `crates/agent/src/store.rs`
- Modify: `crates/app/src/app.rs` (keep it compiling: `persist` fills defaults, `restore_chats` ignores the new fields with `..`)

**Interfaces:**
- Consumes: Task 1 `AccessMode`.
- Produces: `SavedChat { …, cli_session: String, access: AccessMode, effort: String, cwd: String }`.

- [ ] **Step 1: Failing test** — in `store.rs` tests, extend `saved_chats_load_back_oldest_first_and_saving_again_replaces`: set on `chat` `cli_session: "sid".into(), access: AccessMode::Full, effort: "high".into(), cwd: "/tmp/p".into()`, on `older` the defaults (`String::new()`, `AccessMode::Ask`, …), and assert after load `chats[1].cli_session == "sid"`, `chats[1].access == AccessMode::Full`, `chats[1].effort == "high"`, `chats[1].cwd == "/tmp/p"`. In `old_db_loads_as_openrouter` assert `chats[0].access == AccessMode::Ask && chats[0].cli_session.is_empty() && chats[0].cwd.is_empty()`.
- [ ] **Step 2: Run** `cargo test -p aether-agent store` — compile FAIL.
- [ ] **Step 3: Implement**
  - `SCHEMA`: add `cli_session TEXT NOT NULL DEFAULT '', access TEXT NOT NULL DEFAULT 'ask', effort TEXT NOT NULL DEFAULT '', cwd TEXT NOT NULL DEFAULT ''`.
  - Migration: same `if !cols.iter().any(|col| col == "…")` + `ALTER TABLE chats ADD COLUMN …` pattern for each of the four.
  - `load`: select the four columns (the row tuple grows to 11 `String`/`i64` fields), map `access` with `parse_access` (`"autoedit"` → AutoEdit, `"readonly"` → ReadOnly, `"full"` → Full, else Ask).
  - `save`: insert/update the four columns; write access as `access_name` (`ask|autoedit|readonly|full`).
  - `crates/app/src/app.rs`: `persist` sets `cli_session: String::new(), access: AccessMode::Ask, effort: String::new(), cwd: String::new()` for now (Task 7 wires real fields); the `SavedChat { … }` destructure in `restore_chats` ends with `..`. Import `AccessMode`.
- [ ] **Step 4: Run** `cargo test -p aether-agent && cargo build` — PASS.
- [ ] **Step 5: Stop.** No commit.

---

### Task 3: Long-lived driver contract + session manager

**Files:**
- Modify: `crates/agent/src/driver/mod.rs` (options, trait)
- Modify: `crates/agent/src/driver/{claude,pi,opencode}.rs` (compile shims only: `steer` → `prompt`, `resumed` → `false`; read new option fields nowhere yet)
- Modify: `crates/agent/src/driver/catalog.rs` (test struct literal gains the new fields)
- Modify: `crates/agent/src/agent.rs` (session manager; replaces `send_cli`)
- Modify: `crates/agent/src/lib.rs` (export `ChatSession`)
- Modify: `crates/app/src/app.rs` (pass a `ChatSession`)

**Interfaces:**
- Consumes: Task 1 `AccessMode`, `Update::Session`.
- Produces:
  - `DriverStartOptions { binary, cwd, model, effort: String, access: AccessMode, resume: String }`; `for_provider` fills `effort: String::new(), access: AccessMode::default(), resume: String::new()`.
  - `trait Driver: Send { fn prompt(&mut self, String); fn steer(&mut self, String); fn cancel(&mut self); fn answer(&mut self, u64, bool) {} fn resumed(&self) -> bool; }`
  - `pub struct ChatSession { pub key: String, pub cwd: PathBuf, pub access: AccessMode, pub effort: String, pub resume: String }`
  - `Agent::send(&self, session: &str, text: String, settings: &Settings, api_key: &str, skills: Vec<Skill>, history: &[(Role, String)], chat: &ChatSession) -> Reply`
  - `Agent::steer(&self, text: String, settings: &Settings, history: &[(Role, String)], chat: &ChatSession) -> Reply`
  - `Agent::close_session(&self, key: &str)`
  - `Agent::answer` routes CLI asks to their session.

- [ ] **Step 1: Failing tests** — add to `agent.rs` tests (fake driver + factory):

```rust
    use std::sync::Mutex as StdMutex;

    struct Fake {
        tx: UnboundedSender<Update>,
        log: Arc<StdMutex<Vec<String>>>,
        resumed: bool,
    }

    impl Driver for Fake {
        fn prompt(&mut self, text: String) {
            self.log.lock().unwrap().push(format!("prompt:{text}"));
            let _ = self.tx.unbounded_send(Update::Delta(text.clone()));
            if text == "ask" {
                let ask = aether_core::Ask::Approval { tool: "Bash".into(), title: "ls".into(), input: "ls".into() };
                let _ = self.tx.unbounded_send(Update::Ask { id: 7, ask });
            } else if !text.starts_with("hold") {
                let _ = self.tx.unbounded_send(Update::Final(text));
            }
        }
        fn steer(&mut self, text: String) {
            self.log.lock().unwrap().push(format!("steer:{text}"));
            let _ = self.tx.unbounded_send(Update::Final(text));
        }
        fn cancel(&mut self) {
            self.log.lock().unwrap().push("cancel".into());
            let _ = self.tx.unbounded_send(Update::Final(String::new()));
        }
        fn answer(&mut self, id: u64, allow: bool) {
            self.log.lock().unwrap().push(format!("answer:{id}:{allow}"));
            let _ = self.tx.unbounded_send(Update::Final(String::new()));
        }
        fn resumed(&self) -> bool {
            self.resumed
        }
    }

    fn fake_agent(resumed: bool, idle: Duration) -> (Agent, Arc<StdMutex<Vec<String>>>, Arc<AtomicU64>) {
        let log = Arc::new(StdMutex::new(Vec::new()));
        let spawns = Arc::new(AtomicU64::new(0));
        let (l, s) = (log.clone(), spawns.clone());
        let factory: Factory = Arc::new(move |_, _, tx| {
            s.fetch_add(1, Ordering::SeqCst);
            Box::new(Fake { tx, log: l.clone(), resumed })
        });
        (Agent::with_factory(factory, idle).unwrap(), log, spawns)
    }

    fn cli_settings() -> Settings {
        Settings { provider: Provider::Claude, claude_binary: "/bin/sh".into(), ..Settings::default() }
    }

    fn chat(effort: &str) -> ChatSession {
        ChatSession { key: "1".into(), cwd: "/tmp".into(), access: aether_core::AccessMode::Ask, effort: effort.into(), resume: String::new() }
    }

    fn drain(agent: &Agent, mut reply: Reply) -> Vec<Update> {
        agent.rt.block_on(async { let mut out = Vec::new(); while let Some(u) = reply.updates.next().await { out.push(u) } out })
    }

    #[test]
    fn live_session_is_reused_and_seeded_only_once() {
        let (agent, log, spawns) = fake_agent(false, Duration::from_secs(600));
        let s = cli_settings();
        let history = [(Role::User, "earlier".to_string())];
        let first = drain(&agent, agent.send("x", "hi".into(), &s, "", vec![], &history, &chat("")));
        assert!(matches!(first.last(), Some(Update::Final(_))));
        drain(&agent, agent.send("x", "again".into(), &s, "", vec![], &history, &chat("")));
        assert_eq!(spawns.load(Ordering::SeqCst), 1);
        let log = log.lock().unwrap();
        assert!(log[0].starts_with("prompt:") && log[0].contains("earlier") && log[0].contains("hi"));
        assert_eq!(log[1], "prompt:again");
    }

    #[test]
    fn resumed_session_is_not_seeded() {
        let (agent, log, _) = fake_agent(true, Duration::from_secs(600));
        let history = [(Role::User, "earlier".to_string())];
        drain(&agent, agent.send("x", "hi".into(), &cli_settings(), "", vec![], &history, &chat("")));
        assert_eq!(log.lock().unwrap()[0], "prompt:hi");
    }

    #[test]
    fn option_change_respawns() {
        let (agent, _, spawns) = fake_agent(true, Duration::from_secs(600));
        drain(&agent, agent.send("x", "a".into(), &cli_settings(), "", vec![], &[], &chat("")));
        drain(&agent, agent.send("x", "b".into(), &cli_settings(), "", vec![], &[], &chat("high")));
        assert_eq!(spawns.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn steer_ends_the_old_reply_and_continues_in_a_new_one() {
        let (agent, log, _) = fake_agent(true, Duration::from_secs(600));
        let s = cli_settings();
        let mut old = agent.send("x", "hold".into(), &s, "", vec![], &[], &chat(""));
        assert_eq!(agent.rt.block_on(old.updates.next()), Some(Update::Delta("hold".into())));
        let new = agent.steer("more".into(), &s, &[], &chat(""));
        assert_eq!(agent.rt.block_on(old.updates.next()), None);
        assert_eq!(drain(&agent, new), [Update::Final("more".into())]);
        assert_eq!(log.lock().unwrap()[1], "steer:more");
    }

    #[test]
    fn approvals_reach_the_driver_with_its_own_id() {
        let (agent, log, _) = fake_agent(true, Duration::from_secs(600));
        let mut reply = agent.send("x", "ask".into(), &cli_settings(), "", vec![], &[], &chat(""));
        agent.rt.block_on(reply.updates.next());
        let Some(Update::Ask { id, .. }) = agent.rt.block_on(reply.updates.next()) else { panic!() };
        agent.answer(id, Answer::Deny);
        drain(&agent, reply);
        assert_eq!(log.lock().unwrap()[1], "answer:7:false");
    }

    #[test]
    fn idle_sessions_are_reaped_but_running_ones_kept() {
        let (agent, _, spawns) = fake_agent(true, Duration::ZERO);
        let mut running = agent.send("x", "hold".into(), &cli_settings(), "", vec![], &[], &chat(""));
        agent.rt.block_on(running.updates.next());
        agent.reap();
        assert_eq!(agent.live.lock().unwrap().len(), 1);
        running.cancel.cancel();
        drain(&agent, running);
        agent.reap();
        assert!(agent.live.lock().unwrap().is_empty());
        drain(&agent, agent.send("x", "a".into(), &cli_settings(), "", vec![], &[], &chat("")));
        assert_eq!(spawns.load(Ordering::SeqCst), 2);
    }
```

Update the two existing missing-binary tests to pass `&chat("")`-style `ChatSession` (key `"m"`, cwd `/tmp`).

- [ ] **Step 2: Run** `cargo test -p aether-agent agent::tests` — compile FAIL.

- [ ] **Step 3: Implement**

`driver/mod.rs`: extend `DriverStartOptions` and `for_provider` as in Interfaces; replace the trait with:

```rust
pub trait Driver: Send {
    /// Starts a turn with `text`.
    fn prompt(&mut self, text: String);
    /// Adds `text` to the running turn; with no turn running, same as `prompt`.
    fn steer(&mut self, text: String);
    /// Stops the running turn; the session stays alive.
    fn cancel(&mut self);
    /// Answers the driver's own Ask `id`.
    fn answer(&mut self, _id: u64, _allow: bool) {}
    /// True when a non-empty `resume` id was reattached.
    fn resumed(&self) -> bool;
}
```

Existing drivers: add `fn steer(&mut self, text: String) { self.prompt(text) }` and `fn resumed(&self) -> bool { false }` (Tasks 4-6 replace them).

`agent.rs` — add (names exact; adapt imports):

```rust
pub(crate) type Factory =
    Arc<dyn Fn(Provider, DriverStartOptions, UnboundedSender<Update>) -> Box<dyn Driver> + Send + Sync>;

/// Per-chat inputs of a CLI session; the app owns them.
pub struct ChatSession {
    pub key: String,
    pub cwd: PathBuf,
    pub access: AccessMode,
    pub effort: String,
    pub resume: String,
}

#[derive(Clone, Debug, PartialEq)]
struct SessionKey {
    provider: Provider,
    binary: String,
    cwd: PathBuf,
    model: String,
    effort: String,
    access: AccessMode,
}

enum Command {
    Prompt(String, Vec<(Role, String)>),
    Steer(String),
    Cancel,
    Answer(u64, bool),
}

type TurnTx = Arc<Mutex<Option<UnboundedSender<Update>>>>;

struct Live {
    key: SessionKey,
    commands: std::sync::mpsc::Sender<Command>,
    turn: TurnTx,
    last_active: Arc<Mutex<Instant>>,
}
```

`Agent` gains `live: Arc<Mutex<HashMap<String, Live>>>`, `cli_asks: Arc<Mutex<HashMap<u64, (String, u64)>>>`, `factory: Factory`, `idle: Duration`. `Agent::new()` = `Self::with_factory(Arc::new(real_driver), Duration::from_secs(600))`; `pub(crate) fn with_factory(factory, idle) -> io::Result<Self>` builds the struct and spawns the reaper:

```rust
        let weak = Arc::downgrade(&live);
        std::thread::spawn(move || {
            while let Some(live) = { std::thread::sleep(Duration::from_secs(30)); weak.upgrade() } {
                reap(&live, idle);
            }
        });
```

with free fns:

```rust
fn real_driver(p: Provider, opts: DriverStartOptions, tx: UnboundedSender<Update>) -> Box<dyn Driver> {
    match p {
        Provider::Claude => Box::new(ClaudeDriver::spawn(opts, tx)),
        Provider::Pi => Box::new(PiDriver::spawn(opts, tx)),
        Provider::OpenCode => Box::new(OpencodeDriver::spawn(opts, tx)),
        Provider::OpenRouter => unreachable!("OpenRouter has no driver"),
    }
}

/// Drops sessions with no running turn that sat idle for `idle`.
fn reap(live: &Mutex<HashMap<String, Live>>, idle: Duration) {
    live.lock().unwrap().retain(|_, s| {
        s.turn.lock().unwrap().is_some() || s.last_active.lock().unwrap().elapsed() < idle
    });
}
```

and `#[cfg(test)] fn reap(&self) { reap(&self.live, self.idle) }` on `Agent` (name the free fn `reap_idle` if the method name clashes).

`spawn_live(&self, chat_key: &str, key: SessionKey, opts: DriverStartOptions) -> Live`:
- driver thread: `let mut driver = factory(provider, opts, driver_tx); let mut seeded = driver.resumed();` then `while let Ok(command) = rx.recv()`: `Prompt(text, history)` → `let text = if seeded || history.is_empty() { text } else { context_prompt(&history, &text) }; seeded = true; driver.prompt(text)`; `Steer(text)` → `seeded = true; driver.steer(text)`; `Cancel` → `driver.cancel()`; `Answer(id, allow)` → `driver.answer(id, allow)`. Loop ends when every command sender is gone, dropping the driver.
- pump thread: `while let Some(update) = futures::executor::block_on(driver_rx.next())`: touch `last_active`; `Update::Ask { id: local, ask }` → `id = next_request.fetch_add(1)`, `cli_asks.insert(id, (chat_key, local))`, forward `Update::Ask { id, ask }`; lock `turn`, send to it if `Some`, and if the update is `Final` set it to `None`. After the loop, `turn.lock().unwrap().take()`. The pump must not hold a `commands` sender (it would keep the driver alive).

`send` for CLI providers calls `send_cli(text, settings, chat, history, tx, updates)` which: keeps the missing-binary preflight; builds `opts = DriverStartOptions { effort: chat.effort.clone(), access: chat.access, resume: chat.resume.clone(), ..DriverStartOptions::for_provider(settings, provider, chat.cwd.clone()) }`; builds `SessionKey` from `opts` + provider; under the `live` lock removes a session whose key differs, inserts `spawn_live` if missing, sets `turn = Some(tx)`, touches `last_active`, sends `Command::Prompt(text, history.to_vec())`; returns `Reply { updates, cancel: Cancel(CancelInner::Cli(session.commands.clone())) }`. `CancelInner::Cli` now holds `std::sync::mpsc::Sender<Command>` and `cancel()` sends `Command::Cancel`.

`steer`: under the `live` lock, if the chat has a session, replace its `turn` with a new channel (dropping the old sender ends the old reply), touch `last_active`, send `Command::Steer(text)`, return the new `Reply`; otherwise fall through to `send_cli` (same as a normal send). For OpenRouter the app never calls `steer`.

`answer`: first `if let Some((key, local)) = self.cli_asks.lock().unwrap().remove(&id)` → send `Command::Answer(local, answer_allows(&answer))` to that key's session and return; else the existing gate path.

`close_session(key)`: `self.live.lock().unwrap().remove(key);`

Delete the old per-message `send_cli` thread code. Keep `context_prompt`, `answer_allows`, `missing_binary_error`. Mark the steer race with one comment line: `// ponytail: a steer landing as the turn ends starts a new turn whose reply is dropped; rare, needs turn ids to fix`.

`crates/app/src/app.rs` `submit`: pass `&ChatSession { key: chat.id.to_string(), cwd: <settings.workspace_root if set, else aether_agent::driver::spawn::home_dir()>, access: AccessMode::Ask, effort: String::new(), resume: String::new() }` (Task 7 replaces with chat fields).

- [ ] **Step 4: Run** `cargo test -p aether-agent && cargo build` — PASS, no new warnings.
- [ ] **Step 5: Stop.** No commit.

---

### Task 4: Claude driver rewrite

**Files:** Rewrite `crates/agent/src/driver/claude.rs` (keep `ClaudeDriver::spawn(opts, tx)`; replace fake-shape tests).

**Interfaces:** Consumes Task 3 trait/options and Task 1 `Update::{Session, Tool}`, `ToolStatus`, `AccessMode`. Produces a `Driver` per the spec's Claude section.

Requirements (wire per `docs/wayfinder/research/claude-sessions.md`):
- Spawn args: `-p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --replay-user-messages --permission-prompt-tool stdio`, `--permission-mode` (Ask `default`, AutoEdit `acceptEdits`, Full `bypassPermissions` + `--dangerously-skip-permissions`; ReadOnly never reaches Claude, treat as Ask), `--model` / `--effort` when non-empty, session: `resume` non-empty and `<config>/projects/*/<resume>.jsonl` exists (config = `$CLAUDE_CONFIG_DIR` or `~/.claude`) → `--resume <id>` and `resumed() = true`; else generate a v4 UUID (from `/dev/urandom` or `std::collections::hash_map::RandomState`-free std code — no new crate; format `8-4-4-4-12` lowercase hex with version/variant bits) → `--session-id <uuid>`. Emit `Update::Session(id)` right after spawn.
- One process for the session's life. Turn state: `active` flag; `prompt` writes the user payload and sets active; `steer` writes the same payload when active, else behaves as `prompt`; `cancel` writes `{"type":"control_request","request_id":"<n>","request":{"subtype":"interrupt"}}` and does NOT kill; turn settles on `result` (emit usage `Count`/`Usage` if present, then `Final(result text)` once) — if `result` doesn't arrive within 10 s of cancel, emit `Final("")` and kill+respawn is NOT required (just mark the driver dead; later prompts emit `Error` + `Final("")`).
- Mapping (pure fn `map_line(state: &mut State, line: &str) -> Vec<Update>`, tested): `stream_event` text_delta → `Delta`, thinking_delta → `Thinking`; `assistant` message content `tool_use{id,name,input}` → `Tool{Running}` (title/detail per spec table); `user` content `tool_result{tool_use_id,is_error}` → `Tool{Done|Failed}` reusing the cached title/detail; skip `isReplay:true` and any `parent_tool_use_id` non-null; `control_request` subtype `can_use_tool` → `Ask::Approval { tool: request.tool_name, title: <display_name or tool_name>, input: <detail of request.input> }` with a driver-local numeric id mapped to the string `request_id`; `result` → `Final`; `result` with `is_error` → `Error(result text)` then `Final("")`.
- `answer(id, allow)`: `{"type":"control_response","response":{"subtype":"success","request_id":"<orig>","response":{"behavior":"allow","updatedInput":<orig input>}}}` or `{"behavior":"deny","message":"Denied in Aether"}`.
- Drop: close stdin, kill after 3 s.

- [ ] **Step 1: Failing tests** — mapping tests using these exact lines:
  - `{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}}` → `[Delta("hi")]`
  - `{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"echo hi"}}]}}` → `[Tool{id:"toolu_1", title:"Run command", detail:"echo hi", status:Running}]`
  - `{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"hi","is_error":true}]}}` (after the line above, same state) → `[Tool{… status:Failed}]`
  - same `assistant` line with `"parent_tool_use_id":"toolu_9"` → `[]`; a `user` line with `"isReplay":true` → `[]`
  - `{"type":"control_request","request_id":"req-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"rm x"}}}` → one `Ask{ask: Approval{tool:"Bash", input:"rm x", ..}}`, and `answer` on that id writes a line containing `"request_id":"req-1"` and `"behavior":"deny"` (test via a `Vec<u8>` writer seam or by unit-testing the reply-builder fn)
  - `{"type":"result","subtype":"success","is_error":false,"result":"done","usage":{"input_tokens":3,"output_tokens":2}}` → ends with `Final("done")`
  - argument builder test: Full + effort `high` + model `opus` + missing resume file → contains `--permission-mode bypassPermissions`, `--dangerously-skip-permissions`, `--effort high`, `--model opus`, `--session-id`, not `--resume`.
- [ ] **Step 2: Run** `cargo test -p aether-agent driver::claude` — FAIL.
- [ ] **Step 3: Implement** per requirements.
- [ ] **Step 4: Run** `cargo test -p aether-agent && cargo build` — PASS.
- [ ] **Step 5: Live smoke** — add `#[test] #[ignore] fn live_claude_smoke()` that: makes a temp dir; spawns with access Ask, empty model/effort/resume; `prompt("Reply with exactly: ok")`; collects updates until `Final` (120 s timeout); asserts a `Session(id)` and text containing `ok`; drops; respawns with `resume: id`; asserts `resumed()`; `prompt("Reply with exactly: again")` → text contains `again`; then `prompt("Count from 1 to 300, one number per line.")`, `cancel()` after the first `Delta`, expects `Final` within 15 s, then `prompt("Reply with exactly: ok")` works on the same driver. Run once: `cargo test -p aether-agent live_claude_smoke -- --ignored --nocapture`. If `--permission-mode default` is rejected, switch Ask to `manual` and note it. Record the output in the report.
- [ ] **Step 6: Stop.** No commit.

---

### Task 5: Pi driver rewrite

**Files:** Rewrite `crates/agent/src/driver/pi.rs` (keep `PiDriver::spawn(opts, tx)`; replace fake-shape tests).

Requirements (wire per `docs/wayfinder/research/pi-sessions.md`):
- Spawn `pi --mode rpc --approve`, env `PI_SKIP_VERSION_CHECK=1`, cwd = `opts.cwd`, `--thinking <effort>` when set, `--tools read,grep,find,ls` when access is ReadOnly (Ask/AutoEdit never reach Pi; treat as ReadOnly).
- Handshake on a reader thread with a request-id → response map and 30 s timeout: `get_state`; if `resume` non-empty and the file exists → `switch_session{sessionPath}` (`data.cancelled` false → `resumed() = true`); `set_model{provider,modelId}` when model set (split at first `/`); `get_state` → emit `Session(data.sessionFile)`. Handshake failure → `Error`, driver dead (prompts emit `Error` + `Final("")`).
- `prompt` → `{"id":"aether-N","type":"prompt","message":…}` (not awaited); `steer` while active → `{"id":…,"type":"steer","message":…}`, else prompt; `cancel` → `{"type":"abort"}`, never kill; settle on `agent_settled`, on prompt response `success:false` (emit `Error(error)` first), or `data.disposition == "handled"`; exactly one `Final` per turn: the concatenated assistant text of the turn (gap-filled from `message_end` content when no deltas arrived).
- Mapping (pure `map_event(state, &Value) -> Vec<Update>`): `message_update.assistantMessageEvent` `text_delta{delta}` → `Delta`, `thinking_delta{delta}` → `Thinking`, `error` → `Error`; `tool_execution_start{toolCallId,toolName,args}` → `Tool{Running}`; `tool_execution_end{isError}` → `Tool{Done|Failed}`; `message_end` with assistant usage → `Count`; `extension_ui_request` `confirm` → `Ask::Approval { tool: "Pi", title, input: message }`, `select` → `Ask::Question { question: title, choices: options }`, others ignored; `answer` → `{"type":"extension_ui_response","id":…,"confirmed":allow}` (or `"cancelled":true` when deny on select).
- Drop: close stdin, kill after 3 s.

- [ ] **Step 1: Failing tests** with these lines (one state through the sequence):
  - `{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"he"}}` → `[Delta("he")]`
  - `{"type":"tool_execution_start","toolCallId":"call_1","toolName":"bash","args":{"command":"ls -la"}}` → `[Tool{id:"call_1",title:"Run command",detail:"ls -la",status:Running}]`
  - `{"type":"tool_execution_end","toolCallId":"call_1","toolName":"bash","result":{"content":[{"type":"text","text":"x"}]},"isError":false,"durationMs":12}` → `[Tool{… status:Done}]`
  - `{"type":"agent_settled","aborted":false}` → `[Final("he")]`
  - `{"type":"response","id":"aether-1","command":"prompt","success":true,"data":{"disposition":"handled"}}` while active → ends the turn with one `Final`
  - args test: ReadOnly + effort `high` → contains `--tools read,grep,find,ls` and `--thinking high`; model `openrouter/z-ai/glm-5` → `set_model` payload `{"provider":"openrouter","modelId":"z-ai/glm-5"}`.
- [ ] **Step 2-4:** run FAIL → implement → `cargo test -p aether-agent && cargo build` PASS.
- [ ] **Step 5: Live smoke** — `#[ignore] fn live_pi_smoke()` mirroring Task 4 (ReadOnly access; fresh → `Session(file)` + `ok`; respawn with `resume` → `resumed()` + `again`; cancel check with `abort` then a working prompt). Run once with `-- --ignored --nocapture`; record output.
- [ ] **Step 6: Stop.** No commit.

---

### Task 6: OpenCode driver rewrite + service detection

**Files:** Rewrite `crates/agent/src/driver/opencode.rs`; modify `crates/agent/src/driver/catalog.rs` (installed check) and `crates/agent/src/agent.rs` (`missing_binary_error`, remove `opencode_hub_reachable` port-4096 logic in favour of a shared `driver::opencode::service_reachable()`).

Requirements (wire per `docs/wayfinder/research/opencode-sessions.md`):
- Service: parse `${XDG_STATE_HOME:-$HOME/.local/state}/opencode/service.json` (fields per research: url + password); every request and the SSE stream use Basic `opencode:<password>`. Missing/unreachable → spawn `opencode serve --service` (binary via `spawn::command`), poll the file up to 20 s. `pub fn service_reachable() -> bool` = file parses and `GET {url}/api/info` answers within 1 s (blocking, used only off the UI thread).
- Session id: `resume` non-empty → `GET /api/session/{id}` 200 → reuse, `resumed() = true`; 404 → create under that id; empty → mint `ses_…` per research format. Subscribe to `GET /api/event` (SSE, filtered by `sessionID`) before create. Create body `{"id", "location":{"directory": canonical cwd, no trailing slash}, "model":{"providerID","id","variant"?}}` (model omitted when empty; `variant` = effort when set). Emit `Session(id)`.
- `prompt` → `POST /api/session/{id}/prompt {"text"}`; `steer` while active → `{"text","delivery":"steer"}` and `PATCH /api/session/{id}/inbox/{inboxId} {"delivery":"steer"}` if the response delivery isn't `steer`; else prompt. `cancel` → `POST /api/session/{id}/interrupt`; wait for the execution event (10 s fallback `Final("")`).
- Settle on `session.execution.succeeded|failed|interrupted` (not reason `shutdown`): `failed` emits `Error(error.message)` first; then `Final(turn text)` once.
- Mapping (pure `map_event(state, type, &Value) -> Vec<Update>`): `session.text.delta{delta}` → `Delta`; `session.reasoning.delta` → `Thinking`; `session.tool.input.started{id,name}` → `Tool{Running}` (title from name per spec table); `session.tool.called{id,input}` → `Tool{Running}` with detail from input; `session.tool.success` → `Done`; `session.tool.failed` → `Failed`; `permission.asked{id,action,resources}` → auto-reply `once` when access is Full, or AutoEdit and action ∈ {edit, write, patch}; else `Ask::Approval { tool: action, title: resources[0], input: resources.join(" ") }`; `answer` → `POST /api/session/{sid}/permission/{requestId}/reply {"decision":"once"|"reject"}`; `session.renamed{title}` → `Title`.
- Drop: unsubscribe (stop the SSE thread); never delete the session or stop the service.
- HTTP: use `reqwest` blocking if the crate's enabled features allow it; otherwise drive async `reqwest` on a small current-thread tokio runtime owned by the driver (tokio is already a dependency). Do not add features that change other crates' behaviour without noting it.

- [ ] **Step 1: Failing tests** (pure mapping/parse):
  - `service.json` parse from a fixture string with the field names the research doc shows → url + password; missing file → `None`.
  - `{"type":"session.text.delta","data":{"sessionID":"ses_1","delta":"hi"}}` → `[Delta("hi")]`
  - `{"type":"session.tool.input.started","data":{"sessionID":"ses_1","id":"call_1","name":"bash"}}` → `[Tool{id:"call_1",title:"Run command",detail:"",Running}]`; then `session.tool.called` with `"input":{"command":"ls"}` → detail `ls`; then `session.tool.failed` → `Failed`
  - `{"type":"session.execution.succeeded","data":{"sessionID":"ses_1"}}` → `[Final("hi")]`; `session.execution.interrupted` with `"reason":"shutdown"` → `[]`
  - `{"type":"permission.asked","data":{"id":"per_1","sessionID":"ses_1","action":"edit","resources":["src/a.rs"]}}` → AutoEdit: auto-reply decision `once` (assert via the reply-decision fn), Ask: one `Ask::Approval{tool:"edit", title:"src/a.rs", ..}`
  - create-body builder: model `anthropic/claude-x`, effort `high`, cwd `/tmp/p/` → `{"id":…,"location":{"directory":"/tmp/p"} or its canonical form,"model":{"providerID":"anthropic","id":"claude-x","variant":"high"}}`
  - minted id matches the research format (prefix `ses_`, length/charset).
- [ ] **Step 2-4:** run FAIL → implement → `cargo test -p aether-agent && cargo build` PASS.
- [ ] **Step 5: Live smoke** — `#[ignore] fn live_opencode_smoke()` mirroring Task 4 against the user's running service (Ask access; fresh → `Session(ses_…)` + `ok`; new driver with `resume` → `resumed()` + `again`; cancel check). Run once; record output.
- [ ] **Step 6: Stop.** No commit.

---

### Task 7: App per-chat options, persistence, composer chips

**Files:** Modify `crates/app/src/app.rs` (and `settings_view.rs` only if a shared helper is needed).

**Interfaces:** Consumes Tasks 1-3 (`AccessMode`, `effort_levels`, `Update::Session`, `SavedChat` fields, `ChatSession`, `Agent::close_session`).

- [ ] **Step 1: Failing tests** (pure helpers in `app.rs`):

```rust
    #[test]
    fn new_chat_access_defaults_follow_the_provider() {
        assert_eq!(default_access(Provider::Pi), AccessMode::ReadOnly);
        assert_eq!(default_access(Provider::Claude), AccessMode::Ask);
        assert_eq!(fit_access(Provider::Pi, AccessMode::Ask), AccessMode::ReadOnly);
        assert_eq!(fit_access(Provider::Claude, AccessMode::Full), AccessMode::Full);
    }

    #[test]
    fn folder_is_fixed_at_first_send() {
        let mut chat = chat_with("t", &[]);
        assert_eq!(chat_cwd(&chat, "/w"), std::path::PathBuf::from("/w"));
        chat.cwd = "/fixed".into();
        assert_eq!(chat_cwd(&chat, "/w"), std::path::PathBuf::from("/fixed"));
        assert_eq!(folder_label("/Users/me/code/aether"), "aether");
    }
```

- [ ] **Step 2: Run** `cargo test -p aether -- default_access folder_is_fixed` — FAIL.
- [ ] **Step 3: Implement**
  - `Chat` gains `cwd: String`, `access: AccessMode`, `effort: String`, `cli_session: String`, `queue: Vec<String>`, `stopped: bool`; every `Chat { … }` literal (new_chat, duplicate, restore, tests) sets them (duplicate copies `cwd/access/effort`, not `cli_session`; restore reads them from `SavedChat`); `persist` writes the real values.
  - Helpers: `fn default_access(p) -> AccessMode { AccessMode::for_provider(p).first().copied().unwrap_or_default() }`; `fn fit_access(p, current) -> AccessMode` (current if offered, else default); `fn chat_cwd(chat, workspace_root: &str) -> PathBuf` (chat.cwd, else workspace_root, else `aether_agent::driver::spawn::home_dir()`); `fn folder_label(path: &str) -> String` (last path component; `Home` when equal to home dir).
  - `new_chat` sets `access: default_access(provider)`; `pick_provider` sets `chat.access = fit_access(provider, chat.access)` and clears `effort`; the reuse branch of `new_chat` also refits access.
  - `submit`: on the first message of a CLI chat set `chat.cwd = chat_cwd(..).display().to_string()`; build `ChatSession { key: chat.id.to_string(), cwd, access, effort, resume: chat.cli_session.clone() }`.
  - `apply`: on `Update::Session(id)` set `chats[ix].cli_session = id` and `persist(ix)` (before forwarding to `Message::apply`).
  - `rewind` and `delete_chat` call `self.agent.close_session(&chat.id.to_string())`; `rewind` also clears `cli_session`.
  - Composer (CLI chats only, between the provider chip and the model picker): folder chip (`Button` ghost small, label `folder_label`, tooltip full path or "Folder is fixed for this chat" when locked, click → `cx.prompt_for_paths` directories-only setting `chat.cwd` and persisting; disabled when the chat has messages); access chip (`dropdown_menu` of `AccessMode::for_provider`, checked current, disabled while busy; selection sets `chat.access`, persists if the chat has messages); effort chip (`dropdown_menu` of `Default` + `effort_levels(provider)`, hidden when the list is empty, disabled while busy). Same `Button` styling as the provider chip.
- [ ] **Step 4: Run** `cargo build && cargo test -p aether` — PASS, no new warnings.
- [ ] **Step 5: Stop.** No commit.

---

### Task 8: Queue, steer, and stop semantics

**Files:** Modify `crates/app/src/app.rs` (+ `crates/app/src/suggest.rs` only if `/stop` path changes).

**Interfaces:** Consumes Task 3 `Agent::steer`, Task 7 `Chat.queue/stopped`.

- [ ] **Step 1: Failing tests** (pure helper):

```rust
    #[test]
    fn queue_only_advances_after_an_unstopped_run() {
        let mut chat = chat_with("t", &[]);
        chat.queue = vec!["a".into(), "b".into()];
        assert_eq!(next_queued(&mut chat), Some("a".to_string()));
        chat.stopped = true;
        assert_eq!(next_queued(&mut chat), None);
        assert_eq!(chat.queue, ["b"]);
    }
```

- [ ] **Step 2: Run** `cargo test -p aether -- queue_only` — FAIL.
- [ ] **Step 3: Implement**
  - `fn next_queued(chat: &mut Chat) -> Option<String>`: `None` when `stopped` or empty, else `Some(queue.remove(0))`.
  - `submit(&mut self, ix: usize, text: String, steer: bool, window, cx)`: operates on chat `ix` (scroller/remeasure only when `ix == self.active`); clears `stopped`; when `steer` and the chat is busy on a CLI provider: do not cancel the old run (drop `chat.cancel` without calling it), append `Message::user(text)` + a new assistant message, and take the `Reply` from `agent.steer(...)`; OpenRouter steer = existing cancel + send. All existing callers pass `self.active, …, false`.
  - `send`: when the active chat is busy and not editing, push the text to `chat.queue`, clean the composer, `cx.notify()`, return.
  - `finish_run` gains `window` (call site switches to `this.update_in(cx, |this, window, cx| …)`); after finishing, if `chat.run == run`, `if let Some(text) = next_queued(chat)` → `self.submit(ix, text, false, window, cx)`.
  - `stop`: sets `stopped = true` before cancelling.
  - Queue UI: in `composer_card`, when the active chat's queue is non-empty, render a `v_flex` of rows above the editing banner (in the `Composer::header`): each row = one-line truncated text, a `Steer now` ghost xsmall button (label `Send` when idle) calling `steer_queued(i)`, and a `Remove` button. `steer_queued(i)`: remove item `i`; busy → `submit(active, text, true, …)`; idle → `submit(active, text, false, …)`.
- [ ] **Step 4: Run** `cargo build && cargo test --workspace` — PASS, no new warnings.
- [ ] **Step 5: Stop.** No commit.
