# CLI Providers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wrap `claude`, `pi`, and `opencode` CLIs as alternate providers in `crates/agent/src/driver/`, coexisting with the OpenRouter path.

**Architecture:** Shared `Driver` trait + `spawn` helper in `crates/agent/src/driver/`; each CLI translates its stream into `aether-core::Update` direct on the `Reply` channel; `Agent::send` routes OpenRouter to existing `Turn`, CLIs to drivers.

**Tech Stack:** Rust, tokio process + channels, SQLite via sqlx, serde_json / NDJSON / SSE parsing with existing deps only.

**Spec:** `docs/wayfinder/map.md` plus decisions in `docs/wayfinder/tickets/04-driver-trait.md` through `docs/wayfinder/tickets/11-ship-order.md` and wire notes in `docs/wayfinder/research/{claude-wire,opencode-wire,pi-wire}.md` — the plan argues from those; executors read both.

## Global Constraints

- In-process spawn in `crates/agent/src/driver/` only. No daemon, no `waku-daemon` split.
- Reuse `aether-core::Update`; add only `Update::Title(String)` variant.
- First cut = prompt + stream + cancel. No `steer()` on the trait; cancel+reprompt only.
- One shared `spawn.rs` (`find_binary`, `command`, `spawn`, `visible_stderr`); drivers only add args.
- `Settings.model` stays OpenRouter-only; CLI models are opaque free-text via `provider_models` map, empty = omit `--model`.
- Old DBs must load as OpenRouter via `ALTER TABLE ADD COLUMN` defaults.

## Review Focus

- Missing binary shows `"<name> is not installed"` + hint as `Update::Error`, not a panic — needs a test with bogus override path.
- Stderr noise (`transport closed`, heartbeats) must not surface as errors; only `error:` / `fatal` / `no such file` lines do.
- Cancel mid-stream keeps partial text + `Final("")` settle, emits no `Update::Error`.
- `Update::Title` never touches `Message.text`; it is UI-only.
- Old `chats` DB without new columns loads with provider OpenRouter and empty model.

---

### Task 1: `Update::Title` + `Message::apply` no-op

**Files:**
- Modify: `crates/core/src/lib.rs:23-48`
- Test: `crates/core/src/lib.rs:260-335` (extend existing `tests` mod)

**Interfaces:**
- Consumes: nothing.
- Produces: `Update::Title(String)` used by Tasks 5/6/7 and `Message::apply` no-op used by UI.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn title_update_leaves_text_alone() {
    let mut msg = Message::assistant();
    msg.apply(Update::Delta("hi".into()));
    msg.apply(Update::Title("New title".into()));
    assert_eq!(msg.text, "hi");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether-core title_update_leaves_text_alone -v`
Expected: FAIL with `Title` variant not defined.

- [ ] **Step 3: Implement in `crates/core/src/lib.rs`**

Add `Title(String)` to `Update` enum; add `Update::Title(_) => {}` arm in `Message::apply` (before `Update::Error` arm). No other `apply` change.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether-core -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/lib.rs
git commit -m "feat(core): add Update::Title as UI-only signal"
```

### Task 2: `Settings` provider surface + `Provider` enum

**Files:**
- Modify: `crates/core/src/settings.rs:52-66`
- Test: `crates/core/src/settings.rs:121-139` (extend)

**Interfaces:**
- Consumes: Task 1 (nothing direct).
- Produces: `Provider::{OpenRouter,Claude,Pi,OpenCode}` + `Settings.{provider,provider_models,claude_binary,pi_binary,opencode_binary,workspace_root}` consumed by Tasks 4/5/6/7/8.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn provider_defaults_to_openrouter_with_empty_overrides() {
    let s = Settings::default();
    assert_eq!(s.provider, Provider::OpenRouter);
    assert!(s.provider_models.is_empty() && s.claude_binary.is_empty());
    let parsed: Settings = serde_json::from_str(r#"{"model":"x"}"#).unwrap();
    assert_eq!(parsed.provider, Provider::OpenRouter);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether-core provider_defaults -v`
Expected: FAIL, no `Provider` type.

- [ ] **Step 3: Implement in `crates/core/src/settings.rs`**

```rust
pub enum Provider { OpenRouter, Claude, Pi, OpenCode }
```
Serde `rename_all="lowercase"`, `Default = OpenRouter`. Add to `Settings`: `provider: Provider`, `provider_models: BTreeMap<String,String>`, `claude_binary/pi_binary/opencode_binary/workspace_root: String`, all `#[serde(default)]`. Keep `model` untouched.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether-core -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/settings.rs
git commit -m "feat(core): add provider and per-CLI model/binary settings"
```

### Task 3: `driver` module trait + start options

**Files:**
- Create: `crates/agent/src/driver/mod.rs`
- Modify: `crates/agent/src/lib.rs:1-12` (add `pub mod driver;`)
- Test: `crates/agent/src/driver/mod.rs` unit test for options resolution

**Interfaces:**
- Consumes: Task 2 `Provider` + `Settings`.
- Produces: `DriverStartOptions { binary: String, cwd: PathBuf, model: String }`, `trait Driver { prompt, cancel }`, `close` convention, consumed by Tasks 4–8.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn start_options_reads_provider_model_map() {
    let mut s = Settings::default();
    s.provider_models.insert("claude".into(), "sonnet".into());
    let opts = DriverStartOptions::for_provider(&s, Provider::Claude, "/tmp");
    assert_eq!(opts.model, "sonnet");
    assert_eq!(opts.binary, "");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether-agent driver::tests -v`
Expected: FAIL, module not found.

- [ ] **Step 3: Implement `crates/agent/src/driver/mod.rs`**

```rust
pub struct DriverStartOptions { pub binary: String, pub cwd: PathBuf, pub model: String }
impl DriverStartOptions {
    pub fn for_provider(s: &Settings, p: Provider, cwd: impl Into<PathBuf>) -> Self
}
pub trait Driver: Send {
    fn prompt(&mut self, text: String);
    fn cancel(&mut self);
}
```
`for_provider` maps Claude→`claude` key + `claude_binary`, Pi→`pi`, OpenCode→`opencode`; empty map value = `""`. Drivers emit `Update` direct via `UnboundedSender<Update>` handed at construction. `close` = graceful wire-stop then drop; no trait method first cut (plain `Drop` + `cancel`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether-agent driver -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/agent/src/driver/mod.rs crates/agent/src/lib.rs
git commit -m "feat(agent): add driver trait and start options"
```

### Task 4: Shared `spawn` helper

**Files:**
- Create: `crates/agent/src/driver/spawn.rs`
- Test: same file `#[cfg(test)]`

**Interfaces:**
- Consumes: Task 3 `DriverStartOptions`.
- Produces: `find_binary(name, override) -> Option<PathBuf>`, `visible_stderr(line) -> bool`, `spawn_logged(cmd) -> Result<Child>` used by Tasks 5/6/7.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn missing_override_falls_back_to_path_search() {
    assert!(find_binary("definitely-not-a-binary-xyz", "").is_none());
}
#[test]
fn stderr_filter_hides_transport_noise() {
    assert!(!visible_stderr("transport closed"));
    assert!(visible_stderr("error: connection refused"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether-agent spawn -v`
Expected: FAIL, functions undefined.

- [ ] **Step 3: Implement in `crates/agent/src/driver/spawn.rs`**

`find_binary`: expand `~/`, try override first, else login-shell `PATH` + `~/.local/bin`, `~/.cargo/bin`, `/opt/homebrew/bin`, `/usr/local/bin`. `command()`: `tokio::process::Command` with `current_dir(cwd)`, inherit env + repaired `PATH`. `visible_stderr`: true only for `error:`, `fatal`, `no such file` (case-insensitive). Pipe stderr, never stdout.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether-agent spawn -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/agent/src/driver/spawn.rs
git commit -m "feat(agent): add shared CLI spawn and stderr filter"
```

### Task 5: Claude + Pi drivers (one child per session)

**Files:**
- Create: `crates/agent/src/driver/claude.rs`, `crates/agent/src/driver/pi.rs`
- Modify: `crates/agent/src/driver/mod.rs` (re-export)
- Test: `crates/agent/src/driver/claude.rs`, `pi.rs` parser tests with canned lines

**Interfaces:**
- Consumes: Tasks 3–4 + Task 1 mapping.
- Produces: `ClaudeDriver::spawn(opts, tx)`, `PiDriver::spawn(opts, tx)` implementing `Driver`, consumed by Task 8.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn claude_text_event_becomes_delta() {
    assert_eq!(map_claude_line(r#"{"type":"stream_event","delta":{"text":"hi"}}"#), Some(Update::Delta("hi".into())));
}
#[test]
fn pi_message_end_gap_fills_final() {
    assert_eq!(map_pi_line(r#"{"type":"message_end","full_text":"done"}"#), Some(Update::Final("done".into())));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p aether-agent driver::claude driver::pi -v`
Expected: FAIL, constructors missing.

- [ ] **Step 3: Implement both drivers**

Claude args: `-p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --permission-prompt-tool stdio` + `--model <m>` when non-empty; stdin JSONL `user` payload; `interrupt` on cancel; title poll via `session_metadata` → `Update::Title`. Pi args: `--mode rpc` + `--model <m>`; `PI_SKIP_VERSION_CHECK=1` env for pi only; handshake `negotiate v2 → get_state → set_model?`; `abort` on cancel. Mapping both: text→`Delta`, terminal full→`Final`, reasoning→`Thinking`, usage→`Count+Usage`, tool narration→`Delta` code fence, tool allow/deny→`Ask::Approval`, questions→`Ask::Question`, tool failure alone never `Error`. Missing binary → `Update::Error("<name> is not installed ...")`. Cancel → graceful wire-stop, SIGKILL after 3s, keep partial + `Final("")`, no `Error`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p aether-agent driver -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/agent/src/driver/
git commit -m "feat(agent): add claude and pi drivers"
```

### Task 6: OpenCode driver (shared `serve` hub)

**Files:**
- Create: `crates/agent/src/driver/opencode.rs`
- Test: same file SSE/event mapping tests

**Interfaces:**
- Consumes: Tasks 3–4.
- Produces: `OpencodeDriver::spawn(opts, tx)` implementing `Driver`, consumed by Task 8.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn session_renamed_becomes_title() {
    assert_eq!(map_opencode_event("session.renamed", r#"{"title":"T"}"#), Some(Update::Title("T".into())));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether-agent opencode -v`
Expected: FAIL.

- [ ] **Step 3: Implement `crates/agent/src/driver/opencode.rs`**

Adopt-or-spawn one shared `opencode serve --service` (idempotent, 20s budget); subscribe-before-create `POST /api/session` with client id; `GET /api/event` SSE hub routed by `sessionID`; `POST prompt`, `POST interrupt` on cancel; `execution.succeeded/failed/interrupted` settles turn with `Final` gap-fill; `session.renamed`→`Title` (filter placeholder); `permission.asked`→`Ask::Approval`. `close` = unsubscribe only, never kills hub. Same cancel/settle guarantees as Task 5.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether-agent opencode -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/agent/src/driver/opencode.rs
git commit -m "feat(agent): add opencode shared-hub driver"
```

### Task 7: `Agent::send` routing + Store persistence

**Files:**
- Modify: `crates/agent/src/agent.rs:107-147`, `crates/agent/src/store.rs:10-129`
- Test: `crates/agent/src/store.rs:131-206` (migration test), `agent.rs` routing test with closed provider

**Interfaces:**
- Consumes: Tasks 1–6.
- Produces: working provider switch + persisted `provider` / `provider_model` per chat.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn old_db_loads_as_openrouter() { /* legacy table without columns → provider OpenRouter */ }
#[test]
fn cli_send_with_missing_binary_errors_not_panics() { /* send with Claude + bogus binary → Update::Error */ }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p aether-agent store cli_send -v`
Expected: FAIL.

- [ ] **Step 3: Implement routing + persistence**

`Agent::send`: if `settings.provider == OpenRouter` keep existing `Turn` path; else build `DriverStartOptions::for_provider`, spawn matching driver, wire its `cancel` into `Reply::cancel`, forward driver `Update`s (drivers send direct). Rollback/fork/resume = `close` + fresh `prompt` with transcript pairs joined as context; `fork_session` signature unchanged. Store: `SavedChat { provider: Provider, provider_model: String }`, `chats.provider TEXT DEFAULT 'openrouter'`, `chats.provider_model TEXT DEFAULT ''`, `ALTER TABLE` migration like `pinned`/`archived`, update `save`/`load` SQL. Per-chat wins, else `Settings.provider_models` fallback.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p aether-agent -v && cargo test -p aether-core -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/agent/src/agent.rs crates/agent/src/store.rs
git commit -m "feat(agent): route sends by provider and persist provider choice"
```

