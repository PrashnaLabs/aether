# Agent orchestration, phase 2: track and deliver, implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A lead can read, message, and stop its children, and each child's finished turn is pushed into the lead without polling. Deliveries coalesce while the lead is busy.

**Architecture:** Three new tools go through the existing path: socket, then `ChatApp::orchestrate`, then the tool's handler. Delivery hooks `finish_run`. When a child's turn ends, the app builds one message and either submits it to an idle lead or appends it to the lead's queue. Consecutive deliveries in the queue merge into one queued item, so the lead sees them as a single turn.

**Tech Stack:** Rust 2024, gpui (`gpui-kit`), `serde_json`. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-04-agent-orchestration-design.md` (phase 2; decisions 5 and 6; Tools table)

## Global Constraints

- No new crates in any `Cargo.toml`.
- A tool may only touch the calling lead's own children (`child.parent == Some(lead.id)`). Anything else gets `agent {id} isn't one of this lead's children`.
- Results are pushed, never polled. There is no `wait` tool.
- Delivery while the lead is mid-turn waits in the lead's queue and coalesces into one message.
- The lead never answers a child's approval asks. `needs_you` is reported only through `read_agent` and `list_agents` status; it does not trigger a delivery.
- Reply text in deliveries and `read_agent` is truncated to 4000 chars at a char boundary, with `…` appended when cut.
- `message_agent` queues behind a busy child and never steers.
- Tool names: `read_agent`, `message_agent`, `stop_agent`, added after `list_agents` and `spawn_agent` in `tool_specs()` order.
- Tests live in `crates/<crate>/tests/unit/...` via `#[cfg(test)] #[path]`. App tests must never spawn a real CLI: never let a test reach `submit` on a CLI chat.
- Baseline: `cargo fmt --check` red on main; `hackernews::client` is a known flake (`--skip hackernews::client`).
- Commits: Conventional Commits, no `Co-Authored-By` or AI attribution.

## Review Focus

1. **The lead is deleted, or its lead flag is turned off, before a child finishes.** Expected: no delivery and no panic. The child stays an ordinary chat. Test in Task 2.
2. **Several children finish while the lead is busy.** Expected: exactly one queued item, holding every delivery in finish order. Test in Task 2.
3. **The user has typed a queued message into a busy lead, then a child finishes.** Expected: the user's queued item is untouched. The delivery becomes its own queued item after it and does not merge into the user's text. Test in Task 2.
4. **A child id sent as a string (`"171234"`) or a float, or missing.** Expected: string digits are accepted. Anything else gets `missing "id"`. Test in Task 1.
5. **`stop_agent` on an idle child, and `message_agent` on a busy one.** Expected: `{"stopped":false}` with no state change, and `{"status":"queued"}` with the text in the child's queue. Tests in Task 1.

---

## File map

| File | Change |
|------|--------|
| `crates/agent/src/orchestrator/mod.rs` | `tool_specs()` gains the three tools |
| `crates/agent/tests/unit/orchestrator/mcp.rs` | tool-name list assertion updated |
| `crates/app/src/app/orchestrate.rs` | `read_agent`, `message_agent`, `stop_agent`, `own_child`, `clip`, delivery |
| `crates/app/src/app/mod.rs:220` | `Queued.from_children: bool` |
| `crates/app/src/app/chat/send.rs`, `chat/live.rs`, `composer/quote.rs` | `Queued` literals get `from_children: false` |
| `crates/app/src/app/chat/send.rs:520` (`finish_run`) | one call into delivery |
| `crates/app/tests/unit/app/orchestrate.rs` | new tests |

Order: Task 1, then Task 2. Both touch `orchestrate.rs`, so they run sequentially.

---

### Task 1: `read_agent`, `message_agent`, `stop_agent`

**Files:**
- Modify: `crates/agent/src/orchestrator/mod.rs` (`tool_specs`)
- Modify: `crates/agent/tests/unit/orchestrator/mcp.rs` (`tools_list_is_empty_without_token` name list)
- Modify: `crates/app/src/app/orchestrate.rs` (dispatch match in `orchestrate`, new handlers)
- Test: `crates/app/tests/unit/app/orchestrate.rs`

**Interfaces:**
- Consumes:
  - `ChatApp::orchestrate` dispatch (`orchestrate.rs:~98`)
  - `child_status(&Chat) -> &'static str`
  - `ChatApp::stop_chat(ix, cx)` (`chat/send.rs:586`)
  - `ChatApp::submit(ix, text, vec![], window, cx)`
  - `ChatApp::worktree_blocks_send(ix) -> Option<_>`
  - `Chat.queue: Vec<Queued>`, `Chat::busy()`
  - test helper `aether_agent::Cancel::inert()`
  - existing fixtures `lead_app`, `lead_chat` in the test file
- Produces:
  - `pub(super) fn clip(text: &str) -> String`. Keeps the first 4000 chars; if the text was longer, appends `…`.
  - `pub(super) fn own_child(&self, lead_ix: usize, args: &Value) -> Result<usize, String>`. `id` is read as an i64, or as a string of digits. When it is absent or not a number, the error is `missing "id"`. When it names a chat that isn't this lead's child, the error is `agent {id} isn't one of this lead's children`.
  - `pub(super) fn read_agent(&self, lead_ix: usize, args: &Value) -> Result<Value, String>`. Returns `{"id","title","provider","model","status","cwd","branch","replies":[..]}`.
    - `last` defaults to 1 and is clamped to 1..=10.
    - `replies` holds the last `last` Assistant messages' text, run through `clip`, oldest first.
    - `cwd` is `chat_cwd` (or the raw cwd string), and `branch` is `worktree_branch`. Either can be empty.
  - `fn message_agent(&mut self, lead_ix, args, window, cx) -> Result<Value, String>`.
    - Missing or empty `text` gives `missing "text"`.
    - If the child is busy: push `Queued { text, attachments: vec![], delivering: false, from_children: false }` and return `{"status":"queued"}`.
    - Else if `worktree_blocks_send` is set: `Err(reason)`.
    - Else `submit` the raw text (no suffix) and return `{"status":"sent"}`.
  - `fn stop_agent(&mut self, lead_ix, args, cx) -> Result<Value, String>`. If the child is busy, call `stop_chat` and return `{"stopped":true}`; otherwise return `{"stopped":false}`.
  - `tool_specs()` entries:
    - `read_agent{id: integer|string (required), last: integer}`
    - `message_agent{id, text (required)}`
    - `stop_agent{id (required)}`
    - Descriptions stay terse, like the existing ones.
  - `Queued.from_children` is added in Task 2. Until then, write the queue literal with the fields that exist; Task 2 updates every literal.

- [ ] **Step 1: Write the failing tests** in `crates/app/tests/unit/app/orchestrate.rs`. Use the existing `lead_app` fixture, and add one child by pushing a `Chat` with `parent: Some(lead id)` and two Assistant messages, `"first"` and `"second"`.

```rust
#[gpui_kit::test] fn read_agent_returns_last_replies_oldest_first(cx)
    // last absent -> replies == ["second"]; last: 2 -> ["first","second"]; last: 99 -> clamps, no panic
    // status == "idle"; provider/model/title match the child
#[gpui_kit::test] fn read_agent_accepts_string_id_and_rejects_missing(cx)
    // id: "<digits>" works; id absent -> Err(r#"missing "id""#); id: 1.5 -> Err(r#"missing "id""#)
#[gpui_kit::test] fn tools_refuse_another_leads_child(cx)
    // child of lead B, called from lead A, for read/message/stop -> Err("agent <id> isn't one of this lead's children")
#[gpui_kit::test] fn message_agent_queues_behind_busy_child(cx)
    // child.cancel = Some(Cancel::inert()); message_agent{id, text:"more"} -> {"status":"queued"};
    // child.queue.len()==1 && queue[0].text=="more"; missing text -> Err(r#"missing "text""#)
#[gpui_kit::test] fn stop_agent_reports_whether_it_stopped(cx)
    // idle child -> {"stopped":false}; busy (inert cancel) -> {"stopped":true} and child.stopped == true
#[test] fn clip_cuts_at_4000_chars_on_a_char_boundary()
    // "é".repeat(4001) -> 4000 'é' + "…"; short text unchanged
```

Never test the idle `message_agent` path, because it calls `submit` on a CLI chat.

- [ ] **Step 2: Run them and confirm they fail.** `cargo test -p aether orchestrate` should fail to compile on the missing functions.
- [ ] **Step 3: Implement** the handlers and wire them into the `orchestrate` match as `"read_agent"`, `"message_agent"` and `"stop_agent"`. Add the three `tool_specs()` entries and update the mcp test's expected names to `["list_agents","spawn_agent","read_agent","message_agent","stop_agent"]`.
- [ ] **Step 4: Run the tests and confirm they pass.** Run `cargo test -p aether orchestrate`, `cargo test -p aether-agent orchestrator`, then the full `cargo test -p aether -- --skip hackernews::client`.
- [ ] **Step 5: Commit** `feat(app): let leads read, message and stop child agents`.

---

### Task 2: Push child results into the lead

**Files:**
- Modify: `crates/app/src/app/mod.rs:220` (`Queued` gets `pub from_children: bool`), plus the `Queued {` literals in `chat/send.rs:172`, `chat/live.rs:162`, `composer/quote.rs:77` and Task 1's handler (`from_children: false`)
- Modify: `crates/app/src/app/orchestrate.rs` (delivery)
- Modify: `crates/app/src/app/chat/send.rs:520` (`finish_run`)
- Test: `crates/app/tests/unit/app/orchestrate.rs`

**Interfaces:**
- Consumes: `clip` (Task 1); `finish_run`'s `ix`, `reply_ix`, `failed`, and `chat.stopped`; `Chat.lead`, `Chat.parent`; `ChatApp::submit`; `worktree_blocks_send`.
- Produces:
  - `pub(super) fn delivery_text(child: &Chat, reply: &Message, outcome: &str) -> String`. Exact format:

    ```
    [Aether] Agent {id} "{title}" ({provider key}/{model}) {outcome}.

    {clip(body)}

    Call read_agent with id {id} for more.
    ```

    `body` is `reply.text`. When `outcome == "failed"` and `reply.error` is set, `body` is `reply.error`, followed by a blank line and then `reply.text` if that is non-empty.
  - `outcome` is one of `finished`, `failed`, `was stopped` (when `chat.stopped`).
  - `pub(super) fn enqueue_delivery(&mut self, lead_ix: usize, text: String)`.
    - If the lead's last queued item has `from_children && !delivering`, append `"\n\n---\n\n" + text` to it.
    - Otherwise push `Queued { text, attachments: vec![], delivering: false, from_children: true }`.
    - Ends with `cx.notify()`, or leaves that to the caller, whichever matches the surrounding code.
  - `pub(super) fn deliver_to_lead(&mut self, child_ix: usize, reply_ix: usize, failed: bool, window, cx)`.
    - No-op unless the child has a `parent` that names an existing chat with `lead == true`.
    - Otherwise it builds `delivery_text`.
    - If the lead is `busy()`, or `worktree_blocks_send(lead_ix)` is set, it calls `enqueue_delivery`.
    - Otherwise it calls `submit(lead_ix, text, vec![], window, cx)`.
  - In `finish_run`, inside the `chat.epoch == epoch && reply exists` branch, after `persist(ix)` and only when `chat.run == run`, call `self.deliver_to_lead(ix, reply_ix, failed, window, cx)`. The lead's own queue drains through the existing `next_queued` path when its turn ends, which gives coalesced delivery for free.
  - Index safety: `deliver_to_lead` may `submit` on the lead, which never changes `self.chats` indices, so `ix` stays valid for the rest of `finish_run`. Keep the call before the `let blocked = ...` block, so that block re-borrows `self.chats[ix]` afresh.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn delivery_text_formats_finished_failed_and_stopped()
    // finished: exact string per Interfaces; failed with error "boom" + text "partial" -> body "boom\n\npartial";
    // body over 4000 chars is clipped with "…"
#[gpui_kit::test] fn busy_lead_coalesces_deliveries_into_one_queued_item(cx)
    // lead.cancel = Some(Cancel::inert()); deliver_to_lead for two children (each with one Assistant reply)
    // -> lead.queue.len()==1, from_children, text contains both deliveries in order separated by "\n\n---\n\n"
#[gpui_kit::test] fn delivery_does_not_merge_into_users_queued_text(cx)
    // lead busy with queue [Queued{text:"user note", from_children:false}]; one delivery
    // -> queue.len()==2, queue[0].text=="user note", queue[1].from_children
#[gpui_kit::test] fn no_delivery_without_a_live_lead(cx)
    // child.parent = Some(id of a chat with lead=false) and Some(9999 nonexistent) -> no queue change anywhere, no panic
```

Every delivery test keeps the lead busy, using `Cancel::inert()`, so no test reaches `submit`.

- [ ] **Step 2: Run them and confirm they fail.** `cargo test -p aether orchestrate` should fail to compile.
- [ ] **Step 3: Implement.** Add the `Queued.from_children` field and update its literals, then `delivery_text`, `enqueue_delivery`, `deliver_to_lead`, and the `finish_run` call.
- [ ] **Step 4: Run the tests and confirm they pass.** Run `cargo test -p aether orchestrate`, then the full `cargo test -p aether -- --skip hackernews::client`.
- [ ] **Step 5: Commit** `feat(app): deliver child agent results to their lead`.

---

### Task 3: Desktop check (user)

- [ ] **Step 1:** A Claude lead spawns a Codex child with a small task. When the child finishes, a `[Aether] Agent … finished.` message arrives in the lead, and the lead answers it without being prompted.
- [ ] **Step 2:** The lead spawns two children with short tasks and is given a long task of its own. Both deliveries should arrive as one queued message after the lead's turn ends.
- [ ] **Step 3:** The lead calls `read_agent`, `message_agent` (the child continues), and `stop_agent` on a running child (the child stops, and a `was stopped` delivery arrives).
