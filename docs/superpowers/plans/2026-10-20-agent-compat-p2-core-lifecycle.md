# Provider Compat Phase 2: Core Model, Turn Lifecycle and Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the core model and the agent pump everything phases 3 and 4 build on:
- notices, steer markers and timed thinking
- tool nesting and the background status
- cross-reply tool updates
- steers that stay in their reply
- out-of-turn asks that reach the user
- liveness data for the stall card
- saved chats that survive new variants and quitting mid-turn
- OpenCode turns that reattach after a relaunch

**Architecture:**
- **Core.** `aether-core` gains additive types and reducer rules. Every new enum variant reads back through a lenient decoder, so an older or newer saved chat never drops.
- **Agent.** The pump in `aether-agent` stops swapping reply streams on steer. It routes out-of-turn updates to a side turn or to chat-level meta, and it tracks what keeps a session busy.
- **App.** `aether` (app) applies updates through a router that finds the right message. It saves mid-turn and on quit, and restores interrupted rows honestly.
- **Rendering.** UI changes are the minimum that keeps rendering exhaustive. Real visuals are phase 4.

**Tech Stack:** Rust 2024, serde/serde_json, sqlx (SQLite), futures mpsc, GPUI via gpui-kit 0.7.3.

**Spec:** `docs/superpowers/specs/2026-10-20-agent-compat-chat-revamp-design.md`: section 1 (core model, persistence, OpenCode outlives Aether), section 2 (lifecycle) and "Decisions by the user".

## Global Constraints

- Unit tests live in `crates/<crate>/tests/unit/<path>.rs`, included from the source file with `#[cfg(test)] #[path = …] mod tests;` (GLOSSARY "Unit test file").
- Ask the user before every commit. Never add `Co-Authored-By` or AI attribution.
- Saved chats from current builds must still load. Every new field is `#[serde(default)]`. Every new enum variant is covered by a lenient decoder.
- `Update::Error(String)` stays the fatal error. Its first line is the message, and any later lines are details (a stderr tail). Only `Error` sets `failed` and blocks the send queue. The recoverable kind is the new `Update::Warning(String)`.
- `Update::Wake` stays a unit variant. A Driver that knows why it woke sends a `Notice` of kind `Wake` right after it.
- Restore copy, verbatim from the spec: `"Interrupted: Aether closed"`.
- Mid-turn saves happen at most every 2 s (`SAVE_EVERY_MS = 2_000`).
- OpenCode turns outlive Aether: quitting and reaping never interrupt them. Restore leaves OpenCode rows alone and reattaches instead (user decision, 2026-10-20).
- Queue and steer keep today's composer behaviour:
  - Enter while busy queues the message.
  - "Send now" steers it.
  - A steer the CLI has taken shows as a `Step::Steer` in the running reply.
  - A steer made after the turn ended runs as its own reply.
- Out of scope here, by design:
  - Reshaping `Ask`/`Answer` (approval options, multi-question, `Form`, plan markdown) is the first task of phase 3. The Drivers and the cards are what consume it.
  - The stall card, the status line, the trays and the stopped marker are phase 4 views over the data this phase adds.
- Phase 1 must be merged before starting this phase. Task 7 reuses its OpenCode `descends` helper.

## Review Focus

- A chat saved by today's build (current JSON shape, incl. `Plain` tool details and `Expired` requests) must load and re-save byte-identically in meaning (Task 1 test `todays_format_round_trips`).
- A background tool finishing during a *later* turn must update its original row, not create a row in the new reply (Task 4 test `route_sends_known_tool_ids_to_their_own_message`).
- Quitting mid-stream and relaunching must keep the streamed text and show running rows as "Interrupted: Aether closed", never as spinners (Task 4 test `interrupt_stops_live_rows_with_a_reason` + Task 7 restore test).
- A steer that races the end of a turn must not lose its text: it runs as its own reply (Task 5 test `late_steer_runs_as_its_own_reply`).
- An approval raised after the turn's `Final` (e.g. a background subagent asking) must reach the user, not be auto-denied (Task 5 test `asks_outside_a_turn_open_a_side_turn`).

---

### Task 1: Lenient decoding, save-only capping, tolerant store

**Files:**
- Modify: `crates/core/src/lib.rs`: `ToolStatus` (:29), `PlanStatus` (:145), `Step` (:268), `Message` (:276), `finish` (:479), `legacy_detail` (:215) as the pattern
- Modify: `crates/core/src/request.rs`: `Request.ask`, `Request.status`
- Modify: `crates/agent/src/store.rs:135-160` (`load`), `:185-190` (`save`)
- Test: `crates/core/tests/unit/lib.rs`, `crates/agent/tests/unit/store.rs`

**Interfaces:**
- Produces:
  - `Step::Unknown`, plus `ToolStatus::Unknown` and `PlanStatus::Unknown` (both `#[serde(other)]`).
  - `Ask::Unknown`. An unparseable `Request.status` loads as `RequestStatus::Expired`.
  - `pub fn for_save(&self) -> Message` on `Message`: a clone with tool payloads capped to `SAVED_TOOL_BYTES`.
  - `pub fn unreadable() -> Message`: an assistant message with text `"This message was saved by a newer version of Aether and can't be shown."`.
  - `fn parse_messages(json: &str) -> Option<Vec<Message>>` (private, `store.rs`): `None` only when the column isn't a JSON array.

- [ ] **Step 1: Write the failing tests**

`crates/core/tests/unit/lib.rs`:

```rust
#[test]
fn unknown_variants_load_as_unknown() {
    let json = r#"{"role":"Assistant","text":"hi","steps":[{"Thought":"a"},{"Hologram":{"x":1}},{"Tool":{"id":"t","title":"T","detail":{"Plain":""},"status":"Teleporting"}}],"step_positions":[0,0,0],"thought_for":null,"context_tokens":null,"elapsed_ms":null,"ended_at_ms":null,"model":null,"requests":[{"id":1,"at":0,"ask":{"Telepathy":{}},"status":{"Answered":{"Hum":1}}}]}"#;
    let m: Message = serde_json::from_str(json).unwrap();
    assert_eq!(m.steps[1], Step::Unknown);
    assert!(matches!(&m.steps[2], Step::Tool(c) if c.status == ToolStatus::Unknown));
    assert_eq!(m.requests[0].ask, Ask::Unknown);
    assert_eq!(m.requests[0].status, RequestStatus::Expired);
}

#[test]
fn todays_format_round_trips() {
    let mut m = Message::assistant();
    m.apply(Update::Thinking("plan".into()));
    m.apply(Update::Tool(ToolCall::new("t1", "Run command", ToolDetail::Plain("ls".into()))));
    m.apply(Update::Ask { id: 4, ask: Ask::Question { question: "q".into(), choices: vec!["a".into()] } });
    m.apply(Update::Final("done".into()));
    m.finish();
    let back: Message = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(&back).unwrap(), serde_json::to_value(&m).unwrap());
}

#[test]
fn finish_keeps_live_output_and_for_save_caps_it() {
    let mut m = Message::assistant();
    let big = "x".repeat(300 * 1024);
    let detail = ToolDetail::Command { command: "ls".into(), cwd: None, output: big.clone(), exit_code: Some(0) };
    m.apply(Update::Tool(ToolCall { status: ToolStatus::Done, ..ToolCall::new("t", "Run", detail) }));
    m.finish();
    let Step::Tool(live) = &m.steps[0] else { panic!() };
    assert!(matches!(&live.detail, ToolDetail::Command { output, .. } if *output == big));
    let saved = m.for_save();
    let Step::Tool(kept) = &saved.steps[0] else { panic!() };
    assert!(matches!(&kept.detail, ToolDetail::Command { output, .. } if output == NOT_KEPT));
}
```

`crates/agent/tests/unit/store.rs`:

```rust
#[test]
fn a_bad_message_becomes_a_placeholder_not_a_lost_chat() {
    let good = serde_json::to_value(Message::user("hello")).unwrap();
    let json = serde_json::json!([good, {"role": "Robot"}]).to_string();
    let messages = parse_messages(&json).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].text, "hello");
    assert_eq!(messages[1].text, Message::unreadable().text);
    assert!(parse_messages("{}").is_none());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p aether-core --lib && cargo test -p aether-agent --lib store::tests`
Expected: compile errors (`Step::Unknown`, `for_save`, `parse_messages` missing).

- [ ] **Step 3: Implement**

- `#[serde(other)] Unknown` on `ToolStatus` and `PlanStatus`.
- `Step::Unknown` and `Ask::Unknown` are plain unit variants. Their decoding comes from field-level `deserialize_with`: each one tries `Typed(T)` and falls back to `Unknown(IgnoredAny)`, copying the `legacy_detail` untagged-wire pattern.
  - `Message.steps`: a lenient `Vec<Step>`, one element at a time.
  - `Request.ask`.
  - `Request.status`: unknown loads as `Expired`.
- `finish()` no longer calls `cap_tool_payload()`. `for_save()` clones and then calls it.
- `store::save` serializes `chat.messages.iter().map(Message::for_save)`.
- `store::load` uses `parse_messages`: parse a `Vec<serde_json::Value>`, then map each element to `Message` or `Message::unreadable()`.
- Where the UI matches `Step` exhaustively (`crates/ui/src/chat/message.rs:648-790`, `thinking.rs:245-299`), `Step::Unknown` renders nothing. Where it matches `ToolStatus`, `Unknown` renders like `Done`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-core --lib && cargo test -p aether-agent --lib && cargo build -p aether`
Expected: all PASS, and the build succeeds.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/core crates/agent/src/store.rs crates/agent/tests/unit/store.rs crates/ui/src/chat
git commit -m "feat(core): lenient chat decoding, cap tool payload only on save"
```

---

### Task 2: Notices, steer markers, timed thinking, cost, model, warnings

**Files:**
- Modify: `crates/core/src/lib.rs`: `Update` (:233), `Step` (:268), `Message` (:276), `Message::apply` (:375)
- Modify: `crates/core/src/request.rs`: `RequestStatus`
- Modify (minimal rendering): `crates/ui/src/chat/message.rs` (step match, error paragraph), `crates/ui/src/chat/thinking.rs` (step match)
- Test: `crates/core/tests/unit/lib.rs`

**Interfaces:**
- Produces, in `aether-core`:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub id: String,
    pub kind: NoticeKind,
    pub text: String,
    #[serde(default)] pub state: NoticeState,
    /// Unix ms a countdown ends at (retry backoff).
    #[serde(default, skip_serializing_if = "Option::is_none")] pub until_ms: Option<i64>,
    /// Link the notice offers (MCP sign-in).
    #[serde(default, skip_serializing_if = "Option::is_none")] pub url: Option<String>,
    /// Longer text behind "View summary" / "Details".
    #[serde(default, skip_serializing_if = "Option::is_none")] pub detail: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoticeKind { Startup, Retry, RateLimit, Compaction, Hook, ModelChange, ModeChange, Warning, McpAuth, Wake, Info, #[serde(other)] Unknown }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoticeState { Running, #[default] Done, Failed, #[serde(other)] Unknown }
```

  - `Step` variants: `Notice(Notice)`, `Steer(String)`, and `Thinking { text: String, ms: Option<u64> }`. The old `Thought(String)` still loads, but the reducer no longer creates it.
  - `Update` variants: `Notice(Notice)`, `Steered(String)`, `Warning(String)`, `Cost(u64)` (micro-USD), `Model(String)`, `Mode(AccessMode)` and `AskResolved(u64)`.
  - `RequestStatus::Withdrawn`: answered, cancelled or timed out on the CLI side.
  - `Message` fields, all `#[serde(default)]`:
    - `cost_micros: u64`
    - `served_model: Option<String>`
    - `error: Option<String>`

- [ ] **Step 1: Write the failing tests**

```rust
fn notice(id: &str, state: NoticeState) -> Notice {
    Notice { id: id.into(), kind: NoticeKind::Retry, text: "retrying".into(), state, until_ms: None, url: None, detail: None }
}

#[test]
fn notices_upsert_by_id_in_place() {
    let mut m = Message::assistant();
    m.apply(Update::Delta("a".into()));
    m.apply(Update::Notice(notice("r1", NoticeState::Running)));
    m.apply(Update::Delta("b".into()));
    m.apply(Update::Notice(notice("r1", NoticeState::Done)));
    assert_eq!(m.steps, [Step::Notice(notice("r1", NoticeState::Done))]);
    assert_eq!(m.step_positions, [1]);
}

#[test]
fn warnings_are_notices_and_errors_stay_out_of_the_text() {
    let mut m = Message::assistant();
    m.apply(Update::Delta("partial".into()));
    m.apply(Update::Warning("stderr: deprecated flag".into()));
    assert!(!m.failed);
    assert!(matches!(&m.steps[0], Step::Notice(n) if n.kind == NoticeKind::Warning && n.text == "stderr: deprecated flag"));
    m.apply(Update::Error("claude exited (1)\npanic: boom".into()));
    assert!(m.failed);
    assert_eq!(m.text, "partial");
    assert_eq!(m.error.as_deref(), Some("claude exited (1)\npanic: boom"));
}

#[test]
fn thinking_after_text_starts_a_new_timed_step() {
    let mut m = Message::assistant();
    m.apply(Update::Thinking("one".into()));
    m.apply(Update::Thinking(" more".into()));
    m.apply(Update::Delta("text".into()));
    m.apply(Update::Thinking("two".into()));
    let thoughts: Vec<(&str, bool)> = m.steps.iter().filter_map(|s| match s {
        Step::Thinking { text, ms } => Some((text.as_str(), ms.is_some())), _ => None }).collect();
    assert_eq!(thoughts, [("one more", true), ("two", false)]);
    assert_eq!(m.step_positions, [0, 4]);
}

#[test]
fn steered_marks_the_spot_and_final_keeps_text_before_it() {
    let mut m = Message::assistant();
    m.apply(Update::Delta("before".into()));
    m.apply(Update::Steered("also dark mode".into()));
    m.apply(Update::Final("after".into()));
    assert_eq!(m.steps, [Step::Steer("also dark mode".into())]);
    assert_eq!(m.text, "before\n\nafter");
}

#[test]
fn cost_model_and_resolved_asks() {
    let mut m = Message::assistant();
    m.apply(Update::Cost(1_500));
    m.apply(Update::Cost(500));
    m.apply(Update::Model("claude-sonnet-5-5".into()));
    m.apply(Update::Ask { id: 3, ask: Ask::Question { question: "q".into(), choices: vec![] } });
    m.apply(Update::AskResolved(3));
    assert_eq!((m.cost_micros, m.served_model.as_deref()), (2_000, Some("claude-sonnet-5-5")));
    assert_eq!(m.requests[0].status, RequestStatus::Withdrawn);
    assert!(m.pending_request().is_none());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p aether-core --lib`
Expected: compile errors (the new types are missing).

- [ ] **Step 3: Implement the reducer rules in `Message::apply`**

- **`Notice(n)`:** replace the `Step::Notice` with the same `id`, in place. Otherwise push it at `self.text.len()`.
- **`Warning(t)`:** push `Notice { id: format!("warning-{}", self.steps.len()), kind: Warning, text: t, state: Done, .. }`.
- **`Error(t)`:** set `failed = true` and `error = Some(t)`. Leave `text` untouched, and set `turn_start = text.len()`.
- **`Steered(t)`:** push `Step::Steer(t)` at `text.len()`, then set `turn_start = text.len()`.
- **`Thinking(t)`:**
  - If the last step is `Thinking` and its position equals `text.len()`, append to it.
  - Otherwise, when `t.trim()` is non-empty, push a new `Thinking { text, ms: None }` and start a `#[serde(skip)] thinking_since: Option<Instant>`.
  - Any non-`Thinking` update that changes `text` or `steps` closes the open thinking step: `ms = Some(elapsed)` and `thinking_since = None`. `finish()` also closes it.
  - Keep `end_thinking` (for `thought_for`) as it is, so older rendering still works.
- **`Cost(n)`:** add `n` to `cost_micros` (saturating).
- **`Model(m)`:** set `served_model = Some(m)`.
- **`Mode(_)`:** nothing at message level. The app handles it in Task 6.
- **`AskResolved(id)`:** a request with that id still `Pending` becomes `Withdrawn`.
- **`pending_request`:** unchanged. `Withdrawn` is not pending.

Minimal rendering:
- `Step::Notice` renders as one muted line, an icon plus `text`.
- `Step::Steer` renders as a muted line, "You, while it worked: {text}".
- `Step::Thinking` renders like `Thought`, using `ms` for the "Thought for" label when present.
- `message.error` renders after the text as the same `**Error:**` paragraph the UI shows today.

All of these become real views in phase 4.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-core --lib && cargo build -p aether`
Expected: all PASS, and the build succeeds. Fix every `match` on `Update` the compiler flags: `crates/app/src/app/chat/send.rs:424` (`apply`) and the agent pump. They forward the new variants unchanged for now.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/core crates/ui/src/chat crates/app/src crates/agent/src
git commit -m "feat(core): notices, steer markers, timed thinking, cost and warnings"
```

---

### Task 3: Searches resolve by id

**Files:**
- Modify: `crates/core/src/lib.rs` (`Update::Search`, `Update::SearchDone`, `Step::Search`, reducer)
- Modify: `crates/agent/src/events.rs:43-56`
- Modify: every `Step::Search { query, results }` pattern in `crates/ui/src/chat/message.rs` and `crates/ui/src/chat/thinking.rs`, which gains `..`
- Test: `crates/core/tests/unit/lib.rs`, `crates/agent/tests/unit/events.rs`

**Interfaces:**
- Produces:
  - `Update::Search { id: String, query: String }`
  - `Update::SearchDone { id: String, outcome: Result<usize, String> }`
  - `Step::Search { #[serde(default)] id: String, query: String, results: Option<Result<usize, String>> }`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn parallel_searches_resolve_their_own_rows() {
    let mut m = Message::assistant();
    m.apply(Update::Search { id: "a".into(), query: "one".into() });
    m.apply(Update::Search { id: "b".into(), query: "two".into() });
    m.apply(Update::SearchDone { id: "b".into(), outcome: Ok(3) });
    assert!(matches!(&m.steps[0], Step::Search { results: None, .. }));
    assert!(matches!(&m.steps[1], Step::Search { results: Some(Ok(3)), .. }));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p aether-core --lib parallel_searches`
Expected: compile error.

- [ ] **Step 3: Implement**

The reducer resolves the `Step::Search` with a matching `id`. `events.rs` uses the adk tool call id (`call.id`) for `Search`, and the tool result id for `SearchDone`. Update `crates/agent/tests/unit/events.rs` to the new shapes.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-core --lib && cargo test -p aether-agent --lib && cargo build -p aether`
Expected: all PASS.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/core crates/agent crates/ui/src/chat
git commit -m "fix(core): searches resolve by id"
```

---

### Task 4: Tool nesting, new statuses, routing and interrupt

**Files:**
- Modify: `crates/core/src/lib.rs`: `ToolStatus`, `ToolCall` (:43), `ToolDetail::Task` (:113), `finish`, plus the new free fn `route`
- Modify: `crates/driver/src/tool.rs:80-85` (the `Task` construction in `classify`)
- Modify (minimal rendering): `crates/ui/src/chat/tool_row.rs` (status match)
- Test: `crates/core/tests/unit/lib.rs`

**Interfaces:**
- Produces:
  - `ToolStatus::Waiting` and `ToolStatus::Background`.
  - `ToolCall.parent: Option<String>` and `ToolCall.images: Vec<Attachment>`, both `#[serde(default, skip_serializing_if = …)]`.
  - `ToolDetail::Task { description, output, agent: String, prompt: String, tools: u32, tokens: u64, activity: String }`. The new fields are `#[serde(default)]`.
  - `pub fn route(messages: &[Message], reply_ix: usize, update: &Update) -> usize`:
    - `Update::Tool(call)` goes to the index of the message holding a `Step::Tool` with `call.id`.
    - `Update::AskResolved(id)` goes to the index of the message holding that request.
    - Anything else goes to `reply_ix`.
  - `pub fn interrupt(&mut self, why: &str)` on `Message`: `Running`, `Waiting` and `Background` tools become `Stopped`, with `error = Some(why)` when no error is set. Pending searches become `Err(why)`, pending requests become `Expired`, and an open thinking step closes.
  - `finish()` turns `Running` and `Waiting` into `Stopped`. It leaves `Background` alone.

- [ ] **Step 1: Write the failing tests**

```rust
fn tool(id: &str, status: ToolStatus) -> Update {
    Update::Tool(ToolCall { status, ..ToolCall::new(id, "Run command", ToolDetail::Plain(String::new())) })
}

#[test]
fn route_sends_known_tool_ids_to_their_own_message() {
    let mut first = Message::assistant();
    first.apply(tool("bg1", ToolStatus::Background));
    first.finish();
    let messages = vec![Message::user("go"), first, Message::user("next"), Message::assistant()];
    assert_eq!(route(&messages, 3, &tool("bg1", ToolStatus::Done)), 1);
    assert_eq!(route(&messages, 3, &tool("new", ToolStatus::Running)), 3);
    assert_eq!(route(&messages, 3, &Update::Delta("x".into())), 3);
}

#[test]
fn finish_leaves_background_rows_running() {
    let mut m = Message::assistant();
    m.apply(tool("a", ToolStatus::Running));
    m.apply(tool("b", ToolStatus::Background));
    m.apply(tool("c", ToolStatus::Waiting));
    m.finish();
    let st: Vec<ToolStatus> = m.steps.iter().filter_map(|s| match s { Step::Tool(c) => Some(c.status), _ => None }).collect();
    assert_eq!(st, [ToolStatus::Stopped, ToolStatus::Background, ToolStatus::Stopped]);
}

#[test]
fn interrupt_stops_live_rows_with_a_reason() {
    let mut m = Message::assistant();
    m.apply(tool("a", ToolStatus::Running));
    m.apply(tool("b", ToolStatus::Background));
    m.apply(Update::Delta("kept".into()));
    m.interrupt("Interrupted: Aether closed");
    assert_eq!(m.text, "kept");
    for s in &m.steps {
        let Step::Tool(c) = s else { continue };
        assert_eq!((c.status, c.error.as_deref()), (ToolStatus::Stopped, Some("Interrupted: Aether closed")));
    }
}

#[test]
fn nested_calls_keep_their_parent() {
    let mut m = Message::assistant();
    let child = ToolCall { parent: Some("task1".into()), ..ToolCall::new("c1", "Read", ToolDetail::Plain(String::new())) };
    m.apply(Update::Tool(child.clone()));
    let back: Message = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert!(matches!(&back.steps[0], Step::Tool(c) if c.parent.as_deref() == Some("task1")));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p aether-core --lib`
Expected: compile errors.

- [ ] **Step 3: Implement** the fields, statuses, `route` and `interrupt`, and the `finish` change. `tool.rs` builds `Task` with the new fields defaulted, and its tests use `..` patterns where they spell out `Task`. In `tool_row.rs`, `Waiting` and `Background` render like `Running` (a spinner) until phase 4. In `Message::apply`, the `Update::Tool` upsert path keeps a row's existing `parent` if the incoming call has `None`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-core --lib && cargo test -p aether-driver --lib && cargo build -p aether`
Expected: all PASS.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/core crates/driver/src/tool.rs crates/driver/tests crates/ui/src/chat/tool_row.rs
git commit -m "feat(core): tool nesting, background and waiting rows, update routing"
```

---

### Task 5: Agent pump: steers stay in their reply, out-of-turn routing, busy holds, health

**Files:**
- Modify: `crates/agent/src/agent.rs`: `Command::Steer` (:79), `Live` (:85), `spawn_live` (:555-682), `steer` (:684), `answer` (:196), `reap` (:756), and the new `health`
- Modify: `crates/driver/src/lib.rs` (`Driver::pid` with a default), plus `claude.rs`, `codex.rs`, `pi.rs` and `antigravity.rs` (implement `pid` from their `Child`)
- Test: `crates/agent/tests/unit/agent.rs`

**Interfaces:**
- Consumes: `Update::Steered`, `Update::AskResolved` and `ToolStatus::{Background, Waiting}` (Tasks 2 and 4).
- Produces:
  - `fn pid(&self) -> Option<u32> { None }` on `trait Driver`.
  - `pub struct SessionHealth { pub pid: Option<u32>, pub alive: bool }` and `pub fn health(&self, chat_key: &str) -> Option<SessionHealth>` on `Agent`. The result is `None` when no session is live. `alive` is the driver thread's last `driver.alive()` and, when there is a pid, a successful `/bin/kill -0 <pid>`.
  - `Command::Steer(String, Vec<Attachment>, UnboundedSender<Update>, String)` keeps its shape. The `UnboundedSender` is now the fallback reply, used only when the turn already ended.
  - `pub fn steer(...) -> Reply` on `Agent` keeps its signature. Its `Reply` carries updates only if the steer fell back to a new prompt.
  - `SideUpdate::Meta(String, Update)` now also carries `Tool`, `Notice`, `AskResolved`, `Mode`, `Model`, `Cost`, `Usage`, `Count` and `Window` that arrive while no turn runs.

- [ ] **Step 1: Write the failing tests**

Extend `Fake` in `crates/agent/tests/unit/agent.rs`:
- `prompt("late-tool")`: send `Final`, then `Update::Tool(call)` with `status: Done` and id `"t9"`.
- `prompt("bg")`: send `Update::Tool` with id `"bg1"` and `status: Background`, then `Final`.
- `prompt("bg-done")`: send `Update::Tool` with id `"bg1"` and `status: Done`, then `Final`.
- `fn pid(&self) -> Option<u32> { Some(std::process::id()) }`.
- `steer` keeps its current behaviour: it sends `Wake` and `Final(text)`, or returns false for `"late"`.

Replace `steer_ends_the_old_reply_and_continues_in_a_new_one` with:

```rust
#[test]
fn steer_into_a_running_turn_stays_in_its_reply() {
    let (agent, log, _) = fake_agent(true, Duration::from_secs(600));
    let s = cli_settings();
    let old = agent.send("x", "hold".into(), vec![], &s, "", vec![], &[], &chat(""));
    let fallback = agent.steer("more".into(), vec![], &s, &[], &chat(""));
    assert_eq!(drain(&agent, old), [Update::Delta("hold".into()), Update::Steered("more".into()), Update::Final("more".into())]);
    assert_eq!(drain(&agent, fallback), []);
    assert_eq!(log.lock().unwrap()[1], "steer:more");
}

#[test]
fn late_steer_runs_as_its_own_reply() {
    let (agent, _, _) = fake_agent(true, Duration::from_secs(600));
    let s = cli_settings();
    let old = agent.send("x", "hold".into(), vec![], &s, "", vec![], &[], &chat(""));
    let fallback = agent.steer("late".into(), vec![], &s, &[], &chat(""));
    assert_eq!(drain(&agent, old).last(), Some(&Update::Final(String::new())));
    assert_eq!(drain(&agent, fallback), [Update::Delta("late".into()), Update::Final("late".into())]);
}

#[test]
fn asks_outside_a_turn_open_a_side_turn() {
    let (agent, log, _) = fake_agent(true, Duration::from_secs(600));
    let mut side = agent.side_updates().unwrap();
    drain(&agent, agent.send("x", "late-ask".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")));
    let Some(SideUpdate::Turn(key, reply)) = agent.rt.block_on(side.next()) else { panic!() };
    assert_eq!(key, "1");
    let mut updates = reply.updates;
    assert!(matches!(agent.rt.block_on(updates.next()), Some(Update::Ask { .. })));
    assert!(!log.lock().unwrap().iter().any(|l| l.starts_with("answer:")), "not auto-denied");
}

#[test]
fn tool_updates_outside_a_turn_go_to_meta() {
    let (agent, _, _) = fake_agent(true, Duration::from_secs(600));
    let mut side = agent.side_updates().unwrap();
    drain(&agent, agent.send("x", "late-tool".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")));
    let Some(SideUpdate::Meta(_, Update::Tool(call))) = agent.rt.block_on(side.next()) else { panic!() };
    assert_eq!(call.id, "t9");
}

#[test]
fn reaper_keeps_sessions_with_background_work() {
    let (agent, _, _) = fake_agent(true, Duration::ZERO);
    drain(&agent, agent.send("x", "bg".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")));
    agent.reap();
    assert_eq!(agent.live.lock().unwrap().len(), 1);
    drain(&agent, agent.send("x", "bg-done".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")));
    agent.reap();
    assert!(agent.live.lock().unwrap().is_empty());
}

#[test]
fn health_reports_pid_and_liveness() {
    let (agent, _, _) = fake_agent(true, Duration::from_secs(600));
    assert!(agent.health("1").is_none());
    drain(&agent, agent.send("x", "a".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")));
    let h = agent.health("1").unwrap();
    assert_eq!((h.pid, h.alive), (Some(std::process::id()), true));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p aether-agent --lib agent::tests`
Expected: FAIL, or compile errors for `health`, `pid` and `Steered`.

- [ ] **Step 3: Implement**

- **Driver thread, `Command::Steer`:**
  - If `driver.steer(text_with_files, native)` returns true, send `Update::Steered(text)` on `driver_tx`, using the original text without the attached-files block, and drop the fallback sender.
  - If it returns false, set `*turn.lock() = Some(fallback)`. The driver thread holds a clone of the `turn` Arc. Respawn the driver if it is dead, then call `driver.prompt`.
  - Delete `steered`/`next_reply`, and do not send `Wake` here.
- **Driver thread, bookkeeping:** after every command, store `driver.alive()` in `Live.alive: Arc<AtomicBool>` and `driver.pid()` in `Live.pid: Arc<Mutex<Option<u32>>>`. Do the same after each (re)spawn.
- **Pump, side turns.** An `open_side_turn()` helper creates the side `Reply` exactly as today's `Wake` branch does.
  - `Wake` with a live turn is ignored. With no turn, it opens a side turn.
  - `Ask`, `Delta`, `Thinking`, `Steered` and `Notice` of kind `Wake`, arriving with no turn, open a side turn first and then forward.
  - Delete the auto-deny branch.
- **Pump, meta routing:**
  - With no turn, `Tool`, other `Notice`s, `AskResolved`, `Mode`, `Model`, `Cost`, `Usage`, `Count`, `Window`, `Title` and `Session` go to `SideUpdate::Meta`.
  - `Final` with no turn is dropped.
  - `AskResolved(local)` is mapped to its global id through `cli_asks` (reverse lookup), and the entry is removed. An unknown id is dropped.
- **Holds.** `Live.holds: Arc<Mutex<HashSet<String>>>` tracks what keeps a session busy:
  - A `Tool` with status `Running`, `Waiting` or `Background` inserts its id. Any other status removes it.
  - A forwarded `Ask` inserts `format!("ask:{global}")`.
  - `Agent::answer` and `AskResolved` remove that entry.
- **`reap`:** keep a session while `turn` is `Some`, while `holds` is non-empty, or while it is not yet idle.
- **`process_alive(pid)`:** `Command::new("/bin/kill").args(["-0", &pid.to_string()])`, with stdout and stderr null, returning success.
- **Real drivers:** `pid()` returns `self.child.as_ref().map(Child::id)`, or the equivalent field in each driver. OpenCode keeps the default `None`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-agent --lib && cargo test -p aether-driver --lib`
Expected: all PASS. This includes the unchanged `steer_while_idle_prompts_and_respawns_on_new_options` and `stale_asks_do_not_reach_a_reopened_session`.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/agent crates/driver/src
git commit -m "feat(agent): steers stay in their reply, out-of-turn routing, busy holds, health"
```

---

### Task 6: App: steer chips, routed updates, liveness clock, mid-turn and quit saves

**Files:**
- Modify: `crates/app/src/app/mod.rs:94-153` (`Chat`, `Queued`)
- Modify: `crates/app/src/app/chat/send.rs`: `steer_queued` (:104), `submit` (:136), `apply` (:424), `apply_cli_meta` (:398)
- Modify: `crates/app/src/app/new.rs:340-343` (`on_app_quit`)
- Test: `crates/app/tests/unit/app/chat/send.rs`

**Interfaces:**
- Consumes: `route` and `Message::apply` (core), `Agent::steer` fallback semantics (Task 5) and the `SideUpdate::Meta` additions.
- Produces:
  - `Queued.delivering: bool`. `true` means it was handed to the CLI and is waiting for `Steered`. A delivering chip has no Edit or Remove.
  - `Chat.last_event_ms: i64` (Unix ms of the last update applied, 0 when idle).
  - `Chat.saved_ms: i64`.
  - `Chat.mode_in_effect: Option<AccessMode>`.
  - `pub(crate) fn take_delivered(queue: &mut Vec<Queued>, text: &str) -> Option<Queued>` (pure). It removes the first delivering item whose text equals `text`, or else the first delivering item.
  - `pub(crate) fn save_due(now_ms: i64, saved_ms: i64) -> bool` (pure). It returns `now_ms - saved_ms >= SAVE_EVERY_MS`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn steered_removes_the_matching_delivering_chip() {
    let q = |t: &str, d: bool| Queued { text: t.into(), attachments: vec![], delivering: d };
    let mut queue = vec![q("a", false), q("b", true), q("c", true)];
    assert_eq!(take_delivered(&mut queue, "c").map(|x| x.text), Some("c".into()));
    assert_eq!(take_delivered(&mut queue, "zzz").map(|x| x.text), Some("b".into()));
    assert_eq!(take_delivered(&mut queue, "a"), None, "plain queued items stay");
    assert_eq!(queue.len(), 1);
}

#[test]
fn saves_at_most_every_two_seconds() {
    assert!(!save_due(10_000, 9_000));
    assert!(save_due(11_000, 9_000));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p aether --lib app::chat::send::tests`
Expected: compile errors.

- [ ] **Step 3: Implement**

- **`steer_queued(i)`** ("Send now"):
  - When the chat is busy, is an agent chat, and its provider is not OpenRouter:
    - Set `queue[i].delivering = true` and keep the item in the queue.
    - Call `self.agent.steer(...)` as `submit` does today.
    - Spawn a task over the returned `Reply`. On its first update it removes that queue item, pushes `Message::user(text)` (with attachments) plus `Message::assistant()`, bumps `run`, and streams the first and following updates into the new reply through the existing `stream`/`apply` path. If the stream ends without updates, it does nothing.
  - Otherwise call `submit` exactly as today, which also covers OpenRouter.
  - `submit` no longer takes the `steer` path. Remove the `steering` branch and the pending-request deny loop, which existed only because steers swapped bubbles.
- **`apply`:**
  - Set `last_event_ms = now_ms()`.
  - `Steered(t)` calls `take_delivered(&mut chat.queue, &t)`.
  - `Mode(m)` sets `chat.mode_in_effect = Some(m)` and returns.
  - Every other update goes to `messages[route(&messages, reply_ix, &update)]`, then remeasure that index.
  - After applying, call `persist(ix)` when `save_due(now, chat.saved_ms)` holds, and set `saved_ms`.
- **`apply_cli_meta`:**
  - `Tool`, `Notice`, `AskResolved`, `Cost` and `Model` go to `messages[route(&messages, last_assistant, &update)]`. `last_assistant` is the index of the last `Role::Assistant` message. If there is none, drop the update.
  - `Usage`, `Count` and `Window` update that last assistant message the same way.
  - `Mode` is handled as in `apply`.
  - Persist.
- **Finishing a run:** `finish_run` sets `last_event_ms = 0`.
- **`stop_chat`:** set `delivering = false` on every queue item, so a stopped turn's undelivered steers return to the queue as normal, editable chips.
- **`on_app_quit`:** call `self.persist(ix)` for every chat whose `cancel.is_some()`, before `shutdown_terminals`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether --lib && cargo build -p aether`
Expected: all PASS. Update existing send tests that relied on the steer creating a new user message: a steer now creates one only through the fallback path.

- [ ] **Step 5: Manual check**

Run the app with a Claude chat and send a long task. While it runs:
1. Type a message and press Enter. A queued chip appears.
2. Click "Send now". The chip reads Delivering, then disappears, and "You, while it worked: …" appears in the same reply. No second assistant bubble appears.
3. Quit with ⌘Q mid-turn, then relaunch. The streamed text is still there, and running rows read "Interrupted: Aether closed" (see Task 7).

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/app
git commit -m "feat(app): steers land in the running reply, routed updates, mid-turn saves"
```

---

### Task 7: Honest restore and OpenCode reattach

**Files:**
- Modify: `crates/app/src/app/chat/create.rs:262-300` (`restore_chats`)
- Modify: `crates/app/src/app/chat/mod.rs:140` (`select`)
- Modify: `crates/agent/src/agent.rs` (new `attach`, and `Command::Attach`)
- Modify: `crates/driver/src/opencode/session.rs` (`start`: reattach to an active turn)
- Test: `crates/app/tests/unit/app/chat/create.rs`, `crates/agent/tests/unit/agent.rs`

**Interfaces:**
- Consumes:
  - `Message::interrupt` (Task 4) and `SideUpdate::Turn` (Task 5).
  - Phase 1's `descends`, through `resync`.
- Produces:
  - `pub fn attach(&self, settings: &Settings, chat: &ChatSession)` on `Agent`. It spawns the live session without a prompt when none exists (turn `None`), so events the Driver emits open a side turn. When a session already exists, it does nothing.
  - `pub(crate) fn reattach_wanted(chat: &Chat) -> bool` (pure): `provider == OpenCode`, `cli_session` non-empty, and the last message is an assistant message with `elapsed_ms == None` or a `Pending`/`Expired` request.
  - `pub(crate) fn restore_messages(provider: Provider, messages: &mut [Message])` (pure): for providers other than OpenCode, call `m.interrupt("Interrupted: Aether closed")` on every assistant message, then turn `Pending` requests into `Expired` (today's rule). For OpenCode, only that request rule applies.

- [ ] **Step 1: Write the failing tests**

`crates/app/tests/unit/app/chat/create.rs`:

```rust
#[test]
fn restore_interrupts_live_rows_except_opencode() {
    let running = || {
        let mut m = Message::assistant();
        m.apply(Update::Tool(ToolCall::new("t", "Run", ToolDetail::Plain(String::new()))));
        m
    };
    let status = |ms: &[Message]| match &ms[0].steps[0] { Step::Tool(c) => (c.status, c.error.clone()), _ => panic!() };
    let mut claude = vec![running()];
    restore_messages(Provider::Claude, &mut claude);
    assert_eq!(status(&claude), (ToolStatus::Stopped, Some("Interrupted: Aether closed".into())));
    let mut opencode = vec![running()];
    restore_messages(Provider::OpenCode, &mut opencode);
    assert_eq!(status(&opencode), (ToolStatus::Running, None));
}
```

Also add a `reattach_wanted` test that builds a `Chat` the way the existing create tests do. It must cover three cases: OpenCode with an unfinished last reply gives true, OpenCode with a finished one gives false, and Claude with an unfinished one gives false.

`crates/agent/tests/unit/agent.rs`:

```rust
#[test]
fn attach_spawns_without_prompting_and_driver_events_open_a_side_turn() {
    let (agent, log, spawns) = fake_agent(true, Duration::from_secs(600));
    agent.attach(&cli_settings(), &chat(""));
    agent.attach(&cli_settings(), &chat(""));
    assert_eq!(spawns.load(Ordering::SeqCst), 1);
    assert!(log.lock().unwrap().is_empty(), "no prompt sent");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p aether --lib app::chat::create::tests && cargo test -p aether-agent --lib agent::tests::attach`
Expected: compile errors.

- [ ] **Step 3: Implement**

- **`restore_chats`:** call `restore_messages(provider, &mut messages)` in place of the request loop at `:287-291`. After all chats are pushed, call `self.agent.attach(&effective_settings, &session)` for each chat where `reattach_wanted` holds. Build `effective_settings` and `session` the same way `submit` builds them (`:203-217`).
- **`select(ix)`:** when the chat is OpenCode, has a non-empty `cli_session`, and its key is not in `agent.live_sessions()`, call `attach`.
- **`Agent::attach`:** call `spawn_live` with a `turn` of `None`, and send no command. This requires `spawn_live` to accept `Option<UnboundedSender<Update>>`.
- **OpenCode `start`, when `resumed`:**
  - `GET /api/session/active`. If the session is listed:
    - set `state.active = true`
    - send `Update::Wake`, then `Update::Notice(Notice { id: "reattach".into(), kind: NoticeKind::Wake, text: "Reconnected to a running OpenCode turn".into(), .. })`
    - replay `resync(..)` events through `map_event`, so pending asks of the session and its descendants show
  - The session's later `session.execution.*` event ends the reattached turn as usual.
- **OpenCode `Drop`:** never calls `/interrupt`. It does not today; keep it that way.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: all PASS.

- [ ] **Step 5: Manual check (live service)**

Start an OpenCode chat with a long command ("run `sleep 120 && echo done`"). Quit Aether while it runs, then relaunch it. Expected:
- The chat shows Working again, with a "Reconnected to a running OpenCode turn" notice.
- The turn finishes in the app when the command ends.
- Claude chats that were running at quit show "Interrupted: Aether closed".

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/app crates/agent crates/driver/src/opencode
git commit -m "feat: honest restore, OpenCode turns reattach after relaunch"
```

---

### Task 8: Phase gate

- [ ] **Step 1: Full test and lint run**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: all tests pass and clippy reports no warnings.

- [ ] **Step 2: Load a real store**

Back up `~/Library/Application Support/aether/` (or whatever `aether_core::config_dir()` returns). Then launch the app against it. Expected: every chat that existed before phase 2 is still listed and opens. Restore the backup if anything is lost, and treat that as a blocker.
