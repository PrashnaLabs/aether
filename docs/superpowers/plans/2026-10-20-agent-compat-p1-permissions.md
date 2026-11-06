# Provider Compat Phase 1: Permission and Hang Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the unsafe and hanging approval paths in the Claude, OpenCode, Codex and Antigravity Drivers, using the existing `Ask`/`Answer` shapes, so phase 1 ships on its own.

**Architecture:** Driver-local fixes plus one UI fix. Approval cards get the full command or diff. OpenCode accepts ask events from descendant child sessions and lets the server scope "always". Codex surfaces MCP tool approvals and permission requests that are auto-declined or rejected today. Antigravity's warned full-access rerun lists every blocked command. The core types do not change; phase 2 replaces them.

**Tech Stack:** Rust 2024, serde_json, reqwest (blocking via `Api`), GPUI via gpui-kit 0.7.3.

**Spec:** `docs/superpowers/specs/2026-10-20-agent-compat-chat-revamp-design.md` (sections 3 and 4, and "Phasing" item 1). Prototype: `docs/superpowers/specs/2026-10-20-agent-chat-prototype.html`, "Needs approval" chip.

## Global Constraints

- Unit tests live in `crates/<crate>/tests/unit/<path>.rs`, included from the source file with `#[cfg(test)] #[path = …] mod tests;` (GLOSSARY "Unit test file"). Never inline.
- Ask the user before every commit (CLAUDE.md). Never add `Co-Authored-By` or AI attribution. Commit messages follow Conventional Commits, terse.
- Parallel worktrees use their own `CARGO_TARGET_DIR`.
- Anti-goal: "Approving something the user could not fully see." No approval text may be clipped without a way to see the rest.
- Anti-goal: "Any blanket permission grant the user did not knowingly choose."
- Antigravity: verified 2026-10-20 that `agy --help` (1.4.2) offers no per-action approval channel in print mode. The user chose: "Allow" reruns the turn with full permissions, behind a warning that lists every blocked action in full.
- Codex MCP approval "always": the persist shape of the `_meta` reply is unverified. Until a recorded fixture pins it (phase 3), "always" sends the same reply as "once".

## Review Focus

- A 500-line heredoc approval: the card must show every line in a scrolling block, with Allow and Deny still on screen (Task 1, manual check step).
- A grandchild OpenCode session (subagent of a subagent) asking for permission must surface, not only direct children (Task 4 test `descends_walks_grandchildren`).
- An OpenCode child session that 404s or has no parent must be cached as "not ours" without panicking or blocking the stream (Task 4 test `descends_caches_unknown_as_foreign`).
- A Claude Bash approval whose edit field is cleared to empty must run the original command, never an empty one (Task 2 test `empty_bash_edit_keeps_the_original_command`).
- A Codex elicitation in an unknown mode (`openai/userVerification`, `form` without the approval kind) must still be declined at once, never left pending (Task 5 test `other_elicitations_still_decline`).

---

### Task 1: Approval code block shows every line

**Files:**
- Modify: `crates/ui/src/chat/request.rs:103-136` (`RequestCode`)
- Modify: `crates/app/src/hitl.rs:278` (only caller)

**Interfaces:**
- Produces: `RequestCode::new(id: impl Into<ElementId>, text: impl Into<SharedString>) -> RequestCode`

- [ ] **Step 1: Change `RequestCode` to render every line in a scroll container**

Drop `CODE_LINES` truncation and the "… N more lines" row. Give the `v_flex` the new `id`, then `.max_h(px(260.))` and `.overflow_y_scroll()`. The rubric says: "Multi-line fields cap height then scroll." Lines keep `whitespace_nowrap` off, as today.

- [ ] **Step 2: Update the caller**

`crates/app/src/hitl.rs:278`: `RequestCode::new(SharedString::from(format!("request-{id}-code")), input.clone())`.

- [ ] **Step 3: Build**

Run: `cargo build -p aether`
Expected: builds with no warnings from the changed files.

- [ ] **Step 4: Manual check**

Run the app and trigger a Claude Bash approval for a 40-line heredoc (`cat <<'EOF' > /tmp/x.txt` plus 38 lines plus `EOF`). Expected: every line can be reached by scrolling inside the card, and Allow and Deny stay visible.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/ui/src/chat/request.rs crates/app/src/hitl.rs
git commit -m "fix(hitl): approval code block scrolls instead of truncating"
```

---

### Task 2: Claude approvals show the full command or diff, and "always" never switches mode

**Files:**
- Modify: `crates/driver/src/claude.rs:424-447` (`reply`), `:662-690` (`control_request`)
- Test: `crates/driver/tests/unit/claude.rs`

**Interfaces:**
- Produces (private): `fn approval_input(tool: &str, input: &Value) -> String` in `claude.rs`

- [ ] **Step 1: Write the failing tests**

Add to `crates/driver/tests/unit/claude.rs`:

```rust
#[test]
fn bash_approval_shows_every_line_and_edits_keep_them() {
    let cmd = "cat <<'EOF' > a.txt\nline one\nline two\nEOF";
    let line = json!({"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command": cmd}}}).to_string();
    let mut s = State::default();
    let [(id, Ask::Approval { input, .. })] = &asks(map_line(&mut s, &line))[..] else { panic!() };
    assert_eq!(input, cmd);
    let edited = "cat <<'EOF' > b.txt\nline one\nEOF";
    let rep = reply(&mut s, *id, &Answer::Approve { input: edited.into(), always: false }).unwrap();
    assert_eq!(rep["response"]["response"]["updatedInput"]["command"], edited);
}

#[test]
fn empty_bash_edit_keeps_the_original_command() {
    let line = r#"{"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#;
    let mut s = State::default();
    let [(id, _)] = asks(map_line(&mut s, line))[..] else { panic!() };
    let rep = reply(&mut s, id, &Answer::Approve { input: "  ".into(), always: false }).unwrap();
    assert_eq!(rep["response"]["response"]["updatedInput"]["command"], "ls");
}

#[test]
fn write_approval_shows_path_and_diff_and_always_skips_set_mode() {
    let mut s = State::default();
    let [(id, Ask::Approval { input, .. })] = &asks(map_line(&mut s, WRITE_REQUEST))[..] else { panic!() };
    assert!(input.starts_with("/private/tmp/claude-probe/probe1.txt\n"), "{input}");
    assert!(input.lines().any(|l| l == "+hi"), "{input}");
    let rep = reply(&mut s, *id, &Answer::Approve { input: input.clone(), always: true }).unwrap();
    assert!(rep["response"]["response"].get("updatedPermissions").is_none(), "{rep}");
}
```

Update the existing tests to the new behaviour:
- `approval_card_shows_why_and_allows_with_original_input`: the Write `input` is no longer the bare path. Assert `title`, `tool` and `reason` as before, and assert that `input.starts_with("/private/tmp/claude-probe/probe1.txt\n")`.
- `approve_sends_edited_bash_command_and_session_rules`: the edited inputs become `"rm -rf dist"` and `"rm -rf build"`, with no `"\nDeletes files"` suffix. Expected JSON is unchanged.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p aether-driver --lib claude::tests`
Expected: the three new tests FAIL. `input` is the first line only, the edit sends only its first line, and `updatedPermissions` carries `setMode`.

- [ ] **Step 3: Implement**

- `approval_input(tool, input)` calls `classify(tool, input).1` and returns:
  - `ToolDetail::Command { command, .. }`: `command`, with every line.
  - `ToolDetail::Edit { path, diff, .. }`: `path`, plus `"\n"` and `diff` when `diff` is non-empty.
  - Any other detail: `detail.summary()`.
- `control_request` uses `approval_input` for both `shown` and `Ask::Approval.input`.
- `reply`, Bash case: `let edited = edited.trim_end();`. If `tool == "Bash"` and the edited text is non-empty after `trim()` and differs from `shown`, set `input["command"] = edited.into()`. Delete the `.lines().next()`.
- `reply`, `always` case: keep only the suggestions where `r["type"] != "setMode"`. Set `updatedPermissions` only when at least one remains.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib claude::tests`
Expected: all PASS.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/driver/src/claude.rs crates/driver/tests/unit/claude.rs
git commit -m "fix(claude): full command in approvals, always never switches mode"
```

---

### Task 3: OpenCode lets the server scope "always", and unoffered modes are safe

**Files:**
- Modify: `crates/driver/src/opencode/session.rs:177-180` (`reply`), `:271-286` (`rules`)
- Modify: `crates/driver/src/opencode/events.rs:12-14` (`decision`), `:117-148` (`permission.asked`)
- Modify: `crates/driver/src/opencode/mod.rs:52-60` (drop `State.always`), `:183-196` (`answer`)
- Test: `crates/driver/tests/unit/opencode/events.rs`, `crates/driver/tests/unit/opencode/session.rs`

**Interfaces:**
- Produces: `pub(super) fn decision(answer: &Answer) -> &'static str` in `events.rs`. It returns `"always"`, `"once"` or `"reject"`.
- Produces: `pub(super) fn reply(api: &Api, session: &str, request: &str, decision: &str) -> Result<(), String>` in `session.rs`.

- [ ] **Step 1: Write the failing tests**

`crates/driver/tests/unit/opencode/session.rs`:

```rust
#[test]
fn unoffered_modes_never_allow_everything() {
    let effects = |a| -> Vec<(String, String)> {
        rules(a).as_array().unwrap().iter()
            .filter(|r| r["action"] != "external_directory")
            .map(|r| (r["action"].as_str().unwrap().into(), r["effect"].as_str().unwrap().into()))
            .collect()
    };
    let deny = vec![("edit".into(), "deny".into()), ("shell".into(), "deny".into())];
    assert_eq!(effects(AccessMode::ReadOnly), deny);
    assert_eq!(effects(AccessMode::Auto), effects(AccessMode::Ask));
    assert!(effects(AccessMode::Full).is_empty());
}
```

`crates/driver/tests/unit/opencode/events.rs`:

```rust
#[test]
fn shell_approval_lists_each_command_and_what_always_covers() {
    let mut s = State { active: true, ..State::default() };
    let asked = r#"{"type":"permission.asked","data":{"id":"per_1","sessionID":"ses_1","action":"shell","resources":["ls src","cat package.json"],"save":["ls *","cat *"],"message":"Inspect the project"}}"#;
    let [Update::Ask { ask: Ask::Approval { input, reason, .. }, .. }] = &ev(&mut s, asked)[..] else { panic!() };
    assert_eq!(input, "ls src\ncat package.json");
    assert_eq!(reason, "Inspect the project. Always allows: ls *, cat *");
}

#[test]
fn edit_approval_shows_the_patch() {
    let mut s = State { active: true, ..State::default() };
    let asked = r#"{"type":"permission.asked","data":{"id":"per_2","sessionID":"ses_1","action":"edit","resources":["src/a.ts"],"metadata":{"files":[{"file":"src/a.ts","patch":"@@ -1 +1 @@\n-a\n+b"}]}}}"#;
    let [Update::Ask { ask: Ask::Approval { input, .. }, .. }] = &ev(&mut s, asked)[..] else { panic!() };
    assert_eq!(input, "src/a.ts\n@@ -1 +1 @@\n-a\n+b");
}

#[test]
fn always_is_sent_to_the_server_not_remembered_per_action() {
    assert_eq!(decision(&Answer::Approve { input: String::new(), always: true }), "always");
    assert_eq!(decision(&Answer::Approve { input: String::new(), always: false }), "once");
    assert_eq!(decision(&Answer::Reply("x".into())), "once");
    assert_eq!(decision(&Answer::Deny), "reject");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p aether-driver --lib opencode`
Expected: compile errors (`decision` takes `bool`), then FAIL on the input, reason and rules assertions.

- [ ] **Step 3: Implement**

- **`rules`:**
  - `ReadOnly` gets deny rules for `edit` and `shell`. Use the same shape as `ask(…)`, with `"effect": "deny"`.
  - `Auto` shares the `Ask` arm.
  - `Full` stays `[]`.
  - The `external_directory` rule is unchanged.
- **`permission.asked`:**
  - `input` is `resources.join("\n")`. If `data.metadata.files[*].patch` strings exist, append each one on new lines after the resources. Skip any patch whose `file` is not among the resources.
  - `reason` is the non-empty parts of [`data.message`, `"Always allows: " + save.join(", ")`] joined with `". "`.
  - Delete the `s.always.contains(action)` check.
- **`State.always`:** delete the field and its import of `HashSet`, if that becomes unused.
- **`mod.rs` `answer`:**
  - Compute `decision(&answer)`. Call `deny(&mut s, &request)` when it is `"reject"`.
  - Pass the decision to `session::reply`.
  - Auto-approvals in `session::listen` pass `"once"`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib opencode`
Expected: all PASS, including the existing `rejected_permissions_and_cancels_settle_tools_as_denied_or_stopped`.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/driver/src/opencode crates/driver/tests/unit/opencode
git commit -m "fix(opencode): server-scoped always, full approval text, safe mode arms"
```

---

### Task 4: OpenCode surfaces asks from child sessions at any depth

**Files:**
- Modify: `crates/driver/src/opencode/mod.rs:40-44` (`OpencodeAsk`), `:51-76` (`State`), `:183-221` (`answer`)
- Modify: `crates/driver/src/opencode/session.rs:62-133` (`listen` filter), `:136-162` (`resync`)
- Modify: `crates/driver/src/opencode/events.rs:70-160` (form and permission events carry the session), `:174-189` (`ask_form`)
- Test: `crates/driver/tests/unit/opencode/session.rs`, `crates/driver/tests/unit/opencode/events.rs`

**Interfaces:**
- Consumes: `decision`, and `reply(api, session, request, decision)` from Task 3.
- Produces:
  - `OpencodeAsk::Permission { request: String, action: String, session: String }` and `OpencodeAsk::Form { id: String, fields: Vec<Value>, answer: Map<String, Value>, session: String }`. `session` is the session that raised the ask, and replies go to it.
  - `State.root: String`, the chat's own session id, set in `start`.
  - `pub(super) fn descends(root: &str, sid: &str, cache: &mut HashMap<String, bool>, parent_of: impl FnMut(&str) -> Option<String>) -> bool` in `session.rs`. It walks `parentID` links up to 8 levels.
  - `const ASK_EVENTS: [&str; 6] = ["permission.asked", "permission.replied", "permission.rejected", "form.created", "form.replied", "form.cancelled"]`.
  - `pub(super) fn ask_form(s: &mut State, session: String, id: String, fields: Vec<Value>, answer: Map<String, Value>) -> Vec<Update>`.

- [ ] **Step 1: Write the failing tests**

`crates/driver/tests/unit/opencode/session.rs`:

```rust
fn parents(pairs: &[(&str, &str)]) -> impl FnMut(&str) -> Option<String> {
    let map: HashMap<String, String> = pairs.iter().map(|(c, p)| (c.to_string(), p.to_string())).collect();
    move |sid| map.get(sid).cloned()
}

#[test]
fn descends_walks_grandchildren() {
    let mut cache = HashMap::new();
    let mut lookups = parents(&[("ses_c", "ses_root"), ("ses_g", "ses_c")]);
    assert!(descends("ses_root", "ses_g", &mut cache, &mut lookups));
    assert_eq!(cache.get("ses_g"), Some(&true));
    assert_eq!(cache.get("ses_c"), Some(&true));
}

#[test]
fn descends_caches_unknown_as_foreign() {
    let mut cache = HashMap::new();
    assert!(!descends("ses_root", "ses_x", &mut cache, parents(&[])));
    assert!(!descends("ses_root", "ses_y", &mut cache, parents(&[("ses_y", "ses_other")])));
    assert_eq!(cache.get("ses_x"), Some(&false));
    let mut calls = 0;
    assert!(!descends("ses_root", "ses_x", &mut cache, |_| { calls += 1; None }));
    assert_eq!(calls, 0, "cached answers skip the lookup");
}
```

`crates/driver/tests/unit/opencode/events.rs`:

```rust
#[test]
fn child_session_asks_are_labelled_and_answered_on_the_child() {
    let mut s = State { active: true, root: "ses_1".into(), ..State::default() };
    let asked = r#"{"type":"permission.asked","data":{"id":"per_9","sessionID":"ses_child","action":"shell","resources":["pnpm add motion"]}}"#;
    let [Update::Ask { id, ask: Ask::Approval { title, .. } }] = &ev(&mut s, asked)[..] else { panic!() };
    assert_eq!(title, "Subagent · pnpm add motion");
    assert!(matches!(&s.asks[id], OpencodeAsk::Permission { session, .. } if session == "ses_child"));
    let form = r#"{"type":"form.created","data":{"form":{"id":"frm_2","sessionID":"ses_child","fields":[{"key":"k","title":"Pick"}]}}}"#;
    let [Update::Ask { id, .. }] = &ev(&mut s, form)[..] else { panic!() };
    assert!(matches!(&s.asks[id], OpencodeAsk::Form { session, .. } if session == "ses_child"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p aether-driver --lib opencode`
Expected: compile errors (`descends` and `State.root` are missing).

- [ ] **Step 3: Implement**

- **`descends`:**
  - Return the cached value for `sid` if present.
  - Otherwise walk `parent_of` from `sid`, at most 8 hops. It is true if the walk reaches `root`, false on `None`, on a cycle, or past the depth limit.
  - Cache the result for every id visited.
- **`listen`:**
  - Keep a `HashMap<String, bool>` cache.
  - A frame whose sid equals `session` is handled exactly as today.
  - A frame whose type is in `ASK_EVENTS` is handled if `descends(&session, sid, …)` returns true. The lookup is a `GET /api/session/{sid}` that reads `data.parentID`, through `api.send` with `REQUEST_TIMEOUT`. A non-200 response means `None`.
  - Every other foreign frame is skipped, as today.
- **`resync`:**
  - Replace the per-session `/permission` and `/form` lists with `GET /api/permission/request` and `GET /api/form`.
  - Keep only items whose `sessionID` (or `form.sessionID`) equals `session` or descends from it.
  - Thread the cache through, so `resync` takes `&mut HashMap<String, bool>`.
- **`events.rs`:**
  - `permission.asked` stores `session: data.sessionID`.
  - When that session differs from `s.root`, the title is `format!("Subagent · {title}")`.
  - `form.created` passes `form.sessionID` into `ask_form`, which sets it on the ask.
- **`mod.rs` `answer`:** reply with the ask's own `session`, not `c.session`. This covers `reply`, `reply_form` and `cancel_form`. A follow-up `ask_form` call keeps the same session.
- **`start`:** set `state.root = session.clone()` before spawning `listen`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib opencode`
Expected: all PASS.

- [ ] **Step 5: Manual check (live service)**

Run the app with an OpenCode chat in Supervised mode. Prompt: "spawn a subagent that runs `ls` in the shell". Expected: an approval card titled "Subagent · ls…" appears. Allowing it lets the turn finish.

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/driver/src/opencode crates/driver/tests/unit/opencode
git commit -m "fix(opencode): surface child-session permissions and forms"
```

---

### Task 5: Codex surfaces MCP tool approvals and permission requests

**Files:**
- Modify: `crates/driver/src/codex.rs:455-473` (`reply`), `:749-805` (`server_request`), and the `asks` field on `State`
- Test: `crates/driver/tests/unit/codex.rs:~420-440` (existing elicitation assertion), plus new tests

**Interfaces:**
- Produces (private): `enum CodexAsk { Decision, Question(String), McpApproval, Permissions(Value) }`. `State.asks` becomes `HashMap<u64, (Value, CodexAsk)>`, where `Value` is the rpc id.

- [ ] **Step 1: Write the failing tests**

```rust
const MCP_APPROVAL: &str = r#"{"method":"mcpServer/elicitation/request","id":11,"params":{"serverName":"figma","threadId":"t","mode":"form","message":"Approve app tool call?","requestedSchema":{"type":"object","properties":{}},"_meta":{"codex_approval_kind":"mcp_tool_call"}}}"#;
const PERMS: &str = r#"{"method":"item/permissions/requestApproval","id":12,"params":{"threadId":"t","turnId":"u","itemId":"i","cwd":"/p","startedAtMs":1,"reason":"Needs network","permissions":{"network":{"enabled":true}}}}"#;

#[test]
fn mcp_tool_approvals_ask_and_accept() {
    let mut s = State { active: true, ..State::default() };
    let [Update::Ask { id, ask: Ask::Approval { tool, title, input, .. } }] = &ev(&mut s, MCP_APPROVAL)[..] else { panic!() };
    assert_eq!((tool.as_str(), title.as_str(), input.as_str()), ("mcp", "Approve app tool call?", "figma"));
    assert_eq!(reply(&mut s, *id, &Answer::Approve { input: String::new(), always: false }),
        Some(json!({"id": 11, "result": {"action": "accept", "content": {}, "_meta": null}})));
    ev(&mut s, MCP_APPROVAL);
    assert_eq!(reply(&mut s, 2, &Answer::Deny),
        Some(json!({"id": 11, "result": {"action": "decline", "content": null, "_meta": null}})));
}

#[test]
fn url_elicitation_shows_the_link() {
    let mut s = State { active: true, ..State::default() };
    let url = r#"{"method":"mcpServer/elicitation/request","id":13,"params":{"serverName":"linear","threadId":"t","mode":"url","elicitationId":"e","message":"Sign in to Linear","url":"https://linear.app/oauth"}}"#;
    let [Update::Ask { ask: Ask::Approval { input, reason, .. }, .. }] = &ev(&mut s, url)[..] else { panic!() };
    assert_eq!((input.as_str(), reason.as_str()), ("https://linear.app/oauth", "Sign in to Linear"));
}

#[test]
fn permission_requests_grant_for_turn_or_session() {
    let mut s = State { active: true, ..State::default() };
    let [Update::Ask { id, ask: Ask::Approval { tool, reason, .. } }] = &ev(&mut s, PERMS)[..] else { panic!() };
    assert_eq!((tool.as_str(), reason.as_str()), ("permissions", "Needs network"));
    assert_eq!(reply(&mut s, *id, &Answer::Approve { input: String::new(), always: true }),
        Some(json!({"id": 12, "result": {"permissions": {"network": {"enabled": true}}, "scope": "session"}})));
    ev(&mut s, PERMS);
    assert_eq!(reply(&mut s, 2, &Answer::Deny),
        Some(json!({"id": 12, "result": {"permissions": {}, "scope": "turn"}})));
}

#[test]
fn other_elicitations_still_decline() {
    let mut s = State { active: true, ..State::default() };
    let verify = r#"{"method":"mcpServer/elicitation/request","id":14,"params":{"mode":"openai/userVerification","challenge":"c","title":"t","description":"d"}}"#;
    assert_eq!(ev(&mut s, verify), []);
    assert_eq!(s.outbox.last(), Some(&json!({"id": 14, "result": {"action": "decline", "content": null, "_meta": null}})));
}
```

The existing test that asserts `item/tool/call` returns -32601 and that an empty elicitation is declined stays unchanged. Both behaviours remain.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p aether-driver --lib codex::tests`
Expected: FAIL, because the elicitation is declined and permissions get -32601.

- [ ] **Step 3: Implement**

- **`server_request`:**
  - `mcpServer/elicitation/request` where `p._meta.codex_approval_kind == "mcp_tool_call"` becomes:
    `Ask::Approval { tool: "mcp", title: p.message or "Approve app tool call?", input: p.serverName, reason: "" }`, with kind `McpApproval`.
  - `mode == "url"` becomes:
    `Ask::Approval { tool: "mcp", title: format!("{serverName} needs you to open a link"), input: p.url, reason: p.message }`, with kind `McpApproval`.
  - Every other elicitation declines, as today.
  - `item/permissions/requestApproval` becomes:
    `Ask::Approval { tool: "permissions", title: "Grant extra permissions", input: serde_json::to_string_pretty(&p.permissions), reason: p.reason or "" }`, with kind `Permissions(p.permissions.clone())`.
  - Side-thread requests (`side` true) for both decline at once, the same as the existing side-thread approvals. Permissions use `{"permissions": {}, "scope": "turn"}`.
- **`reply`:** match on `CodexAsk`.
  - `McpApproval`: an approve or reply sends `{"action":"accept","content":{},"_meta":null}`. Anything else sends a decline. `always` is treated as once; see Global Constraints.
  - `Permissions(req)`: approve sends `{"permissions": req, "scope": if always {"session"} else {"turn"}}`. Deny sends `{"permissions": {}, "scope": "turn"}`.
  - `Decision` and `Question` keep their current behaviour.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib codex::tests`
Expected: all PASS.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/driver/src/codex.rs crates/driver/tests/unit/codex.rs
git commit -m "fix(codex): ask for mcp tool approvals and extra permissions"
```

---

### Task 6: Antigravity's full-access rerun lists every blocked command

**Files:**
- Modify: `crates/driver/src/antigravity.rs`. The changes are `State.last_empty`, the tool step handling around `:540-546`, `result` at `:586-625` and `answer` at `:292-306`.
- Test: `crates/driver/tests/unit/antigravity.rs:168-198`

**Interfaces:**
- Produces (private): `State.blocked: Vec<ToolCall>` replaces `last_empty: Option<ToolCall>`. It holds the turn's finished tool rows that had no output, oldest first, and is cleared when each turn starts. `s.ask` changes from `(u64, String)` to `(u64, Vec<String>)`, the full blocked actions.

- [ ] **Step 1: Write the failing tests**

Replace the expected `Ask` in `denied_actions_mark_the_row_and_ask_to_retry_without_final`:

```rust
Ask::Approval {
    tool: "Antigravity".into(),
    title: "Rerun with full access?".into(),
    input: "touch denied2.txt".into(),
    reason: "agy blocked these actions. Allowing reruns this turn with full permissions: every action in it runs without asking, not only these.".into(),
}
```

Add:

```rust
const TWO_DENIED: &str = r#"{"event":"step_update","step_update":{"conversation_id":"c","step_index":1,"state":"DONE","step_type":"tool","tool_name":"run_command","duration_seconds":0.1,"tool_info":{"name":"run_command","parameters":{"CommandLine":"touch one.txt"}}}}
{"event":"step_update","step_update":{"conversation_id":"c","step_index":2,"state":"DONE","step_type":"tool","tool_name":"run_command","duration_seconds":0.1,"tool_info":{"name":"run_command","parameters":{"CommandLine":"touch denied2.txt"}}}}
{"event":"result","result":{"conversation_id":"c","status":"SUCCESS","response":"","denied_actions":[{"action":"command","display_name":"RunCommand"},{"action":"command","display_name":"RunCommand"}]}}"#;

#[test]
fn every_denied_action_marks_its_own_row() {
    let mut s = running();
    let out = feed(&mut s, TWO_DENIED);
    let denied = out.iter().filter(|u| matches!(u, Update::Tool(c) if c.status == ToolStatus::Denied)).count();
    assert_eq!(denied, 2, "{out:?}");
    let Some(Update::Ask { ask: Ask::Approval { input, .. }, .. }) = out.last() else { panic!("{out:?}") };
    assert_eq!(input, "touch one.txt\ntouch denied2.txt");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p aether-driver --lib antigravity::tests`
Expected: FAIL on title, input and reason, and on the denied count (1).

- [ ] **Step 3: Implement**

- **Tool steps:** each tool step that finishes with no output is pushed onto `s.blocked`. This replaces the assignment to `last_empty`. Clear `s.blocked` wherever a turn starts (`prompt`).
- **`result`, marking rows:** with `n = denied.len()`, take the last `n` entries of `s.blocked`, set each to `ToolStatus::Denied`, and emit each one.
- **`result`, the approval card:** list one line per marked row:
  - `ToolDetail::Command { command, .. }` gives `command`.
  - `ToolDetail::Edit { path, .. }` gives `path`.
  - Anything else gives `detail.summary()`.
  - If no rows matched, list the `display_name`s instead.
- **`result`, copy:** use the exact title and reason from Step 1.
- **`answer`:** the retry message names the same full list: `"The user approved the blocked actions:\n{list}\nRetry them and continue."`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib antigravity::tests`
Expected: all PASS, including `denied_actions_under_full_access_just_end_the_turn`.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/driver/src/antigravity.rs crates/driver/tests/unit/antigravity.rs
git commit -m "fix(antigravity): warned rerun lists every blocked action"
```

---

### Task 7: Phase gate

- [ ] **Step 1: Full test and lint run**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: all tests pass and clippy reports no warnings.

- [ ] **Step 2: Manual pass**

In the app, check that each case brings up a correct card:
- a Claude Bash heredoc approval
- a Claude Write approval with "Allow for session" (the mode chip must not change)
- an OpenCode subagent shell approval
- a Codex chat using an MCP tool that needs approval, if one is configured; otherwise rely on the tests
- an Antigravity blocked command in Auto-accept edits mode
