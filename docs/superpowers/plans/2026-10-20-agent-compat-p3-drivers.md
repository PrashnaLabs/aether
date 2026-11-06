# Provider Compat Phase 3: Drivers on the New Model Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the five Drivers and the built-in OpenRouter agent to the phase 2 core model. Every approval, question, form, subagent, background task, notice, cost, model and mode a CLI reports must reach the transcript, and recorded CLI streams must prove it.

**Architecture:**
- **Task 1** reshapes `Ask`/`Answer` with no change to the wire format, and gives the app functional cards for the new shapes.
- **Tasks 2-3** add a record mode, a headless recording example, real fixtures per Provider, and replay helpers. The helpers feed a fixture through each Driver's pure map function.
- **Task 4** adds the Driver-side pieces every track shares:
  - trait hooks
  - stderr tail
  - notice ids
  - an unknown-event log
  - steer refusal

  After it, no two tracks edit the same lines.
- **Tasks 5-19** form one track per Driver. Tracks are independent and run in parallel worktrees. Inside a track, the tasks run in order.
- **Task 20** adapts OpenRouter. **Task 21** adds version gates and probe data. **Task 22** is the phase gate.

**Tech Stack:** Rust 2024, serde/serde_json, reqwest (OpenCode), futures mpsc, GPUI via gpui-kit 0.7.3 (cards only).

**Spec:** `docs/superpowers/specs/2026-10-20-agent-compat-chat-revamp-design.md`:
- section 3 (permissions and asks)
- section 4 (Drivers)
- section 5, "Also in scope" (startup checklist, version chip)
- section 6 (verification)
- "Decisions by the user"

Evidence for the event shapes is in `docs/wayfinder/research/2026-10-20-driver-audits.md`. This plan builds on the phase 1 plan (`…-p1-permissions.md`) and the phase 2 plan (`…-p2-core-lifecycle.md`). Treat both plans' Interfaces blocks as already implemented.

## Global Constraints

- Unit tests live in `crates/<crate>/tests/unit/<path>.rs`, included with `#[cfg(test)] #[path = …] mod tests;`. Never inline (GLOSSARY "Unit test file"). Shared test-only helpers are `#[cfg(test)] pub(crate)` items in the source module they serve.
- Ask the user before every commit. Implementers never stage or commit. Never add `Co-Authored-By` or AI attribution. Commit messages follow Conventional Commits and stay terse.
- Phases 1 and 2 are merged before Task 1 starts. Line numbers below are approximate, so find code by function name. Phase 1 already did the following, so nothing here plans them again:
  - Claude builds approval input from the raw input, and filters out `setMode`.
  - OpenCode handles child-session asks through `descends` and `is_family`, resync passes the location, and "always" sends "once" for `save: ["*"]`.
  - Codex handles MCP approvals, URL elicitations and permission requests.
  - Antigravity lists every blocked action.
- **Parallel work.**
  - Each worktree uses its own target dir (`CARGO_TARGET_DIR=/tmp/aether-target-<track>`).
  - After Tasks 1-4, these tracks are independent and parallelizable:
    - Claude: Tasks 5-8
    - Codex: Tasks 9-12
    - OpenCode: Tasks 13-15
    - Pi: Tasks 16-17
    - Antigravity: Tasks 18-19
    - OpenRouter: Task 20
  - Task 21 starts after every track has merged.
- **Anti-goals (spec).**
  - No hidden blocked state: every ask a CLI raises reaches the user, including asks from subagents and child sessions.
  - No blanket grant: no `--dangerously-skip-permissions` outside the warned rerun, no per-tool "always", and `setMode` is never folded into "always".
  - No silent drop:
    - Every CLI event a mapper does not handle goes through `log_unknown` (Task 4).
    - Every unknown server request gets an error reply plus `Update::Warning`.
  - No per-provider special case in a view: cards render `Ask` only.
- **Errors.** `Update::Warning(text)` is for anything recoverable. `Update::Error(text)` is only for a failure that ends the turn or the session. Its first line is the message and the rest is the stderr tail, from `StderrTail::fatal` (Task 4).
- **Cost.** `Update::Cost(u64)` is in micro-USD (phase 2): `(usd * 1_000_000.0).round() as u64`. A CLI that reports a running session total sends the difference from its last report.
- **Steers.** A Driver whose `reports_steered()` is true emits `Update::Steered(text)` when the CLI consumes the steer. A steer the CLI refuses is sent back as `Update::SteerRefused(text)` (Task 4), never as a hidden new turn.
- Tested CLI versions: Claude Code 2.1.304, Codex 0.163.0, OpenCode 2.0.32, Pi 1.3.0, agy 1.4.2.
- **Fixtures.**
  - Recorded in a throwaway project under `/tmp`, and scrubbed before commit.
  - Hand-written fixtures are named `<scenario>.hand.jsonl`.
  - Regenerable schemas (`codex app-server generate-json-schema`, OpenCode `/openapi.json`) are never committed.
- **Replay tests** assert facts that survive re-recording ("some row has `ToolDetail::Task` and another row's `parent` is its id"). They never assert exact ids, counts or text the model chose.

## Review Focus

- A chat saved before this phase, holding `Approval` and `Plan` requests, must load and re-save as identical JSON (Task 1 test `old_ask_json_round_trips_unchanged`).
- A form with a required field left empty, or a field hidden by a `when` rule, must never send a reply the CLI rejects. An empty required field blocks submit with a message, and a hidden field is left out of the answer (Task 1 test `form_answer_requires_visible_required_fields`, Task 14 test `hidden_fields_reply_with_their_defaults`).
- A steer the CLI refuses must go back to the queue as an editable message. It must never vanish or start a turn the user can't see. Codex refuses with `activeTurnNotSteerable` during /review or /compact (Task 4 test `refused_steer_returns_to_the_queue`, Task 9 test `unsteerable_turn_refuses_the_steer`).
- Stop while a Pi editor dialog is open must close the dialog and leave the session alive (Task 16 test `cancel_closes_open_dialogs_first`).
- A recording must never carry the user's home path, email address or MCP server names into the repo (Task 2 test `scrub_replaces_home_cwd_and_emails`, Task 3 step "Scrub gate").

---

### Task 1: Ask and Answer reshape, functional cards

**Files:**
- Modify: `crates/core/src/request.rs` (`Ask`, `Answer`, new types), `crates/core/src/lib.rs` (re-exports)
- Create: `crates/core/tests/unit/request.rs`, and add the `#[cfg(test)] #[path = "../tests/unit/request.rs"] mod tests;` line to `request.rs`
- Modify, mechanically, every construction and match site:
  - `crates/driver/src/claude.rs` (`control_request`, `reply`)
  - `crates/driver/src/codex.rs` (`server_request`)
  - `crates/driver/src/opencode/events.rs` (`permission.asked`)
  - `crates/driver/src/pi.rs` (`extension_ui_request`)
  - `crates/driver/src/antigravity.rs` (`result`)
  - those Drivers' unit test files
  - `crates/agent/src/gate.rs`, `crates/agent/src/tools/propose_plan.rs`, and `crates/agent/src/agent.rs` (`answer_allows`)
  - `crates/app/src/app/mod.rs` (`status`), `crates/app/src/app/sidebar.rs`, `crates/app/src/app/chat/send.rs` (the notification text), `crates/app/src/project_page/model.rs`
  - `crates/ui/src/chat/message.rs` (the approval-to-row match)
- Modify: `crates/app/src/hitl.rs` (cards and answer builders), `crates/app/src/app/mod.rs` (`request_picks`)
- Test: `crates/app/tests/unit/hitl.rs`

**Interfaces:**
- Produces, in `aether-core` (all re-exported from `lib.rs`):

```rust
pub enum Ask {
    Approval(Approval),              // was a struct variant; same JSON
    Question { question: String, choices: Vec<String> }, // OpenRouter and old chats
    Questions(Vec<Question>),
    Form(Form),
    Plan(Plan),                      // was a struct variant; same JSON
    Unknown,                         // phase 2
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub tool: String,
    pub title: String,
    /// Full command, path or payload. Never clipped.
    pub input: String,
    #[serde(default)] pub reason: String,
    /// Id of the tool row this approval is for.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")] pub kind: ApprovalKind,
    /// Unified diff of a proposed edit.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub diff: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub cwd: Option<String>,
    /// The subagent or child session that asks. `None` for the main agent.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub source: Option<String>,
    /// Scoped choices beyond allow once and deny, such as "Always allow `pnpm add *`".
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub options: Vec<ApprovalOption>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalKind { Command, Edit, Fetch, Mcp, #[default] Other, #[serde(other)] Unknown }
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalOption { pub id: String, pub label: String }

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    #[serde(default)] pub header: String,
    pub question: String,
    #[serde(default)] pub options: Vec<Choice>,
    #[serde(default)] pub multi: bool,
    /// Offers a free-text "Something else" answer.
    #[serde(default)] pub other: bool,
    #[serde(default)] pub secret: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")] pub description: String,
    /// What the CLI receives. Empty means `label`.
    #[serde(default, skip_serializing_if = "String::is_empty")] pub value: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Form {
    #[serde(default)] pub title: String,
    #[serde(default)] pub message: String,
    #[serde(default)] pub fields: Vec<Field>,
    /// A link to open first (MCP URL elicitation, OpenCode `external` field, sign-in).
    #[serde(default, skip_serializing_if = "Option::is_none")] pub url: Option<String>,
    /// The CLI drops the dialog after this long.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub source: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub key: String,
    #[serde(default)] pub label: String,
    #[serde(default)] pub description: String,
    #[serde(default)] pub kind: FieldKind,
    #[serde(default)] pub options: Vec<Choice>,
    #[serde(default)] pub required: bool,
    #[serde(default)] pub default: Vec<String>,
    #[serde(default)] pub secret: bool,
    /// Also accepts typed text besides `options` (OpenCode `custom`).
    #[serde(default)] pub other: bool,
    /// Shown only while every rule holds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub when: Vec<When>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldKind { #[default] Text, Choice, Multi, Bool, Number, #[serde(other)] Unknown }
/// Holds when field `key`'s first value equals `value` (or differs, when `not`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct When { pub key: String, #[serde(default)] pub not: bool, pub value: String }

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    #[serde(default)] pub steps: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")] pub markdown: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub options: Vec<ApprovalOption>,
}

pub enum Answer {
    Approve { input: String, always: bool },
    Deny,
    Reply(String),
    Plan { approved: bool, text: String },
    /// One of the ask's `options`, by id.
    Pick(String),
    /// Deny, with a note for the agent.
    DenyWith(String),
    /// One entry per question: the picked labels plus any typed text. Empty means skipped.
    Answers(Vec<Vec<String>>),
    /// Field key and its values: `Choice::value` (or label), "true"/"false", typed text.
    Form(Vec<(String, Vec<String>)>),
}
```

  `fn is_default<T: Default + PartialEq>(v: &T) -> bool` is private to `request.rs`.
- Produces, in `crates/app/src/hitl.rs`, pure functions that the tests pin:
  - `fn questions_answer(questions: &[Question], picks: &[BTreeSet<usize>], typed: &[String]) -> Answer`. Picked option labels come first, in option order. Then the trimmed typed text, only when `other` is set and the text is non-empty.
  - `fn field_visible(field: &Field, fields: &[Field], values: &[Vec<String>]) -> bool`.
  - `fn form_answer(form: &Form, values: &[Vec<String>]) -> Result<Answer, String>`. `values[i]` belongs to `form.fields[i]`. Hidden fields are skipped. An empty visible required field gives `Err(format!("Fill in {}", label or key))`.
- Produces: `ChatApp.request_picks: HashMap<u64, Vec<BTreeSet<usize>>>`, the option indices picked per question or field.

- [ ] **Step 1: Write the failing core tests** in `crates/core/tests/unit/request.rs`

```rust
use super::*;

#[test]
fn old_ask_json_round_trips_unchanged() {
    for old in [
        r#"{"Approval":{"tool":"Bash","title":"Run command","input":"ls\nrm -rf dist","reason":""}}"#,
        r#"{"Plan":{"steps":["a","b"]}}"#,
        r#"{"Question":{"question":"q","choices":["a"]}}"#,
    ] {
        let ask: Ask = serde_json::from_str(old).unwrap();
        assert_eq!(serde_json::to_string(&ask).unwrap(), old);
    }
    let Ask::Approval(a) = serde_json::from_str(r#"{"Approval":{"tool":"Bash","title":"t","input":"ls"}}"#).unwrap() else { panic!() };
    assert_eq!((a.kind, a.options.len(), a.tool_id), (ApprovalKind::Other, 0, None));
}

#[test]
fn new_shapes_and_answers_round_trip() {
    let option = ApprovalOption { id: "rule-0".into(), label: "Always allow `pnpm add *`".into() };
    let asks = [
        Ask::Approval(Approval { tool: "Bash".into(), tool_id: Some("t1".into()), kind: ApprovalKind::Command,
            cwd: Some("/w".into()), source: Some("Explore".into()), options: vec![option.clone()], ..Approval::default() }),
        Ask::Questions(vec![Question { header: "DB".into(), question: "Which?".into(), multi: true, other: true,
            options: vec![Choice { label: "Postgres".into(), ..Choice::default() }], ..Question::default() }]),
        Ask::Form(Form { title: "Sign in".into(), url: Some("https://x.test".into()), timeout_ms: Some(30_000),
            fields: vec![Field { key: "k".into(), kind: FieldKind::Bool, required: true,
                when: vec![When { key: "a".into(), not: false, value: "yes".into() }], ..Field::default() }],
            ..Form::default() }),
        Ask::Plan(Plan { markdown: "# Plan\n- a".into(), options: vec![option], ..Plan::default() }),
    ];
    for ask in asks {
        assert_eq!(serde_json::from_str::<Ask>(&serde_json::to_string(&ask).unwrap()).unwrap(), ask);
    }
    for answer in [Answer::Pick("rule-0".into()), Answer::DenyWith("use yarn".into()),
        Answer::Answers(vec![vec!["Postgres".into()], vec![]]), Answer::Form(vec![("k".into(), vec!["true".into()])])] {
        assert_eq!(serde_json::from_str::<Answer>(&serde_json::to_string(&answer).unwrap()).unwrap(), answer);
    }
}

#[test]
fn unknown_kinds_load_as_unknown() {
    let a: Approval = serde_json::from_str(r#"{"tool":"x","title":"t","input":"i","kind":"Teleport"}"#).unwrap();
    assert_eq!(a.kind, ApprovalKind::Unknown);
    let f: Field = serde_json::from_str(r#"{"key":"k","kind":"Slider"}"#).unwrap();
    assert_eq!(f.kind, FieldKind::Unknown);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-core --lib request::tests`
Expected: compile errors (`Approval`, `Question`, `Form` and `Plan` are missing).

- [ ] **Step 3: Implement the core types** exactly as in Interfaces. `Ask::Approval` and `Ask::Plan` are newtype variants. Serde's external tagging makes `{"Approval":{…}}` identical for a struct variant and a newtype around a struct, and `skip_serializing_if` keeps old JSON byte-identical.

- [ ] **Step 4: Convert every site mechanically**
  - Construction becomes `Ask::Approval(Approval { tool, title, input, reason, ..Approval::default() })` and `Ask::Plan(Plan { steps, ..Plan::default() })`. Only the four old fields are filled here. The Driver tracks fill the rest.
  - Patterns become `Ask::Approval(Approval { input, .. })`.
  - Matches on `Answer` treat `Pick(_)` like `Approve { always: false }`, and `DenyWith(_)` like `Deny`. `Answers` and `Form` are treated like `Reply` of their first value joined with ", ". The Driver tracks replace each of these with the real mapping.
  - `answer_allows` returns true for `Pick`, `Answers` and `Form`.
  - In the app and sidebar `status`, `Questions` gives "Answer"/"Needs answer", and `Form` gives "Answer"/"Needs input".
  - `crates/ui/src/chat/message.rs`: an approval marks the row whose id equals `tool_id`. Only when `tool_id` is `None` does it fall back to today's input-summary match.

- [ ] **Step 5: Write the failing card tests** in `crates/app/tests/unit/hitl.rs`

```rust
fn choice(label: &str, value: &str) -> Choice {
    Choice { label: label.into(), value: value.into(), ..Choice::default() }
}

#[test]
fn questions_answer_keeps_picks_and_other_text_per_question() {
    let q = |multi, other| Question { question: "q".into(), multi, other,
        options: vec![choice("A", ""), choice("B", ""), choice("C", "")], ..Question::default() };
    let picks = [BTreeSet::from([2, 0]), BTreeSet::new(), BTreeSet::from([1])];
    let typed = [String::new(), " mine ".into(), "ignored".into()];
    assert_eq!(
        questions_answer(&[q(true, false), q(false, true), q(false, false)], &picks, &typed),
        Answer::Answers(vec![vec!["A".into(), "C".into()], vec!["mine".into()], vec!["B".into()]])
    );
}

#[test]
fn form_answer_requires_visible_required_fields() {
    let form = Form { fields: vec![
        Field { key: "mode".into(), kind: FieldKind::Choice, required: true,
            options: vec![choice("Fast", "fast"), choice("Safe", "safe")], ..Field::default() },
        Field { key: "why".into(), label: "Reason".into(), required: true,
            when: vec![When { key: "mode".into(), not: false, value: "safe".into() }], ..Field::default() },
    ], ..Form::default() };
    assert_eq!(form_answer(&form, &[vec!["fast".into()], vec![]]),
        Ok(Answer::Form(vec![("mode".into(), vec!["fast".into()])])));
    assert_eq!(form_answer(&form, &[vec!["safe".into()], vec![]]), Err("Fill in Reason".into()));
    assert_eq!(form_answer(&form, &[vec![], vec![]]), Err("Fill in mode".into()));
    assert!(!field_visible(&form.fields[1], &form.fields, &[vec!["fast".into()], vec![]]));
}
```

- [ ] **Step 6: Run them to verify they fail**

Run: `cargo test -p aether --lib hitl::tests`
Expected: compile errors (the functions are missing).

- [ ] **Step 7: Implement the pure functions, then the cards in `render_request`.** The cards are functional only. Phase 4 owns the visuals, keyboard and stepper.
  - **Approval.**
    - The heading and subject use the existing helpers.
    - The description appends `reason`, `cwd` (as "in {cwd}") and `source` (as "asked by {source}").
    - The code block shows `diff` when present, otherwise `input`.
    - Each option becomes a leading ghost button that sends `Answer::Pick(id)`.
    - For agent chats with no options, keep today's "Allow for session" fallback (`remembers_approvals`). Task 21 deletes it.
    - Agent chats get a one-line field "Tell the agent why (optional)". Deny sends `DenyWith(note)` when the note is non-empty, otherwise `Deny`.
    - Non-agent chats keep the editable input.
  - **Questions.**
    - The heading is `"{who} has {n} question(s)"`.
    - Each question shows its header, then the question as markdown, then a `RequestChoice` per option. A click toggles the index in `request_picks[id][i]` (multi) or replaces it (single).
    - Each `other` question gets a line field; a `secret` question gets a masked field.
    - "Send" calls `questions_answer`, and "Skip" sends `Deny`.
    - The per-question line fields live in a new `RequestInput::Fields(Vec<Entity<InputState>>)`.
  - **Form.**
    - The title and message come first. A `url` adds an "Open link" button (`cx.open_url`).
    - Each visible field renders by kind:
      - Text and Number: a line field prefilled with `default`.
      - Bool: two choices, "Yes" (`true`) and "No" (`false`).
      - Choice and Multi: choice rows, plus a line field when `other` is set. Typed text joins the picked values.
    - "Submit" calls `form_answer`. An `Err` is shown as one line of error text above the actions.
    - "Cancel" sends `Deny`.
    - When `timeout_ms` is set, the description ends with "Closes after N s".
  - **Plan.** A non-empty `markdown` renders through `TextView::markdown` in place of `PlanSteps`. Options become leading buttons that send `Pick`.
  - **Resolved lines:**
    - `Pick(id)` gives "Allowed · {option label}".
    - `DenyWith(n)` gives "Declined · {subject} · {n}".
    - `Answers` gives "{first question} → {answers joined}".
    - `Form` gives "{title} → sent". A form cancelled with `Deny` gives "{title} → cancelled".

- [ ] **Step 8: Run the tests and build**

Run: `cargo test -p aether-core --lib && cargo test -p aether-driver --lib && cargo test -p aether-agent --lib && cargo test -p aether --lib && cargo build -p aether`
Expected: all PASS. Existing Driver tests pass after their patterns are converted.

- [ ] **Step 9: Commit (after user approval)**

```bash
git add crates/core crates/driver crates/agent crates/app crates/ui
git commit -m "feat(core): approval options, questions, forms, plan markdown"
```

---

### Task 2: Record mode, headless recorder, replay helpers

**Files:**
- Create: `crates/driver/src/record.rs`, `crates/driver/tests/unit/record.rs`, `crates/driver/examples/record.rs`
- Modify: `crates/driver/src/lib.rs` (`pub mod record;`)
- Modify (record hooks and state extraction): `crates/driver/src/claude.rs`, `codex.rs`, `pi.rs`, `antigravity.rs`, `opencode/session.rs` (`listen`), `opencode/mod.rs`
- Modify (add a `replay` helper per Driver): `crates/driver/tests/unit/{claude,codex,pi,antigravity}.rs`, `crates/driver/tests/unit/opencode/events.rs`

**Interfaces:**
- Produces, in `crates/driver/src/record.rs`:
  - `pub struct Recorder` (a `Mutex<File>` plus the scrub inputs).
  - `pub fn from_env(provider: &str, cwd: &Path) -> Option<Arc<Recorder>>`. It returns `None` unless `AETHER_RECORD` names a directory, which it creates. The file is `<dir>/<provider>-<unix ms>.jsonl`.
  - `pub fn create(dir: &Path, provider: &str, cwd: &Path) -> std::io::Result<Recorder>`.
  - `pub fn line(&self, raw: &str)`, which appends `scrub(raw)` plus `\n`. Write errors are ignored.
  - `pub fn scrub(raw: &str, home: &str, cwds: &[&str]) -> String`. The scrub runs in this order:
    1. Each cwd, longest first, becomes `/work`. The cwds are the given cwd and its `canonicalize` form, so `/tmp` and `/private/tmp` both match.
    2. The home dir becomes `~`.
    3. Every `local@domain.tld` becomes `user@example.com`. A token is an email when it has `[A-Za-z0-9._%+-]+` before the `@` and at least one dot in `[A-Za-z0-9.-]+` after it. The scanner is hand-written, with no regex dependency.
  - Test-only helpers:
    - `#[cfg(test)] pub(crate) fn fixture(provider: &str, scenario: &str) -> Vec<String>`: the non-empty lines of `tests/fixtures/<provider>/<scenario>.jsonl`.
    - `#[cfg(test)] pub(crate) fn fixtures(provider: &str) -> Vec<(String, Vec<String>)>`: every fixture of that Provider, by scenario name.
    - `#[cfg(test)] pub(crate) fn last_calls(updates: &[Update]) -> Vec<ToolCall>`: the last snapshot per tool id, in first-seen order.
- **Marks.** Driver actions are recorded as lines with an `"aether"` key:
  - `{"aether":"prompt","text":…}`
  - `{"aether":"steer","text":…}`
  - `{"aether":"cancel"}`
  - `{"aether":"answer","id":<local id>,"answer":<Answer JSON>}`

  Replay applies a mark through the same state change the Driver method makes. Extract that state part, where it is still inline, into `fn on_prompt(s: &mut State, text: &str)`, `fn on_steer(s: &mut State, text: &str) -> bool` and `fn on_cancel(s: &mut State) -> bool` in each Driver. I/O stays in the method. Codex's `start_turn` and `steer_turn` already have this shape.
- **What each Driver records**, all through `Option<Arc<Recorder>>` held on the Driver:

  | Driver | Records | Seed for `replay(scenario)` | Mark handling in replay |
  |---|---|---|---|
  | Claude | every stdout line, before `map_line` | `State { active: true, ..default }` | prompt `on_prompt`; steer `on_steer`; cancel `on_cancel`; answer `reply` |
  | Codex | every stdout line, before parse (handshake replies included) | `State { active: true, thread: <first result.thread.id in the file> }` | prompt `start_turn`; steer `steer_turn`; cancel `on_cancel`; answer `reply` |
  | OpenCode | every frame passed to `handle` in `listen` (after the session filter, resync frames included), as `{"type":…,"data":…}` | `State { active: true, root: <first data.sessionID> }` | prompt `on_prompt`; steer `on_steer`; cancel `on_cancel`; answer: no-op (replies go over HTTP) |
  | Pi | every stdout line | `State::default()` | prompt `on_prompt` (sets `s.prompt` from the next `response` id); steer `on_steer`; cancel `on_cancel`; answer `reply` |
  | agy | every stdout line of every process epoch (one shared recorder per Driver) | lines before `init` go through `startup`, then `State { active: true, conversation }` | prompt `on_prompt`; steer `on_steer`; cancel `on_cancel`; answer: no-op |

  Each Driver test file gains `fn replay(scenario: &str) -> Vec<Update>`, which seeds and dispatches as in the table, and `use crate::record::{fixtures, last_calls};`. Both are used from Task 3 on.
- **The `record` example (CLI).** It needs no new dependencies, and parses arguments by hand:

  | Flag | Meaning |
  |---|---|
  | `--provider claude\|codex\|opencode\|pi\|agy` | which Driver to spawn |
  | `--cwd DIR` | the project folder |
  | `--prompt TEXT` | the first prompt |
  | `--then TEXT` | a second prompt, sent after the first `Final` (repeatable) |
  | `--access ask\|read-only\|auto-edit\|auto\|full` | access mode, default `ask` |
  | `--model ID`, `--effort E`, `--resume ID` | `DriverStartOptions` fields |
  | `--answer allow\|deny\|always\|note:TEXT\|first` | the ask policy, default `allow`. `always` picks the first option. `first` picks the first choice of every question or field. `note:` sends `DenyWith`. |
  | `--steer-after MS --steer TEXT` | steer once, MS ms after the first prompt |
  | `--cancel-after MS` | cancel once, MS ms after the first prompt |
  | `--linger SECS` | keep printing this long after the last `Final` (wakes, background completions) |
  | `--timeout SECS` | give up, default 240 |

  It prints each `Update` as `format!("{:>6}ms {update:?}", ms)`. It exits 0 after the last `Final` plus the linger, 2 on timeout and 1 on bad arguments (after printing usage).

- [ ] **Step 1: Write the failing tests** in `crates/driver/tests/unit/record.rs`

```rust
use super::*;

#[test]
fn scrub_replaces_home_cwd_and_emails() {
    let raw = r#"{"cwd":"/private/tmp/aether-fx/src","file":"/Users/iris/.claude/x.json","who":"iris.h+dev@gmail.com","at":"a @ b","pkg":"@scope/x"}"#;
    assert_eq!(
        scrub(raw, "/Users/iris", &["/tmp/aether-fx", "/private/tmp/aether-fx"]),
        r#"{"cwd":"/work/src","file":"~/.claude/x.json","who":"user@example.com","at":"a @ b","pkg":"@scope/x"}"#
    );
}

#[test]
fn recorder_appends_scrubbed_lines_and_marks() {
    let dir = std::env::temp_dir().join(format!("aether-rec-test-{}", std::process::id()));
    let rec = Recorder::create(&dir, "pi", Path::new("/tmp/fx")).unwrap();
    rec.line(r#"{"type":"agent_start","cwd":"/tmp/fx"}"#);
    rec.line(&serde_json::json!({"aether": "steer", "text": "also"}).to_string());
    let file = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
    let name = file.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("pi-") && name.ends_with(".jsonl"), "{name}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(),
        "{\"type\":\"agent_start\",\"cwd\":\"/work\"}\n{\"aether\":\"steer\",\"text\":\"also\"}\n");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn last_calls_keeps_the_latest_snapshot_per_id() {
    let call = |id: &str, status| Update::Tool(ToolCall { status, ..ToolCall::new(id, "Run", ToolDetail::default()) });
    let out = [call("a", ToolStatus::Running), call("b", ToolStatus::Running), call("a", ToolStatus::Done)];
    let last: Vec<(String, ToolStatus)> = last_calls(&out).into_iter().map(|c| (c.id, c.status)).collect();
    assert_eq!(last, [("a".into(), ToolStatus::Done), ("b".into(), ToolStatus::Running)]);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib record::tests`
Expected: compile errors (`record` module missing).

- [ ] **Step 3: Implement `record.rs`, the Driver hooks and state extraction, and the example**

- [ ] **Step 4: Run the tests and check the example**

Run: `cargo test -p aether-driver --lib && cargo run -q -p aether-driver --example record -- --help; echo "exit $?"`
Expected: all tests PASS. The example prints the usage table and `exit 1`.

- [ ] **Step 5: One live sanity run (cheap)**

Run: `mkdir -p /tmp/aether-fx && AETHER_RECORD=/tmp/aether-rec cargo run -q -p aether-driver --example record -- --provider claude --cwd /tmp/aether-fx --access read-only --prompt "Reply with exactly: ok"`
Expected:
- The output ends in a line containing `Final(`.
- `/tmp/aether-rec/claude-*.jsonl` exists and contains a `{"aether":"prompt"` line.
- `grep -c "$HOME" /tmp/aether-rec/claude-*.jsonl` prints `0`.

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/driver
git commit -m "feat(driver): record mode, headless recorder, replay helpers"
```

---

### Task 3: Capture fixtures (operational)

A subagent with Bash runs this task. The user approved about 10 to 12 short real turns per Provider on their accounts, run in a throwaway project and scrubbed before commit. It needs Task 2's example, and can run in parallel with Task 4.

**Files:**
- Create: `crates/driver/tests/fixtures/{claude,codex,opencode,pi,agy}/<scenario>.jsonl`, plus the hand-written `<scenario>.hand.jsonl`
- Modify: each Driver test file (`crates/driver/tests/unit/{claude,codex,pi,antigravity}.rs`, `opencode/events.rs`) gains one smoke test, `every_fixture_replays_without_panicking`

**Interfaces:**
- Consumes: the `record` example, `Recorder`, `fixtures(provider)` and each Driver's `replay` (Task 2).
- Produces: the fixture files that Tasks 5-19 name. A scenario name is the file stem, so `retry.hand` is a valid name.

**Scenarios.** "full" means `--access full`. Agy has no Ask mode, so its `approval` runs in `auto-edit`, where a command is blocked and the rerun card appears.

| Scenario | Flags | Prompt | Claude | Codex | OpenCode | Pi | agy |
|---|---|---|---|---|---|---|---|
| `text` | `--access read-only` (agy: `auto-edit`) | "Think it through briefly, then answer in one sentence: what does README.md say?" | ✓ | ✓ | ✓ | ✓ | ✓ |
| `tools` | full | "Read README.md, append a line `checked` to it, then run `ls`." | ✓ | ✓ | ✓ | ✓ | ✓ |
| `tool-error` | full | "Run `cat missing.txt` once and tell me the error. Don't retry." | ✓ | ✓ | ✓ | ✓ | ✓ |
| `approval` | `--access ask --answer always` (agy: `auto-edit --answer allow`) | "Create notes.txt containing hi, then run `echo done`." | ✓ | ✓ | ✓ | – | ✓ |
| `subagent` | full | "Use a subagent to read src/app.ts and report its first line." | ✓ | ✓ | ✓ | – | ✓ |
| `subagent-approval` | `--access ask --answer allow` | "Use a subagent to run `touch sub.txt`." | ✓ | ✓ | ✓ | – | – |
| `background` | full `--linger 30` | "Start `sleep 8 && echo bg-done` as a background command (or a background subagent), don't wait for it, and reply 'started'." | ✓ | ✓ | ✓ | – | ✓ |
| `question` | `--answer first` | "Before doing anything, use your question tool to ask me two questions: which colors (red, blue, green; I may pick several) and which size (S or L). Then repeat my answers." | ✓ | Task 12 | ✓ | – | – |
| `compaction` | full `--then "/compact"` | "Summarize README.md in one line." | ✓ | ✓ | ✓ | ✓ | – |
| `cancel` | full `--cancel-after 4000` | "Count from 1 to 400, one number per line." | ✓ | ✓ | ✓ | ✓ | ✓ |
| `steer` | full `--steer-after 3000 --steer "Also end with the word done."` | "Count from 1 to 60, one number per line." | ✓ | ✓ | ✓ | ✓ | ✓ |
| `resume` | `--resume <Session id from text>` | "What did I ask you in my last message? One line." | ✓ | ✓ | ✓ | ✓ | ✓ |
| `fatal` | `--model does-not-exist-9` | "hi" | ✓ | ✓ | ✓ | ✓ | ✓ |
| `auto-mode` | `--access auto` | "Reply with exactly: ok" | – | – | – | – | ✓ |

Codex's `question` needs plan mode (`/plan`, added in Task 11). Task 12 records it as its first step with `--prompt "/plan Before planning, ask me …"`.

Hand-written fixtures follow the shapes in the audits and in the research notes of this plan's mapping tables:

| File | Lines |
|---|---|
| `claude/retry.hand.jsonl` | `system/api_retry {attempt:1,max_retries:10,retry_delay_ms:2000,error_status:529,error:"overloaded"}`, a `rate_limit_event {status:"allowed_warning",resetsAt,utilization:0.9,rateLimitType:"five_hour"}`, a text delta, `result` success |
| `codex/retry.hand.jsonl` | `error {willRetry:true}`, `account/rateLimits/updated` at 91 %, an agentMessage delta, `turn/completed` |
| `opencode/retry.hand.jsonl` | `session.retry.scheduled`, `session.step.started`, `session.text.delta`, `session.execution.succeeded` |
| `pi/retry.hand.jsonl` | `auto_retry_start`, `auto_retry_end {success:true}`, `message_end`, `agent_settled` |

Every hand-written line keeps the ids of the recorded file it copies from. If a ✓ scenario can't be produced on the user's setup in two tries (for example Codex multi-agent tools are off), write `<scenario>.hand.jsonl` from the audit shapes and the existing unit-test constants instead, and list it in the step 6 report.

- [ ] **Step 1: Make the throwaway project**

```bash
rm -rf /tmp/aether-fx && mkdir -p /tmp/aether-fx/src && cd /tmp/aether-fx && git init -q
printf '# Fixture app\nA tiny project for recording CLI streams.\n' > README.md
printf 'export const greet = (name: string) => `hi ${name}`;\n' > src/app.ts
printf '{"name":"fx","private":true}\n' > package.json
git add -A && git -c user.email=fx@example.com -c user.name=fx commit -qm init
```

- [ ] **Step 2: Record each ✓ cell, in table order per Provider**

```bash
rec() {  # rec <provider> <scenario> <example flags…>
  p=$1; sc=$2; shift 2
  git -C /tmp/aether-fx checkout -q . && git -C /tmp/aether-fx clean -fdq
  rm -rf /tmp/aether-rec/$p && mkdir -p /tmp/aether-rec/$p crates/driver/tests/fixtures/$p
  AETHER_RECORD=/tmp/aether-rec/$p cargo run -q -p aether-driver --example record -- \
    --provider $p --cwd /tmp/aether-fx "$@" | tee /tmp/aether-rec/$p.$sc.out
  mv /tmp/aether-rec/$p/$p-*.jsonl crates/driver/tests/fixtures/$p/$sc.jsonl
}
# example: rec claude subagent --access full --prompt "Use a subagent to read src/app.ts and report its first line."
```

Expected per run: the `.out` file ends with a `Final(` line (or `Error(` for `fatal`), and exactly one fixture file is moved. For `resume`, take the id from the `Session("…")` line of `/tmp/aether-rec/<p>.text.out`.

- [ ] **Step 3: Write the hand-written fixtures** from the table above.

- [ ] **Step 4: Scrub gate**
  - Run: `rg -n -e "$HOME" -e '/Users/' -e '/private/var/' -e '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}' -e 'sk-[A-Za-z0-9]' crates/driver/tests/fixtures | grep -v 'user@example.com'`. Expected: no output.
  - List the user's MCP server names from `~/.claude.json` (`mcpServers` keys), `~/.codex/config.toml` (`[mcp_servers.*]`) and the OpenCode config. Then `rg -n -e '<name>' …` over the fixtures. Replace every hit with `mcp-1`, `mcp-2` and so on, using one `sed -i ''` per name, and re-run until the search finds nothing.
  - Replace account-identifying values (org or account uuids, plan names next to an email) with `"redacted"` by hand.
  - Expected: both searches come back empty, and `du -sh crates/driver/tests/fixtures` is under 3 MB.

- [ ] **Step 5: Add the smoke tests** (one per Driver test file)

```rust
#[test]
fn every_fixture_replays_without_panicking() {
    let all = crate::record::fixtures("claude"); // "codex", "opencode", "pi", "agy" in their files
    assert!(!all.is_empty());
    for (name, _) in all {
        let out = replay(&name);
        assert!(!out.is_empty(), "{name} produced no updates");
    }
}
```

Run: `cargo test -p aether-driver --lib every_fixture`
Expected: 5 PASS.

- [ ] **Step 6: Report.** Give a table of recorded, hand-written and skipped cells per Provider, with each Provider's turn count, and send it to the user.

- [ ] **Step 7: Commit (after user approval)**

```bash
git add crates/driver/tests/fixtures crates/driver/tests/unit
git commit -m "test(driver): recorded and hand-written replay fixtures per provider"
```

---

### Task 4: Shared Driver toolkit

**Files:**
- Modify: `crates/driver/src/lib.rs` (trait hooks, `DriverStartOptions.attachments`)
- Modify: `crates/driver/src/spawn.rs` (`StderrTail`, `log_unknown`)
- Modify: `crates/driver/src/tool.rs` (`classify` for subagent tools, `save_images`, `child_activity`, `now_ms`)
- Modify: `crates/core/src/lib.rs` (`Notice::new`, `Update::SteerRefused`)
- Modify: `crates/agent/src/agent.rs` (steer emission, `Command::StopTool`, `Command::NoticeAction`, the `SteerRefused` route, `session_options`)
- Modify: `crates/app/src/app/chat/send.rs` (`apply`: `SteerRefused`)
- Test: `crates/driver/tests/unit/spawn.rs`, `crates/driver/tests/unit/tool.rs`, `crates/agent/tests/unit/agent.rs`, `crates/app/tests/unit/app/chat/send.rs`

**Interfaces:**
- Produces, in `tool.rs`, shared by every track so that no two tracks edit it:
  - `classify` treats `task`, `agent` and `subagent` as `ToolDetail::Task`:
    - `agent` from `subagent_type`, `agent` or `agentType`
    - `prompt` from `prompt`, clipped to `DESCRIPTION_CHARS`
    - `description` from `description`, else `prompt`
    - the title stays `titled("Run agent", &["description"])`
  - `pub(crate) fn save_images(content: &Value, dir: Option<&Path>) -> Vec<Attachment>`. It decodes the image blocks in a tool result, in either shape:
    - `{type:"image", source:{type:"base64", media_type, data}}` (Claude)
    - `{type:"image", data, mimeType}` (Pi, Codex MCP)

    Each image is written to `dir/tool-<unix ms>-<i>.<ext>`, with the extension from `mime_for`'s inverse (png, jpg, gif, webp). The function returns an `Attachment` per file written, and nothing when `dir` is `None`.
  - `pub(crate) fn child_activity(task: &mut ToolCall, child: &ToolCall, new: bool)`. It sets the Task's `activity` to `format!("{} {}", child.title, child.detail.summary())`. When `new`, it also adds 1 to `tools`. It is a no-op on non-Task details.
  - `pub(crate) fn now_ms() -> i64` (Unix ms), for `Notice.until_ms`.
- Produces, on `trait Driver`, all with defaults:
  - `fn reports_steered(&self) -> bool { false }`: true when the Driver itself emits `Update::Steered` once the CLI consumes the steer.
  - `fn stop_tool(&mut self, _id: &str) -> bool { false }`: stops one background task or terminal, by tool row id. It returns false when the CLI offers no way to stop it.
  - `fn notice_action(&mut self, _action: &str) {}`: runs a notice action. A notice offers one by setting `url` to `"aether:<action>"`.
- Produces: `DriverStartOptions.attachments: Option<PathBuf>`, this chat's attachment folder. `session_options` sets it to `attachments_dir().map(|d| d.join(&chat.key))`, and `for_provider` sets `None`.
- Produces, in `spawn.rs`:
  - `pub(crate) struct StderrTail`, a ring of the last `STDERR_TAIL = 40` non-blank lines, with ANSI removed through `tool::clean`. Its methods:
    - `fn push(&mut self, line: &str)`
    - `fn last(&self) -> Option<&str>`
    - `fn fatal(&self, headline: &str) -> String`, which returns `headline`, plus `"\n"` and the tail joined by `"\n"` when the tail is non-empty.
  - `pub(crate) fn log_unknown(provider: &str, what: &str) -> bool`. It writes `eprintln!("{provider}: unhandled {what}")` once per `(provider, what)` per process, keeping the seen pairs in a `static Mutex<HashSet<(String, String)>>`. It returns true the first time.
- Produces, in `aether-core`:
  - `impl Notice { pub fn new(id: impl Into<String>, kind: NoticeKind, text: impl Into<String>) -> Self }`. The state is `Done`, and `until_ms`, `url` and `detail` are `None`.
  - `Update::SteerRefused(String)`: the CLI refused a steer, and its text goes back to the queue.
- Produces, on `Agent`:
  - `pub fn stop_tool(&self, chat_key: &str, tool_id: &str)`
  - `pub fn notice_action(&self, chat_key: &str, action: &str)`

  Each sends its `Command` to the live session, and does nothing when no session is live.
- Produces, in `send.rs`: `pub(crate) fn refuse_delivered(queue: &mut [Queued], text: &str) -> bool`. It sets `delivering = false` on the delivering item with that text, or else on the first delivering item.
- **Notice ids**, used by every track. `<n>` is a per-Driver counter in `State`.

  | Kind | id |
  |---|---|
  | Startup | `startup-<step>`, where step is `version`, `spawn`, `session` or `ready` |
  | Retry | `retry-<turn>`, one line per turn, upserted with the attempt count |
  | RateLimit | `ratelimit-<window>` (the CLI's limit type, or `default`) |
  | Compaction | `compact-<n>` |
  | Hook | `hook-<hook id>` |
  | ModelChange | `model-<n>` |
  | ModeChange | `mode-<n>` |
  | Warning | assigned by the core reducer (`Update::Warning`) |
  | McpAuth | `mcp-<server>` |
  | Wake | `wake-<n>` |
  | Info | `info-<key>` |

- [ ] **Step 1: Write the failing tests**

`crates/driver/tests/unit/spawn.rs`:

```rust
#[test]
fn stderr_tail_keeps_the_last_lines_after_the_headline() {
    let mut tail = StderrTail::default();
    for i in 0..45 { tail.push(&format!("line {i}")); }
    tail.push("\x1b[31mthread 'main' panicked at src/x.rs:1:1\x1b[0m");
    tail.push("   ");
    tail.push("note: run with `RUST_BACKTRACE=1`");
    let fatal = tail.fatal("codex exited (exit status: 101)");
    let lines: Vec<&str> = fatal.lines().collect();
    assert_eq!(lines[0], "codex exited (exit status: 101)");
    assert_eq!(lines.len(), 41);
    assert!(lines.contains(&"thread 'main' panicked at src/x.rs:1:1"));
    assert_eq!(StderrTail::default().fatal("pi exited"), "pi exited");
}

#[test]
fn unknown_events_are_logged_once() {
    assert!(log_unknown("test", "system/teleport"));
    assert!(!log_unknown("test", "system/teleport"));
    assert!(log_unknown("test", "system/other"));
}
```

`crates/driver/tests/unit/tool.rs`:

```rust
#[test]
fn subagent_tools_are_tasks_with_type_and_prompt() {
    for (name, input) in [
        ("Agent", json!({"description": "Find styles", "prompt": "Look in src", "subagent_type": "Explore"})),
        ("subagent", json!({"description": "Find styles", "prompt": "Look in src", "agent": "Explore"})),
    ] {
        let (_, detail) = classify(name, &input);
        assert!(matches!(detail, ToolDetail::Task { ref description, ref agent, ref prompt, .. }
            if description == "Find styles" && agent == "Explore" && prompt == "Look in src"), "{name}");
    }
}

#[test]
fn tool_result_images_are_saved_as_attachments() {
    let dir = std::env::temp_dir().join(format!("aether-img-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let png = "iVBORw0KGgo="; // base64 of the PNG signature, enough for a file
    let content = json!([
        {"type": "text", "text": "shot"},
        {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": png}},
        {"type": "image", "data": png, "mimeType": "image/png"},
    ]);
    let saved = save_images(&content, Some(&dir));
    assert_eq!(saved.len(), 2);
    assert!(saved.iter().all(|a| a.mime == "image/png" && a.path.exists() && a.path.starts_with(&dir)));
    assert!(save_images(&content, None).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn child_activity_counts_new_children_only() {
    let mut task = start("t", "Agent", &json!({"description": "d"}));
    let child = start("c", "Read", &json!({"file_path": "src/a.css"}));
    child_activity(&mut task, &child, true);
    child_activity(&mut task, &child, false);
    assert!(matches!(task.detail, ToolDetail::Task { tools: 1, ref activity, .. } if activity == "Read file src/a.css"));
}
```

`crates/agent/tests/unit/agent.rs`. Extend `Fake`:
- `prompt("hold-reporter")` behaves like `"hold"` and sets `reports = true`. `reports_steered()` returns `reports`.
- When `reports` is set, `steer` sends `Update::Steered(format!("cli:{text}"))` before its `Final(text)`.
- `stop_tool(id)` logs `stop:{id}` and returns true. `notice_action(a)` logs `action:{a}`.

```rust
#[test]
fn drivers_that_report_steers_are_not_marked_twice() {
    let (agent, _, _) = fake_agent(true, Duration::from_secs(600));
    let s = cli_settings();
    let old = agent.send("x", "hold-reporter".into(), vec![], &s, "", vec![], &[], &chat(""));
    agent.steer("more".into(), vec![], &s, &[], &chat(""));
    assert_eq!(drain(&agent, old), [Update::Delta("hold-reporter".into()), Update::Steered("cli:more".into()), Update::Final("more".into())]);
}

#[test]
fn stop_tool_and_notice_actions_reach_the_driver_in_order() {
    let (agent, log, _) = fake_agent(true, Duration::from_secs(600));
    drain(&agent, agent.send("x", "a".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")));
    agent.stop_tool("1", "bg1");
    agent.notice_action("1", "trust-project");
    drain(&agent, agent.send("x", "b".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")));
    let log = log.lock().unwrap();
    let at = |l: &str| log.iter().position(|x| x == l).unwrap();
    assert!(at("stop:bg1") < at("action:trust-project") && at("action:trust-project") < at("prompt:b"));
}
```

`crates/app/tests/unit/app/chat/send.rs`:

```rust
#[test]
fn refused_steer_returns_to_the_queue() {
    let q = |t: &str, d: bool| Queued { text: t.into(), attachments: vec![], delivering: d };
    let mut queue = vec![q("a", false), q("b", true)];
    assert!(refuse_delivered(&mut queue, "b"));
    assert_eq!(queue, [q("a", false), q("b", false)]);
    assert!(!refuse_delivered(&mut queue, "b"), "nothing left delivering");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib -- spawn::tests tool::tests && cargo test -p aether-agent --lib agent::tests && cargo test -p aether --lib app::chat::send::tests`
Expected: compile errors.

- [ ] **Step 3: Implement**
  - **Agent driver thread, `Command::Steer`.** Phase 2 sends `Steered(text)` when `driver.steer()` returns true. It now does so only when `!driver.reports_steered()`.
  - **Agent driver thread, new commands.**
    - `Command::StopTool(id)` calls `driver.stop_tool(&id)`. When that returns false, it sends `Update::Warning("This agent can't stop background work from Aether")` on `driver_tx`.
    - `Command::NoticeAction(a)` calls `driver.notice_action(&a)`.
  - **Pump.** It routes `SteerRefused` and `Warning` to the turn when one runs, otherwise as `SideUpdate::Meta`. Startup and config warnings often arrive between turns. `apply_cli_meta` applies a `Warning` to the last assistant message, like a `Notice`.
  - **App, `apply` and `apply_cli_meta`.** `SteerRefused(t)` calls `refuse_delivered(&mut chat.queue, &t)`, then returns. The item then runs as a normal prompt once the turn ends, through today's queue.
  - **Reducer.** `Message::apply` ignores `SteerRefused`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib && cargo test -p aether-agent --lib && cargo test -p aether --lib && cargo build -p aether`
Expected: all PASS.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/driver crates/core crates/agent crates/app
git commit -m "feat(driver): shared toolkit for steer reporting, stderr tails and notices"
```

---

## Claude track (Tasks 5-8, parallelizable)

Shapes come from Claude Code 2.1.304 and the SDK 0.3.220 types (`sdk.d.ts`). `map_line`, `asks`, `calls`, `BASH_USE`, `WRITE_REQUEST` and `ASK_REQUEST` are the existing test helpers in `crates/driver/tests/unit/claude.rs`.

### Task 5: Claude handshake, launch flags, session state, robustness

**Files:**
- Modify: `crates/driver/src/claude.rs`: `launch_args`, `spawn` (initialize, resume fallback, `StderrTail`), `cancel` (watchdog), `map_value` (`system/init`, `system/status`), `exit_reason`
- Test: `crates/driver/tests/unit/claude.rs`

**Interfaces:**
- Consumes: `StderrTail`, `log_unknown`, the notice ids (Task 4).
- Produces:
  - `fn initialize_request() -> Value`. It returns `{"type":"control_request","request_id":"aether-init","request":{"subtype":"initialize","sdkForwardNestedSubagentToolCalls":true,"agentProgressSummaries":true,"forwardSubagentText":true,"supportedDialogKinds":DIALOG_KINDS}}`, with `const DIALOG_KINDS: &[&str] = &[]`. Task 8 owns that list.
  - `spawn` writes `initialize_request()` first and waits up to 30 s for its `control_response`, like the Codex and Pi handshakes. An `error` response gives `Warning(format!("Claude Code didn't accept Aether's handshake ({e}). Subagent activity and progress are off."))`.
  - `enum ResumeFix { Fresh(String), Fork }` and `fn resume_fix(stderr: &StderrTail) -> Option<ResumeFix>`:
    - a tail line containing `No conversation found` gives `Fresh("it no longer exists")`
    - a line containing `--fork-session` or `running in the background` gives `Fork`

    When stdout closes before the initialize reply and `resume_fix` matches, `spawn` respawns once:
    - `Fresh`: `--session-id <new uuid>`, plus `Notice::new("info-resume", Info, "Couldn't resume the Claude session because {reason}. Started a new one.")`, with `resumed = false`
    - `Fork`: `--resume <id> --fork-session`, plus `Notice::new("info-resume", Info, "This Claude session was open elsewhere, so the chat continues in a fork.")`
  - `fn claude_mode(mode: &str) -> Option<AccessMode>`:
    - `default` and `manual` give Ask
    - `acceptEdits` gives AutoEdit
    - `auto` gives Auto
    - `bypassPermissions` gives Full
    - `plan` and `dontAsk` give ReadOnly
    - anything else gives `None`, through `log_unknown`
  - `launch_args` changes:
    - `--permission-prompt-tool stdio` becomes `--permission-prompts host`
    - the `default` mode becomes `manual`
    - `--include-hook-events` is added
    - `--thinking-display summarized` stays, since it has no public replacement
  - `State.stderr: StderrTail` replaces the `VecDeque`. `fn exit_error(status: Option<ExitStatus>, tail: &StderrTail) -> String` returns `tail.fatal("claude exited ({status})")` and replaces `exit_reason`.

**Mapping:**

| Event | Maps to |
|---|---|
| `system/init {model}` | `Model(model)` (and `s.model`, as today) |
| `system/init {permissionMode}`, `system/status {permissionMode}` | `Mode(claude_mode(..))`. When it differs from the requested access, also a ModeChange notice `mode-<n>`: "Claude is in {mode} mode. {requested} isn't available right now (plan, model or fast mode)." |
| `system/init {mcp_servers[]}` with `status == "failed"` | `Warning(format!("MCP server {name} failed to start"))` |
| `status == "needs-auth"` | `Notice` McpAuth, id `mcp-<name>`, "{name} needs you to sign in. Run /mcp in Claude Code's terminal." Claude sends no URL. |
| stdout ends during a turn | `Error(exit_error(..))` |
| cancel watchdog after 10 s | kill the child, then `Error(s.stderr.fatal("claude did not stop after cancel"))` and `Final("")` |
| startup | `startup-spawn` "Starting claude" (Running, Done at the initialize reply); `startup-session` "Starting session" or "Resuming session" |

- [ ] **Step 1: Write the failing tests**

```rust
fn ask_opts() -> DriverStartOptions {
    DriverStartOptions { binary: String::new(), cwd: "/tmp".into(), model: String::new(), effort: String::new(),
        access: AccessMode::Ask, resume: String::new(), attachments: None }
}

#[test]
fn launch_uses_public_flags_and_hook_events() {
    let (args, _, _) = launch_args(&ask_opts());
    let pair = |a: &str, b: &str| args.windows(2).any(|w| w[0] == a && w[1] == b);
    assert!(pair("--permission-prompts", "host") && pair("--permission-mode", "manual"));
    assert!(args.iter().any(|a| a == "--include-hook-events"));
    assert!(!args.iter().any(|a| a == "--permission-prompt-tool"));
}

#[test]
fn initialize_asks_for_nested_calls_and_progress() {
    let r = &initialize_request()["request"];
    assert_eq!((r["subtype"].as_str(), r["sdkForwardNestedSubagentToolCalls"].as_bool(), r["agentProgressSummaries"].as_bool()),
        (Some("initialize"), Some(true), Some(true)));
}

#[test]
fn init_reports_model_mode_and_mcp_trouble() {
    let mut s = State { active: true, requested: AccessMode::Auto, ..State::default() };
    let init = r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude-sonnet-5-5","permissionMode":"default","claude_code_version":"2.1.304","mcp_servers":[{"name":"mcp-1","status":"failed"},{"name":"mcp-2","status":"needs-auth"},{"name":"mcp-3","status":"connected"}]}"#;
    let out = map_line(&mut s, init);
    assert!(out.contains(&Update::Model("claude-sonnet-5-5".into())) && out.contains(&Update::Mode(AccessMode::Ask)));
    assert!(out.iter().any(|u| matches!(u, Update::Notice(n) if n.kind == NoticeKind::ModeChange)));
    assert!(out.iter().any(|u| matches!(u, Update::Warning(w) if w.contains("mcp-1"))));
    assert!(out.iter().any(|u| matches!(u, Update::Notice(n) if n.kind == NoticeKind::McpAuth && n.id == "mcp-mcp-2")));
}

#[test]
fn resume_failures_pick_fresh_or_fork() {
    let mut tail = StderrTail::default();
    tail.push("No conversation found with session ID: 1234");
    assert!(matches!(resume_fix(&tail), Some(ResumeFix::Fresh(_))));
    let mut tail = StderrTail::default();
    tail.push("Session 1234 is running in the background. Add --fork-session to continue it here.");
    assert!(matches!(resume_fix(&tail), Some(ResumeFix::Fork)));
    assert!(resume_fix(&StderrTail::default()).is_none());
}

#[test]
fn fatal_fixture_fails_and_resume_fixture_does_not() {
    assert!(replay("fatal").iter().any(|u| matches!(u, Update::Error(_))));
    assert!(!replay("resume").iter().any(|u| matches!(u, Update::Error(_))));
}
```

`State` gains `requested: AccessMode`, set from `opts.access` in `spawn`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib claude::tests`
Expected: compile errors, then FAIL.

- [ ] **Step 3: Implement the mapping and the spawn handshake.** Update `init_tracks_model_and_reports_a_new_session` for the extra `Model`/`Mode` updates.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib claude::tests`
Expected: all PASS.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/driver/src/claude.rs crates/driver/tests/unit/claude.rs
git commit -m "feat(claude): sdk handshake, public flags, real mode and model, resume fallback"
```

---

### Task 6: Claude subagents and background tasks

**Files:**
- Modify: `crates/driver/src/claude.rs`: `map_value` (drop the `!sub` guards, `system/task_*`), `wake`, `stop_tool`
- Test: `crates/driver/tests/unit/claude.rs`

**Interfaces:**
- Consumes: `child_activity`, `ToolStatus::Background`, the notice ids (Task 4).
- Produces:
  - `State` gains:
    - `tasks: HashMap<String, (String, String)>`: Claude task id to (tool row id, description)
    - `agents: HashMap<String, String>`: `agent_id` to subagent type, used as `source` in Task 8
    - `wake_reason: Option<String>`
    - `wakes: u32`
  - `fn strip_agent_trailer(text: &str) -> (String, Option<(u64, u32)>)`. It removes each line that starts with `agentId: ` and contains `(internal ID`, and any `<usage>…</usage>` block. It returns the cleaned text, plus `(total_tokens, tool_uses)` when the usage block parses.
  - `fn stop_tool(&mut self, id: &str) -> bool`. It writes `{"type":"control_request","request_id":"aether-stop-<n>","request":{"subtype":"stop_task","task_id":…}}` for the task whose row is `id`, and returns true when one exists.

**Mapping:**

| Line | Maps to |
|---|---|
| `assistant` with `parent_tool_use_id: P`, `tool_use` blocks | each block's row with `parent = Some(P)`, plus row `P` updated by `child_activity(new = true)` |
| `user` with `parent_tool_use_id: P`, `tool_result` blocks | that child row settled (today's code path), with `parent = Some(P)` |
| `assistant` / `stream_event` text with `parent_tool_use_id: P` (forwarded subagent text) | row `P`'s Task `activity` = the latest text's first line, clipped to 120 chars. It never reaches the reply text. |
| `tool_result` of an `Agent`/`Task` row | `output = strip_agent_trailer(text).0`. The usage, when present, sets Task `tokens` and `tools`. |
| `tool_result` whose text starts with `Command running in background with ID:` or `Async agent launched` | the row's status becomes `Background`, not Done |
| `system/task_started {task_id, tool_use_id, description, subagent_type, prompt}` | `tasks[task_id] = (tool_use_id, description)`. That row's Task `agent` and `prompt` are filled when empty. |
| `system/task_progress {task_id, usage{total_tokens, tool_uses}, last_tool_name, summary, description}` | row update: `tokens`, `tools`, and `activity = summary or description` |
| `system/task_updated {task_id, patch{status, error, is_backgrounded}}` | `running` + `is_backgrounded` gives Background; `failed` gives Failed with `error`; `killed` gives Stopped; `completed` gives Done |
| `system/task_notification {task_id, status, summary, output_file, usage}` | the row's final status (`completed` Done, `failed` Failed, `stopped` Stopped). Output gains `"\n\n{summary}\n\nFull output: {output_file}"`. `wake_reason = format!("background task \"{description}\" {status}")`. |
| `system/background_tasks_changed` | ignored on purpose |
| a turn the CLI starts on its own (`wake()` returns true) | `[Wake, Notice(wake-<n>, Wake, "Claude started a turn on its own: {wake_reason}")]`, or the text without the reason |

These lines arrive outside a turn too. Every update above is sent regardless of `s.active`, and the phase 2 router puts it on the original row. The agent pump's busy holds keep the session alive while any row is Background.

- [ ] **Step 1: Write the failing tests.** Replace `subagent_and_replay_lines_are_skipped`.

```rust
const AGENT_USE: &str = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"tool_use","id":"toolu_a","name":"Agent","input":{"description":"Find styles","prompt":"look in src","subagent_type":"Explore"}}]}}"#;

#[test]
fn subagent_calls_nest_under_their_agent_row() {
    let mut s = State { active: true, ..State::default() };
    map_line(&mut s, AGENT_USE);
    let child = BASH_USE.replace(r#""parent_tool_use_id":null"#, r#""parent_tool_use_id":"toolu_a""#);
    let out = calls(map_line(&mut s, &child));
    assert!(out.iter().any(|c| c.id == "toolu_1" && c.parent.as_deref() == Some("toolu_a")));
    assert!(out.iter().any(|c| c.id == "toolu_a" && matches!(c.detail, ToolDetail::Task { tools: 1, .. })));
    let replay = r#"{"type":"user","isReplay":true,"parent_tool_use_id":null,"message":{"role":"user","content":[{"type":"text","text":"hi"}]}}"#;
    assert_eq!(map_line(&mut s, replay), []);
}

#[test]
fn agent_results_lose_the_internal_trailer() {
    let raw = "Found it in src/a.css.\nagentId: a1b2 (internal ID - do not mention to user. Use SendMessage with to: 'a1b2' to continue this agent.)\n<usage>total_tokens: 1200\ntool_uses: 3\nduration_ms: 900</usage>";
    assert_eq!(strip_agent_trailer(raw), ("Found it in src/a.css.".into(), Some((1200, 3))));
}

#[test]
fn background_tasks_settle_their_original_row() {
    let mut s = State { active: true, ..State::default() };
    map_line(&mut s, &BASH_USE.replace(r#""command":"echo hi""#, r#""command":"sleep 8","run_in_background":true"#));
    let started = r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"Command running in background with ID: b1"}]}}"#;
    assert_eq!(calls(map_line(&mut s, started))[0].status, ToolStatus::Background);
    map_line(&mut s, r#"{"type":"system","subtype":"task_started","task_id":"b1","tool_use_id":"toolu_1","description":"sleep 8","task_type":"local_bash"}"#);
    s.active = false;
    let done = r#"{"type":"system","subtype":"task_notification","task_id":"b1","tool_use_id":"toolu_1","status":"completed","output_file":"/tmp/b1.out","summary":"exit 0"}"#;
    let row = &calls(map_line(&mut s, done))[0];
    assert_eq!((row.id.as_str(), row.status), ("toolu_1", ToolStatus::Done));
    let woke = map_line(&mut s, r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"delta":{"type":"text_delta","text":"It finished."}}}"#);
    assert!(matches!(&woke[..], [Update::Wake, Update::Notice(n), ..] if n.text.contains("sleep 8")), "{woke:?}");
}

#[test]
fn subagent_and_background_fixtures_nest_and_settle() {
    let calls = last_calls(&replay("subagent"));
    let task = calls.iter().find(|c| matches!(c.detail, ToolDetail::Task { .. })).expect("task row");
    assert!(calls.iter().any(|c| c.parent.as_deref() == Some(task.id.as_str())));
    assert!(matches!(&task.detail, ToolDetail::Task { output, .. } if !output.contains("agentId:")));
    assert!(last_calls(&replay("background")).iter().all(|c| !matches!(c.status, ToolStatus::Running | ToolStatus::Background)));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib claude::tests`
Expected: FAIL.

- [ ] **Step 3: Implement the mapping.** `ToolDetail::Task`'s `description`, `agent` and `prompt` come from `classify` (Task 4).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib claude::tests`
Expected: all PASS, including `live_capture_maps_every_tool_to_a_typed_row`.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/driver/src/claude.rs crates/driver/tests/unit/claude.rs
git commit -m "feat(claude): nested subagent rows, background tasks settle their own row"
```

---

### Task 7: Claude notices, results, to-dos, text fallback, images

**Files:**
- Modify: `crates/driver/src/claude.rs`: `map_value` (`system/*`, `rate_limit_event`, `assistant` text blocks, `result`, `TaskCreate`/`TaskUpdate`/`Skill`/`NotebookEdit` tool uses)
- Modify: `crates/driver/src/tool.rs` (`classify`: `notebookedit` becomes an Edit with `notebook_path` and `new_source`). The Claude track is the only one that edits `classify` after Task 4.
- Test: `crates/driver/tests/unit/claude.rs`, `crates/driver/tests/unit/tool.rs`

**Interfaces:**
- Consumes: `save_images`, `now_ms`, the notice ids (Task 4). `State` gains `attachments: Option<PathBuf>`.
- Produces:
  - `State` gains:
    - `retry: Option<Notice>`
    - `compactions: u32`
    - `models: u32`
    - `cost_seen: f64`: the last `total_cost_usd`, a process-wide running total
    - `usage_seen: [u64; 3]`: modelUsage sums for prompt, completion and cached
    - `todos: Vec<(String, PlanItem)>`
    - `todo_uses: HashMap<String, PlanItem>`: TaskCreate tool_use id to its item, until the result names the task id
    - `texted: HashSet<String>`: assistant message ids that streamed text deltas
    - `turn_texts: Vec<(String, String)>`: (message uuid, text) for the current text segment, used to retract refusals
  - `const TODOS_ID: &str = "todos"`. This is the single Plan row that `TaskCreate` and `TaskUpdate` fold into. It upserts across replies through the phase 2 router.
  - `fn usage_delta(model_usage: &Value, seen: &mut [u64; 3]) -> Option<Update>`. It sums every model's `inputTokens + cacheReadInputTokens + cacheCreationInputTokens`, `outputTokens` and `cacheReadInputTokens`, then returns `Count` of the difference from `seen`. That includes subagent models.

**Mapping:**

| Line | Maps to |
|---|---|
| `system/api_retry {attempt, max_retries, retry_delay_ms, error_status, error}` | Retry, id `retry-<turn>`, Running, `until_ms = now_ms() + retry_delay_ms`, `"Retrying ({attempt}/{max_retries}): {error_status} {error}"`. The next `stream_event` or `assistant` line closes it as Done. |
| `rate_limit_event {rate_limit_info{status, resetsAt, rateLimitType, utilization}}` | `allowed_warning`: RateLimit, id `ratelimit-<type>`, Running, "Claude usage at {utilization×100}% of the {type} limit", `until_ms = resetsAt×1000`. `rejected`: the same id, Failed, "Claude usage limit reached". `allowed`: Done. |
| `system/status {status: "compacting"}` | Compaction, id `compact-<n>` (n = ++`compactions`), Running, "Compacting context" |
| `system/compact_boundary {compact_metadata{trigger, pre_tokens, post_tokens}}` | the open compaction notice, Done, `"Compacted {pre} → {post} tokens"` (without the arrow when `post` is missing), plus `Usage(post)`. A boundary with no open notice creates its own `compact-<n>`. It replaces the `compact-{turn}` row. |
| `system/status {compact_result: "failed", compact_error}` | the open compaction notice, Failed with the error |
| `system/permission_denied {tool_use_id, decision_reason, message}` | that row Denied, with `error = decision_reason or message` |
| `system/informational {content, level}` | `warning` gives `Warning(content)`; otherwise `Notice` Info, id `info-<uuid>` |
| `system/notification {key, text}` | `Notice` Info, id `info-<key>` |
| `system/model_fallback` (any shape) and `system/model_refusal_fallback {original_model, fallback_model, retracted_message_uuids, content}` | ModelChange, id `model-<n>`, `"Switched from {original} to {fallback}"`, plus `Model(fallback)`. For a refusal fallback, the texts of `retracted_message_uuids` are dropped from `turn_texts`, and `Final(<the remaining segment text>)` replaces the segment. |
| `system/model_refusal_no_fallback {content}` | `Warning(content)` |
| `system/hook_started {hook_id, hook_name, hook_event}` | Hook, id `hook-<hook_id>`, Running, `"{hook_event} hook: {hook_name}"` |
| `system/hook_progress` | ignored on purpose |
| `system/hook_response {hook_id, outcome, exit_code, stderr}` | the same id: `success` Done; `error` Failed with `detail = stderr`; `cancelled` Done |
| `system/local_command`, `memory_recall`, `commands_changed`, `thinking_tokens` | ignored on purpose |
| `assistant` text blocks of a message id that is not in `texted` (non-streaming fallback) | `Delta(text)`. `texted` gets the id from `stream_event` `message_start.message.id` once a `text_delta` follows it. |
| `assistant` `tool_use` `TaskCreate {subject, activeForm}` | no row. Store `PlanItem { text: subject, status: Pending }` under the tool_use id. |
| `tool_result` of a TaskCreate | parse `Task #(\d+)` from the text, push `(id, item)` onto `todos`, then send `Update::Tool(TODOS_ID, Plan{items})` |
| `TaskUpdate {taskId, status}` | update that item: `pending`, `in_progress` gives Active, `completed` gives Done, `deleted` removes it. Send the Plan row, and add no row of its own. |
| `Skill {skill}` | `Update::Skill(skill)`, and no row |
| `tool_result` content image blocks | `call.images = save_images(content, s.attachments.as_deref())` |
| `result` with an error subtype | `Error(errors.join("\n"))`. With no errors, the subtype humanized ("Claude stopped: max turns reached"). |
| `result {total_cost_usd}` | `Cost(micros(total - cost_seen))` when > 0, then `cost_seen = total` |
| `result {modelUsage}` | `usage_delta(..)`, which replaces today's main-loop-only `Count` |

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn retries_rate_limits_and_compaction_are_notices() {
    let mut s = State { active: true, ..State::default() };
    let retry = r#"{"type":"system","subtype":"api_retry","attempt":2,"max_retries":10,"retry_delay_ms":4000,"error_status":529,"error":"overloaded"}"#;
    assert!(matches!(&map_line(&mut s, retry)[..], [Update::Notice(n)] if n.kind == NoticeKind::Retry && n.until_ms.is_some()));
    let warn = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","resetsAt":1800000000,"rateLimitType":"five_hour","utilization":0.9}}"#;
    assert!(matches!(&map_line(&mut s, warn)[..], [Update::Notice(n)] if n.kind == NoticeKind::RateLimit && n.text.contains("90")));
    let compacting = r#"{"type":"system","subtype":"status","status":"compacting"}"#;
    let boundary = r#"{"type":"system","subtype":"compact_boundary","compact_metadata":{"trigger":"manual","pre_tokens":90000,"post_tokens":12000}}"#;
    let ids: Vec<String> = [compacting, boundary, compacting, boundary].iter().flat_map(|l| map_line(&mut s, l))
        .filter_map(|u| match u { Update::Notice(n) => Some(n.id), _ => None }).collect();
    assert_eq!((ids.len(), ids[0] == ids[1], ids[1] == ids[2]), (4, true, false));
}

#[test]
fn hooks_denials_and_model_fallbacks_show() {
    let mut s = State { active: true, ..State::default() };
    map_line(&mut s, BASH_USE);
    let denied = r#"{"type":"system","subtype":"permission_denied","tool_name":"Bash","tool_use_id":"toolu_1","decision_reason":"blocked by policy","message":"no"}"#;
    assert!(matches!(&calls(map_line(&mut s, denied))[..], [c] if c.status == ToolStatus::Denied && c.error.as_deref() == Some("blocked by policy")));
    let hook = r#"{"type":"system","subtype":"hook_started","hook_id":"h1","hook_name":"lint","hook_event":"PreToolUse"}"#;
    assert!(matches!(&map_line(&mut s, hook)[..], [Update::Notice(n)] if n.id == "hook-h1"));
    let fallback = r#"{"type":"system","subtype":"model_refusal_fallback","trigger":"refusal","direction":"retry","original_model":"a","fallback_model":"b","request_id":null,"retracted_message_uuids":[],"content":"x"}"#;
    assert!(map_line(&mut s, fallback).contains(&Update::Model("b".into())));
}

#[test]
fn unstreamed_assistant_text_is_kept() {
    let mut s = State { active: true, ..State::default() };
    let msg = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"id":"msg_1","content":[{"type":"text","text":"Before the tool."}]}}"#;
    assert_eq!(map_line(&mut s, msg), [Update::Delta("Before the tool.".into())]);
}

#[test]
fn task_tools_fold_into_one_plan_row() {
    let mut s = State { active: true, ..State::default() };
    let create = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"tool_use","id":"toolu_c","name":"TaskCreate","input":{"subject":"Write tests","activeForm":"Writing tests"}}]}}"#;
    assert_eq!(map_line(&mut s, create), []);
    let created = r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_c","content":"Task #3 created successfully: Write tests"}]}}"#;
    let update = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"tool_use","id":"toolu_u","name":"TaskUpdate","input":{"taskId":"3","status":"in_progress"}}]}}"#;
    map_line(&mut s, created);
    let rows = calls(map_line(&mut s, update));
    assert!(matches!(&rows[..], [c] if c.id == TODOS_ID && matches!(&c.detail, ToolDetail::Plan { items } if items[0].status == PlanStatus::Active)));
}

#[test]
fn results_report_errors_cost_and_subagent_tokens() {
    let mut s = State { active: true, ..State::default() };
    let failed = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"errors":["Reached max turns (5)"],"total_cost_usd":0.25,"usage":{},"modelUsage":{"claude-sonnet-5-5":{"inputTokens":100,"outputTokens":10,"cacheReadInputTokens":50,"cacheCreationInputTokens":0},"claude-haiku-5-5":{"inputTokens":40,"outputTokens":5,"cacheReadInputTokens":0,"cacheCreationInputTokens":0}}}"#;
    let out = map_line(&mut s, failed);
    assert!(out.contains(&Update::Error("Reached max turns (5)".into())));
    assert!(out.contains(&Update::Cost(250_000)));
    assert!(out.contains(&Update::Count { prompt: 190, completion: 15, cached: 50 }));
}

#[test]
fn compaction_fixture_is_a_notice_not_a_row() {
    let out = replay("compaction");
    assert!(out.iter().any(|u| matches!(u, Update::Notice(n) if n.kind == NoticeKind::Compaction && n.state == NoticeState::Done)));
    assert!(!last_calls(&out).iter().any(|c| c.title == "Compact context"));
}
```

`crates/driver/tests/unit/tool.rs`: `classify("NotebookEdit", &json!({"notebook_path": "/w/a.ipynb", "new_source": "x = 1"}))` gives `ToolDetail::Edit` with path `/w/a.ipynb` and `added == 1`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib -- claude::tests tool::tests`
Expected: FAIL.

- [ ] **Step 3: Implement the mapping.** Update `result_reports_last_call_context_window_and_turn_counts` for the `usage_delta` counts, and `zero_usage_results_leave_the_meter_alone`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib`
Expected: all PASS.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/driver/src/claude.rs crates/driver/src/tool.rs crates/driver/tests/unit
git commit -m "feat(claude): system notices, cost and subagent usage, task to-dos, text fallback"
```

---

### Task 8: Claude asks and steers

**Files:**
- Modify: `crates/driver/src/claude.rs`: `control_request`, `reply`, `Kind`, `control_cancel_request`, `steer`, the `isReplay` echo handling, `reports_steered`, `DIALOG_KINDS`
- Test: `crates/driver/tests/unit/claude.rs`

**Interfaces:**
- Consumes: `Approval`, `Questions`, `Form` and `Plan` (Task 1); `State.agents` (Task 6).
- Produces:
  - `Kind` becomes:
    - `Approval { tool, shown, suggestions: Vec<Value> }`
    - `Questions(Vec<String>)` (question texts, in order)
    - `Plan { suggestions: Vec<Value> }`
    - `Elicitation(Vec<(String, String)>)` (field key and JSON type)
    - `Dialog`
  - `fn approval_options(suggestions: &[Value]) -> Vec<ApprovalOption>`. One option per suggestion, ids `"s-<i>"`:

    | Suggestion | Label |
    |---|---|
    | `addRules {rules[{toolName, ruleContent}]}` | "Always allow `{ruleContent}`", or "Always allow {toolName}" without content. `Bash(pnpm add:*)` reads "Always allow `pnpm add *`". |
    | `addDirectories {directories}` | "Always allow edits in {dirs joined}" |
    | `setMode {mode: "acceptEdits"}` | "Allow and switch this chat to Accept edits" |
    | `setMode` with another mode | "Allow and switch this chat to {mode}" |
    | `replaceRules`, `removeRules`, `removeDirectories` | no option |

    `setMode` is its own option and never folds into another (spec section 3).
  - `fn elicitation_form(r: &Value) -> (Form, Vec<(String, String)>)`. It maps `requested_schema` like Codex's `schema_form` (Task 12), but in this file: `title`, `message`, `url` when `mode == "url"`, and `source = mcp_server_name`.
  - `fn reports_steered(&self) -> bool { true }`, with `State.echoes: VecDeque<Option<String>>` replacing the `u32`. `None` stands for a prompt echo, `Some(text)` for a steer echo.
  - `DIALOG_KINDS` stays `&[]`. The SDK publishes no payload schema for any dialog kind, and an undeclared kind makes the CLI fall back to its no-dialog behavior.

**Mapping:**

| Request or line | Maps to |
|---|---|
| `can_use_tool` | `Approval`:<br>- `tool_id = tool_use_id`<br>- `kind`: Bash gives Command; Write, Edit, MultiEdit and NotebookEdit give Edit; WebFetch and WebSearch give Fetch; `mcp__*` gives Mcp; else Other<br>- `input`: phase 1's `approval_input` (built from the raw input) stays, except that edits return only the path<br>- `diff` = the `Edit` detail's diff, which phase 1 appended to `input`<br>- `source = agents[agent_id]` when `agent_id` is set<br>- `options = approval_options(permission_suggestions)` |
| `Answer::Pick("s-<i>")` on an approval | `{"behavior":"allow","updatedInput":input,"updatedPermissions":[suggestion i with "destination":"session"]}` |
| `Answer::Approve` | allow with the edited input (phase 1). `always: true` now behaves as once, since options replace it. |
| `Answer::DenyWith(n)` | `deny(n)`: Claude passes the message to the model |
| `AskUserQuestion` | one `Ask::Questions` with every question: `header`, `question`, `options` (label, description), `multi = multiSelect`, `other = true` |
| `Answer::Answers(vals)` on `Questions` | `{"behavior":"allow","updatedInput":{"questions":…,"answers":{question_i: vals[i].join(", ")}}}` |
| `ExitPlanMode {plan}` | `Ask::Plan(Plan { markdown: plan, options: approval_options(permission_suggestions), ..Plan::default() })` |
| `Answer::Pick` on a plan | allow, plus that suggestion as `updatedPermissions` |
| `elicitation` | `Ask::Form(elicitation_form(..))`. `Form` replies `{"action":"accept","content":{…}}`, and `Deny` replies `{"action":"decline"}`. |
| `request_user_dialog {dialog_kind}` | reply `{"behavior":"cancelled"}` (the SDK's rule for kinds the host can't render), plus `Warning(format!("Claude asked for a {dialog_kind} dialog Aether can't show yet"))` |
| any other control request subtype | the error reply (today), plus `Warning` naming the subtype |
| `control_cancel_request {request_id}` | `AskResolved(local)` for each ask with that request id, then remove them |
| `prompt(text)` | push `None` onto `echoes`, except for a `/command`, which has no echo (today's rule) |
| `steer(text)` | push `Some(without_attached_files(text))` onto `echoes`. No `Wake`. |
| `user` line with `isReplay: true` | pop the front echo. `Some(text)` gives `Steered(text)`. The `<local-command-stdout>` exception stays. |

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn approval_options_keep_set_mode_separate() {
    let opts = approval_options(&[
        json!({"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"pnpm add:*"}],"behavior":"allow","destination":"session"}),
        json!({"type":"setMode","mode":"acceptEdits","destination":"session"}),
        json!({"type":"removeRules","rules":[],"behavior":"allow","destination":"session"}),
    ]);
    let labels: Vec<&str> = opts.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(labels, ["Always allow `pnpm add *`", "Allow and switch this chat to Accept edits"]);
}

#[test]
fn write_approval_links_its_row_and_picks_one_suggestion() {
    let mut s = State::default();
    let [(id, Ask::Approval(a))] = &asks(map_line(&mut s, WRITE_REQUEST))[..] else { panic!() };
    assert_eq!((a.kind, a.tool_id.as_deref(), a.input.as_str()), (ApprovalKind::Edit, Some("toolu_01Ly3im2TnrYv8cB2Gd5FJBg"), "/private/tmp/claude-probe/probe1.txt"));
    assert!(a.diff.as_deref().is_some_and(|d| d.lines().any(|l| l == "+hi")));
    let rep = reply(&mut s, *id, &Answer::Pick("s-0".into())).unwrap();
    assert_eq!(rep["response"]["response"]["updatedPermissions"], json!([{"type":"setMode","mode":"acceptEdits","destination":"session"}]));
}

#[test]
fn questions_are_one_ask_and_multi_select_joins_labels() {
    let mut s = State::default();
    let [(id, Ask::Questions(qs))] = &asks(map_line(&mut s, ASK_REQUEST))[..] else { panic!() };
    assert_eq!(qs.len(), 2);
    let rep = reply(&mut s, *id, &Answer::Answers(vec![vec!["Red".into(), "Blue".into()], vec!["S".into()]])).unwrap();
    assert_eq!(rep["response"]["response"]["updatedInput"]["answers"], json!({"Which color do you prefer?": "Red, Blue", "Which size?": "S"}));
}

#[test]
fn cancelled_requests_resolve_their_asks() {
    let mut s = State::default();
    let [(id, _)] = asks(map_line(&mut s, WRITE_REQUEST))[..] else { panic!() };
    let cancel = r#"{"type":"control_cancel_request","request_id":"984a7c5a-0adf-43be-bffc-1ee49a7ca0fd"}"#;
    assert_eq!(map_line(&mut s, cancel), [Update::AskResolved(id)]);
}

#[test]
fn elicitations_are_forms_and_dialogs_are_cancelled() {
    let mut s = State { active: true, ..State::default() };
    let el = r#"{"type":"control_request","request_id":"e1","request":{"subtype":"elicitation","mcp_server_name":"mcp-1","message":"Pick a project","requested_schema":{"type":"object","properties":{"p":{"type":"string","enum":["WEB","API"]}},"required":["p"]}}}"#;
    let [Update::Ask { id, ask: Ask::Form(f) }] = &map_line(&mut s, el)[..] else { panic!() };
    assert_eq!((f.fields[0].kind, f.source.as_deref()), (FieldKind::Choice, Some("mcp-1")));
    let rep = reply(&mut s, *id, &Answer::Form(vec![("p".into(), vec!["API".into()])])).unwrap();
    assert_eq!(rep["response"]["response"], json!({"action":"accept","content":{"p":"API"}}));
    let dialog = r#"{"type":"control_request","request_id":"d1","request":{"subtype":"request_user_dialog","dialog_kind":"refusal_fallback_prompt","payload":{}}}"#;
    assert!(matches!(&map_line(&mut s, dialog)[..], [Update::Warning(_)]));
    assert_eq!(s.outbox.last().unwrap()["response"]["response"], json!({"behavior":"cancelled"}));
}

#[test]
fn steers_are_reported_when_claude_echoes_them() {
    let mut s = State { active: true, ..State::default() };
    on_prompt(&mut s, "count");
    on_steer(&mut s, "also say done");
    let echo = |t: &str| json!({"type":"user","isReplay":true,"parent_tool_use_id":null,"message":{"role":"user","content":[{"type":"text","text":t}]}}).to_string();
    assert_eq!(map_line(&mut s, &echo("count")), []);
    assert_eq!(map_line(&mut s, &echo("also say done")), [Update::Steered("also say done".into())]);
}

#[test]
fn approval_and_question_fixtures_are_answerable() {
    for scenario in ["approval", "subagent-approval"] {
        let out = replay(scenario);
        let a = out.iter().find_map(|u| match u { Update::Ask { ask: Ask::Approval(a), .. } => Some(a.clone()), _ => None }).expect(scenario);
        assert!(a.tool_id.is_some(), "{scenario}");
        if scenario == "subagent-approval" { assert!(a.source.is_some()); }
    }
    assert!(replay("question").iter().any(|u| matches!(u, Update::Ask { ask: Ask::Questions(q), .. } if q.len() == 2)));
    assert!(replay("steer").iter().any(|u| matches!(u, Update::Steered(_))));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib claude::tests`
Expected: FAIL.

- [ ] **Step 3: Implement the mapping.** Update phase 1's tests to the new shapes:
  - `write_approval_shows_path_and_diff_and_always_skips_set_mode` now expects the diff in `a.diff`, not `input`, and `always: true` sends no `updatedPermissions`.
  - `ask_user_question_asks_each_question_and_answers_once` becomes the single-ask form above.
  - `cancelled_control_request_drops_its_asks` expects `AskResolved`.
  - `unknown_control_request_gets_an_error_reply` also expects a Warning.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib claude::tests`
Expected: all PASS.

- [ ] **Step 5: Manual check**

In a Claude chat in Ask mode, prompt "use AskUserQuestion to ask me two questions, one multi-select". Expected:
- One card with both questions.
- The answer reaches Claude, which repeats both answers.
- A later Write approval shows "Allow and switch this chat to Accept edits" as its own button.

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/driver/src/claude.rs crates/driver/tests/unit/claude.rs
git commit -m "feat(claude): scoped approval options, question sets, mcp forms, steer echoes"
```

---


## Codex track (Tasks 9-12, parallelizable)

Shapes come from Codex 0.163.0's app-server v2 schema, regenerated with `codex app-server generate-json-schema --out <tmp dir> --experimental` and never committed. `ev`, `tool`, `MAIN`, `SPAWNED` and the `SUB_*` constants are the existing test helpers and fixtures in `crates/driver/tests/unit/codex.rs`.

### Task 9: Codex turn identity, steers, lifecycle robustness

**Files:**
- Modify: `crates/driver/src/codex.rs`: `State`, `spawn` (stderr), `handshake`, `prompt`, `steer`, `cancel`, `start_turn`, `turn_started`, `response`, `map_event` (`turn/*`), the commandExecution output path, `reports_steered`
- Test: `crates/driver/tests/unit/codex.rs`

**Interfaces:**
- Consumes: `StderrTail`, `log_unknown`, the notice ids, `SteerRefused` (Task 4).
- Produces:
  - `State` gains:
    - `stderr: StderrTail` (replaces `stderr: String`)
    - `stale: HashSet<String>`: interrupted turn ids whose completion must not end a newer turn
    - `next: Option<(String, Vec<Attachment>)>`: a prompt waiting for the cancelled turn to finish
    - `wake_reason: Option<String>`
    - `wakes: u32`
    - `live: HashMap<String, LiveOutput>` (replaces `output: HashMap<String, String>`)
  - `pub(crate) struct LiveOutput { done: String, partial: String }`, with `fn push(&mut self, delta: &str) -> String`. Complete lines are cleaned once and appended to `done`. The unfinished last line stays raw in `partial`, so ANSI codes and `\r` redraws split across deltas still clean right. `done` keeps at most `LIVE_OUTPUT_CAP = 64 * 1024` bytes, dropping whole lines from the front. The return value is `done + clean(partial)`.
  - `fn reports_steered(&self) -> bool { true }`.
  - `fn steer_refused(error: &Value) -> Option<&'static str>`. It returns the reason when a `turn/steer` error means the turn can't take the steer:
    - `data.codexErrorInfo.activeTurnNotSteerable.turnKind` is `"review"` or `"compact"`
    - the message is `"no active turn to steer"` or starts with `"expected active turn id"`
    - the message is `"cannot steer a review turn"` or `"cannot steer a compact turn"`

    Match on these, never on the numeric code.
  - `fn thread_mode(policy: &str, sandbox: &Value, reviewer: &str) -> Option<AccessMode>`. It inverts `access()`:
    - `never` + `dangerFullAccess` gives Full
    - `on-request` + `workspaceWrite` + `auto_review` gives Auto
    - `untrusted` + `workspaceWrite` gives AutoEdit
    - `untrusted` + `readOnly` gives Ask
    - `never` + `readOnly` gives ReadOnly

**Mapping:**

| Event or action | Maps to |
|---|---|
| `turn/started {turn.id}` while active with `turn_id == None` | ours: `turn_started(id)` (today) |
| `turn/started` while `!active` | a server-started turn: `active = true`, `turn_id = id`, then `[Wake, Notice(wake-<n>, Wake, "Codex started a turn on its own: {wake_reason}")]`. Without a reason the text is just "Codex started a turn on its own". |
| `thread/queue/changed` | `wake_reason = "a queued message"` |
| `thread/goal/updated {goal.objective}` | `wake_reason = "goal: {objective}"` |
| `turn/completed {turn.id}` with `Some(id) == turn_id` | today's turn end. Then, if `next` is set, `start_turn` with it. |
| `turn/completed` with an id in `stale` | remove it, and nothing else |
| `turn/completed` with any other id | `log_unknown("codex", "turn/completed for another turn")` |
| `cancel` with a live turn | insert the turn id into `stale`, then send `turn/interrupt` (today) |
| `prompt` while `active && cancelled` | store it in `next`. It is not sent until the interrupted turn completes. |
| `Call::Steer(text)` reply ok | `Steered(without_attached_files(text))` |
| `Call::Steer(text)` error where `steer_refused` is `Some(kind)` | `SteerRefused(text)`. When the turn kind is review or compact, also `Notice::new("info-steer", Info, "Codex can't take messages during /{kind}. Yours stays queued.")`. The old fallback that started a turn plus `Wake` is deleted. |
| other `Call::Steer` errors | `SteerRefused(text)` plus `Warning(message)` |
| `turn/start` | no `sandboxPolicy`, `approvalPolicy` or `approvalsReviewer`: the thread keeps what `thread/start`/`resume` set |
| `thread/start` / `thread/resume` reply | `Model(result.model)`, then `Mode(thread_mode(..))`. When the mode differs from `opts.access`, also a ModeChange notice `mode-<n>`. |
| `thread/resume` error | fall back to `thread/start` (today), plus `Notice::new("info-resume", Info, …)`. The text is "Couldn't resume the Codex thread because it's open in another Codex window. Started a new one." when the message contains `thread writer lock`, otherwise "Couldn't resume the Codex thread ({message}). Started a new one." |
| `item/commandExecution/outputDelta` | `live[item].push(delta)`, which replaces re-cleaning the whole buffer |
| stdout ends | `Error(s.stderr.fatal("codex exited"))` |
| cancel watchdog after 10 s | kill the child, then `Error(s.stderr.fatal("codex did not stop after cancel"))` and `Final("")` |
| startup | `startup-spawn` "Starting codex app-server" (Running, Done after `initialize`); `startup-session` "Starting thread" or "Resuming thread" (Running, Done after the reply) |

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_stale_completion_never_ends_the_next_turn() {
    let mut s = State { thread: "t".into(), ..State::default() };
    let started = |s: &mut State, line: Value, turn: &str| {
        map_event(s, &json!({"id": line["id"], "result": {"turn": {"id": turn, "status": "inProgress"}}}))
    };
    let first = start_turn(&mut s, "first", &[]);
    started(&mut s, first, "turn-1");
    on_cancel(&mut s);
    let second = start_turn(&mut s, "second", &[]);
    started(&mut s, second, "turn-2");
    let old = r#"{"method":"turn/completed","params":{"threadId":"t","turn":{"id":"turn-1","items":[],"status":"interrupted","error":null}}}"#;
    assert_eq!(ev(&mut s, old), []);
    assert!(s.active && s.turn_id.as_deref() == Some("turn-2"));
}

#[test]
fn server_started_turns_wake_with_their_reason() {
    let mut s = State { thread: "t".into(), ..State::default() };
    ev(&mut s, r#"{"method":"thread/queue/changed","params":{"threadId":"t"}}"#);
    let out = ev(&mut s, r#"{"method":"turn/started","params":{"threadId":"t","turn":{"id":"turn-9","items":[],"status":"inProgress","error":null}}}"#);
    assert!(matches!(&out[..], [Update::Wake, Update::Notice(n)] if n.kind == NoticeKind::Wake && n.text.contains("a queued message")), "{out:?}");
}

#[test]
fn unsteerable_turn_refuses_the_steer() {
    let mut s = State { thread: "t".into(), active: true, turn_id: Some("turn-1".into()), ..State::default() };
    let line = steer_turn(&mut s, "also this".into(), vec![]);
    let id = line["id"].as_u64().unwrap();
    let err = json!({"id": id, "error": {"code": -32600, "message": "cannot steer a review turn",
        "data": {"message": "x", "codexErrorInfo": {"activeTurnNotSteerable": {"turnKind": "review"}}}}});
    let out = map_event(&mut s, &err);
    assert!(out.contains(&Update::SteerRefused("also this".into())), "{out:?}");
    assert!(!out.contains(&Update::Wake) && s.outbox.iter().all(|l| l["method"] != "turn/start"));
}

#[test]
fn accepted_steers_are_reported() {
    let mut s = State { thread: "t".into(), active: true, turn_id: Some("turn-1".into()), ..State::default() };
    let line = steer_turn(&mut s, "also this".into(), vec![]);
    let ok = json!({"id": line["id"], "result": {"turnId": "turn-1"}});
    assert_eq!(map_event(&mut s, &ok), [Update::Steered("also this".into())]);
}

#[test]
fn turns_no_longer_override_the_thread_sandbox() {
    let mut s = State { thread: "t".into(), access: AccessMode::AutoEdit, ..State::default() };
    let line = start_turn(&mut s, "hi", &[]);
    for key in ["sandboxPolicy", "approvalPolicy", "approvalsReviewer"] { assert!(line["params"].get(key).is_none(), "{key}"); }
}

#[test]
fn live_output_cleans_split_escapes_and_stays_bounded() {
    let mut out = LiveOutput::default();
    out.push("a\x1b[3");
    assert_eq!(out.push("1mred\x1b[0m\nload 10%\rload 99%"), "ared\nload 99%");
    let big = "x".repeat(1000) + "\n";
    for _ in 0..100 { out.push(&big); }
    assert!(out.push("").len() <= LIVE_OUTPUT_CAP + 1100);
}

#[test]
fn thread_modes_invert_access() {
    for mode in [AccessMode::Ask, AccessMode::ReadOnly, AccessMode::AutoEdit, AccessMode::Auto, AccessMode::Full] {
        let [policy, _, sandbox, reviewer] = access(mode);
        assert_eq!(thread_mode(policy, &json!({"type": sandbox}), reviewer), Some(mode));
    }
}

#[test]
fn cancel_and_steer_fixtures_end_cleanly() {
    assert!(replay("steer").iter().any(|u| matches!(u, Update::Steered(_))));
    let cancel = replay("cancel");
    assert_eq!(cancel.iter().filter(|u| matches!(u, Update::Final(_))).count(), 1);
    assert!(last_calls(&cancel).iter().all(|c| c.status != ToolStatus::Running));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib codex::tests`
Expected: compile errors, then FAIL.

- [ ] **Step 3: Implement the mapping**
  - Delete `steers_queue_and_fall_back_to_a_new_turn` and `a_steer_the_turn_ended_under_runs_as_its_own_turn_in_a_new_reply`. Replace them with `steers_queued_before_the_turn_id_are_sent_at_turn_start`, which keeps today's first half: a steer sent before the turn id is queued, and sent as `turn/steer` at `turn_started`.
  - Update `turn_start_request_and_reply` to the request without `sandboxPolicy`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib codex::tests`
Expected: all PASS.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/driver/src/codex.rs crates/driver/tests/unit/codex.rs
git commit -m "feat(codex): turn ids, steer delivery and refusal, stderr tail, live output"
```

---

### Task 10: Codex subagents and background terminals

**Files:**
- Modify: `crates/driver/src/codex.rs`: `adopt`, `agent_event`, `side_event` (grandchildren), `completed`, `item_calls` (collab actions), the turn-end drain, `stop_tool`, `handshake` (list on resume)
- Test: `crates/driver/tests/unit/codex.rs`

**Interfaces:**
- Consumes: `child_activity` (Task 4), `ToolStatus::Background` (phase 2).
- Produces:
  - `State.agents: HashMap<String, String>` still maps a thread to its spawn row id, for children and grandchildren alike.
  - `State.terminals: HashMap<String, String>`: row id to `processId`, for commands still running at turn end.
  - `fn stop_tool(&mut self, id: &str) -> bool`. It sends `thread/backgroundTerminals/terminate {threadId, processId}` and returns true when `id` is in `terminals`.
  - `fn background_rows(list: &Value) -> Vec<ToolCall>`. It turns the `thread/backgroundTerminals/list` result (`data[]{itemId, processId, command, cwd}`) into `Background` Command rows with id `itemId`. `handshake` calls the list after a resume and sends these rows.

**Mapping:**

| Event | Maps to |
|---|---|
| main `turn/completed` | Running command rows with a `processId` become `Background` (kept in `tools` and in `terminals`). Running spawn rows whose child thread still runs become `Background`. Every other Running row becomes `Stopped`, as today. `tools` is no longer drained of Background rows. |
| a later `item/completed` for a Background command | its original row, settled (`Done`/`Failed`), routed across replies by the phase 2 router. Remove it from `terminals`. |
| child thread `item/started` / `item/completed` | the child's own row through `item_calls` and `completed`, with `parent = Some(agents[thread])`, plus the spawn row updated by `child_activity` |
| child `item/completed` agentMessage | the spawn row's Task `output` = the message text (the result markdown) |
| child `thread/tokenUsage/updated` | the spawn row's Task `tokens = total.totalTokens` |
| child `error` (not `willRetry`) | the spawn row's `error` = the message |
| child `turn/completed` | spawn row Done, Stopped or Failed (today) |
| child `collabAgentToolCall` spawnAgent with `receiverThreadIds` | `agents[receiver] = that child row id` (grandchildren nest under the child's row) |
| `collabAgentToolCall` tool `wait`, `sendMessage`, `sendInput`, `followupTask`, `resumeAgent`, `closeAgent`, `interruptAgent`, `listAgents` | a row titled "Wait for agents", "Message agent", "Send input to agent", "Follow up with agent", "Resume agent", "Close agent", "Interrupt agent" or "List agents", with `ToolDetail::Other { name: tool, input: prompt or the receiver list }`. With exactly one receiver, `parent` = that receiver's spawn row. Status follows the item status. |
| `subAgentActivity` kind `interacted` | spawn row `activity = "Working"`; `interrupted` gives Stopped; `completed` gives Done |
| child `item/fileChange/requestApproval` | resolves paths from the child's nested rows (`{item}#{i}`), which `tools` now holds |

- [ ] **Step 1: Write the failing tests.** Replace `sub_agent_streams_into_its_task_row`. It asserted that child activity is flattened into the Task output and dropped at turn end.

```rust
#[test]
fn sub_agent_calls_nest_and_outlive_the_parent_turn() {
    let mut s = State { active: true, thread: MAIN.into(), totals: Some([0; 3]), ..State::default() };
    let row = tool(&ev(&mut s, SPAWNED)).clone();
    let out = ev(&mut s, SUB_ECHO_STARTED);
    let calls = last_calls(&out);
    assert!(calls.iter().any(|c| c.parent.as_deref() == Some(row.id.as_str()) && matches!(c.detail, ToolDetail::Command { .. })), "{out:?}");
    assert!(calls.iter().any(|c| c.id == row.id && matches!(c.detail, ToolDetail::Task { tools: 1, .. })));
    let main_done = TURN_DONE.replace("01a15157-b7e2-7e01-ad4c-0dcb40b02092", MAIN);
    s.turn_id = Some("01a15157-d0ec-70f3-9473-d9702bd26670".into());
    let end = ev(&mut s, &main_done);
    assert!(end.iter().any(|u| matches!(u, Update::Tool(c) if c.id == row.id && c.status == ToolStatus::Background)), "{end:?}");
    let done = tool(&ev(&mut s, SUB_TURN_DONE)).clone();
    assert_eq!((done.id, done.status), (row.id, ToolStatus::Done));
}

#[test]
fn background_terminals_survive_turn_end_and_settle_later() {
    let mut s = State { active: true, thread: "t".into(), turn_id: Some("u".into()), ..State::default() };
    let started = r#"{"method":"item/started","params":{"threadId":"t","item":{"type":"commandExecution","id":"exec-1","command":"/bin/zsh -lc 'pnpm dev'","cwd":"/w","processId":"pty-7","source":"agent","status":"inProgress","commandActions":[{"type":"unknown","command":"pnpm dev"}],"aggregatedOutput":null,"exitCode":null,"durationMs":null}}}"#;
    ev(&mut s, started);
    let end = ev(&mut s, r#"{"method":"turn/completed","params":{"threadId":"t","turn":{"id":"u","items":[],"status":"completed","error":null}}}"#);
    assert!(end.iter().any(|u| matches!(u, Update::Tool(c) if c.id == "exec-1" && c.status == ToolStatus::Background)));
    assert_eq!(s.terminals.get("exec-1").map(String::as_str), Some("pty-7"));
    let late = started.replace("item/started", "item/completed").replace("\"inProgress\"", "\"completed\"").replace("\"exitCode\":null", "\"exitCode\":0");
    assert_eq!(tool(&ev(&mut s, &late)).status, ToolStatus::Done);
    assert!(s.terminals.is_empty());
}

#[test]
fn listed_background_terminals_become_rows() {
    let rows = background_rows(&json!({"data": [{"itemId": "exec-1", "processId": "pty-7", "command": "pnpm dev", "cwd": "/w", "osPid": 4242}]}));
    assert!(matches!(&rows[..], [c] if c.id == "exec-1" && c.status == ToolStatus::Background && c.detail.summary() == "pnpm dev"));
}

#[test]
fn collab_actions_are_rows() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let wait = r#"{"method":"item/started","params":{"threadId":"t","item":{"type":"collabAgentToolCall","id":"call_w","tool":"wait","status":"inProgress","senderThreadId":"t","receiverThreadIds":["a","b"],"prompt":null,"model":null,"reasoningEffort":null,"agentsStates":{}}}}"#;
    assert_eq!(tool(&ev(&mut s, wait)).title, "Wait for agents");
}

#[test]
fn subagent_fixture_nests_and_counts() {
    let calls = last_calls(&replay("subagent"));
    let task = calls.iter().find(|c| matches!(c.detail, ToolDetail::Task { .. })).expect("task row");
    assert!(calls.iter().any(|c| c.parent.as_deref() == Some(task.id.as_str())));
    assert!(matches!(task.detail, ToolDetail::Task { tools, .. } if tools > 0));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib codex::tests`
Expected: FAIL.

- [ ] **Step 3: Implement the mapping.** Update `spawn_agent_call_routes_its_new_thread_and_fails_alone` and `interrupted_turn_stops_open_tools`. Interrupted rows without a `processId` still become Stopped.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib codex::tests`
Expected: all PASS.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/driver/src/codex.rs crates/driver/tests/unit/codex.rs
git commit -m "feat(codex): nested subagents, grandchildren, background terminals across turns"
```

---

### Task 11: Codex items and notifications

**Files:**
- Modify: `crates/driver/src/codex.rs`: `map_event`, `completed`, `item_calls`, `say`/`think` (phase and raw reasoning), `start_turn` (`/plan`)
- Test: `crates/driver/tests/unit/codex.rs`

**Interfaces:**
- Consumes: `save_images`, `now_ms`, the notice ids (Task 4). `State` gains `attachments: Option<PathBuf>` from `opts`.
- Produces:
  - `State` gains:
    - `phases: HashMap<String, String>`: agentMessage item id to its `phase`, from `item/started`
    - `reasoning: HashMap<String, bool>`: per reasoning item, whether summary deltas were seen
    - `turn_diff: String`
    - `retry: Option<Notice>`
  - `fn copy_image(path: &str, dir: Option<&Path>) -> Option<Attachment>`. It copies a local image file into the chat's attachment folder (ADR 0001).
  - `/plan <text>` starts a turn with `collaborationMode: {"mode": "plan", "settings": {"model": s.model, "reasoning_effort": s.effort or null, "developer_instructions": null}}`.

**Items:**

| Item | Maps to |
|---|---|
| `agentMessage` with `phase == "commentary"` (from `item/started`) | its deltas and gap-fill go out as `Thinking`, not `Delta` |
| `agentMessage.questions [{title, options}]` | appended to the text as `"\n\n**{title}**\n- {option}…"`. They are prompts in prose with no server request, so there is no card. |
| `reasoning` `summaryTextDelta` | `Thinking` (today) |
| `reasoning` `textDelta` (raw) | `Thinking` only while that item has had no summary delta; otherwise ignored |
| `plan {text}` and `item/plan/delta` | handled like an `agentMessage` (`Delta`, with gap-fill on completion) |
| `enteredReviewMode {review}` | `Notice::new(format!("info-review-{id}"), Info, format!("Reviewing {review}"))` |
| `exitedReviewMode {review}` | `Delta("\n\n" + review)`: the review findings are the answer |
| `imageView {path}` | a Done Read row "View image", `images = copy_image(path)` |
| `imageGeneration {status, revisedPrompt, savedPath, failure}` | an Other row "Generate image", input = revisedPrompt, `images = copy_image(savedPath)`, Failed when `failure` is set |
| `sleep {durationMs}` | an Other row titled `"Wait {secs} s"`, Running on start and Done on completion |
| `hookPrompt {fragments}` | `Notice` Hook, id `hook-prompt-<id>`, the fragments' text joined |
| `dynamicToolCall {tool, arguments, contentItems, success}` | `start(id, tool, arguments)`. The result is the `inputText` items, `Failed` when `success == false`, and `images` come from `inputImage` data URLs through `save_images`. |
| `fileChange` change with `kind.type == "update"` and `move_path` | title "Rename file", path `"{path} → {move_path}"` |
| `mcpToolCall` result | `images = save_images(result.content)`. When the result has no text, `structuredContent` is pretty-printed as the output. |

**Notifications:**

| Notification | Maps to |
|---|---|
| `error` with `willRetry: true` | Retry, id `retry-<turn>`, Running, text `"Retrying: {message}"`. The next delta or item closes it as Done. |
| `error` (not retrying) with `codexErrorInfo` `usageLimitExceeded` or `rateLimitExceeded` | a RateLimit notice `ratelimit-default`, plus today's single Error |
| `turn/diff/updated {diff}` | store it in `turn_diff`. At `turn/completed`, fileChange rows with an empty diff take their file's section of `turn_diff`, split on `diff --git` headers. |
| `account/rateLimits/updated {rateLimits}` | when `rateLimitReachedType` is set: RateLimit, id `ratelimit-<limitId or default>`, "Codex usage limit reached ({type})", Failed. When `primary.usedPercent >= 80`: Running, "Codex usage at {n}%", with `until_ms` from `resetsAt`. Otherwise nothing. |
| `model/rerouted {fromModel, toModel, reason}` | `Model(toModel)` plus a ModelChange notice `"Codex switched from {from} to {to}"` |
| `warning {message}`, `guardianWarning {message}` | `Warning(message)` |
| `configWarning {summary, details}`, `deprecationNotice {summary, details}` | `Warning(summary + details)` |
| `hook/started {run}` / `hook/completed {run}` | Hook, id `hook-<run.id>`, text `"{eventName} hook"`, plus ": {statusMessage}" when set. Running, then Done; `failed` or `blocked` give Failed. `detail` = the entries' text. |
| `item/autoApprovalReview/started` / `completed` | Info, id `info-review-<reviewId>`, `"Auto-review {status}: {action summary}"`, `detail = review.rationale` |
| `autoApprovalReview/strictReviewRequired` | `Notice::new("info-strict-review", Info, "Codex needs a stricter review for the next action")` |
| `item/commandExecution/terminalInteraction {itemId, stdin}` | that row's output gains `"\n> {stdin}"` |
| `item/fileChange/patchUpdated {itemId, changes}` | re-sends the fileChange rows from `changes` (a live diff) |
| `item/mcpToolCall/progress {itemId, message}` | that row's output = message |
| `mcpServer/oauthLogin/completed` | McpAuth, id `mcp-<name>`, Done, "Signed in to {name}" |
| `thread/status/changed`, `thread/started`, `thread/compacted`, `skills/changed`, `rawResponse*`, `fs/changed`, `account/updated`, `turn/moderationMetadata`, `model/verification`, `thread/settings/updated`, `thread/readState/changed` | ignored on purpose |
| anything else | `log_unknown("codex", method)` |

- [ ] **Step 1: Write the failing tests**

```rust
fn note(method: &str, params: Value) -> Value { json!({"method": method, "params": params}) }

#[test]
fn retries_rate_limits_reroutes_and_warnings_are_notices() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let retrying = MODEL_ERROR.replace(r#""willRetry":false"#, r#""willRetry":true"#);
    assert!(matches!(&ev(&mut s, &retrying)[..], [Update::Notice(n)] if n.kind == NoticeKind::Retry && n.state == NoticeState::Running));
    let limits = note("account/rateLimits/updated", json!({"rateLimits": {"limitId": "codex", "primary": {"usedPercent": 91, "windowDurationMins": 300, "resetsAt": 1800000000}, "rateLimitReachedType": null}}));
    assert!(matches!(&map_event(&mut s, &limits)[..], [Update::Notice(n)] if n.kind == NoticeKind::RateLimit && n.text.contains("91")));
    let rerouted = note("model/rerouted", json!({"threadId": "t", "turnId": "u", "fromModel": "gpt-a", "toModel": "gpt-b", "reason": "highRiskCyberActivity"}));
    let out = map_event(&mut s, &rerouted);
    assert!(out.contains(&Update::Model("gpt-b".into())) && out.iter().any(|u| matches!(u, Update::Notice(n) if n.kind == NoticeKind::ModelChange)));
    assert_eq!(map_event(&mut s, &note("warning", json!({"threadId": null, "message": "slow disk"}))), [Update::Warning("slow disk".into())]);
}

#[test]
fn hooks_run_as_notices() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let run = |status: &str| json!({"id": "h1", "eventName": "preToolUse", "handlerType": "command", "status": status, "statusMessage": null, "entries": []});
    let started = map_event(&mut s, &note("hook/started", json!({"threadId": "t", "turnId": "u", "run": run("running")})));
    let done = map_event(&mut s, &note("hook/completed", json!({"threadId": "t", "turnId": "u", "run": run("failed")})));
    assert!(matches!((&started[..], &done[..]), ([Update::Notice(a)], [Update::Notice(b)]) if a.id == "hook-h1" && b.id == "hook-h1" && b.state == NoticeState::Failed));
}

#[test]
fn review_images_sleep_and_commentary_items_show() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let review = note("item/completed", json!({"threadId": "t", "item": {"type": "exitedReviewMode", "id": "r1", "review": "1 issue: x"}}));
    assert_eq!(map_event(&mut s, &review), [Update::Delta("\n\n1 issue: x".into())]);
    let sleep = note("item/started", json!({"threadId": "t", "item": {"type": "sleep", "id": "s1", "durationMs": 5000}}));
    assert_eq!(tool(&map_event(&mut s, &sleep)).title, "Wait 5 s");
    let commentary = note("item/started", json!({"threadId": "t", "item": {"type": "agentMessage", "id": "m1", "text": "", "phase": "commentary"}}));
    map_event(&mut s, &commentary);
    let delta = note("item/agentMessage/delta", json!({"threadId": "t", "itemId": "m1", "delta": "I'll run ls"}));
    assert_eq!(map_event(&mut s, &delta), [Update::Thinking("I'll run ls".into())]);
}

#[test]
fn renames_show_both_paths() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let mv = note("item/started", json!({"threadId": "t", "item": {"type": "fileChange", "id": "f1", "status": "inProgress",
        "changes": [{"path": "/w/a.rs", "kind": {"type": "update", "move_path": "/w/b.rs"}, "diff": ""}]}}));
    let call = tool(&map_event(&mut s, &mv)).clone();
    assert_eq!((call.title.as_str(), call.detail.summary().as_str()), ("Rename file", "/w/a.rs → /w/b.rs"));
}

#[test]
fn unknown_notifications_are_logged_not_mapped() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    assert_eq!(map_event(&mut s, &note("thread/teleported", json!({"threadId": "t"}))), []);
    assert!(!log_unknown("codex", "thread/teleported"), "already logged by map_event");
}

#[test]
fn tools_and_compaction_fixtures_settle_every_row() {
    for scenario in ["tools", "tool-error", "compaction"] {
        let calls = last_calls(&replay(scenario));
        assert!(calls.iter().all(|c| c.status != ToolStatus::Running), "{scenario}: {calls:?}");
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib codex::tests`
Expected: FAIL.

- [ ] **Step 3: Implement the two tables.** Update `errors_report_once_and_retries_are_skipped`: the retrying error now gives a Retry notice instead of `[]`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib codex::tests`
Expected: all PASS.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/driver/src/codex.rs crates/driver/tests/unit/codex.rs
git commit -m "feat(codex): review, image, plan and hook items; retry, rate-limit and model notices"
```

---

### Task 12: Codex asks

**Files:**
- Modify: `crates/driver/src/codex.rs`: `server_request`, `reply`, `CodexAsk`, `map_event` (`serverRequest/resolved`)
- Test: `crates/driver/tests/unit/codex.rs`

**Interfaces:**
- Consumes: the phase 1 `CodexAsk` and `State.asks: HashMap<u64, (Value, CodexAsk)>`; `Ask::Questions` and `Ask::Form` (Task 1).
- Produces, as `CodexAsk` variants:
  - `Decision { session: bool, execpolicy: Option<Value>, network: Option<Value> }`
  - `Questions(Vec<String>)` (question ids, in order)
  - `McpApproval`
  - `McpForm(Vec<(String, String)>)` (field key and JSON type)
  - `Permissions(Value)`
- Produces: `fn schema_form(message: &str, schema: &Value) -> (Form, Vec<(String, String)>)`. It maps `requestedSchema.properties`:
  - `string` gives Text; with `enum` (or `oneOf` consts) it gives Choice
  - `number` / `integer` give Number
  - `boolean` gives Bool
  - an `array` whose `items` has an `enum` gives Multi

  Each field takes `title` → label, `description`, `default`, and `required` from the schema's `required` list.

**Mapping:**

| Server request or notification | Maps to |
|---|---|
| `item/commandExecution/requestApproval` | `Approval { tool: "Bash", title: "Run command", input: command_text, kind: Command, cwd, tool_id: itemId, reason, source }`. The reason gains `"Network access to {host}"` when `networkApprovalContext` is set. `source` is the spawn row's agent for a child thread. Options come from `availableDecisions`, or from `["accept", "acceptForSession", "decline"]` when that is absent: `acceptForSession` gives `{id: "session", label: "Allow this command for the session"}`; `{acceptWithExecpolicyAmendment: {execpolicy_amendment: argv}}` gives `{id: "execpolicy", label: "Always allow `{argv joined}`"}`; `{applyNetworkPolicyAmendment: {network_policy_amendment: {host}}}` gives `{id: "network", label: "Always allow network access to {host}"}`. |
| `item/fileChange/requestApproval` | `Approval { tool: "Edit", kind: Edit, input: paths, diff: the item's change diffs joined, tool_id: itemId }`, with options `[{id: "session", label: "Allow edits for the session"}]` |
| `item/tool/requestUserInput` | `Ask::Questions` with every question: `header`, `question`, `options` (label and description), `other: isOther`, `secret: isSecret`, `multi: false` |
| `mcpServer/elicitation/request` with `_meta.codex_approval_kind == "mcp_tool_call"` | phase 1's approval, plus `kind: Mcp`. No options until a recorded fixture pins the persist shape (phase 1 constraint). |
| `mcpServer/elicitation/request` mode `form`, `openai/form` or `openaiForm` | `Ask::Form(schema_form(message, requestedSchema))`, with `source = serverName` |
| `mcpServer/elicitation/request` mode `url` | `Ask::Form { title: "{serverName} needs you to open a link", message, url, fields: [] }`. It replaces phase 1's Approval. |
| `mcpServer/elicitation/request` mode `openai/userVerification` | decline at once (today), plus `Warning("Codex asked for a user verification Aether can't show, so it was declined")` |
| `item/permissions/requestApproval` | phase 1's approval, with options `[{id: "session", label: "Grant for the session"}]` |
| `currentTime/read` | reply `{"currentTimeAt": now in seconds}` |
| any other request (`item/tool/call`, `account/chatgptAuthTokens/refresh`, `attestation/generate`, `applyPatchApproval`, `execCommandApproval`, unknown) | reply `-32601`, plus `Warning("Codex asked for {method}, which Aether doesn't support")` |
| `serverRequest/resolved {requestId}` | `AskResolved(local)` for the ask whose rpc id equals `requestId`, if still pending, then remove it |

**Replies (`reply`):**

| Ask | Answer | Result |
|---|---|---|
| `Decision` | `Approve` / `Reply` | `{"decision": "accept"}` |
| `Decision` | `Pick("session")` | `{"decision": "acceptForSession"}` |
| `Decision` | `Pick("execpolicy")` | `{"decision": {"acceptWithExecpolicyAmendment": {"execpolicy_amendment": stored}}}` |
| `Decision` | `Pick("network")` | `{"decision": {"applyNetworkPolicyAmendment": {"network_policy_amendment": stored}}}` |
| `Decision` | `Deny` | `{"decision": "decline"}` |
| `Decision` | `DenyWith(n)` | `{"decision": "decline"}`, then a `turn/steer` with `n` when a turn is live |
| `Questions(ids)` | `Answers(vals)` | `{"answers": {ids[i]: {"answers": vals[i]}}}` |
| `Questions` | `Deny` | `{"answers": {}}` |
| `McpForm(keys)` | `Form(values)` | `{"action": "accept", "content": {key: typed value}, "_meta": null}` |
| `McpForm` / url `Form` | `Deny` | `{"action": "decline", "content": null, "_meta": null}` |
| url `Form` | `Form(_)` | `{"action": "accept", "content": null, "_meta": null}` |
| `Permissions(req)` | `Pick("session")` / `Approve` | `{"permissions": req, "scope": "session"}` / `"turn"` |

- [ ] **Step 1: Write the failing tests**

```rust
const CMD_APPROVAL: &str = r#"{"method":"item/commandExecution/requestApproval","id":21,"params":{"kind":"command","threadId":"t","turnId":"u","itemId":"exec-1","startedAtMs":1,"command":"/bin/zsh -lc 'pnpm add motion'","cwd":"/w","reason":"Needs network","networkApprovalContext":{"host":"registry.npmjs.org","protocol":"https"},"availableDecisions":["accept",{"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["pnpm","add"]}},{"applyNetworkPolicyAmendment":{"network_policy_amendment":{"host":"registry.npmjs.org","action":"allow"}}},"decline"]}}"#;

#[test]
fn command_approvals_offer_the_clis_own_decisions() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let [Update::Ask { id, ask: Ask::Approval(a) }] = &ev(&mut s, CMD_APPROVAL)[..] else { panic!() };
    assert_eq!((a.kind, a.tool_id.as_deref(), a.cwd.as_deref(), a.input.as_str()), (ApprovalKind::Command, Some("exec-1"), Some("/w"), "pnpm add motion"));
    assert!(a.reason.contains("registry.npmjs.org"));
    let ids: Vec<&str> = a.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["execpolicy", "network"]);
    assert_eq!(reply(&mut s, *id, &Answer::Pick("execpolicy".into())),
        Some(json!({"id": 21, "result": {"decision": {"acceptWithExecpolicyAmendment": {"execpolicy_amendment": ["pnpm", "add"]}}}})));
}

#[test]
fn every_question_is_asked_at_once() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let req = r#"{"method":"item/tool/requestUserInput","id":22,"params":{"threadId":"t","turnId":"u","itemId":"i","isBlocking":true,"questions":[{"id":"q1","header":"DB","question":"Which DB?","isOther":true,"isSecret":false,"options":[{"label":"pg","description":"Postgres"}]},{"id":"q2","header":"Key","question":"API key?","isOther":false,"isSecret":true,"options":null}]}}"#;
    let [Update::Ask { id, ask: Ask::Questions(qs) }] = &ev(&mut s, req)[..] else { panic!() };
    assert_eq!((qs.len(), qs[0].other, qs[1].secret), (2, true, true));
    assert_eq!(reply(&mut s, *id, &Answer::Answers(vec![vec!["pg".into()], vec!["sk-1".into()]])),
        Some(json!({"id": 22, "result": {"answers": {"q1": {"answers": ["pg"]}, "q2": {"answers": ["sk-1"]}}}})));
}

#[test]
fn mcp_form_elicitations_become_forms() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let req = r#"{"method":"mcpServer/elicitation/request","id":23,"params":{"threadId":"t","turnId":"u","serverName":"jira","mode":"form","message":"Pick a project","requestedSchema":{"type":"object","properties":{"project":{"type":"string","enum":["WEB","API"],"title":"Project"},"notify":{"type":"boolean","default":true}},"required":["project"]},"_meta":null}}"#;
    let [Update::Ask { id, ask: Ask::Form(f) }] = &ev(&mut s, req)[..] else { panic!() };
    assert_eq!((f.fields[0].kind, f.fields[0].required, f.fields[1].kind), (FieldKind::Choice, true, FieldKind::Bool));
    assert_eq!(f.source.as_deref(), Some("jira"));
    assert_eq!(reply(&mut s, *id, &Answer::Form(vec![("project".into(), vec!["API".into()]), ("notify".into(), vec!["false".into()])])),
        Some(json!({"id": 23, "result": {"action": "accept", "content": {"project": "API", "notify": false}, "_meta": null}})));
}

#[test]
fn asks_resolved_elsewhere_are_withdrawn() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let [Update::Ask { id, .. }] = &ev(&mut s, CMD_APPROVAL)[..] else { panic!() };
    let resolved = r#"{"method":"serverRequest/resolved","params":{"threadId":"t","requestId":21}}"#;
    assert_eq!(ev(&mut s, resolved), [Update::AskResolved(*id)]);
    assert!(s.asks.is_empty());
}

#[test]
fn unknown_requests_get_an_error_and_a_warning() {
    let mut s = State { active: true, thread: "t".into(), ..State::default() };
    let out = ev(&mut s, r#"{"method":"item/tool/call","id":24,"params":{"threadId":"t","turnId":"u","callId":"c","namespace":null,"tool":"x","arguments":{}}}"#);
    assert!(matches!(&out[..], [Update::Warning(w)] if w.contains("item/tool/call")));
    assert_eq!(s.outbox.last().unwrap()["error"]["code"], -32601);
}

#[test]
fn approval_fixtures_link_cards_to_rows() {
    for scenario in ["approval", "subagent-approval"] {
        let out = replay(scenario);
        let a = out.iter().find_map(|u| match u { Update::Ask { ask: Ask::Approval(a), .. } => Some(a.clone()), _ => None }).expect(scenario);
        assert!(a.tool_id.is_some(), "{scenario}");
        assert!(last_calls(&out).iter().any(|c| Some(&c.id) == a.tool_id.as_ref()), "{scenario}");
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib codex::tests`
Expected: FAIL.

- [ ] **Step 3: Implement the request and reply tables.** Update the phase 1 tests:
  - `url_elicitation_shows_the_link` now expects `Ask::Form` with `url` set.
  - `always_approve_accepts_for_the_session` uses `Pick("session")`.
  - `questions_and_unsupported_requests_are_answered` expects `Ask::Questions` and a Warning.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib codex::tests`
Expected: all PASS.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/driver/src/codex.rs crates/driver/tests/unit/codex.rs
git commit -m "feat(codex): scoped decisions, all questions, mcp forms, resolved asks"
```

---

## OpenCode track (Tasks 13-15, parallelizable)

Shapes come from OpenCode 2.0.32 (`/openapi.json` and the binary's event schemas). Every event's `data` carries `sessionID`. `ev(&mut s, line)` in the tests is the existing helper.

### Task 13: OpenCode child sessions, nested rows, Wake

**Files:**
- Modify: `crates/driver/src/opencode/mod.rs` (`State`)
- Modify: `crates/driver/src/opencode/events.rs` (`map_event`, `tool`, the new `child_event`)
- Modify: `crates/driver/src/opencode/session.rs` (`listen` filter, `start` resync on every resume)
- Test: `crates/driver/tests/unit/opencode/events.rs`, `crates/driver/tests/unit/opencode/session.rs`

**Interfaces:**
- Consumes: `classify` for `subagent`, `child_activity`, `log_unknown` and the notice ids (Task 4); phase 1's `descends` and `is_family`, which retry the parent lookup and cache only definitive answers; `State.root`; `replay` (Task 2).
- Produces, in `State`:
  - `children: HashMap<String, Child>`, keyed by child session id. `struct Child { task: String, agent: String }`: the subagent call id that owns the session, and the agent's name.
  - `background: HashMap<String, ToolCall>`: Task rows that outlive their turn, by call id.
  - `wakes: u32`.
- `listen` handles a frame when either:
  - its sid is the session, or a key of `children`; or
  - it is an `ASK_EVENTS` frame and phase 1's `is_family` holds (unchanged).
- `start` replays `resync` for every resumed session, not only active ones, so pending asks of children show on reopen.

**Mapping (child sessions and nesting):**

| Event | Maps to |
|---|---|
| `session.tool.progress` / `success` of a `subagent` or `task` call with `metadata.sessionID` | insert `children[sessionID] = Child { task: call id, agent: input.agent }` |
| any `session.tool.*` whose sid is a child | the child's row with `parent = Some(children[sid].task)`, plus the Task row updated by `child_activity` (`new` on `input.started`) |
| child `session.step.ended` | Task `tokens += input + output + reasoning`, plus `Update::Cost` of its `cost` |
| child `session.text.ended` | Task `activity` = the text's first line, clipped to 120 chars |
| child `session.execution.failed` | Task row `Failed` with `error` = the error message |
| child `session.text.delta`, `reasoning.*`, `step.started` | nothing (the reply shows only the main session's text) |
| `subagent` success with `metadata.status == "running"` | Task row `ToolStatus::Background`, kept in `background` |
| child `session.execution.succeeded` / `failed` for a call in `background` | that row `Done` / `Failed`, then remove it |
| `execute` success or failure with `metadata.toolCalls[i]` | one row per call: id `"{call}#{i}"`, `parent = call`, name with its `opencode.` prefix stripped and classified with its `input`, `Done` when `status` is `"completed"`, otherwise `Failed` |
| `skill` tool (`input.started`/`called`/`success`) | no row. `Update::Skill(metadata.name or input.id)` on success |
| `session.skill.activated {name}` | `Update::Skill(name)` |
| main `session.execution.started` while `!s.active` | `s.active = true`, `turn += 1`, then `[Update::Wake, Update::Notice(Notice::new(format!("wake-{n}"), Wake, "OpenCode resumed this session on its own"))]` |
| any other event type | `log_unknown("opencode", ty)` |

- [ ] **Step 1: Write the failing tests**

```rust
const SUB_START: &str = r#"{"type":"session.tool.input.started","data":{"sessionID":"ses_1","assistantMessageID":"m","id":"call_t","name":"subagent"}}"#;
const SUB_CALLED: &str = r#"{"type":"session.tool.called","data":{"sessionID":"ses_1","id":"call_t","input":{"agent":"explore","description":"Find styles","prompt":"look in src"},"executed":true}}"#;
const SUB_PROGRESS: &str = r#"{"type":"session.tool.progress","data":{"sessionID":"ses_1","id":"call_t","metadata":{"sessionID":"ses_c","status":"running"}}}"#;
const CHILD_READ: &str = r#"{"type":"session.tool.input.started","data":{"sessionID":"ses_c","assistantMessageID":"m2","id":"call_r","name":"read"}}"#;

fn root() -> State { State { active: true, root: "ses_1".into(), ..State::default() } }

#[test]
fn child_session_tools_nest_under_their_subagent_row() {
    let mut s = root();
    for line in [SUB_START, SUB_CALLED, SUB_PROGRESS] { ev(&mut s, line); }
    let calls = last_calls(&ev(&mut s, CHILD_READ));
    assert!(calls.iter().any(|c| c.id == "call_r" && c.parent.as_deref() == Some("call_t")), "{calls:?}");
    assert!(calls.iter().any(|c| c.id == "call_t" && matches!(&c.detail, ToolDetail::Task { tools: 1, agent, .. } if agent == "explore")));
    let child_text = r#"{"type":"session.text.delta","data":{"sessionID":"ses_c","assistantMessageID":"m2","ordinal":0,"delta":"hi"}}"#;
    assert!(!ev(&mut s, child_text).iter().any(|u| matches!(u, Update::Delta(_))));
}

#[test]
fn background_subagent_stays_live_until_its_session_ends() {
    let mut s = root();
    for line in [SUB_START, SUB_CALLED, SUB_PROGRESS] { ev(&mut s, line); }
    let ok = r#"{"type":"session.tool.success","data":{"sessionID":"ses_1","id":"call_t","content":[{"type":"text","text":"started"}],"metadata":{"sessionID":"ses_c","status":"running"},"executed":true}}"#;
    assert_eq!(tool_status(&ev(&mut s, ok), "call_t"), Some(ToolStatus::Background));
    let done = r#"{"type":"session.execution.succeeded","data":{"sessionID":"ses_c"}}"#;
    assert_eq!(tool_status(&ev(&mut s, done), "call_t"), Some(ToolStatus::Done));
    assert!(s.background.is_empty());
}

#[test]
fn execute_tool_calls_become_nested_rows() {
    let mut s = root();
    let ok = r#"{"type":"session.tool.success","data":{"sessionID":"ses_1","id":"call_x","content":[],"executed":true,"metadata":{"toolCalls":[{"tool":"opencode.read","status":"completed","input":{"filePath":"a.ts"}},{"tool":"opencode.bash","status":"error","input":{"command":"ls"}}]}}}"#;
    ev(&mut s, r#"{"type":"session.tool.input.started","data":{"sessionID":"ses_1","id":"call_x","name":"execute"}}"#);
    let calls = last_calls(&ev(&mut s, ok));
    let nested: Vec<(&str, ToolStatus)> = calls.iter().filter(|c| c.parent.as_deref() == Some("call_x")).map(|c| (c.id.as_str(), c.status)).collect();
    assert_eq!(nested, [("call_x#0", ToolStatus::Done), ("call_x#1", ToolStatus::Failed)]);
}

#[test]
fn skill_tool_is_a_skill_step_not_a_row() {
    let mut s = root();
    let start = r#"{"type":"session.tool.input.started","data":{"sessionID":"ses_1","id":"call_s","name":"skill"}}"#;
    let ok = r#"{"type":"session.tool.success","data":{"sessionID":"ses_1","id":"call_s","content":[],"executed":true,"metadata":{"name":"pdf"}}}"#;
    let out = [ev(&mut s, start), ev(&mut s, ok)].concat();
    assert_eq!(out, [Update::Skill("pdf".into())]);
}

#[test]
fn execution_started_without_a_turn_wakes_once() {
    let mut s = State { root: "ses_1".into(), ..State::default() };
    let started = r#"{"type":"session.execution.started","data":{"sessionID":"ses_1"}}"#;
    assert!(matches!(&ev(&mut s, started)[..], [Update::Wake, Update::Notice(n)] if n.kind == NoticeKind::Wake));
    assert!(s.active);
    assert_eq!(ev(&mut s, started), []);
}

#[test]
fn subagent_fixture_nests_child_rows() {
    let calls = last_calls(&replay("subagent"));
    let task = calls.iter().find(|c| matches!(c.detail, ToolDetail::Task { .. })).expect("a task row");
    assert!(calls.iter().any(|c| c.parent.as_deref() == Some(task.id.as_str())), "{calls:?}");
    assert_eq!(task.status, ToolStatus::Done);
}

#[test]
fn background_fixture_ends_with_a_settled_row() {
    let calls = last_calls(&replay("background"));
    assert!(calls.iter().all(|c| c.status != ToolStatus::Running), "{calls:?}");
}
```

`fn tool_status(out: &[Update], id: &str) -> Option<ToolStatus>` is a local test helper: the last `Update::Tool` with that id.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib opencode`
Expected: FAIL (child rows have no parent, no Background, no Skill, no Wake).

- [ ] **Step 3: Implement the mapping table**
  - `tool()` reads the frame's sid. It sets `parent` when the sid is a child, and it also emits the updated Task row.
  - `listen` reads `children` under the state lock before filtering.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib opencode`
Expected: all PASS, including the phase 1 child-ask tests.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/driver/src/opencode crates/driver/tests/unit/opencode
git commit -m "feat(opencode): nested subagent rows, background subagents, wake"
```

---

### Task 14: OpenCode notices, asks, forms, steers, model options, images

**Files:**
- Modify: `crates/driver/src/opencode/events.rs`, `crates/driver/src/opencode/mod.rs` (`answer`, `steer`, `set_options`, `prompt`, `reports_steered`, `OpencodeAsk`), `crates/driver/src/opencode/session.rs` (`reply` gains a message, `submit` gains files, the new `model_switch`, the `aether.diff` frame in `listen`)
- Modify: `crates/core/src/attachment.rs` (`native_image_split`: OpenCode takes web images natively)
- Test: `crates/driver/tests/unit/opencode/events.rs`, `crates/driver/tests/unit/opencode/session.rs`, `crates/core/tests/unit/attachment.rs`

**Interfaces:**
- Produces:
  - `pub(super) fn decision(answer: &Answer) -> (&'static str, Option<String>)`:
    - `Pick("always")` gives `("always", None)`
    - `Approve`, `Reply` and other `Pick`s give `("once", None)`
    - `DenyWith(n)` gives `("reject", Some(n))`
    - everything else gives `("reject", None)`
  - `reply(api, session, request, decision, message: Option<&str>)` sends `{"decision": …, "message": …}`.
  - `pub(super) fn form_ask(form: &Value, source: Option<String>) -> (Form, Vec<Value>)`: the `Ask::Form` plus the raw fields, kept in `OpencodeAsk::Form { id, fields, session }`.
  - `pub(super) fn form_reply(fields: &[Value], answer: &Answer) -> Value`, which builds `{"answer": {key: value}}`.
  - `pub(super) fn model_switch(model: &str, effort: &str, current: Option<&Value>, default: &Value) -> Option<Value>`. It returns the `/model` body, or `None` when nothing changes.
  - `pub(super) fn prompt_body(text: &str, images: &[Attachment], delivery: Option<&str>) -> Value`, which adds `files: [{"uri": "file://<path>"}]`.
  - `State` gains:
    - `model: Option<Value>`, the last `{providerID, id, variant}` seen
    - `streamed: HashMap<(String, u64), usize>`, delta bytes per (assistantMessageID, ordinal)
    - `steers: HashMap<String, String>`, steer text by inbox id
    - `edited: HashSet<String>`, files with an Edit row this turn
    - `changed: Vec<String>`, step `files`
    - `retry: Option<Notice>`
    - `compactions: u32`
  - `fn reports_steered(&self) -> bool { true }`.

**Mapping:**

| Event or action | Maps to |
|---|---|
| `session.retry.scheduled {attempt, at, error}` | `Notice` Retry, id `retry-<turn>`, `Running`, `until_ms = at`, text `"Retrying (attempt {attempt}): {error.message}"`, kept in `s.retry` |
| next `session.step.started` or text delta after a retry | the same notice, `Done` |
| `session.step.ended {cost, tokens, finish, files}` | `Cost(micros(cost))` plus today's usage; `finish == "length"` gives `Warning("The reply hit the model's output limit")`; `files` extend `changed` |
| `session.step.failed` with `error.type == "provider.content-filter"` | `Warning("The provider's content filter stopped this reply")`, and no `step_error` |
| `session.text.ended {assistantMessageID, ordinal, text}` | `Delta(text[streamed..])` when deltas were missed (`text.len() > streamed`) |
| `session.compaction.started` | `Notice` Compaction, id `compact-<n>` (n = ++`compactions`), `Running`, "Compacting context" |
| `session.compaction.ended {text}` / `.failed {error}` | the open compaction notice, `Done` with `detail = text`, or `Failed` with the error. This replaces the `"compaction"` tool row. |
| `session.model.selected {model, previous}` | `Update::Model("{providerID}/{id}")`; with `previous`, also a ModelChange notice `"Model changed to {id}"` |
| `session.synthetic {text}` | `Notice` Info, id `info-synthetic-<n>`, the text's first line |
| `session.inbox.delivered {inboxID}` in `steers` | `Steered(text)` |
| `session.inbox.cancelled {inboxID}` in `steers` | `SteerRefused(text)` (replaces `Error("Steer was cancelled")`) |
| `permission.asked` | see "Approval" below |
| `permission.replied` / `rejected` / `form.replied` / `form.cancelled` for an ask still in `s.asks` | `AskResolved(local id)`, then remove it |
| `form.created` | `Ask::Form(form_ask(..))`, see "Form" below |
| `session.execution.succeeded` with `changed` files not in `edited` | before it, `listen` GETs `/api/session/{id}/diff` and feeds `{"type":"aether.diff","data":{"files":[…]}}`. Each uncovered file becomes a `Done` Edit row, id `diff-<turn>-<file>`, titled "Changed by command", with its patch. |
| edit/write/patch success with `metadata.files[]` | one Edit row per file: the first keeps the call id, the rest are `"{id}#{i}"`. The patch goes through `apply_patch_text`. A `write` whose file `status == "modified"` is titled "Overwrite file". |
| `session.usage.updated`, `session.step.streamed`, `session.text.started`, `session.reasoning.started/ended`, `session.tool.input.delta/ended`, `session.inbox.enqueued`, `session.agent.selected`, `server.connected` | ignored on purpose, with no log |

**Approval** (`permission.asked`). The fields of `Approval`:
- `input`: `resources.join("\n")`
- `diff`: the joined `metadata.files[].patch`
- `kind`: shell gives Command; edit, write and patch give Edit; webfetch gives Fetch; anything else gives Other
- `tool_id`: `source.id`
- `source`: `children[sid].agent` when sid is not the root
- `reason`: `message`
- `options`: `[ApprovalOption { id: "always", label: format!("Always allow {}", save.join(", ")) }]`, but only when `save` is non-empty and is not exactly `["*"]`. Phase 1 already sends "once" for `["*"]`, which would otherwise allow every later write, and with an empty `save` the server treats "always" as once.

**Form** (`form_ask`). Each raw field maps by `type`:
- `string` with options gives `Choice`, otherwise `Text`
- `number` / `integer` give `Number`
- `boolean` gives `Bool`
- `multiselect` gives `Multi`
- `external` gives `Form.url = url` and adds no field

Fields with `hidden: true` are left out. Each `when[]` rule maps to `When { key, not: op == "neq", value }`, with `value` rendered as a string. `custom` maps to `other`. `default` maps to `Vec<String>`.

`form_reply` builds each value by the raw field's type:
- string: the first value
- number/integer: the parsed number
- boolean: `== "true"`
- multiselect: an array

Hidden fields send their `default`. Fields hidden by `when` are omitted.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn retry_is_a_countdown_notice_that_closes_on_progress() {
    let mut s = root();
    let retry = r#"{"type":"session.retry.scheduled","data":{"sessionID":"ses_1","assistantMessageID":"m","attempt":2,"at":1800000000000,"error":{"type":"provider.rate-limit","message":"overloaded"}}}"#;
    let [Update::Notice(n)] = &ev(&mut s, retry)[..] else { panic!() };
    assert_eq!((n.kind, n.state, n.until_ms), (NoticeKind::Retry, NoticeState::Running, Some(1_800_000_000_000)));
    assert!(n.text.contains("attempt 2") && n.text.contains("overloaded"));
    let step = r#"{"type":"session.step.started","data":{"sessionID":"ses_1","assistantMessageID":"m","agent":"build","model":{"id":"x","providerID":"p"},"started":1}}"#;
    assert!(ev(&mut s, step).iter().any(|u| matches!(u, Update::Notice(d) if d.id == n.id && d.state == NoticeState::Done)));
}

#[test]
fn step_cost_length_and_content_filter_are_not_errors() {
    let mut s = root();
    let ended = r#"{"type":"session.step.ended","data":{"sessionID":"ses_1","assistantMessageID":"m","finish":"length","cost":0.0123,"tokens":{"input":1,"output":1,"reasoning":0,"cache":{"read":0,"write":0}}}}"#;
    let out = ev(&mut s, ended);
    assert!(out.contains(&Update::Cost(12_300)));
    assert!(out.iter().any(|u| matches!(u, Update::Warning(_))));
    let filtered = r#"{"type":"session.step.failed","data":{"sessionID":"ses_1","assistantMessageID":"m","finish":"content-filter","error":{"type":"provider.content-filter","message":"blocked"}}}"#;
    assert!(ev(&mut s, filtered).iter().any(|u| matches!(u, Update::Warning(_))));
    let ok = r#"{"type":"session.execution.succeeded","data":{"sessionID":"ses_1"}}"#;
    assert!(!ev(&mut s, ok).iter().any(|u| matches!(u, Update::Error(_))));
}

#[test]
fn text_ended_repairs_missed_deltas() {
    let mut s = root();
    ev(&mut s, r#"{"type":"session.text.delta","data":{"sessionID":"ses_1","assistantMessageID":"m","ordinal":0,"delta":"Hel"}}"#);
    let ended = r#"{"type":"session.text.ended","data":{"sessionID":"ses_1","assistantMessageID":"m","ordinal":0,"text":"Hello"}}"#;
    assert_eq!(ev(&mut s, ended), [Update::Delta("lo".into())]);
}

#[test]
fn compactions_get_unique_notice_ids_and_a_summary() {
    let mut s = root();
    let started = r#"{"type":"session.compaction.started","data":{"sessionID":"ses_1","reason":"manual","recent":1}}"#;
    let ended = r#"{"type":"session.compaction.ended","data":{"sessionID":"ses_1","reason":"manual","text":"Summary.","recent":1}}"#;
    let ids: Vec<String> = [started, ended, started, ended].iter().flat_map(|l| ev(&mut s, l)).filter_map(|u| match u { Update::Notice(n) => Some(n.id), _ => None }).collect();
    assert_eq!(ids.len(), 4);
    assert_eq!(ids[0], ids[1]);
    assert_ne!(ids[0], ids[2]);
}

#[test]
fn approvals_offer_the_save_patterns_and_resolve_elsewhere() {
    let mut s = root();
    let asked = r#"{"type":"permission.asked","data":{"id":"per_1","sessionID":"ses_1","action":"shell","resources":["ls src"],"save":["ls *"],"message":"Inspect","source":{"type":"tool","messageID":"m","id":"call_b"}}}"#;
    let [Update::Ask { id, ask: Ask::Approval(a) }] = &ev(&mut s, asked)[..] else { panic!() };
    assert_eq!((a.kind, a.tool_id.as_deref(), a.reason.as_str()), (ApprovalKind::Command, Some("call_b"), "Inspect"));
    assert_eq!(a.options, [ApprovalOption { id: "always".into(), label: "Always allow ls *".into() }]);
    assert_eq!(decision(&Answer::Pick("always".into())), ("always", None));
    assert_eq!(decision(&Answer::DenyWith("use yarn".into())), ("reject", Some("use yarn".into())));
    let replied = r#"{"type":"permission.replied","data":{"sessionID":"ses_1","requestID":"per_1","reply":"once"}}"#;
    assert_eq!(ev(&mut s, replied), [Update::AskResolved(*id)]);
    let write = r#"{"type":"permission.asked","data":{"id":"per_2","sessionID":"ses_1","action":"edit","resources":["a.txt"],"save":["*"]}}"#;
    assert!(matches!(&ev(&mut s, write)[..], [Update::Ask { ask: Ask::Approval(a), .. }] if a.options.is_empty()));
}

#[test]
fn hidden_fields_reply_with_their_defaults() {
    let form = json!({"id":"frm_1","sessionID":"ses_1","title":"Deploy","fields":[
        {"key":"env","type":"string","title":"Env","options":[{"value":"prod","label":"Production"}],"required":true},
        {"key":"token","type":"string","hidden":true,"default":"abc"},
        {"key":"why","type":"string","title":"Why","when":[{"key":"env","op":"eq","value":"prod"}]},
        {"key":"n","type":"integer","default":2},
        {"key":"tags","type":"multiselect","options":[{"value":"a","label":"A"}],"custom":true},
        {"key":"docs","type":"external","url":"https://docs.test"}]});
    let (ask, raw) = form_ask(&form, None);
    assert_eq!(ask.url.as_deref(), Some("https://docs.test"));
    let keys: Vec<&str> = ask.fields.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["env", "why", "n", "tags"]);
    assert_eq!((ask.fields[0].kind, ask.fields[3].kind, ask.fields[3].other), (FieldKind::Choice, FieldKind::Multi, true));
    assert_eq!(ask.fields[1].when, [When { key: "env".into(), not: false, value: "prod".into() }]);
    let answer = Answer::Form(vec![("env".into(), vec!["prod".into()]), ("why".into(), vec!["hotfix".into()]),
        ("n".into(), vec!["3".into()]), ("tags".into(), vec!["a".into(), "b".into()])]);
    assert_eq!(form_reply(&raw, &answer), json!({"answer": {"env": "prod", "token": "abc", "why": "hotfix", "n": 3, "tags": ["a", "b"]}}));
}

#[test]
fn steers_report_delivery_and_refusal() {
    let mut s = root();
    s.steers.insert("inb_1".into(), "also dark mode".into());
    s.steers.insert("inb_2".into(), "never mind".into());
    assert_eq!(ev(&mut s, r#"{"type":"session.inbox.delivered","data":{"sessionID":"ses_1","inboxID":"inb_1"}}"#), [Update::Steered("also dark mode".into())]);
    assert_eq!(ev(&mut s, r#"{"type":"session.inbox.cancelled","data":{"sessionID":"ses_1","inboxID":"inb_2"}}"#), [Update::SteerRefused("never mind".into())]);
}
```

`crates/driver/tests/unit/opencode/session.rs`:

```rust
#[test]
fn model_switch_resets_to_the_default_and_applies_effort() {
    let default = json!({"providerID": "opencode", "id": "big"});
    let current = json!({"providerID": "openrouter", "id": "glm"});
    assert_eq!(model_switch("", "", Some(&current), &default), Some(json!({"model": {"providerID": "opencode", "id": "big"}})));
    assert_eq!(model_switch("openrouter/glm", "high", Some(&current), &default), Some(json!({"model": {"providerID": "openrouter", "id": "glm", "variant": "high"}})));
    assert_eq!(model_switch("openrouter/glm", "", Some(&current), &default), None);
}

#[test]
fn prompts_send_images_as_file_uris() {
    let img = Attachment { name: "a.png".into(), path: "/c/a.png".into(), mime: "image/png".into(), size: 1 };
    assert_eq!(prompt_body("look", &[img], Some("steer")), json!({"text": "look", "files": [{"uri": "file:///c/a.png"}], "delivery": "steer"}));
}
```

`crates/core/tests/unit/attachment.rs`: assert that `native_image_split(Provider::OpenCode, &[png])` puts the PNG in the native half.

Also add a fixture test:

```rust
#[test]
fn approval_fixtures_link_cards_to_rows() {
    for scenario in ["approval", "subagent-approval"] {
        let out = replay(scenario);
        let ask = out.iter().find_map(|u| match u { Update::Ask { ask: Ask::Approval(a), .. } => Some(a.clone()), _ => None }).expect(scenario);
        assert!(ask.tool_id.is_some(), "{scenario}");
        if scenario == "subagent-approval" { assert!(ask.source.is_some()); }
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib opencode && cargo test -p aether-core --lib attachment`
Expected: FAIL or compile errors.

- [ ] **Step 3: Implement the mapping**
  - `answer` dispatches on `OpencodeAsk`:
    - A `Permission` uses `decision`. When it rejects, it calls `deny(&mut s, &request)`.
    - A `Form` sends `form_reply` for `Answer::Form`. `Deny` sends `cancel_form`, as today.
  - The one-field-at-a-time `ask_form`/`form_value` path is deleted.
  - `steer` posts `prompt_body(text, images, Some("steer"))`, records `steers[inbox id] = without_attached_files(text)`, and on an HTTP error sends `SteerRefused(text)` plus `Warning(e)`.
  - `set_options` uses `model_switch`. The default comes from `GET /api/model/default` (`data.providerID`, `data.id`). A failure is a `Warning`.
  - Every other recoverable HTTP failure (auto-reply, reply, cancel_form) is a `Warning`, not an `Error`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib opencode && cargo test -p aether-core --lib`
Expected: all PASS. Update these older tests:
- Rename `question_forms_ask_one_field_at_a_time` to `question_forms_are_one_card`, with the single-card shape.
- Move phase 1's `always_is_sent_to_the_server_not_remembered_per_action` to the tuple `decision`.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/driver/src/opencode crates/driver/tests/unit/opencode crates/core/src/attachment.rs crates/core/tests/unit/attachment.rs
git commit -m "feat(opencode): notices, scoped always, full forms, steer delivery, model reset"
```

---

### Task 15: OpenCode service lifecycle and startup steps

**Files:**
- Modify: `crates/driver/src/opencode/session.rs` (`listen` reconnect, `start` startup notices, Mode and Model)
- Modify: `crates/driver/src/opencode/service.rs` (`connect`: stderr to a log file, `service_log_tail`)
- Modify: `crates/driver/src/opencode/events.rs` (`session.execution.interrupted`)
- Test: `crates/driver/tests/unit/opencode/events.rs`, `crates/driver/tests/unit/opencode/service.rs`

**Interfaces:**
- Produces:
  - `pub(super) const RECONNECT_DEADLINE: Duration = Duration::from_secs(30)`.
  - `pub(super) fn lost_service(s: &mut State, log_tail: &str) -> Vec<Update>`. It returns `Error(format!("OpenCode service stopped responding\n{log_tail}").trim_end())` plus `settle(s)`, sets `s.dead`, and returns `[]` when not active.
  - `pub(super) fn service_log() -> Option<PathBuf>`: `config_dir()/logs/opencode-service.log`. `connect` opens it in append mode for the spawned service's stdout and stderr. A service Aether didn't start has no log.
  - `pub(super) fn service_log_tail(lines: usize) -> String`: the last `lines` lines of that log, or empty.
- **`session.execution.interrupted` by reason:**
  - `"shutdown"`, when not dismissed: `Notice` Info, id `info-service`, "OpenCode's service stopped. This turn resumes when it restarts." The turn stays active, and the reconnect deadline applies.
  - `"inactivity"` or `"superseded"`: `Warning(format!("OpenCode stopped this turn ({reason})"))` plus `settle`.
  - `"user"`: `settle`, as today.
- **Startup notices** (Task 4 ids):
  - `startup-spawn`: "Starting OpenCode service" (Running), then "Service ready" (Done). When an existing service is adopted, a single Done notice "Service ready".
  - `startup-session`: "Creating session" or "Resuming session", Running then Done.
- **Mode and Model.**
  - After the rules are applied (create or PATCH), send `Update::Mode(opts.access)`.
  - From the session info's `model`, send `Update::Model("{providerID}/{id}")`, and seed `s.model`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn shutdown_keeps_the_turn_waiting_with_a_notice() {
    let mut s = root();
    let shutdown = r#"{"type":"session.execution.interrupted","data":{"sessionID":"ses_1","reason":"shutdown"}}"#;
    assert!(matches!(&ev(&mut s, shutdown)[..], [Update::Notice(n)] if n.id == "info-service"));
    assert!(s.active);
    let idle = r#"{"type":"session.execution.interrupted","data":{"sessionID":"ses_1","reason":"inactivity"}}"#;
    assert!(matches!(&ev(&mut s, idle)[..], [Update::Warning(_), Update::Final(_)]));
}

#[test]
fn a_lost_service_fails_the_turn_with_its_log() {
    let mut s = root();
    let out = lost_service(&mut s, "panic: db locked");
    assert!(matches!(&out[..], [Update::Error(e), Update::Final(_)] if e == "OpenCode service stopped responding\npanic: db locked"));
    assert!(s.dead.is_some());
    assert_eq!(lost_service(&mut s, ""), []);
}
```

`crates/driver/tests/unit/opencode/service.rs`: assert that `service_log()` ends with `logs/opencode-service.log`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib opencode`
Expected: FAIL.

- [ ] **Step 3: Implement**
  - The reconnect loop in `listen` tracks the time since the stream dropped. Past `RECONNECT_DEADLINE`, it sends `lost_service(&mut s, &service_log_tail(20))` and returns.
  - `connect` makes the logs dir and points the service's stdout and stderr at `service_log()`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib opencode`
Expected: all PASS. Rewrite the old shutdown assertion in `text_delta_then_succeeded_settles_once` to expect the notice.

- [ ] **Step 5: Manual check (live service)**

Start an OpenCode chat with a long command. Quit the service (`pkill -f "opencode serve"`) mid-turn. Expected:
- The "service stopped" notice shows.
- After 30 s, a fatal card reads "OpenCode service stopped responding", with the log tail as its details.

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/driver/src/opencode crates/driver/tests/unit/opencode
git commit -m "feat(opencode): reconnect deadline, service log, startup steps"
```

---

## Pi track (Tasks 16-17, parallelizable)

Shapes come from Pi 1.3.0: `docs/rpc*.md`, `docs/message-types.md`, `dist/modes/rpc/rpc-types.d.ts` and `docs/security.md`. `ev(&mut s, line)` in the tests is the existing helper.

### Task 16: Pi steering, cancel and dialogs, Wake, stderr, launch, trust

**Files:**
- Modify: `crates/driver/src/pi.rs` (`State`, `spawn`, `handshake`, `steer`, `cancel`, `map_event`, `launch_args`, `reports_steered`, `notice_action`)
- Test: `crates/driver/tests/unit/pi.rs`

**Interfaces:**
- Consumes: `StderrTail`, `log_unknown`, the notice ids and `notice_action` (Task 4); `Ask::Form` (Task 1).
- Produces:
  - `State` gains:
    - `steers: VecDeque<String>`, the pending steer texts, without the attached-files block
    - `clearing: String`, the id of the in-flight `clear_queue`
    - `wakes: u32`
    - `stderr: StderrTail`
  - `fn reports_steered(&self) -> bool { true }`.
  - `fn cancel_lines(s: &mut State, clear_id: &str) -> (Vec<Value>, Vec<Update>)`. It is called by `cancel` after Task 2's `on_cancel`. It returns the lines to write, in order:
    1. one `{"type":"extension_ui_response","id":…,"cancelled":true}` per open dialog
    2. `{"id": clear_id, "type":"clear_queue"}`
    3. `{"type":"abort"}`

    The updates are one `AskResolved(local)` per closed dialog.
  - `fn expire_dialog(s: &mut State, local: u64) -> Option<Update>`: `AskResolved(local)` if that ask is still open. When a dialog has a `timeout`, the stdout thread arms a timer thread that calls it after `timeout` ms. Pi tells the host nothing when a dialog times out.
  - `fn stderr_update(line: &str) -> Option<Update>`: `Warning(line)` for lines that start with `Warning:` or `Error:`, or that pass `visible_stderr`. Every non-blank line is also pushed into `s.stderr`.
  - `fn launch_args(opts: &DriverStartOptions, approve: bool) -> Vec<String>`:
    - adds `--session <opts.resume>` when `opts.resume` is an existing file path
    - adds `--approve` when `approve`
    - `switch_session` and `PI_SKIP_VERSION_CHECK` are removed. Task 21 owns the version check that replaces the latter.
  - `fn untrusted_project(cwd: &Path, trust: &Value, default_trust: &str) -> bool`. It is true when:
    - a protected resource exists (`.pi/settings.json`, `.pi/mcp.json`, `.pi/extensions`, `.pi/skills`, `.pi/prompts`, `.pi/themes`, `.pi/SYSTEM.md`, `.pi/APPEND_SYSTEM.md` in `cwd`, or `.agents/skills` in `cwd` or an ancestor), and
    - the closest saved decision in `trust` (`~/.pi/agent/trust.json`, an object from canonical dir to bool) for `cwd` or an ancestor is not `true`, and
    - `default_trust` (`defaultProjectTrust` in `~/.pi/agent/settings.json`, default `"ask"`) is not `"always"`.

**Mapping:**

| Event or action | Maps to |
|---|---|
| `steer(text)` | push `without_attached_files(text)` onto `steers`. No `Wake`, and no `s.text.clear()`. |
| `message_start` with `message.role == "user"`, its text equal to `steers.front()` | pop it, then `Steered(text)` |
| `queue_update {steering}` | each pending steer that is in neither `steering` nor already Steered gives `Steered(text)`, in order |
| `response` with `id == s.clearing` | `SteerRefused(t)` for each `data.steering[i]`, then drop those texts from `steers` |
| `agent_start` while `!s.active` | `active = true`, `turn += 1`, then `[Wake, Notice(wake-<n>, Wake, "Pi started a run on its own")]` |
| `extension_error {extensionPath, error}` | `Warning("{path}: {error}")` (was `Error`) |
| stdout ends | `Error(s.stderr.fatal("pi exited"))` when a turn is active |
| handshake fails | `dead = s.stderr.fatal(&format!("pi didn't start: {e}"))` |
| after the handshake, `untrusted_project(..)` and not `approve` | `Notice::new("info-trust", Info, "Pi skipped this project's extensions, skills and settings because the project isn't trusted.")` with `url = Some("aether:trust-project")` |
| `notice_action("trust-project")` | `approve = true`. Respawn now with `--approve` and `--session <current session file>` when idle; otherwise at the next `prompt`. |
| after the handshake | `Mode(AccessMode::Full)` when `opts.access == Full`, else `Mode(AccessMode::ReadOnly)`, plus `Notice::new("info-readonly", Info, "Read only is a tool allowlist. Extension commands can still change files.")` |
| startup | `startup-spawn` "Starting pi" (Running, then Done after `get_state`); with a resume, `startup-session` "Loading session" |

- [ ] **Step 1: Write the failing tests**

```rust
fn user_start(text: &str) -> String {
    json!({"type":"message_start","message":{"role":"user","content":[{"type":"text","text":text}]}}).to_string()
}

#[test]
fn steers_are_marked_when_pi_takes_them() {
    let mut s = State { active: true, ..State::default() };
    on_steer(&mut s, "also dark mode");
    on_steer(&mut s, "and tests");
    assert_eq!(ev(&mut s, &user_start("also dark mode")), [Update::Steered("also dark mode".into())]);
    assert_eq!(ev(&mut s, r#"{"type":"queue_update","steering":[],"followUp":[]}"#), [Update::Steered("and tests".into())]);
    assert_eq!(ev(&mut s, &user_start("and tests")), []);
}

#[test]
fn cancel_closes_open_dialogs_first() {
    let mut s = State { active: true, ..State::default() };
    let editor = r#"{"type":"extension_ui_request","id":"ui-1","method":"editor","title":"Edit","prefill":"x"}"#;
    let [Update::Ask { id, .. }] = ev(&mut s, editor)[..] else { panic!() };
    let (lines, updates) = cancel_lines(&mut s, "aether-9");
    let kinds: Vec<&str> = lines.iter().map(|l| l["type"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["extension_ui_response", "clear_queue", "abort"]);
    assert_eq!(lines[0], json!({"type":"extension_ui_response","id":"ui-1","cancelled":true}));
    assert_eq!(updates, [Update::AskResolved(id)]);
}

#[test]
fn cleared_steers_return_to_the_queue() {
    let mut s = State { active: true, clearing: "aether-9".into(), ..State::default() };
    on_steer(&mut s, "also x");
    let cleared = r#"{"type":"response","id":"aether-9","command":"clear_queue","success":true,"data":{"steering":["also x"],"followUp":[]}}"#;
    assert_eq!(ev(&mut s, cleared), [Update::SteerRefused("also x".into())]);
    assert!(s.steers.is_empty());
}

#[test]
fn dialog_timeouts_resolve_the_ask() {
    let mut s = State { active: true, ..State::default() };
    let select = r#"{"type":"extension_ui_request","id":"ui-2","method":"select","title":"Pick","options":["a","b"],"timeout":5000}"#;
    let [Update::Ask { id, ask: Ask::Form(f) }] = &ev(&mut s, select)[..] else { panic!() };
    assert_eq!(f.timeout_ms, Some(5000));
    assert_eq!(expire_dialog(&mut s, *id), Some(Update::AskResolved(*id)));
    assert_eq!(expire_dialog(&mut s, *id), None);
}

#[test]
fn runs_pi_starts_alone_wake_the_chat() {
    let mut s = State::default();
    assert!(matches!(&ev(&mut s, r#"{"type":"agent_start"}"#)[..], [Update::Wake, Update::Notice(n)] if n.kind == NoticeKind::Wake));
    assert!(s.active);
}

#[test]
fn stderr_warns_instead_of_failing() {
    assert_eq!(stderr_update("Warning: Extension \"x\" project_trust error: boom"), Some(Update::Warning("Warning: Extension \"x\" project_trust error: boom".into())));
    assert_eq!(stderr_update("debug: cache warm"), None);
}

#[test]
fn launch_resumes_by_session_file_and_can_approve() {
    let file = std::env::temp_dir().join(format!("aether-pi-{}.jsonl", std::process::id()));
    std::fs::write(&file, "").unwrap();
    let opts = DriverStartOptions { resume: file.to_string_lossy().into(), ..opts_for_test() };
    let args = launch_args(&opts, true);
    assert!(args.windows(2).any(|w| w == ["--session", opts.resume.as_str()]));
    assert!(args.contains(&"--approve".to_string()));
    assert!(!launch_args(&DriverStartOptions { resume: "/nope.jsonl".into(), ..opts }, false).iter().any(|a| a == "--session" || a == "--approve"));
    std::fs::remove_file(file).unwrap();
}

#[test]
fn untrusted_projects_offer_trust() {
    let dir = std::env::temp_dir().join(format!("aether-pi-trust-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".pi")).unwrap();
    assert!(!untrusted_project(&dir, &json!({}), "ask"), "a bare .pi needs no trust");
    std::fs::create_dir_all(dir.join(".pi/extensions")).unwrap();
    assert!(untrusted_project(&dir, &json!({}), "ask"));
    let canonical = std::fs::canonicalize(&dir).unwrap().to_string_lossy().into_owned();
    assert!(!untrusted_project(&dir, &json!({ canonical: true }), "ask"));
    assert!(!untrusted_project(&dir, &json!({}), "always"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn steer_fixture_marks_the_steer() {
    assert!(replay("steer").iter().any(|u| matches!(u, Update::Steered(_))));
}
```

`opts_for_test()` is the existing option builder in this test file. If there is none, add it next to `read_only_args_and_model_split`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib pi::tests`
Expected: compile errors, then FAIL.

- [ ] **Step 3: Implement the mapping.**
  - `cancel` writes the `cancel_lines` lines and sends its updates.
  - Its 10 s watchdog now also kills the child, and sends `Error(s.stderr.fatal("pi did not stop after cancel"))` plus `Final("")`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib pi::tests`
Expected: all PASS. Rewrite `dialogs_ask_and_replies_echo_request_id` and `handled_prompt_waits_for_a_run_an_extension_starts` to the new shapes (`Ask::Form`, and a `Wake` for a late run).

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/driver/src/pi.rs crates/driver/tests/unit/pi.rs
git commit -m "feat(pi): steer delivery, safe cancel, dialog timeouts, wake, trust notice"
```

---

### Task 17: Pi tools, results, notices, extension UI, commands

**Files:**
- Modify: `crates/driver/src/pi.rs` (`map_event`, `reply`, `prompt_cmd`, the new `command`)
- Test: `crates/driver/tests/unit/pi.rs`

**Interfaces:**
- Consumes: `save_images`, `now_ms`, the notice ids (Task 4); `DriverStartOptions.attachments`. `State` gains `attachments: Option<PathBuf>`, set in `spawn`.
- Produces:
  - `State` gains:
    - `args: HashMap<u64, (String, String)>`: content index to (tool call id, streamed argument JSON)
    - `retry: Option<Notice>`
    - `compactions: u32`
    - `served: String`, the last model reported
    - `effort: String`, the requested level
  - `enum PiCommand { Rpc(Value), TuiOnly(String) }` and `fn command(text: &str, id: &str) -> Option<PiCommand>`. `None` means a normal prompt.
  - `reply` maps `Answer` onto dialogs:
    - select or input or editor with `Answer::Form([("value", [v])])` gives `{"value": v}`
    - confirm with `Answer::Form(_)` gives `{"confirmed": true}`
    - `Deny` gives `{"cancelled": true}`, or `{"confirmed": false}` for a confirm

**Mapping:**

| Event | Maps to |
|---|---|
| `message_update` `toolcall_start {contentIndex, id, toolName}` | a Running row `start(id, toolName, Null)`, plus `args[contentIndex] = (id, "")` |
| `toolcall_delta {contentIndex, delta}` | append to the args. When they parse as JSON, re-classify and send the row. |
| `toolcall_end {toolCall}` | re-classify with `toolCall.arguments` |
| `tool_execution_start/update/end` with `parentToolCallId` | `parent = Some(parentToolCallId)` |
| `tool_execution_end` `result.details.patch` | `apply_patch_text(&mut call.detail, patch)` |
| `result.details.truncation.truncated == true` | output gains `"\n… Showing {outputLines} of {totalLines} lines. Full output: {fullOutputPath}"`, without the last part when there is no path |
| `result.content` image blocks | `call.images = save_images(content, s.attachments.as_deref())` |
| `message_end` (assistant) `usage.cost.total` | `Cost(micros(total))` when > 0 |
| `message_end` `stopReason == "length"` | `Warning("The reply hit the model's output limit")` |
| `message_end` `responseModel` or `model` differs from `served` | `Model(it)`, then store it |
| `message_end` role `bashExecution {command, output, exitCode}` | a `Done` (or `Failed` on a non-zero exit) Command row, id `bash-<n>` |
| `message_end` role `custom` with `display: true` | `Notice` Info, id `info-custom-<n>`, the content's first text line |
| `auto_retry_start {attempt, maxAttempts, delayMs, errorMessage}` | Retry, id `retry-<turn>`, Running, `until_ms = now_ms() + delayMs`, text `"Retrying ({attempt}/{maxAttempts}): {errorMessage}"` |
| `auto_retry_end {success, finalError}` | the same id, `Done`, or `Failed` with `finalError` |
| `summarization_retry_scheduled {…}` / `summarization_retry_finished` | Retry, id `retry-summary-<turn>`, Running, then Done |
| `compaction_start {reason}` | Compaction, id `compact-<n>`, Running, `"Compacting context ({reason})"` (replaces the `compaction` tool row) |
| `compaction_end {result, aborted, errorMessage}` | Done "Compacted {tokensBefore} → ~{estimatedTokensAfter} tokens" with `detail = result.summary`; Done "Compaction stopped" when aborted; Failed with the error. `Usage(estimatedTokensAfter)` as today. |
| `thinking_level_changed {level}` with level ≠ `effort` | `Notice::new("info-thinking", Info, "Pi is using thinking level {level}. {effort} isn't available for this model.")` |
| `extension_ui_request` `select {title, options, timeout}` | `Ask::Form { title, fields: [Field { key: "value", kind: Choice, options }], timeout_ms }` |
| `confirm {title, message, timeout}` | `Ask::Form { title, message, fields: [], timeout_ms }`. Submit confirms and Cancel declines. |
| `input {title, placeholder, timeout}` | a `Form` with one Text field `"value"`, labelled with the placeholder |
| `editor {title, prefill}` | a `Form` with one Text field `"value"`, `default = [prefill]` |
| `notify {message, notifyType}` | `error` gives `Warning(message)`. A message with an `http(s)://` URL gives `Notice` McpAuth, id `mcp-<url host>`, `url` set. Otherwise `Notice` Info, id `info-notify-<n>`. |
| `setStatus {statusKey, statusText}` | `Notice` Info, id `info-status-<key>`, text = statusText, or `""` when cleared (views skip empty notices) |
| `setWidget {widgetKey, widgetLines}` | `Notice` Info, id `info-widget-<key>`, text = the first line, `detail` = all lines |
| `setTitle {title}` | `Title(title)` |
| `set_editor_text`, `entry_appended`, `turn_start`, `turn_end`, `agent_end`, `summarization_retry_attempt_start` | ignored on purpose (`set_editor_text` and `entry_appended` go through `log_unknown`, since nothing shows them) |

**Commands** (`command`):

| Typed | Action |
|---|---|
| `/compact [text]` | `compact` (today) |
| `/model <provider/id>` | `set_model`, then `Model(id)` and `Final("")` |
| `/thinking <level>` | `set_thinking_level`, then `Final("")` |
| `/name <name>` | `set_session_name`, then `Title(name)` and `Final("")` |
| `/session` | `get_session_stats`, then `Notice` Info (`info-session`) with the stats as `detail`, and `Final("")` |
| `/export [path]` | `export_html`, then `Notice` Info "Exported to {path}" and `Final("")` |
| any other built-in (`settings`, `scoped-models`, `login`, `logout`, `llama`, `share`, `bug`, `trust`, `reload`, `hotkeys`, `changelog`, `tree`, `fork`, `clone`, `resume`, `new`, `import`, `copy`, `quit`) | `TuiOnly`: `Warning("`/{cmd}` only works in Pi's terminal")` then `Final("")`, sent to nothing |

- [ ] **Step 1: Write the failing tests**

```rust
fn upd(event: Value) -> String { json!({"type":"message_update","assistantMessageEvent": event}).to_string() }

#[test]
fn tool_input_streams_before_execution() {
    let mut s = State { active: true, ..State::default() };
    ev(&mut s, &upd(json!({"type":"toolcall_start","contentIndex":1,"id":"tc1","toolName":"bash"})));
    ev(&mut s, &upd(json!({"type":"toolcall_delta","contentIndex":1,"delta":"{\"command\":\"ls"})));
    let out = ev(&mut s, &upd(json!({"type":"toolcall_delta","contentIndex":1,"delta":" -la\"}"})));
    assert!(matches!(&last_calls(&out)[..], [c] if matches!(&c.detail, ToolDetail::Command { command, .. } if command == "ls -la")));
}

#[test]
fn details_parents_and_images_are_kept() {
    let mut s = State { active: true, ..State::default() };
    let end = json!({"type":"tool_execution_end","toolCallId":"p/1","toolName":"edit","parentToolCallId":"p","isError":false,
        "result":{"content":[{"type":"text","text":"ok"}],"details":{"patch":"--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b"}}}).to_string();
    let [Update::Tool(c)] = &ev(&mut s, &end)[..] else { panic!() };
    assert_eq!(c.parent.as_deref(), Some("p"));
    assert!(matches!(&c.detail, ToolDetail::Edit { added: 1, removed: 1, .. }));
    let bash = json!({"type":"tool_execution_end","toolCallId":"b","toolName":"bash","isError":false,
        "result":{"content":[{"type":"text","text":"tail"}],"details":{"truncation":{"truncated":true,"outputLines":2000,"totalLines":9000},"fullOutputPath":"/tmp/pi-out.txt"}}}).to_string();
    let [Update::Tool(c)] = &ev(&mut s, &bash)[..] else { panic!() };
    assert!(matches!(&c.detail, ToolDetail::Command { output, .. } if output.ends_with("Showing 2000 of 9000 lines. Full output: /tmp/pi-out.txt")));
}

#[test]
fn message_end_reports_cost_length_and_model() {
    let mut s = State { active: true, ..State::default() };
    let end = json!({"type":"message_end","message":{"role":"assistant","content":[],"model":"glm-5","stopReason":"length",
        "usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"totalTokens":2,"cost":{"total":0.0042}}}}).to_string();
    let out = ev(&mut s, &end);
    assert!(out.contains(&Update::Cost(4_200)) && out.contains(&Update::Model("glm-5".into())));
    assert!(out.iter().any(|u| matches!(u, Update::Warning(_))));
}

#[test]
fn retries_and_compaction_are_notices() {
    let mut s = State { active: true, ..State::default() };
    let [Update::Notice(r)] = &ev(&mut s, r#"{"type":"auto_retry_start","attempt":1,"maxAttempts":3,"delayMs":2000,"errorMessage":"overloaded"}"#)[..] else { panic!() };
    assert_eq!((r.kind, r.state, r.until_ms.is_some()), (NoticeKind::Retry, NoticeState::Running, true));
    assert!(matches!(&ev(&mut s, r#"{"type":"auto_retry_end","success":true,"attempt":1}"#)[..], [Update::Notice(d)] if d.id == r.id && d.state == NoticeState::Done));
    ev(&mut s, r#"{"type":"compaction_start","reason":"manual"}"#);
    let out = ev(&mut s, r#"{"type":"compaction_end","reason":"manual","aborted":false,"willRetry":false,"result":{"summary":"Did X.","tokensBefore":90000,"estimatedTokensAfter":12000}}"#);
    assert!(out.iter().any(|u| matches!(u, Update::Notice(n) if n.kind == NoticeKind::Compaction && n.detail.as_deref() == Some("Did X."))));
}

#[test]
fn extension_ui_feeds_notices_titles_and_forms() {
    let mut s = State { active: true, ..State::default() };
    let notify = r#"{"type":"extension_ui_request","id":"u1","method":"notify","message":"Sign in to Linear: https://linear.app/oauth?x=1","notifyType":"info"}"#;
    assert!(matches!(&ev(&mut s, notify)[..], [Update::Notice(n)] if n.kind == NoticeKind::McpAuth && n.url.as_deref() == Some("https://linear.app/oauth?x=1")));
    assert_eq!(ev(&mut s, r#"{"type":"extension_ui_request","id":"u2","method":"setTitle","title":"Fix login"}"#), [Update::Title("Fix login".into())]);
    let status = r#"{"type":"extension_ui_request","id":"u3","method":"setStatus","statusKey":"mcp","statusText":"2 servers"}"#;
    assert!(matches!(&ev(&mut s, status)[..], [Update::Notice(n)] if n.id == "info-status-mcp" && n.text == "2 servers"));
    let confirm = r#"{"type":"extension_ui_request","id":"u4","method":"confirm","title":"Delete?","message":"rm -rf dist"}"#;
    let [Update::Ask { id, ask: Ask::Form(f) }] = &ev(&mut s, confirm)[..] else { panic!() };
    assert!(f.fields.is_empty());
    assert_eq!(reply(&mut s, *id, Answer::Form(vec![])), Some(json!({"type":"extension_ui_response","id":"u4","confirmed":true})));
}

#[test]
fn tui_commands_are_intercepted_or_reported() {
    assert!(matches!(command("/thinking high", "a1"), Some(PiCommand::Rpc(v)) if v["type"] == "set_thinking_level" && v["level"] == "high"));
    assert!(matches!(command("/tree", "a2"), Some(PiCommand::TuiOnly(c)) if c == "tree"));
    assert!(command("/my-extension-cmd arg", "a3").is_none());
    assert!(command("hello", "a4").is_none());
}

#[test]
fn every_fixture_settles() {
    for (name, _) in crate::record::fixtures("pi") {
        let out = replay(&name);
        assert!(out.iter().any(|u| matches!(u, Update::Final(_) | Update::Ask { .. } | Update::Error(_))), "{name}");
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib pi::tests`
Expected: FAIL.

- [ ] **Step 3: Implement the mapping and command tables.** `prompt` checks `command` first. `Rpc` commands are written with the prompt's id, and their `response` produces the table's updates plus `Final("")`. `TuiOnly` sends the Warning and `Final("")` at once. Rewrite `compact_command_settles_on_its_reply` to expect a Compaction notice instead of a tool row.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib pi::tests`
Expected: all PASS.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/driver/src/pi.rs crates/driver/tests/unit/pi.rs
git commit -m "feat(pi): streamed tool input, diffs, cost, retries, extension ui, tui commands"
```

---

## Antigravity track (Tasks 18-19, parallelizable)

Shapes come from agy 1.4.2 (`agy --help`, `agy changelog`, the existing fixtures). Every line is `{"event": "init"|"step_update"|"result", …}`. There is no per-action approval channel in print mode, so phase 1's warned full-permission rerun stays (user decision). There is also no thinking text in stream-json: `usage.thinking_tokens` is all there is, so there is nothing to map.

### Task 18: agy turns, steers, Wake, non-blocking respawn, launch, mode

**Files:**
- Modify: `crates/driver/src/antigravity.rs` (`State`, `start`, `prompt`, `steer`, `answer`, `read`, `map_event`, `launch_args`, `reports_steered`)
- Modify: `crates/driver/src/catalog.rs` (`probe`: drop the `antigravity` fallback), `crates/agent/src/agent.rs` (`missing_binary_error`: drop the `antigravity` fallback)
- Test: `crates/driver/tests/unit/antigravity.rs`

**Interfaces:**
- Consumes: `StderrTail`, `log_unknown`, the notice ids, `DriverStartOptions.attachments` (Task 4).
- Produces:
  - `State` gains:
    - `inputs: VecDeque<Input>`, with `struct Input { text: String, steer: bool }`: inputs written whose `result` has not arrived. It replaces `pending`.
    - `taken: usize`, inputs whose `user_input` step has arrived
    - `wakes: u32`
    - `requested: AccessMode`
    - `stderr: StderrTail`
  - `fn reports_steered(&self) -> bool { true }`.
  - `fn agy_mode(permission_mode: &str) -> Option<AccessMode>`. The mapping:
    - `"accept-edits"` gives AutoEdit
    - `"auto"` gives Auto
    - `"request-review"` gives Ask
    - `"plan"` gives ReadOnly
    - anything else gives `None`, through `log_unknown`

    If the `text` fixture (recorded with `--access auto-edit`, so launched with `--mode accept-edits`) shows `init.permission_mode == "request-review"`, then `request-review` maps to AutoEdit instead. The Auto fallback is then detected from a stderr line that contains `--mode auto` and either `unavailable` or `not available`.
  - `start(&mut self, full: bool, wait: bool)`. Only the first spawn in `spawn` waits for `init`, because `resumed()` must be known before the agent decides whether to seed history. Respawns in `prompt` (mode change, dead process) and in `answer` (the full-permission rerun) do not wait: they spawn, write the input, and let the reader handle `init`. The old child is reaped on a background thread.
  - `launch_args(opts, conversation, full)` passes `--add-dir <opts.attachments>` (creating the folder) in place of the shared attachments dir. It passes no `--add-dir` when `attachments` is `None`.

**Mapping:**

| Event or action | Maps to |
|---|---|
| `prompt(text)` | push `Input { text, steer: false }` |
| `steer(text)` | push `Input { text: without_attached_files(text), steer: true }`. No `Wake`. |
| `step_update` `user_input` (DONE) | `inputs[taken]`; when it is a steer, `Steered(text)`; then `taken += 1` |
| `result` | pop the front input (`taken -= 1`). When `inputs` is empty and no rerun ask is pending, `Final("")` ends the turn. |
| `step_update` or `result` while `!s.active` | `active = true`, `turn += 1`, then `[Wake, Notice(wake-<n>, Wake, "agy started a turn on its own")]`, then the event's own updates. A `result` with no input pending ends that turn. |
| `step_update` with an unknown `step_type` | `log_unknown("agy", step_type)` |
| `init {permission_mode}` | `Mode(agy_mode(..))` (or `Mode(Full)` when launched with `--dangerously-skip-permissions`). When it differs from `requested`, also a ModeChange notice `mode-<n>`: "agy is in {mode} mode. {requested} isn't available for this account." |
| `init` with `conversation_id ≠` the resume id | `Notice::new("info-resume", Info, "Couldn't find the earlier agy conversation, so this is a new one.")`, and `resumed = false` |
| startup | `startup-spawn` "Starting agy" (Running), then Done at `init`. The rerun respawn sends `Notice::new("info-rerun", Info, "Restarting agy with full permissions for this turn")`. |
| stderr line `AGY_ERROR: {json}` | push to `stderr`. The process-exit Error uses `stderr.fatal(<json message, or "agy exited">)`. |
| a new `prompt` while the rerun ask is pending | `AskResolved(id)`, then clear `s.ask` |

- [ ] **Step 1: Write the failing tests**

```rust
const USER_INPUT: &str = r#"{"event":"step_update","step_update":{"conversation_id":"c","step_index":5,"state":"DONE","step_type":"user_input"}}"#;
const RESULT_OK: &str = r#"{"event":"result","result":{"conversation_id":"c","status":"SUCCESS","response":"","duration_seconds":1,"num_turns":1,"usage":{"input_tokens":1,"output_tokens":1,"thinking_tokens":0,"cache_read_tokens":0,"total_tokens":2}}}"#;

#[test]
fn steers_are_marked_at_their_user_input_step_and_results_pair_up() {
    let mut s = State::default();
    on_prompt(&mut s, "count");
    on_steer(&mut s, "also say done");
    assert_eq!(feed(&mut s, USER_INPUT), []);
    assert_eq!(feed(&mut s, USER_INPUT), [Update::Steered("also say done".into())]);
    assert_eq!(feed(&mut s, RESULT_OK), [], "the steer's own agy turn is still running");
    assert_eq!(feed(&mut s, RESULT_OK), [Update::Final(String::new())]);
}

#[test]
fn events_outside_a_turn_wake_it() {
    let mut s = State::default();
    let text = r#"{"event":"step_update","step_update":{"conversation_id":"c","step_index":9,"state":"DONE","step_type":"agent_response","text_delta":"bg done"}}"#;
    let out = feed(&mut s, text);
    assert!(matches!(&out[..], [Update::Wake, Update::Notice(n), Update::Delta(_)] if n.kind == NoticeKind::Wake), "{out:?}");
    assert_eq!(feed(&mut s, RESULT_OK), [Update::Final(String::new())]);
}

#[test]
fn init_reports_the_mode_in_effect() {
    let mut s = State { requested: AccessMode::Auto, ..State::default() };
    let out = init(&mut s, &serde_json::from_str(&INIT.replace("request-review", "accept-edits")).unwrap());
    assert!(out.contains(&Update::Mode(AccessMode::AutoEdit)));
    assert!(out.iter().any(|u| matches!(u, Update::Notice(n) if n.kind == NoticeKind::ModeChange)));
}

#[test]
fn launch_scopes_add_dir_to_the_chat_and_never_uses_the_ide_binary() {
    let dir = std::env::temp_dir().join(format!("aether-agy-att-{}", std::process::id()));
    let o = DriverStartOptions { attachments: Some(dir.clone()), ..opts("", "", AccessMode::AutoEdit) };
    let args = launch_args(&o, "", false);
    assert!(args.windows(2).any(|w| w[0] == "--add-dir" && w[1] == dir.to_string_lossy()));
    assert!(!launch_args(&opts("", "", AccessMode::AutoEdit), "", false).contains(&"--add-dir".to_string()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn steer_fixture_marks_the_steer_and_ends_once() {
    let out = replay("steer");
    assert!(out.iter().any(|u| matches!(u, Update::Steered(_))));
    assert_eq!(out.iter().filter(|u| matches!(u, Update::Final(_))).count(), 1);
}
```

`init(s, v) -> Vec<Update>` is the extracted `init` handling, which the reader calls. `opts` gains `attachments: None`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib antigravity::tests`
Expected: compile errors, then FAIL.

- [ ] **Step 3: Implement the mapping, the non-blocking respawn and the launch changes.**
  - The test helper `running()` becomes `State { active: true, inputs: VecDeque::from([Input { text: "go".into(), steer: false }]), ..State::default() }`.
  - Rewrite `mid_turn_steer_runs_as_the_next_agy_turn_in_the_steer_reply` so it expects `Steered` and one `Final` instead of `Wake`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib antigravity::tests && cargo test -p aether-agent --lib`
Expected: all PASS.

- [ ] **Step 5: Manual check**

In an Antigravity chat, Auto-accept edits mode, prompt "run `touch blocked.txt`". When the rerun card shows, press Stop at once. Expected: the turn stops within a second, with no 45 s freeze.

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/driver/src/antigravity.rs crates/driver/src/catalog.rs crates/agent/src/agent.rs crates/driver/tests/unit/antigravity.rs
git commit -m "feat(antigravity): steer delivery, wake, non-blocking respawn, real mode"
```

---

### Task 19: agy tools, background tasks, denied rows, context window

**Files:**
- Modify: `crates/driver/src/antigravity.rs` (`tool_input`, `tool`, `result`, `window`)
- Test: `crates/driver/tests/unit/antigravity.rs`

**Interfaces:**
- Produces:
  - `State` gains `background: HashMap<String, String>`: agy task id to row id.
  - `fn window(model: &str) -> Option<u64>`. It is a static table:
    - a slug starting `gemini-` gives `1_048_576`
    - a slug starting `claude-` gives `200_000`
    - anything else gives `None`

    agy reports no window. Mark the table in code with a `ponytail:` comment: it is a static table that goes wrong when agy changes limits, and the upgrade path is to read agy's `context_window` once a print-mode report exposes it. Send `Update::Window(w)` at `init` when it is `Some`.
  - `const BACKGROUND_MARK: &str`: the phrase in a `run_command` result that says the command went to the background, plus the pattern its task id follows. Both are pinned from the `background` fixture (Task 3) and asserted by `background_fixture_settles_its_row`.

**Mapping (`tool_input` and the tool lifecycle):**

| agy tool | Classified as |
|---|---|
| `invoke_subagent {Subagents:[{TypeName, Role, Prompt}]}` | `("agent", {"description": Role or TypeName, "prompt": Prompt, "subagent_type": TypeName})`. With more than one subagent, the description is `"{n} subagents: {TypeName, …}"`. |
| `browser_subagent {TaskName, TaskSummary, Task}` | `("agent", {"description": TaskName or TaskSummary, "prompt": Task, "subagent_type": "browser"})` |
| `view_file {AbsolutePath, StartLine, EndLine}` | `("read", {"file_path", "offset": StartLine, "limit": EndLine - StartLine + 1})` |
| `search_web {query, domain}` | `("websearch", {"query": query, falling back to Query})`. 1.4.2's schema tags are lowercase, so the audit's casing bug turns into a fallback. |
| `manage_task {Action, TaskId, Input}`, `command_status {CommandId}`, `send_command_input {CommandId, Input, Terminate}` | their own Other row. They also update the row in `background[TaskId or CommandId]`: its output is set to this step's output. `Action == "kill"` or `Terminate == true` gives Stopped, an output with a finished marker or exit code gives Done or Failed, and the row is then removed from `background`. |
| `run_command` whose DONE output matches `BACKGROUND_MARK` | the row's status becomes `Background`, and `background[task id] = row id` |

**Denied rows.** `result.denied_actions` entries are `{action, display_name}`, with no ids. Each one is matched to the latest unmatched row in `s.blocked` whose `tool_name`, converted to PascalCase, equals `display_name` (`run_command` matches `RunCommand`). Only when no name matches does it fall back to phase 1's order. Exactly the matched row ids are marked Denied. The approval card is phase 1's, filled with:
- `tool_id`: the first marked row
- `kind`: `Command` when every marked row is a command, otherwise `Other`
- `options`: none, because the rerun is the only way to allow

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn subagent_and_view_file_inputs_are_typed() {
    let (name, input) = tool_input("invoke_subagent", &json!({"Subagents":[{"TypeName":"researcher","Role":"Find docs","Prompt":"look up X"}]}));
    assert!(matches!(classify(name, &input).1, ToolDetail::Task { ref agent, ref prompt, .. } if agent == "researcher" && prompt == "look up X"));
    let (name, input) = tool_input("view_file", &json!({"AbsolutePath":"/w/a.rs","StartLine":10,"EndLine":19}));
    assert!(matches!(classify(name, &input).1, ToolDetail::Read { range: Some(ref r), .. } if r == "lines 10–19"));
    let (_, input) = tool_input("search_web", &json!({"Query":"rust 1.99"}));
    assert_eq!(input["query"], "rust 1.99");
}

#[test]
fn denied_actions_mark_rows_by_tool_name() {
    let lines = [
        r#"{"event":"step_update","step_update":{"conversation_id":"c","step_index":1,"state":"DONE","step_type":"tool","tool_name":"write_to_file","tool_info":{"name":"write_to_file","parameters":{"TargetFile":"/w/a.txt","CodeContent":"x"}}}}"#,
        r#"{"event":"step_update","step_update":{"conversation_id":"c","step_index":2,"state":"DONE","step_type":"tool","tool_name":"run_command","tool_info":{"name":"run_command","parameters":{"CommandLine":"touch b"}}}}"#,
        r#"{"event":"result","result":{"conversation_id":"c","status":"SUCCESS","response":"","denied_actions":[{"action":"command","display_name":"RunCommand"}]}}"#,
    ].join("\n");
    let mut s = State { conversation: "c".into(), ..running() };
    let out = feed(&mut s, &lines);
    let denied: Vec<String> = out.iter().filter_map(|u| match u { Update::Tool(c) if c.status == ToolStatus::Denied => Some(c.id.clone()), _ => None }).collect();
    assert_eq!(denied, ["c:2"]);
    assert!(matches!(out.last(), Some(Update::Ask { ask: Ask::Approval(a), .. }) if a.tool_id.as_deref() == Some("c:2") && a.kind == ApprovalKind::Command));
}

#[test]
fn context_window_comes_from_the_model_family() {
    assert_eq!(window("gemini-3.8-flash-medium"), Some(1_048_576));
    assert_eq!(window("claude-opus-5-5-high"), Some(200_000));
    assert_eq!(window(""), None);
}

#[test]
fn background_fixture_settles_its_row() {
    let calls = last_calls(&replay("background"));
    assert!(calls.iter().any(|c| c.status == ToolStatus::Done && matches!(c.detail, ToolDetail::Command { .. })), "{calls:?}");
    assert!(calls.iter().all(|c| c.status != ToolStatus::Running));
}

#[test]
fn subagent_fixture_has_a_task_row() {
    assert!(last_calls(&replay("subagent")).iter().any(|c| matches!(c.detail, ToolDetail::Task { .. })));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib antigravity::tests`
Expected: FAIL.

- [ ] **Step 3: Implement the tables.** Pin `BACKGROUND_MARK` by reading `tests/fixtures/agy/background.jsonl`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-driver --lib antigravity::tests`
Expected: all PASS.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/driver/src/antigravity.rs crates/driver/tests/unit/antigravity.rs
git commit -m "feat(antigravity): subagent and background rows, denied rows by name, window"
```

---

## Cross-driver tasks

### Task 20: OpenRouter Warning versus Error (parallelizable)

**Files:**
- Modify: `crates/agent/src/events.rs` (`to_updates`)
- Test: `crates/agent/tests/unit/events.rs`

**Interfaces:**
- Consumes: `Update::Warning` (phase 2).
- **Rules.** Nothing else changes (spec: "No other changes").
  - A stream item `Err(e)` stays `Error(e)`. It ends the run.
  - `llm_response.error_message` on a partial chunk becomes `Warning(message)`, because the stream goes on. On a final chunk it stays `Error(message)`.
  - A final chunk with `finish_reason == Some(FinishReason::MaxTokens)` adds `Warning("The reply hit the model's output limit")`.
  - A final chunk with `Some(FinishReason::Safety)` adds `Warning("The provider's safety filter stopped this reply")`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn partial_errors_and_cut_off_replies_are_warnings() {
    let mut chunk = event(vec![Part::Text { text: "Hi".into() }], true).unwrap();
    chunk.llm_response.error_message = Some("upstream hiccup".into());
    assert_eq!(to_updates(Ok(chunk)), [Update::Warning("upstream hiccup".into())]);
    let mut done = event(vec![Part::Text { text: "Hi".into() }], false).unwrap();
    done.llm_response.finish_reason = Some(adk_rust::FinishReason::MaxTokens);
    assert_eq!(to_updates(Ok(done)), [Update::Warning("The reply hit the model's output limit".into()), Update::Final("Hi".into())]);
    let mut failed = event(vec![], false).unwrap();
    failed.llm_response.error_message = Some("bad key".into());
    assert_eq!(to_updates(Ok(failed)), [Update::Error("bad key".into())]);
}
```

If `FinishReason` is not re-exported at `adk_rust::FinishReason`, import it from where `LlmResponse` comes from (`adk_rust::prelude`).

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p aether-agent --lib events::tests`
Expected: FAIL (a partial error gives `Error`).

- [ ] **Step 3: Implement the rules in `to_updates`**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-agent --lib`
Expected: all PASS.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/agent/src/events.rs crates/agent/tests/unit/events.rs
git commit -m "feat(agent): openrouter warnings for recoverable stream errors"
```

---

### Task 21: Version gates, probe data, last per-provider UI fallback

Runs after every Driver track has merged.

**Files:**
- Create: `crates/driver/src/version.rs`, `crates/driver/tests/unit/version.rs`
- Modify: `crates/driver/src/lib.rs` (`pub mod version;`), `crates/driver/src/catalog.rs` (`ProviderProbe`, `probe`)
- Modify: `crates/driver/src/claude.rs`, `codex.rs`, `opencode/service.rs` (`connect`), `pi.rs`, `antigravity.rs` (gate at spawn)
- Modify: `crates/app/src/hitl.rs` (delete `remembers_approvals` and its fallback button), `crates/app/tests/unit/hitl.rs` (delete `only_clis_that_keep_a_session_allowance_offer_it`), `crates/driver/tests/unit/catalog.rs`

**Interfaces:**
- Produces, in `version.rs`:
  - `pub type Version = (u32, u32, u32);`
  - `pub fn parse(text: &str) -> Option<Version>`: the first `x.y.z` in `text`, with an optional `v` in front.
  - `pub fn cli_version(binary: &Path) -> Option<String>`: runs `<binary> --version` with a 5 s timeout, and returns the first `x.y.z` as text.
  - `pub enum Support { Ok, Degraded(String), Unsupported(String) }` (`Clone, Debug, PartialEq`).
  - `pub fn support(provider: Provider, v: Version) -> Support`. It walks the table below in order, and the first rule that matches wins.
  - `pub fn capabilities(provider: Provider) -> &'static [&'static str]`: the chips.
- **Gates.** `{v}` is the found version. A version newer than the tested one is `Ok`.

  | Provider | Version source | Unsupported below | Degraded below (message) | Tested |
  |---|---|---|---|---|
  | Claude | `claude --version` at spawn | none | 2.1.304: "Claude Code {v} is older than 2.1.304, the version Aether is tested with. Some events may not show." | 2.1.304 |
  | Codex | `initialize` result `userAgent`, after its first `/` | none | 0.163.0: the same sentence, for Codex | 0.163.0 |
  | OpenCode | `GET /api/info` `version` | 2.0.0: "OpenCode {v} isn't supported. Update to 2.0 or later." | 2.0.32: the same tested sentence | 2.0.32 |
  | Pi | `pi --version` before spawn | 0.80.4: "Pi {v} can't tell Aether when a run ends. Update to 0.80.4 or later." | 0.99.0: "Pi {v} is older than 0.99.0: runs that extensions handle may end early, and nested tool calls show flat." Then 1.3.0: the tested sentence. | 1.3.0 |
  | agy | `agy --version` before spawn | 1.1.15: "agy {v} can't take Aether's input. Update to 1.1.15 or later." | 1.1.27: "agy {v} doesn't report blocked actions, so their approvals can't show. Update to 1.1.27 or later." Then 1.4.2: the tested sentence. | 1.4.2 |

- **At spawn.** Each Driver sends `Notice::new("startup-version", Startup, "Found {name} {v}")`, or "Found {name} (version unknown)" when parsing fails. Then:
  - `Unsupported(m)` makes the Driver dead with `m`, so the first prompt shows the fatal card.
  - `Degraded(m)` sends `Warning(m)`.
  - `Ok`, or an unknown version, sends nothing more.
- **Capabilities** (the chips):
  - Claude: "Subagents", "Background tasks", "Approvals", "Questions", "Forms", "Plans", "Cost"
  - Codex: "Subagents", "Background terminals", "Approvals", "Questions", "Forms", "Review", "Plans"
  - OpenCode: "Subagents", "Background", "Approvals", "Forms", "Cost", "Keeps running after Aether quits"
  - Pi: "Extensions", "Dialogs", "Cost"
  - agy: "Subagents", "Background", "Rerun with full access"
- **Produces, on `ProviderProbe`**, all `#[derive(Default)]`-friendly:
  - `pub version: Option<String>`
  - `pub status: String`
  - `pub chips: Vec<String>`
  - `pub degraded: Option<String>`

  `pub fn probe_status(installed: bool, service: bool, support: Option<&Support>) -> (String, Option<String>)` gives the status and the degraded reason:
  - not installed: `("Not installed", None)`
  - `Unsupported(m)`: `("Update needed", Some(m))`
  - `Degraded(m)`: `("Limited", Some(m))`
  - an OpenCode service with no binary: `("Service running", None)`
  - otherwise: `("Ready", None)`

  `probe` fills all four fields. Phase 4's health cards and header version chip read them.

- [ ] **Step 1: Write the failing tests** in `crates/driver/tests/unit/version.rs`

```rust
use super::*;

#[test]
fn parse_finds_the_first_version() {
    assert_eq!(parse("2.1.304 (Claude Code)"), Some((2, 1, 304)));
    assert_eq!(parse("codex_cli_rs/0.163.0 (Mac OS 27.0.0; arm64) iTerm"), Some((0, 163, 0)));
    assert_eq!(parse("v2.0.32"), Some((2, 0, 32)));
    assert_eq!(parse("agy 1.4.2"), Some((1, 4, 2)));
    assert_eq!(parse("unknown"), None);
}

#[test]
fn support_applies_floors_then_the_tested_version() {
    use aether_core::Provider::*;
    assert!(matches!(support(Pi, (0, 80, 3)), Support::Unsupported(m) if m.contains("0.80.4")));
    assert!(matches!(support(Pi, (0, 98, 0)), Support::Degraded(m) if m.contains("0.99.0")));
    assert!(matches!(support(Pi, (1, 2, 0)), Support::Degraded(m) if m.contains("1.3.0")));
    assert_eq!(support(Pi, (1, 3, 0)), Support::Ok);
    assert_eq!(support(Pi, (2, 0, 0)), Support::Ok);
    assert!(matches!(support(Antigravity, (1, 1, 10)), Support::Unsupported(_)));
    assert!(matches!(support(Antigravity, (1, 1, 20)), Support::Degraded(m) if m.contains("1.1.27")));
    assert!(matches!(support(OpenCode, (1, 9, 0)), Support::Unsupported(m) if m.contains("2.0")));
    assert!(matches!(support(Claude, (2, 1, 200)), Support::Degraded(_)));
    assert_eq!(support(Codex, (0, 170, 0)), Support::Ok);
}
```

`crates/driver/tests/unit/catalog.rs`:

```rust
#[test]
fn probe_status_says_why() {
    assert_eq!(probe_status(false, false, None), ("Not installed".into(), None));
    assert_eq!(probe_status(true, false, Some(&Support::Degraded("old".into()))), ("Limited".into(), Some("old".into())));
    assert_eq!(probe_status(true, false, Some(&Support::Unsupported("no".into()))), ("Update needed".into(), Some("no".into())));
    assert_eq!(probe_status(false, true, None), ("Service running".into(), None));
    assert_eq!(probe_status(true, false, None), ("Ready".into(), None));
}
```

Update `claude_falls_back_to_named_aliases_only_when_found`: a missing binary now gives `ProviderProbe { status: "Not installed".into(), ..ProviderProbe::default() }`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-driver --lib -- version::tests catalog::tests`
Expected: compile errors.

- [ ] **Step 3: Implement `version.rs`, the spawn gates and the probe fields.** Delete `remembers_approvals` in `hitl.rs`: every Driver now sends its own options.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: all PASS.

- [ ] **Step 5: Live check**

Run: `for p in claude codex opencode pi agy; do cargo run -q -p aether-driver --example record -- --provider $p --cwd /tmp/aether-fx --access full --prompt "Reply with exactly: ok" | grep -m1 'startup-version'; done`
Expected: five lines, each holding `Found <name> <version>` with today's installed versions (2.1.304, 0.163.0, 2.0.32, 1.3.0, 1.4.2).

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/driver crates/app/src/hitl.rs crates/app/tests/unit/hitl.rs
git commit -m "feat(driver): version gates, provider health probe, options-only approval cards"
```

---

### Task 22: Phase gate

- [ ] **Step 1: Full test and lint run**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: all tests pass and clippy reports no warnings.

- [ ] **Step 2: Every fixture replays**

Run: `cargo test -p aether-driver --lib fixture`
Expected: every `*fixture*` test passes for all five Providers.

- [ ] **Step 3: Scrub re-check**

Run: `rg -n -e "$HOME" -e '/Users/' -e '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}' crates/driver/tests/fixtures | grep -v 'user@example.com'`
Expected: no output.

- [ ] **Step 4: Manual pass in the app.** Phase 4 polishes the visuals; this pass checks that each ask is answerable and nothing hangs.

  | Provider | Check |
  |---|---|
  | Claude | AskUserQuestion with two questions, one multi-select. A Write approval offers "Allow edits in …" options, but never "Switch to Accept edits" folded into Allow. A subagent's rows nest. A background `sleep 20` completes in its original row after the turn ends. |
  | Codex | `/review` shows its findings. A command approval shows the execpolicy option. Steering during `/compact` puts the message back in the queue. |
  | OpenCode | A subagent asking for shell approval shows "asked by explore". A question form with two fields is answerable in one card. A model reset to the default sticks. |
  | Pi | Steer mid-run: the chip disappears and the "You, while it worked" marker appears in the same reply. Stop while an editor dialog is open: the session survives. |
  | agy | Auto mode on an account without it shows the mode change notice. A blocked command lists itself on the rerun card. Stop during the rerun respawn is immediate. |

  Every new-chat probe shows a version and the status "Ready".
