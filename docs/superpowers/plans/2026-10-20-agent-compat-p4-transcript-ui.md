# Provider Compat Phase 4: Transcript and Chat UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the agent transcript, composer trays, sidebar statuses and new-chat screen to parity with the approved prototype, within `docs/ux-rubric.md`. Fix the transcript-pipeline bugs and the streaming cost that phases 1-2 left.

**Architecture:**
- **Rows, not replies.** A reply becomes a list of `Block`s, built by one pure function (`timeline.rs`). The transcript's virtual list gets one row per block instead of one row per message.
  - A long reply then lays out only the rows in view.
  - An update remeasures only the rows of the message it touched.
  - Read folding, subagent nesting, the finished-turn fold and the steer-as-user rule are all decided in that pure function, where they are unit-tested.
- **One owner per block.** Task 3 plumbs every block kind to a component or app builder in the file that will own it, moving today's rendering there. Tasks 5-15 then each change only their own files, so most of them run in parallel worktrees.
- **Markdown in one view.** A text chunk renders through a single gpui-base `TextView`. Fenced code is highlighted by gpui-component's tree-sitter highlighter, which is switched on through gpui-kit features. File links are resolved off the UI thread. The app-side fence splitter (`code_block.rs`) is deleted.

**Tech Stack:** Rust 2024, GPUI via gpui-kit 0.7.3 (gpui, gpui-base `TextView`, gpui-component `highlighter`, `Kbd`, `Clipboard`, `Segmented`), aether-core types from phases 2-3.

**Spec:** `docs/superpowers/specs/2026-10-20-agent-compat-chat-revamp-design.md`: section 5 (every bullet, including "Also in scope from the prototype" and "Prototype vs rubric"), with sections 1-2 for the data the views read. The visual target is `docs/superpowers/specs/2026-10-20-agent-chat-prototype.html`. The audit findings this plan closes are in the "Transcript pipeline" section of `docs/wayfinder/research/2026-10-20-driver-audits.md`.

## Global Constraints

- **`docs/ux-rubric.md` is binding for every view.**
  - Theme tokens only, with no raw hex in views. For small coloured text, use `theme::danger_text`, `success_text`, `info_text` and `mango_text`. For fills and borders, use `cx.theme().danger`, `.success`, `.info` and `theme::mango()` at an alpha.
  - Both themes are checked.
  - Spacing is on the 4/8 px grid. Radius comes from the scale: 6 for small controls, pills and rows; 9 for code, terminal and diff boxes; 12 for cards.
  - No side stripes wider than 1 px, and no nested cards.
  - No unicode glyphs as icons (↕ ↓ ↳ ✓ ⏎ as icons). Use the Lucide set through `gpui_kit::assets::IconName`. Keys render through `gpui_kit::component::kbd::Kbd`.
  - No gradient text and no shimmer. `ShimmerText` is removed from the transcript.
  - Status is always colour plus icon plus text.
  - Hover-only actions also appear on keyboard focus.
  - Every flow has a keyboard path, and Esc closes the topmost layer only.
  - Motion is 100-200 ms and only for state changes, and reduce-motion is honoured. The only loop is a real in-progress spinner. Heartbeat dots are static.
  - Streaming text never animates per token (keep `TextViewMotion` at its zero default). Layout never jumps while streaming: timers use tabular numerals, and rows reserve a transparent 1 px border.
- **Prototype vs rubric (spec, verbatim):**
  - Subagent cards use a 1 px status-coloured border plus a status icon. The rubric bans side stripes wider than 1 px.
  - A subagent's approval card renders directly below the subagent card as a sibling, labelled with its source, not nested inside it.
  - No unicode glyphs as icons (↕ ↓ ↳). Use the icon set.
  - No gradient shimmer on "Thinking". Use a plain label with a spinner.
  - Colours come from theme tokens, and both themes are checked.
- **More rubric calls this plan makes:**
  - The prototype's violet (subagents) becomes the `info` token, because the theme has no violet token.
  - The approval card's 3 px glow ring is dropped. It keeps a tinted border only, since elevation is declared once.
  - The blinking caret is dropped. The status line's spinner is the in-progress indicator.
- **Views render the core model only.** Driver differences end at the Driver boundary. The only per-Provider fact in the app is the stall card's TUI command (Task 10). It is an action, not a view branch.
- **Unit tests** live in `crates/<crate>/tests/unit/<path>.rs`, included with `#[cfg(test)] #[path = …] mod tests;`. Never inline them.
- **Commits.** Ask the user before every commit. Implementers never stage or commit. Never add `Co-Authored-By` or AI attribution. Commit messages follow Conventional Commits and stay terse.
- **Ordering.**
  - Phases 1 and 2 are merged before Task 1.
  - Task 9 needs phase 3 Task 4 (`Agent::notice_action`).
  - Tasks 10 and 11 need phase 3 Task 1 (`Ask::Approval(Approval)`, `Ask::Questions`, `Ask::Form`, `Ask::Plan(Plan)`, `Answer::Pick`, `DenyWith`, `Answers`, `Form`).
  - Task 12's Stop button needs phase 3 Task 4 (`Agent::stop_tool`).
  - Task 15 needs phase 3 Task 21 (probe data).
  - Phase 3 Task 1 also edits the approval-to-row match in `crates/ui/src/chat/message.rs`. If Task 3 here lands first, that match is already gone and phase 3 skips that bullet.
- **Parallel worktrees.** Tasks 1-4 run in order. After Task 4, each task in the map below touches files no other task touches. Give each worktree its own target dir, `CARGO_TARGET_DIR=/tmp/aether-target-p4-<task>`.

  | Runs after Task 4, in parallel | Owns |
  |---|---|
  | 5, then 6 | `diff_view.rs`, then `tool_row.rs` and `changes.rs` |
  | 7 | `thinking.rs` |
  | 8 | `subagent.rs` |
  | 9 | `notice.rs`, app `chat/manage.rs` |
  | 10 | `live.rs`, app `chat/live.rs` and `new.rs` |
  | 11 | `request.rs`, `timeline.rs`, app `hitl.rs` and `actions.rs` |
  | 12 | `composer.rs`, app `composer/mod.rs` |
  | 13 | `turn.rs`, app `chat/turn.rs` and `format.rs` |
  | 14 | `sidebar/*`, app `mod.rs`, `sidebar.rs`, `project_page/model.rs` and `projects.rs` |
  | 15 | `hero.rs`, app `start.rs`, `composer/header.rs` and `providers.rs` |

  Files are under `crates/ui/src/chat/` unless they say "app", which means `crates/app/src/app/` (and `crates/app/src/` for `hitl.rs`, `actions.rs` and `project_page/`). Task 16 runs last.
- **Thresholds and copy pinned by the prototype:**
  - The heartbeat reads "live" under 4 s, then "quiet {t}", then "no output {t}" in the warning colour from 30 s.
  - The stall card shows after 60 s with no events and nothing pending, and re-checks the process every 5 s.
  - Live timers tick at 1 Hz.
  - Command output shows its last 12 lines. A subagent card shows its last 5 nested rows. A code block caps at 340 px.
  - `clock(ms)` formats every duration: "0s"…"59s", "1m 05s", "1h 3m". Sub-second finished times use `short_ms` ("400ms").
- Keep the Drivers' 10 Hz snapshot throttle (`SNAPSHOT_EVERY` in `crates/driver/src/tool.rs`). This phase does not touch Drivers.

## Review Focus

- **Enter pressed while typing must not approve.** An approval that arrives while the user types in the composer must not take Enter. The card takes focus only from an empty composer in the active chat (Task 11 test `ask_takes_focus_only_from_an_empty_composer`).
- **Waiting is never stalled.** An approval or question left for ten minutes must read as waiting on the user, not stalled or quiet. That holds in the status line, the stall card and the sidebar (Task 10 test `stall_needs_sixty_quiet_seconds_and_nothing_pending`, Task 14 test `sidebar_status_precedence`).
- **No broken code blocks.** A tool call or notice that arrives in the middle of a streamed code block must not split it. The step renders after the fence closes (Task 2 test `splits_never_land_inside_an_open_fence`).
- **Background work stays visible.** A turn that ended with background work still running must not fold away its live rows (Task 2 test `replies_without_steps_or_with_background_work_do_not_fold`).
- **No absurd timers.** Rows saved by older builds have no start time, and clocks can skew. Neither may show a timer like "56 years" or a negative one (Task 2 test `since_is_none_without_a_start_and_never_negative`, Task 6 test `timers_follow_status_and_skip_unknown_starts`).

---

### Task 1: Start times for live timers

**Files:**
- Modify: `crates/core/src/lib.rs`: `ToolCall` (field and `new`), `Message::apply` (`Update::Tool` and `Update::Ask` arms), and a new `Message::live_thinking_ms`
- Modify: `crates/core/src/request.rs` (`Request`)
- Test: `crates/core/tests/unit/lib.rs`

**Interfaces:**
- Consumes: phase 2's `thinking_since: Option<Instant>` on `Message`.
- Produces:
  - `ToolCall.started_ms: Option<i64>`, as `#[serde(default, skip_serializing_if = "Option::is_none")]`. It is Unix ms. The reducer stamps it when a row is first inserted and the Driver left it `None`. An upsert keeps the existing value.
  - `Request.asked_ms: Option<i64>`, with the same serde attributes. It is stamped when `Update::Ask` pushes the request.
  - `pub fn live_thinking_ms(&self) -> Option<u64>` on `Message`. It returns the elapsed time of the open thinking step, or `None` when none is open.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn live_rows_and_asks_carry_their_start_time() {
    let mut m = Message::assistant();
    m.apply(Update::Tool(ToolCall::new("t", "Run", ToolDetail::Plain(String::new()))));
    let Step::Tool(first) = &m.steps[0] else { panic!() };
    let started = first.started_ms.expect("stamped on insert");
    m.apply(Update::Tool(ToolCall { status: ToolStatus::Done, ..ToolCall::new("t", "Run", ToolDetail::Plain(String::new())) }));
    let Step::Tool(done) = &m.steps[0] else { panic!() };
    assert_eq!(done.started_ms, Some(started), "an upsert keeps the first stamp");
    m.apply(Update::Ask { id: 1, ask: Ask::Unknown });
    assert!(m.requests[0].asked_ms.is_some());
}

#[test]
fn a_driver_supplied_start_time_wins() {
    let mut m = Message::assistant();
    m.apply(Update::Tool(ToolCall { started_ms: Some(42), ..ToolCall::new("t", "Run", ToolDetail::Plain(String::new())) }));
    let Step::Tool(c) = &m.steps[0] else { panic!() };
    assert_eq!(c.started_ms, Some(42));
}

#[test]
fn live_thinking_time_only_while_the_thought_is_open() {
    let mut m = Message::assistant();
    assert_eq!(m.live_thinking_ms(), None);
    m.apply(Update::Thinking("hmm".into()));
    assert!(m.live_thinking_ms().is_some());
    m.apply(Update::Delta("x".into()));
    assert_eq!(m.live_thinking_ms(), None);
}

#[test]
fn rows_saved_without_a_start_time_load() {
    let call: ToolCall = serde_json::from_str(r#"{"id":"t","title":"T","status":"Done"}"#).unwrap();
    assert_eq!(call.started_ms, None);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-core --lib`
Expected: compile errors (`started_ms`, `asked_ms` and `live_thinking_ms` are missing).

- [ ] **Step 3: Implement.** Stamp with `SystemTime::now()` in Unix ms, the same source `finish` uses for `ended_at_ms`. Fix the struct literals the compiler flags, adding `started_ms: None` and `asked_ms: None`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-core --lib && cargo build -p aether`
Expected: all PASS, including phase 2's `todays_format_round_trips`, and the build succeeds.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/core
git commit -m "feat(core): start times for tool rows, asks and live thoughts"
```

---

### Task 2: Transcript model

**Files:**
- Create: `crates/ui/src/chat/timeline.rs`, `crates/ui/tests/unit/chat/timeline.rs`
- Create: `crates/ui/src/chat/live.rs` (formatters only, for now), `crates/ui/tests/unit/chat/live.rs`
- Modify: `crates/ui/src/chat/mod.rs` (`mod timeline; mod live; pub use timeline::*; pub use live::*;`)

**Interfaces:**
- Consumes: phase 2's `Step::{Thinking, Notice, Steer, Unknown}`, `ToolCall.parent`, `ToolStatus::{Waiting, Background}`, `Message.error`, and `reply_edits` from `changes.rs`.
- Produces, in `timeline.rs`:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    User,
    /// A side turn the CLI started from a late steer: its first step, shown as a user bubble.
    SteerAsUser(usize),
    /// `message.text[range]`; never ends inside an open code fence.
    Text(Range<usize>),
    Thought(usize),
    Tool(usize),
    /// Two or more consecutive finished top-level Read rows.
    Reads(Vec<usize>),
    /// A Task row and every descendant row in this reply, flattened in order.
    Subagent { step: usize, children: Vec<usize> },
    Search(usize),
    Skill(usize),
    Notice(usize),
    /// Startup notices, until the reply has text or another step.
    Startup(Vec<usize>),
    Steer(usize),
    /// Index into `message.requests`.
    Request(usize),
    Fatal,
    Stopped,
    Fold,
    Changes,
    Live,
    Footer,
}

pub struct ReplyView<'a> {
    /// The reply is streaming (last message of a busy chat).
    pub live: bool,
    /// The user expanded the finished reply's timeline.
    pub open: bool,
    /// The user stopped the run that produced this reply.
    pub stopped: bool,
    /// Agent chat: a finished reply lists its edits in the changes card.
    pub agent: bool,
    /// Tool ids whose rows the user opened. They stay visible when the reply folds.
    pub kept: &'a dyn Fn(&str) -> bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptRow { pub message: usize, pub block: Block }

pub fn reply_blocks(m: &Message, view: &ReplyView) -> Vec<Block>;
pub fn transcript_rows<'a>(messages: &[Message], view: &dyn Fn(usize) -> ReplyView<'a>) -> Vec<TranscriptRow>;
/// `at`, or the end of the closing fence line when `at` falls inside an open fence (text.len() if unclosed).
pub fn safe_split(text: &str, at: usize) -> usize;
/// Top padding in px for `row`: 0 first, 32 between messages, 12 around text and cards, 2 between step rows.
pub fn gap_before(prev: Option<&TranscriptRow>, row: &TranscriptRow) -> f32;
```

- Produces, in `live.rs`:
  - `pub fn clock(ms: u64) -> String`
  - `pub fn short_ms(ms: u64) -> String`: "{ms}ms" under 1 s, else `clock`.
  - `pub fn since(start_ms: Option<i64>, now_ms: i64) -> Option<String>`: `clock` of the elapsed time, floored at 0. It returns `None` when `start_ms` is `None`.
- **Rules `reply_blocks` encodes,** in this order:
  1. **Sort.** Steps and requests are sorted stably by position (`step_at`, `Request.at`). Steps come before requests at the same position.
  2. **Text.** A `Text` block covers `cursor..safe_split(text, pos)`, followed by the trailing text.
  3. **Nesting.** A step whose `parent` is a Task row in this reply is nested under its top-level Task's `Subagent` block, and so are its descendants. A row whose parent isn't in this reply renders top-level.
  4. **Reads.** Two or more consecutive top-level `Read` rows with status `Done`, and no text between them, become `Reads`.
  5. **Steers.** A `Step::Steer` that is the first step at position 0 becomes `SteerAsUser`.
  6. **Startup.** Startup notices are one `Startup` block while the reply has no text and no other step. After that, `Done` and `Running` startup notices are dropped, and `Failed` ones become `Notice(i)`.
  7. **Unknown.** `Step::Unknown` is skipped. A legacy `Step::Thought` is a `Thought`.
  8. **Tail.** `Fatal` (when `error` is set), then `Stopped` (when a row is `Stopped`, or when `view.stopped` holds and the reply isn't live), then `Changes` (not live, `agent`, and `reply_edits` non-empty), then `Live` when live, else `Footer`.
  9. **Fold.** A finished reply folds when it has steps, no `Running`/`Waiting`/`Background` row and `!view.open`. Its blocks become `Fold`, then the kept blocks in order, then `Changes` and `Footer`. Kept blocks are the trailing `Text` blocks after the last non-text block, `Tool(i)` where `kept(id)` holds, `Fatal` and `Stopped`. With `view.open`, the blocks are `Fold` followed by everything.

- [ ] **Step 1: Write the failing tests**

`crates/ui/tests/unit/chat/timeline.rs`:

```rust
use super::*;
use aether_core::{Message, Notice, NoticeKind, NoticeState, ToolCall, ToolDetail, ToolStatus, Update};

fn row(id: &str, detail: ToolDetail, status: ToolStatus) -> Update {
    Update::Tool(ToolCall { status, ..ToolCall::new(id, "T", detail) })
}
fn read(id: &str) -> Update {
    row(id, ToolDetail::Read { path: format!("src/{id}.rs"), range: None }, ToolStatus::Done)
}
fn agent() -> ToolDetail {
    ToolDetail::Task { description: "audit".into(), output: String::new(), agent: "explore".into(),
        prompt: String::new(), tools: 0, tokens: 0, activity: String::new() }
}
fn under(id: &str, parent: &str, detail: ToolDetail) -> Update {
    Update::Tool(ToolCall { parent: Some(parent.into()), status: ToolStatus::Done, ..ToolCall::new(id, "T", detail) })
}
fn view(live: bool, open: bool, stopped: bool, kept: &dyn Fn(&str) -> bool) -> ReplyView<'_> {
    ReplyView { live, open, stopped, agent: true, kept }
}
fn live(m: &Message) -> Vec<Block> { reply_blocks(m, &view(true, false, false, &|_| false)) }

#[test]
fn finished_reads_in_a_row_fold_into_one_block() {
    let mut m = Message::assistant();
    for id in ["a", "b", "c"] { m.apply(read(id)); }
    m.apply(Update::Delta("x".into()));
    m.apply(read("d"));
    m.apply(row("e", ToolDetail::Read { path: "e".into(), range: None }, ToolStatus::Running));
    assert_eq!(live(&m), [Block::Reads(vec![0, 1, 2]), Block::Text(0..1), Block::Tool(3), Block::Tool(4), Block::Live]);
}

#[test]
fn subagent_rows_nest_and_grandchildren_flatten_under_the_top_card() {
    let mut m = Message::assistant();
    m.apply(row("t1", agent(), ToolStatus::Running));
    m.apply(under("c1", "t1", ToolDetail::Plain(String::new())));
    m.apply(under("t2", "t1", agent()));
    m.apply(under("g1", "t2", ToolDetail::Plain(String::new())));
    m.apply(read("r"));
    m.apply(under("o1", "gone", ToolDetail::Plain(String::new())));
    assert_eq!(live(&m), [Block::Subagent { step: 0, children: vec![1, 2, 3] }, Block::Tool(4), Block::Tool(5), Block::Live]);
}

#[test]
fn a_steer_that_opens_a_reply_reads_as_a_user_message() {
    let mut m = Message::assistant();
    m.apply(Update::Steered("also dark mode".into()));
    m.apply(Update::Delta("ok".into()));
    m.apply(Update::Steered("and mobile".into()));
    assert_eq!(live(&m), [Block::SteerAsUser(0), Block::Text(0..2), Block::Steer(1), Block::Live]);
}

#[test]
fn splits_never_land_inside_an_open_fence() {
    assert_eq!(safe_split("p\n```rs\nx\n```\nq", 9), 14);
    assert_eq!(safe_split("~~~\nx\n~~~\ny", 5), 10);
    assert_eq!(safe_split("````\n```\nx\n```\n````\nz", 9), 20);
    assert_eq!(safe_split("1. step\n   ```sh\n   ls\n   ```\n2. next", 20), 30);
    assert_eq!(safe_split("```\nx", 4), 5);
    assert_eq!(safe_split("a\n```\nx\n```\nb", 1), 1);
    let mut m = Message::assistant();
    m.apply(Update::Delta("intro\n```rust\nlet a = 1;\n".into()));
    m.apply(row("t", ToolDetail::Plain(String::new()), ToolStatus::Done));
    m.apply(Update::Delta("```\nafter".into()));
    let close = m.text.find("after").unwrap();
    assert_eq!(live(&m), [Block::Text(0..close), Block::Tool(0), Block::Text(close..m.text.len()), Block::Live]);
}

#[test]
fn a_finished_reply_folds_to_its_answer_changes_and_footer() {
    let mut m = Message::assistant();
    m.apply(read("a"));
    m.apply(row("e", ToolDetail::Edit { path: "src/x.rs".into(), diff: "@@ -1 +1 @@\n-a\n+b".into(), added: 1, removed: 1 }, ToolStatus::Done));
    m.apply(Update::Delta("Done.".into()));
    m.finish();
    let text = Block::Text(0..m.text.len());
    assert_eq!(reply_blocks(&m, &view(false, false, false, &|_| false)), [Block::Fold, text.clone(), Block::Changes, Block::Footer]);
    assert_eq!(reply_blocks(&m, &view(false, false, false, &|id| id == "e")), [Block::Fold, Block::Tool(1), text.clone(), Block::Changes, Block::Footer]);
    assert_eq!(reply_blocks(&m, &view(false, true, false, &|_| false)), [Block::Fold, Block::Tool(0), Block::Tool(1), text, Block::Changes, Block::Footer]);
}

#[test]
fn replies_without_steps_or_with_background_work_do_not_fold() {
    let mut plain = Message::assistant();
    plain.apply(Update::Delta("hi".into()));
    plain.finish();
    assert_eq!(reply_blocks(&plain, &view(false, false, false, &|_| false)), [Block::Text(0..2), Block::Footer]);
    let mut bg = Message::assistant();
    bg.apply(row("dev", ToolDetail::Command { command: "pnpm dev".into(), cwd: None, output: String::new(), exit_code: None }, ToolStatus::Background));
    bg.finish();
    assert_eq!(reply_blocks(&bg, &view(false, false, false, &|_| false)), [Block::Tool(0), Block::Footer]);
}

#[test]
fn stopped_rows_and_errors_add_markers_that_survive_the_fold() {
    let mut m = Message::assistant();
    m.apply(row("c", ToolDetail::Plain(String::new()), ToolStatus::Running));
    m.apply(Update::Error("OpenCode service stopped responding\npanic".into()));
    m.finish();
    assert_eq!(reply_blocks(&m, &view(false, false, false, &|_| false)), [Block::Fold, Block::Fatal, Block::Stopped, Block::Footer]);
    let mut cut = Message::assistant();
    cut.apply(Update::Delta("partial".into()));
    cut.finish();
    assert_eq!(reply_blocks(&cut, &view(false, false, true, &|_| false)), [Block::Text(0..7), Block::Stopped, Block::Footer]);
}

#[test]
fn startup_steps_show_until_work_begins_and_failures_stay() {
    let step = |id: &str, state| Update::Notice(Notice { id: id.into(), kind: NoticeKind::Startup, text: id.into(), state, until_ms: None, url: None, detail: None });
    let mut m = Message::assistant();
    m.apply(step("startup-version", NoticeState::Done));
    m.apply(step("startup-session", NoticeState::Running));
    assert_eq!(live(&m), [Block::Startup(vec![0, 1]), Block::Live]);
    m.apply(Update::Delta("hi".into()));
    assert_eq!(live(&m), [Block::Text(0..2), Block::Live]);
    let mut failed = Message::assistant();
    failed.apply(step("startup-spawn", NoticeState::Failed));
    failed.apply(Update::Delta("hi".into()));
    assert_eq!(live(&failed), [Block::Notice(0), Block::Text(0..2), Block::Live]);
}

#[test]
fn a_long_reply_becomes_one_row_per_block() {
    let mut m = Message::assistant();
    for i in 0..300 {
        m.apply(row(&i.to_string(), ToolDetail::Plain(String::new()), ToolStatus::Done));
        m.apply(Update::Delta(format!("step {i}\n")));
    }
    let never = |_: &str| false;
    let rows = transcript_rows(&[Message::user("go"), m], &|ix| ReplyView { live: ix == 1, open: false, stopped: false, agent: true, kept: &never });
    assert!(rows.len() > 600);
    assert_eq!(rows[0], TranscriptRow { message: 0, block: Block::User });
}

#[test]
fn gaps_separate_messages_more_than_steps() {
    let at = |message, block| TranscriptRow { message, block };
    assert_eq!(gap_before(None, &at(0, Block::User)), 0.);
    assert_eq!(gap_before(Some(&at(0, Block::User)), &at(1, Block::Text(0..1))), 32.);
    assert_eq!(gap_before(Some(&at(1, Block::Text(0..1))), &at(1, Block::Tool(0))), 12.);
    assert_eq!(gap_before(Some(&at(1, Block::Tool(0))), &at(1, Block::Tool(1))), 2.);
    assert_eq!(gap_before(Some(&at(1, Block::Tool(1))), &at(1, Block::Live)), 12.);
}
```

`crates/ui/tests/unit/chat/live.rs`:

```rust
use super::*;

#[test]
fn clock_matches_the_prototype() {
    assert_eq!(clock(0), "0s");
    assert_eq!(clock(59_999), "59s");
    assert_eq!(clock(72_000), "1m 12s");
    assert_eq!(clock(123_000), "2m 03s");
    assert_eq!(clock(3_780_000), "1h 3m");
    assert_eq!(short_ms(400), "400ms");
    assert_eq!(short_ms(3_000), "3s");
}

#[test]
fn since_is_none_without_a_start_and_never_negative() {
    assert_eq!(since(None, 5_000), None);
    assert_eq!(since(Some(9_000), 5_000).as_deref(), Some("0s"));
    assert_eq!(since(Some(1_000), 13_000).as_deref(), Some("12s"));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-ui --lib timeline && cargo test -p aether-ui --lib chat::live`
Expected: compile errors.

- [ ] **Step 3: Implement `timeline.rs` and the three formatters in `live.rs`.**
  - **Fences.** A fence opens on a line whose text after any leading spaces starts with 3 or more backticks or tildes. It closes on a line of the same character, at least as long, with only whitespace after it.
  - **Gaps.** `gap_before` treats `Text`, `User`, `SteerAsUser`, `Request`, `Fatal`, `Stopped`, `Changes`, `Live` and `Footer` as spaced (12). Every other block is a step row (2).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-ui --lib timeline && cargo test -p aether-ui --lib chat::live`
Expected: all PASS.

- [ ] **Step 5: Commit (after user approval)**

```bash
git add crates/ui/src/chat/timeline.rs crates/ui/src/chat/live.rs crates/ui/src/chat/mod.rs crates/ui/tests/unit/chat/timeline.rs crates/ui/tests/unit/chat/live.rs
git commit -m "feat(ui): transcript blocks model with read folding, nesting and fold rules"
```

---

### Task 3: One list row per block

The transcript's virtual list moves from one row per message to one row per `Block`. Every block kind is wired to the file that owns it, and today's rendering moves there unchanged. This task fixes:
- whole-reply layout on every update, and the double step clones
- render caches that are never evicted, or keyed by `body.len()`
- the scroll reset on every send
- approvals matched to rows by summary string (removed: the card shows the full command)

**Files:**
- Create: `crates/ui/src/chat/cache.rs`, `crates/ui/tests/unit/chat/cache.rs`
- Create, by moving today's code: `crates/ui/src/chat/markdown.rs` (`markdown()` and `MarkdownStates` from `message.rs`, plus `render_text_chunk`), `notice.rs` (phase 2's minimal notice and steer lines, and the `**Error:**` paragraph), `subagent.rs`, `turn.rs` (the footer, turn-usage popover and Copy/Retry from `AssistantMessage::render`, and `summary` from `thinking.rs`), `crates/ui/tests/unit/chat/turn.rs` (the `summary_*` tests from `tests/unit/chat/thinking.rs`)
- Create: `crates/app/src/app/chat/live.rs`, `crates/app/src/app/chat/turn.rs`
- Modify:
  - ui: `crates/ui/src/chat/mod.rs`, `message.rs` (delete `AssistantMessage`, `ThoughtLabel`, `TimelineItem`, `ThoughtStates`, `render_inline_step`, `render_text_chunk` and `markdown`, and keep `UserMessage` plus `actions_row`/`copy_button`/`menu_item` as `pub(crate)`), `thinking.rs` (delete `ThinkingPanel`, `PanelLabel`, `LIVE_ROWS` and `summary`, and add `ThoughtRow` from `render_inline_step`'s thought arm), `tool_row.rs`, `live.rs`, `scroller.rs` (the row wrapper drops `.pb_8()`)
  - ui tests: `crates/ui/tests/unit/chat/message.rs` (keep `user_message_renders_its_attachment_strip`, and delete the `AssistantMessage` tests), `crates/ui/tests/unit/chat/thinking.rs`
  - app: `crates/app/src/app/mod.rs` (`ChatApp.rows`, new `Chat` fields), `crates/app/src/app/composer/message.rs` (`render_message` becomes `render_row`), `crates/app/src/app/render.rs` (the scroller renderer), `crates/app/src/app/chat/{mod.rs, send.rs, edit.rs, commands.rs, manage.rs}`, `crates/app/src/hitl.rs` (`focus_ask`)
  - app assets: `crates/app/src/assets.rs` (icons), `crates/app/tests/unit/assets.rs`

**Interfaces:**
- Consumes: Task 2 (`Block`, `ReplyView`, `TranscriptRow`, `transcript_rows`, `gap_before`, `since`), and phase 2's `Chat.last_event_ms`, `Agent::health`, `Agent::attach` and `Message::error`.
- Produces, in ui:
  - `cache.rs`: `pub(crate) struct RenderCache<V>`. `new(cap: usize)`, `get(&mut self, key: &SharedString) -> Option<&mut V>` (marks the entry used), `insert(&mut self, key: SharedString, value: V)` (evicts the least recently used entry past `cap`). It backs `MarkdownStates` (cap 512), `ToolRows.open` (cap 4096) and `ToolRows.diffs` (cap 256).
  - **Stable keys**, where `msg` is `"{chat.session}-{message ix}"`:
    - text `"{msg}-t{start}"`
    - thought `"{msg}-s{step}"`
    - tool and subagent `tool_key(session, id)`
    - reads `"{msg}-r{first step}"`
    - notice `"{msg}-n{notice id}"`
    - fold `"{msg}-fold"`, footer `"{msg}-foot"`, live `"{msg}-live"`, fatal `"{msg}-fatal"`
  - `tool_row.rs`:
    - `pub fn tool_key(session: &str, tool_id: &str) -> SharedString` (`"{session}-{id}"`)
    - `pub fn open_tool(cx: &mut App, key: SharedString)`
    - `pub fn opened_tools(cx: &mut App, session: &str) -> HashSet<String>` (the tool ids of open rows)
    - `pub type Reflow`, `pub type OnOpen`
    - `ToolRow` becomes `pub`. It has `new(key, call)`, `.cwd(Option<SharedString>)`, `.default_open(bool)`, `.on_diff(Option<OnOpen>)`, `.on_reflow(Option<Reflow>)` and `.nested(bool)`. Nested rows are 24 px.
    - `pub struct ReadGroup`, with `new(key, Vec<ToolCall>)`, `.cwd(..)` and `.on_reflow(..)`.
    - `pub struct SearchRow::new(query: String, results: Option<Result<usize, String>>)` and `pub struct SkillRow::new(name: String)`, both moved from `render_inline_step`.
  - `thinking.rs`: `pub struct ThoughtRow`, with `new(key, text: SharedString, ms: Option<u64>, live: bool)` (`ms` is the live elapsed time while live), `.cwd(..)` and `.on_reflow(..)`.
  - `subagent.rs`: `pub struct SubagentCard`, with `new(key, call: ToolCall, children: Vec<ToolCall>)`, `.cwd(..)`, `.on_reflow(..)` and `.on_show_ask(Option<OnOpen>)`. It starts minimal: the Task row as a `ToolRow`, then the children as nested `ToolRow`s.
  - `markdown.rs`: `pub struct Markdown`, with `new(key, text: SharedString)`, `.cwd(Option<SharedString>)`, `.live(bool)`, `.muted(bool)` and `.on_reflow(Option<Reflow>)`.
  - **Minimal views whose constructors are called only from their owner's app builder,** so the owner may change them:
    - `live.rs`: `StatusLine` (a spinner, "Working" and `since(turn_at)`)
    - `turn.rs`: `FoldLine` and `TurnFooter`
    - `notice.rs`: `NoticeLine`, `StartupList`, `SteerMarker`, `StoppedMarker` and `FatalCard`
  - `mod.rs`:
    - new modules `cache`, `markdown`, `notice`, `subagent` and `turn`
    - glob re-exports (`pub use x::*;`) for `composer`, `live`, `markdown`, `notice`, `request`, `subagent`, `thinking`, `timeline`, `tool_row`, `turn` and `hero`, so later tasks never edit `mod.rs`
    - remove the `AssistantMessage`, `TurnUsage`, `ThinkingPanel` and `PanelLabel` exports
- Produces, in app:
  - `ChatApp.rows: Vec<TranscriptRow>` for the active chat.
  - `fn sync_rows(&mut self, touched: Option<usize>, cx: &mut Context<Self>)`:
    1. Rebuild with `transcript_rows`. The view is `live = busy && ix == last`, `open = open_steps[ix]`, `stopped = chat.stopped && ix == last`, `agent = is_agent_chat`, and `kept` from `opened_tools`.
    2. Splice the scroller with `splice_range(&old, &new)`.
    3. Remeasure every row of message `touched`.
  - `fn remeasure_row(&mut self, row: usize, cx)` replaces `remeasure(message)`. Components' `on_reflow` closures remeasure their own row.
  - `fn reveal_tool(&mut self, tool_id: &str, cx)`: `open_tool`, then `scroller.scroll_to_item(row containing it)`.
  - `fn restart_agent(&mut self, ix: usize, cx)` in `chat/manage.rs`:
    - `agent.close_session(key)`, then `agent.attach(settings, session)`, built as `submit` builds them.
    - `chat.restarted_ms = now_ms()`.
    - It does nothing for OpenRouter.
  - `fn focus_ask(&mut self, window, cx)` in `hitl.rs`: scrolls to the active chat's first pending request row.
  - **Builders the dispatcher calls,** each in its owner's file. All take `(&self, msg: usize, view: &Entity<Self>) -> AnyElement`, with a `step: usize` or `steps: &[usize]` argument where the block carries one:
    - `live_row` in `chat/live.rs` (Task 10)
    - `fold_row` and `footer_row` in `chat/turn.rs` (Task 13)
    - `notice_row`, `startup_row`, `steer_row`, `stopped_row` and `fatal_row` in `chat/manage.rs` (Task 9)
  - `Chat` fields:
    - `health: Option<(SessionHealth, i64)>`: the last health check and when it ran
    - `stall_kept_ms: i64`: the `last_event_ms` at which the user chose "Keep waiting"
    - `restarted_ms: i64`
  - `assets.rs` gains `Lightbulb`, `Shield`, `ShieldX`, `Layers`, `Zap`, `ArrowLeftRight`, `Plug`, `Files`, `FilePlus`, `Clock`, `UnfoldVertical` and `LogIn`.
- **The dispatcher, `render_row(row, view)`.** Each row is wrapped in `div().pt(px(gap_before(prev, row)))`.

  | Block | Renders |
  |---|---|
  | `User` | the existing `UserMessage` code |
  | `SteerAsUser(i)` | `UserMessage` with the steer text, and no Edit |
  | `Text(r)` | `Markdown`, with `.live(true)` only for the last `Text` block of a live reply |
  | `Thought(i)` | `ThoughtRow`. `ms` is `Step::Thinking.ms`, or `live_thinking_ms()` for the open one. Legacy `Thought` gets `None`. |
  | `Tool(i)`, `Reads`, `Search`, `Skill` | the tool row types above |
  | `Subagent` | `SubagentCard`. Its `on_show_ask` calls `focus_ask`. |
  | `Request(i)` | `render_request` |
  | `Changes` | today's `ChangesCard` |
  | everything else | its builder |

- [ ] **Step 1: Write the failing tests**

`crates/ui/tests/unit/chat/cache.rs`:

```rust
use super::*;

#[test]
fn evicts_the_least_recently_used_entry_past_its_cap() {
    let mut cache = RenderCache::new(2);
    cache.insert("a".into(), 1);
    cache.insert("b".into(), 2);
    assert_eq!(cache.get(&"a".into()).copied(), Some(1));
    cache.insert("c".into(), 3);
    assert!(cache.get(&"b".into()).is_none(), "b was the least recently used");
    assert_eq!((cache.get(&"a".into()).copied(), cache.get(&"c".into()).copied()), (Some(1), Some(3)));
}
```

`crates/app/tests/unit/assets.rs`:

```rust
#[test]
fn serves_transcript_icons() {
    use gpui_kit::assets::IconName as I;
    for icon in [I::Lightbulb, I::Shield, I::ShieldX, I::Layers, I::Zap, I::ArrowLeftRight, I::Plug,
        I::Files, I::FilePlus, I::Clock, I::UnfoldVertical, I::LogIn] {
        let path = icon.path();
        assert!(AppAssets.load(&path).unwrap().is_some(), "{path}");
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-ui --lib cache && cargo test -p aether --lib assets`
Expected: compile error (`RenderCache`), and a FAIL for the missing icons.

- [ ] **Step 3: Implement the cache and icons, then move the code into its owner files.** Change no visuals beyond:
  - removing `ShimmerText`
  - removing the auto-open of rows whose summary matched an approval
  - the new row gaps
  - `RenderCache` eviction is O(n) on overflow. Mark it `// ponytail: O(n) LRU scan on overflow, switch to a linked map if profiles show it`.

- [ ] **Step 4: Rewire the app.**
  - `apply`, `apply_cli_meta`, `finish_run`, `take_unprompted`, `answer_request`, `set_steps_open` and `submit` call `sync_rows` with the message they touched.
  - `submit` no longer calls `scroller.reset`. It appends through `sync_rows`, then calls `scroll_to_end`. This fixes chats over 100 messages losing their measured heights on every send.
  - Only `select` (chat switch), `rewind` (`edit.rs`) and clear (`commands.rs`) reset the scroller, with `rows.len()`.

- [ ] **Step 5: Run the tests and build**

Run: `cargo test -p aether-ui --lib && cargo test -p aether --lib && cargo build -p aether`
Expected: all PASS, and the build succeeds.

- [ ] **Step 6: Manual check** (`cargo run -p aether`)
  1. Open a chat with over 100 messages and send a message. The scroll position doesn't jump, and earlier rows keep their heights.
  2. Run a Claude turn with 50+ tool calls. Scrolling stays smooth while it streams, with no visible relayout of rows offscreen.
  3. Every block kind still renders: text, thought, tool, reads, search, skill, notice, steer, request card, changes, footer and status line.

- [ ] **Step 7: Commit (after user approval)**

```bash
git add crates/ui crates/app
git commit -m "perf(chat): one virtual-list row per transcript block, bounded render caches"
```

---

### Task 4: Markdown and syntax highlighting

**Files:**
- Modify: `Cargo.toml` (workspace `gpui-kit` features)
- Create: `crates/ui/src/chat/highlight.rs`, `crates/ui/tests/unit/chat/highlight.rs`
- Modify: `crates/ui/src/chat/markdown.rs`, `crates/ui/src/chat/mod.rs` (`mod highlight; pub use highlight::*;`, and drop `mod code_block` and its exports)
- Delete: `crates/ui/src/chat/code_block.rs`, `crates/ui/tests/unit/chat/code_block.rs`

**Interfaces:**
- Consumes: `Markdown` (Task 3), and `link_inline_code` and `open_link` (`links.rs`).
- Produces, in `highlight.rs`:
  - `pub fn highlight_with(code: &str, lang: &str, theme: &HighlightTheme) -> Vec<(Range<usize>, HighlightStyle)>`. It uses `gpui_kit::component::highlighter::SyntaxHighlighter` with a thread-local cache per language, the way gpui-component's own code-block highlighter does, and `gpui_kit::component::Rope`.
  - `pub fn highlight(code: &str, lang: &str, cx: &App) -> Vec<(Range<usize>, HighlightStyle)>`: `highlight_with` with `&cx.theme().highlight_theme`.
  - `pub fn styled_code(code: SharedString, lang: &str, cx: &App) -> StyledText`, for command lines and diff lines.
  - `pub fn lang_for_path(path: &str) -> &'static str`, by extension:
    - rs → rust, ts → typescript, tsx → tsx, js/jsx/mjs/cjs → javascript, json → json
    - css → css, html → html, py → python, go → go, toml → toml, yml/yaml → yaml, md → markdown
    - sh/bash/zsh → bash, sql → sql, swift → swift, java → java, c/h → c, cpp/cc/hpp → cpp, rb → ruby
    - anything else → "text"
  - `pub fn lang_for_fence(info: &str) -> &'static str`: the same names from a fence's first word, plus the aliases sh/shell/zsh/console → bash, ts → typescript, js → javascript, py → python, yml → yaml, rs → rust.
- **`Markdown` internals** (constructor unchanged):
  - One `TextView` per chunk. Fenced code renders inside it, so `~~~` fences, longer fences, fences in list items and tables all follow gpui-base's CommonMark parser. Selection runs across code blocks.
  - `code_block_actions`: the language label, then a `Clipboard` copy button ("Copy code").
  - Code-block style: radius 9, max height 340 px, `overflow.y = Scroll`, and the theme mono family. This removes the hard-coded "JetBrainsMonoNL Nerd Font".
  - **While live,** only `state.set_text(raw)` runs (gpui-base's append path). It never calls `link_inline_code`.
  - **Once not live,** when `cwd` is set, the text contains a backtick and no linked copy is cached: run `link_inline_code` on `cx.background_executor()`. Then store the linked text, `set_text(linked)` and reflow. `path.exists()` never runs on the UI thread.
  - `.muted(true)` sets the text colour to `muted_foreground` (for thoughts).
- **Workspace feature set:** `gpui-kit = { version = "0.7.3", features = ["tree-sitter-rust", "tree-sitter-typescript", "tree-sitter-tsx", "tree-sitter-javascript", "tree-sitter-bash", "tree-sitter-css", "tree-sitter-html", "tree-sitter-python", "tree-sitter-go", "tree-sitter-toml", "tree-sitter-yaml", "tree-sitter-markdown", "tree-sitter-diff", "tree-sitter-sql", "tree-sitter-swift", "tree-sitter-java", "tree-sitter-c", "tree-sitter-cpp", "tree-sitter-ruby"] }`. JSON comes with `tree-sitter`. This curated set is the lightest that covers the languages agents write, so `tree-sitter-languages` (all) is not used.

- [ ] **Step 1: Write the failing tests**

```rust
use super::*;
use gpui_kit::component::highlighter::HighlightTheme;

#[test]
fn languages_follow_the_extension_and_fence() {
    assert_eq!(lang_for_path("src/components/Hero.tsx"), "tsx");
    assert_eq!(lang_for_path("a/b.rs"), "rust");
    assert_eq!(lang_for_path("Makefile"), "text");
    assert_eq!(lang_for_fence("sh"), "bash");
    assert_eq!(lang_for_fence("ts title=a.ts"), "typescript");
}

#[test]
fn code_gets_coloured_ranges_and_plain_text_does_not() {
    let theme = HighlightTheme::default_dark();
    assert!(highlight_with("fn main() {}", "rust", &theme).iter().any(|(_, s)| s.color.is_some()));
    assert!(highlight_with("plain words", "text", &theme).iter().all(|(_, s)| s.color.is_none()));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-ui --lib highlight`
Expected: compile errors.

- [ ] **Step 3: Implement** the feature list, `highlight.rs` and the `Markdown` internals. Then delete `code_block.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-ui --lib && cargo build -p aether`
Expected: all PASS, and the build succeeds.

- [ ] **Step 5: Manual check against the prototype chips "Writing" and "Done", in both themes**
  1. A ```` ```tsx ```` block, a `~~~css` block and a four-backtick block are coloured, and each has a language label and Copy.
  2. A fenced block inside "1. … 2. … 3." keeps the numbering.
  3. The table from the "Done" scenario renders with a header row.
  4. `` `src/styles.css` `` becomes a link after the reply ends, and clicking it opens the file.
  5. A 600-line code block scrolls inside its 340 px box.

- [ ] **Step 6: Commit (after user approval)**

```bash
git add Cargo.toml Cargo.lock crates/ui
git commit -m "feat(chat): highlighted markdown in one view, file links resolved off the UI thread"
```

---

### Task 5: Diffs

**Files:**
- Modify: `crates/ui/src/chat/diff_view.rs`
- Test: `crates/ui/tests/unit/chat/diff_view.rs`

**Interfaces:**
- Consumes: `parse`, `DiffRow` and `hunk_header` (existing), `styled_code` and `lang_for_path` (Task 4), and `Segmented` (`components`).
- Produces:
  - `pub fn word_marks(old: &str, new: &str) -> Option<(Range<usize>, Range<usize>)>`. Take the common char prefix and suffix. Return `None` when prefix plus suffix is fewer than 4 chars. Ranges are byte offsets on char boundaries.
  - `pub fn line_marks(rows: &[DiffRow]) -> Vec<Option<Range<usize>>>`, one entry per row. Within each run of removed lines followed by added lines, the i-th removed line pairs with the i-th added line.
  - `pub fn hunk_title(header: &str) -> Option<(String, Option<String>)>`: `("Line {new_start}", context after the second @@)`.
  - `#[derive(Debug, PartialEq, Eq)] pub struct Gap { pub before_row: usize, pub old_start: u32, pub new_start: u32, pub lines: u32 }` and `pub fn gaps(rows: &[DiffRow]) -> Vec<Gap>`: the unchanged lines between positioned hunks.
  - `#[derive(Debug, PartialEq, Eq)] pub enum Split { Full(usize), Pair(Option<usize>, Option<usize>) }` and `pub fn split_rows(rows: &[DiffRow]) -> Vec<Split>`. A context row pairs with itself, removed and added runs pair index by index, and hunk, file and note rows are `Full`.
  - `DiffView` keeps its constructor and builders, and adds `.file(Option<PathBuf>)` (the absolute path, used to expand gaps).
- **The view** (prototype `.diff`):

  | Part | Shows |
  |---|---|
  | Header | `File` icon, directory muted plus file name in the foreground (mono, middle-truncated), a "new" pill (success tint) when the first hunk is `@@ -0,0`, `+N` and `−M`, a `Segmented` "Unified \| Split", Copy, and "Open" (`ExternalLink`, calls `on_open`) |
  | Hunk row | `ChevronRight`, "Line N" and the mono context muted, on an `info` wash |
  | Gap row | `UnfoldVertical` and "{n} unchanged lines". Click reads `file` on a background task and inserts those lines as context rows, numbered on both sides. If the file is gone, the row reads "File not found". |
  | Lines | Both line numbers, a `+`/`−` gutter, and an add/remove wash at 0.10. Word marks use the same token at 0.32. Code is coloured with `styled_code(text, lang_for_path(path))`. |
  | Split | Two columns. An empty side is a `muted` wash. |

  The split state lives per diff key in a `RenderCache<bool>`. Unified is the default. The side panel's `DiffLines` reuses the same row look, so it gains word marks too.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn word_marks_cover_the_changed_middle() {
    assert_eq!(word_marks("        See work", "        See selected work"), Some((12..12, 12..21)));
    assert_eq!(word_marks("let a = 1;", "let b = 2;"), Some((4..9, 4..9)));
    assert_eq!(word_marks("naïve one", "naïve two"), Some((7..10, 7..10)));
    assert_eq!(word_marks("abc", "xyz"), None);
}

#[test]
fn line_marks_pair_removed_and_added_runs() {
    let rows = parse("@@ -1,2 +1,2 @@\n-let a = 1;\n-x\n+let b = 2;\n+y\n");
    assert_eq!(line_marks(&rows), [None, Some(4..9), None, Some(4..9), None]);
}

#[test]
fn hunk_headers_read_line_and_context() {
    assert_eq!(hunk_title("@@ -40,6 +44,7 @@ function Stats"), Some(("Line 44".into(), Some("function Stats".into()))));
    assert_eq!(hunk_title("@@ -1 +1 @@"), Some(("Line 1".into(), None)));
    assert_eq!(hunk_title("@@"), None);
}

#[test]
fn gaps_between_hunks_count_hidden_lines() {
    let rows = parse("@@ -1,3 +1,3 @@\n a\n-b\n+c\n d\n@@ -10,2 +10,2 @@\n x\n-y\n+z\n");
    assert_eq!(gaps(&rows), [Gap { before_row: 5, old_start: 4, new_start: 4, lines: 6 }]);
}

#[test]
fn split_pairs_removed_and_added_runs() {
    let rows = parse("@@ -1,3 +1,4 @@\n a\n-b\n-c\n+x\n+y\n+z\n d\n");
    assert_eq!(split_rows(&rows), [Split::Full(0), Split::Pair(Some(1), Some(1)), Split::Pair(Some(2), Some(4)),
        Split::Pair(Some(3), Some(5)), Split::Pair(None, Some(6)), Split::Pair(Some(7), Some(7))]);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-ui --lib diff_view`
Expected: compile errors.

- [ ] **Step 3: Implement the functions and the view.**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-ui --lib diff_view && cargo build -p aether`
Expected: all PASS.

- [ ] **Step 5: Manual check against the prototype chip "Done"** (the Hero.tsx diff), in both themes:
  - Toggle Unified and Split.
  - "See work" → "See selected work" marks "selected ".
  - The second hunk header reads "Line 44 · function Stats".
  - Expanding the gap fills 24 lines with numbers.
  - "Open" opens the file at the hunk's line.

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/ui/src/chat/diff_view.rs crates/ui/tests/unit/chat/diff_view.rs
git commit -m "feat(chat): split diffs, word marks, readable hunk headers, expandable gaps"
```

---

### Task 6: Tool rows (after Task 5)

**Files:**
- Modify: `crates/ui/src/chat/tool_row.rs`, `crates/ui/src/chat/changes.rs` (`applied_edit`)
- Test: `crates/ui/tests/unit/chat/tool_row.rs`, `crates/ui/tests/unit/chat/changes.rs`

**Interfaces:**
- Consumes:
  - `ToolCall.started_ms` (Task 1)
  - `since`, `short_ms` and `clock` (Task 2)
  - `ToolRow`, `ReadGroup`, `SearchRow` and `SkillRow`, whose constructors stay as Task 3 set them
  - `styled_code` (Task 4)
  - `DiffView::file` (Task 5)
- Produces:
  - `pub const OUTPUT_LINES: usize = 12;`
  - `pub fn tail(output: &str, lines: usize) -> (&str, usize)`: the last `lines` lines and how many are hidden.
  - `pub fn status_word(call: &ToolCall) -> Option<String>`:
    - "Exit {code}" when the exit code is non-zero, else "Failed" for `Failed`
    - "Needs approval", "Background", "Stopped" and "Denied" for those statuses
    - `None` otherwise, including exit code 0
  - `#[derive(Debug, PartialEq, Eq, Clone, Copy)] pub enum Tone { Plain, Danger, Warning, Info }` and `pub fn tone(status: ToolStatus) -> Tone`. `Failed` is `Danger`, `Waiting` is `Warning`, `Background` is `Info`, and every other status is `Plain` (spec: colour only for failed, waiting and background).
  - `pub fn timer_text(call: &ToolCall, now_ms: i64) -> Option<String>`:
    - `Running`, `Waiting` or `Background`: `since(started_ms, now)`
    - `Done`: `clock(ms)` when it is at least 1 s
    - `Failed`: `short_ms(ms)`
    - otherwise `None`
  - `pub fn read_group_label(calls: &[ToolCall]) -> (String, String)`: ("Read N files", the file names joined with ", ").
  - `pub fn is_new_file(diff: &str) -> bool`: the first hunk header is `-0,0`.
- **The row** (prototype `.row`): 28 px tall, or 24 px when nested. The rubric's transparent 1 px border and the focus ring stay.
  - **Line.** Kind icon (15 px), title (13 px, medium, foreground), target (mono 12.5, muted, middle-truncated for paths), then right-aligned meta and a chevron when expandable.
  - **Icons.** Command `SquareTerminal`, Read `FileText`, Edit `FilePen`, a new file `FilePlus`, Search `Search`, Web `Globe`, an `Other` named `mcp__…` `Plug`, Skill `BookOpen`, Task `Bot`, Plan `ListChecks`, anything else `Wrench`. The icon takes the tone colour, and is muted when `Plain`.
  - **Meta.** For live rows (`Running`, `Waiting`, `Background`): a spinner for `Running`, a pill with the icon and `status_word` (`Shield` for Waiting, a static `info` dot for Background), and `timer_text`. Otherwise: `+N` and `−M` for edits, "{n} matches" for searches, then `status_word` as a pill (`ShieldX` Denied, `Square` Stopped, and the danger tint for Failed), then `timer_text`.
  - **Error line.** For `Failed`, the first line of `error` stays visible under the row, in mono 12 and `danger_text`. For `Denied`, the reason shows in muted text.
  - **Command body** (prototype `.term`):
    - Header: `SquareTerminal`, `$` plus `styled_code(command, "bash")`, then the cwd muted on the right.
    - Output: `tail(output, OUTPUT_LINES)` in mono 12. While running it follows the tail. "Show all {n} lines" swaps in the full output, in a scroll box capped at 420 px.
    - Footer: "exit {n}" in danger only when non-zero, the duration, "{n} lines" and Copy. A `Background` row is stopped from the background tray (Task 12), so the row needs no new callback.
    - Auto-open: rows that are `Running` with output, or `Waiting`, open by default. On first render, that open state is written into `ToolRows`, so the row stays open when it finishes and the layout doesn't jump.
  - **Edits.** A finished edit's click toggles its inline `DiffView`, with `.file(abs path)`. "Open diff" (the side panel, `on_diff`) moves to the context menu. A streaming edit shows its growing diff, open.
  - **`ReadGroup`.** `Files` icon, "Read N files", the names as the target, and a chevron. Expanding it lists the nested rows.
  - **Nested.** A nested row's body has no outer border, so no card sits inside the subagent card.
- `changes.rs`: only `Done` edits (and `Unknown`, which renders like `Done`) count as applied. `Stopped`, `Running` and `Waiting` edits are left out.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn output_tail_keeps_the_last_twelve_lines() {
    let out = (1..=20).map(|i| i.to_string()).collect::<Vec<_>>().join("\n");
    assert_eq!(tail(&out, OUTPUT_LINES), ("9\n10\n11\n12\n13\n14\n15\n16\n17\n18\n19\n20", 8));
    assert_eq!(tail("a\nb\n", OUTPUT_LINES), ("a\nb\n", 0));
}

#[test]
fn status_words_show_exit_codes_only_when_nonzero() {
    let cmd = |code, status| ToolCall { status, ..ToolCall::new("c", "Run command",
        ToolDetail::Command { command: "ls".into(), cwd: None, output: String::new(), exit_code: code }) };
    assert_eq!(status_word(&cmd(Some(0), ToolStatus::Done)), None);
    assert_eq!(status_word(&cmd(Some(2), ToolStatus::Failed)).as_deref(), Some("Exit 2"));
    assert_eq!(status_word(&cmd(None, ToolStatus::Failed)).as_deref(), Some("Failed"));
    assert_eq!(status_word(&cmd(None, ToolStatus::Waiting)).as_deref(), Some("Needs approval"));
    assert_eq!(status_word(&cmd(None, ToolStatus::Background)).as_deref(), Some("Background"));
}

#[test]
fn only_failed_waiting_and_background_rows_take_colour() {
    use ToolStatus::*;
    assert_eq!([Running, Done, Stopped, Denied, Failed, Waiting, Background].map(tone),
        [Tone::Plain, Tone::Plain, Tone::Plain, Tone::Plain, Tone::Danger, Tone::Warning, Tone::Info]);
}

#[test]
fn timers_follow_status_and_skip_unknown_starts() {
    let mut c = ToolCall::new("c", "Run", ToolDetail::Plain(String::new()));
    c.started_ms = Some(1_000);
    assert_eq!(timer_text(&c, 13_000).as_deref(), Some("12s"));
    c.status = ToolStatus::Done;
    c.ms = Some(400);
    assert_eq!(timer_text(&c, 13_000), None, "sub-second finished rows hide their time");
    c.ms = Some(4_200);
    assert_eq!(timer_text(&c, 13_000).as_deref(), Some("4s"));
    c.status = ToolStatus::Failed;
    c.ms = Some(400);
    assert_eq!(timer_text(&c, 13_000).as_deref(), Some("400ms"));
    c.status = ToolStatus::Running;
    c.started_ms = None;
    assert_eq!(timer_text(&c, 13_000), None, "rows saved before timers show none");
}

#[test]
fn read_groups_name_their_files_and_new_files_are_detected() {
    let r = |p: &str| ToolCall::new(p, "Read", ToolDetail::Read { path: p.into(), range: None });
    assert_eq!(read_group_label(&[r("src/routes/index.tsx"), r("src/components/Header.tsx")]),
        ("Read 2 files".to_string(), "index.tsx, Header.tsx".to_string()));
    assert!(is_new_file("@@ -0,0 +1,14 @@\n+:root {"));
    assert!(!is_new_file("@@ -1,2 +1,3 @@\n a"));
}
```

`crates/ui/tests/unit/chat/changes.rs`:

```rust
#[test]
fn only_finished_edits_count_as_applied() {
    let edit = |id: &str, status| Step::Tool(ToolCall { status, ..ToolCall::new(id, "Edit",
        ToolDetail::Edit { path: format!("{id}.rs"), diff: String::new(), added: 1, removed: 0 }) });
    let steps = [edit("a", ToolStatus::Done), edit("b", ToolStatus::Stopped), edit("c", ToolStatus::Running), edit("d", ToolStatus::Waiting)];
    assert_eq!(reply_edits(&steps, None).iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["a.rs"]);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-ui --lib tool_row && cargo test -p aether-ui --lib changes`
Expected: compile errors, and a FAIL for `only_finished_edits_count_as_applied`.

- [ ] **Step 3: Implement the functions and the row.** Update `durations_read_short`, or delete it if `duration` is left unused.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-ui --lib && cargo build -p aether`
Expected: all PASS.

- [ ] **Step 5: Manual check against the prototype chips "Running tools", "Errors" and "Background",** in both themes:
  - The `pnpm build` output streams with a timer, shows its last 12 lines, and stays open when done.
  - The `git log` row shows "Exit 128" and its error line without expanding.
  - Three reads fold into "Read 3 files".
  - The finished Hero.tsx edit expands inline.
  - The `pnpm dev` row reads "Background" in the info colour with a live timer.
  - Running rows are not coloured.

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/ui/src/chat/tool_row.rs crates/ui/src/chat/changes.rs crates/ui/tests/unit/chat/tool_row.rs crates/ui/tests/unit/chat/changes.rs
git commit -m "feat(chat): tool rows with live timers, capped output, inline diffs, read groups"
```

---

### Task 7: Thoughts

**Files:**
- Modify: `crates/ui/src/chat/thinking.rs` (`ThoughtRow`; `live_label` stays for `project_page`)
- Test: `crates/ui/tests/unit/chat/thinking.rs`

**Interfaces:**
- Consumes: `ThoughtRow::new(key, text, ms, live)` (Task 3), `Markdown` with `.muted(true)` (Task 4), and `short_ms` (Task 2).
- Produces:
  - `pub fn first_sentence(text: &str) -> String`. It strips `**`, then cuts at the first ". " or ".\n" (keeping the period). Failing that, it cuts at the first line break. It trims the result.
  - `pub fn thought_label(ms: Option<u64>) -> String`: "Thought for {short_ms}", or "Thought".
- **The row** (prototype `.thought`):
  - **Done.** `Lightbulb` muted, then `thought_label` (regular weight, muted), then the `first_sentence` preview (muted, one line, ellipsis) while closed, and a chevron. Click toggles the body.
  - **Live.** A spinner, "Thinking" (plain label, no shimmer) and the live timer from `short_ms(ms)`. The row is open by default, and the user can close it.
  - **Body.** A 1 px left border in `border`, 14 px of padding, and `Markdown` muted 13.5 px.
  - Open state is kept per key in a `RenderCache<bool>`, which replaces `ThoughtStates`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn preview_is_the_first_sentence_without_bold() {
    assert_eq!(first_sentence("**Scoping the redesign.** Looking at routes first."), "Scoping the redesign.");
    assert_eq!(first_sentence("No git repo, so I cannot lean on history. Mapping `src` directly."), "No git repo, so I cannot lean on history.");
    assert_eq!(first_sentence("one line no period\nsecond"), "one line no period");
    assert_eq!(first_sentence("  "), "");
}

#[test]
fn each_thought_has_its_own_duration() {
    assert_eq!(thought_label(Some(3_000)), "Thought for 3s");
    assert_eq!(thought_label(Some(400)), "Thought for 400ms");
    assert_eq!(thought_label(Some(72_000)), "Thought for 1m 12s");
    assert_eq!(thought_label(None), "Thought");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-ui --lib thinking`
Expected: compile errors.

- [ ] **Step 3: Implement.**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-ui --lib thinking && cargo build -p aether`
Expected: all PASS, including `live_label_names_the_latest_running_tool`.

- [ ] **Step 5: Manual check against the prototype chip "Thinking",** in both themes. The live thought streams open as markdown (bold renders) with a ticking timer. When done, it collapses to "Thought for 3s · Scoping the redesign.". A reply with three thoughts shows three durations.

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/ui/src/chat/thinking.rs crates/ui/tests/unit/chat/thinking.rs
git commit -m "feat(chat): timed thoughts with previews, live thought as markdown"
```

---

### Task 8: Subagent cards

**Files:**
- Modify: `crates/ui/src/chat/subagent.rs`
- Create: `crates/ui/tests/unit/chat/subagent.rs`

**Interfaces:**
- Consumes:
  - `SubagentCard::new(key, call, children)`, `.on_show_ask` (Task 3)
  - `ToolRow::nested` (Task 3)
  - `ToolDetail::Task { description, output, agent, prompt, tools, tokens, activity }` (phase 2)
  - `Markdown` (Task 4), `since`/`clock` (Task 2) and `format_tokens` (`context_ring.rs`)
- Produces:
  - `pub const NESTED_ROWS: usize = 5;`
  - `pub fn earlier(reported: u32, known: usize, all: bool) -> (usize, Range<usize>)`. With `total = max(reported, known)`, it returns the hidden count and the range of known children to show (the last `NESTED_ROWS`, or all of them).
  - `pub fn earlier_label(n: usize) -> String`: "+ {n} earlier tool call(s)".
  - `#[derive(Debug, PartialEq, Eq, Clone, Copy)] pub enum CardStatus { Starting, Running, Waiting, Background, Done, Failed, Stopped }` and `pub fn card_status(call: &ToolCall, children: &[ToolCall]) -> CardStatus`. A live Task with a `Waiting` child is `Waiting`. Otherwise the status follows the Task row, and `Running` with no `started_ms` and no children is `Starting`.
  - `pub fn waiting_line(children: &[ToolCall]) -> Option<String>`: "Waiting for your approval below · {title} {summary}" for the first `Waiting` child.
- **The card** (prototype `.agent`, with the rubric fixes):
  - **Shell.** Radius 12, a `secondary` wash, and a 1 px border in the status colour. The colours are `info` for running, starting and background, `mango` for waiting, `danger` for failed, `success` at 0.6 for done, and `border` for stopped. No side stripe.
  - **Header.** `Bot` in the status colour, the name in semibold (`agent`, falling back to the title), the type pill (`agent`), and the description (muted, ellipsis). On the right:
    - the status pill with its icon: a spinner and "Running", `Shield` and "Needs approval", `CircleX` and "Failed", `CircleCheck` and "Done", `Square` and "Stopped", an `info` dot and "Background", or "Starting"
    - the timer (live `since`, else `clock(ms)`)
    - "{tools} tools", "{format_tokens(tokens)} tok" and a chevron
  - **Second line, while collapsed:**
    - running or background: a spinner and `activity` (muted, ellipsis)
    - waiting: `Shield`, then `waiting_line` in `mango_text`. Clicking it calls `on_show_ask`.
    - failed: the first line of `error` in mono, `danger_text`
    - done: `output` as `Markdown` (muted)
  - **Expanded.** "Prompt · {prompt}" muted, with a 1 px dashed rule below it. Then `earlier_label`, which shows all known rows on click. Then the nested `ToolRow`s for the shown range, and the result as `Markdown` when done. An approval card is never rendered inside the card: Task 11 places it as the next row.
  - A waiting card opens by default. Open state lives in a `RenderCache<bool>`.

- [ ] **Step 1: Write the failing tests**

```rust
use super::*;
use aether_core::{ToolCall, ToolDetail, ToolStatus};

#[test]
fn nested_rows_show_the_last_five_and_count_the_rest() {
    assert_eq!(earlier(14, 3, false), (11, 0..3));
    assert_eq!(earlier(0, 12, false), (7, 7..12));
    assert_eq!(earlier(0, 12, true), (0, 0..12));
    assert_eq!(earlier(20, 12, true), (8, 0..12));
    assert_eq!(earlier_label(1), "+ 1 earlier tool call");
    assert_eq!(earlier_label(11), "+ 11 earlier tool calls");
}

#[test]
fn card_status_reads_the_row_and_its_children() {
    let row = |id: &str, status| ToolCall { status, started_ms: Some(1), ..ToolCall::new(id, "Run", ToolDetail::Plain(String::new())) };
    let waiting = row("c", ToolStatus::Waiting);
    assert_eq!(card_status(&row("t", ToolStatus::Running), &[waiting.clone()]), CardStatus::Waiting);
    assert_eq!(card_status(&row("t", ToolStatus::Running), &[]), CardStatus::Running);
    assert_eq!(card_status(&ToolCall::new("t", "Run", ToolDetail::Plain(String::new())), &[]), CardStatus::Starting);
    assert_eq!(card_status(&row("t", ToolStatus::Background), &[]), CardStatus::Background);
    assert_eq!(card_status(&row("t", ToolStatus::Done), &[waiting.clone()]), CardStatus::Done);
    assert_eq!(card_status(&row("t", ToolStatus::Stopped), &[]), CardStatus::Stopped);
    assert_eq!(waiting_line(&[waiting]).as_deref(), Some("Waiting for your approval below · Run"));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-ui --lib subagent`
Expected: compile errors.

- [ ] **Step 3: Implement.** `waiting_line` drops the summary when it is empty, so the test row reads "· Run".

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-ui --lib subagent && cargo build -p aether`
Expected: all PASS.

- [ ] **Step 5: Manual check against the prototype chips "Subagents" and "Stopped",** in both themes:
  - The Explore card shows "14 tools" and "18.4k tok", with a live activity line and a timer.
  - Designer reads "Needs approval", its approval card is directly below it as a separate card, and the amber line focuses that card.
  - Researcher shows its 403 error. Done Explore shows the result as markdown.
  - Expanded cards show "+ 11 earlier tool calls".
  - Stopped cards read "Stopped".

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/ui/src/chat/subagent.rs crates/ui/tests/unit/chat/subagent.rs
git commit -m "feat(chat): subagent cards with status, timer, nested rows and results"
```

---

### Task 9: Notices, markers and the fatal card (needs phase 3 Task 4)

**Files:**
- Modify: `crates/ui/src/chat/notice.rs`, `crates/app/src/app/chat/manage.rs` (the builder bodies)
- Create: `crates/ui/tests/unit/chat/notice.rs`

**Interfaces:**
- Consumes: `Notice` and `NoticeKind`/`NoticeState` (phase 2), `Agent::notice_action` (phase 3 Task 4), `restart_agent` (Task 3), the existing `retry` and `copy_message`, `Markdown` (Task 4), and `clock` (Task 2).
- Produces, in `notice.rs`:
  - `pub fn lead(kind: NoticeKind, state: NoticeState) -> Option<&'static str>`:

    | Kind | Running | Done | Failed |
    |---|---|---|---|
    | Retry | "Retrying" | "Recovered" | "Retry failed" |
    | RateLimit | "Rate limited" | "Rate limited" | "Rate limited" |
    | Compaction | "Compacting context" | "Context compacted" | "Compaction failed" |
    | Hook | "Hook" | "Hook" | "Hook blocked" |
    | ModelChange | "Model changed" | "Model changed" | "Model changed" |
    | ModeChange | "Permission mode" | "Permission mode" | "Permission mode" |
    | Warning | "Warning" | "Warning" | "Warning" |
    | McpAuth | "Sign-in needed" | "Sign-in needed" | "Sign-in needed" |
    | Info, Wake, Startup, Unknown | `None` | `None` | `None` |

  - `#[derive(Debug, PartialEq, Eq, Clone, Copy)] pub enum NoticeTone { Muted, Ok, Warn, Bad }` and `pub fn notice_tone(kind, state) -> NoticeTone`:
    - `Failed` is `Bad`.
    - Warning, RateLimit, McpAuth and ModelChange are `Warn`.
    - Retry and Compaction when `Done` are `Ok`.
    - Everything else is `Muted`.
  - `pub fn countdown(until_ms: Option<i64>, now_ms: i64) -> Option<String>`: the seconds left, rounded up, as `clock`. It returns `None` at or past the deadline.
  - `pub fn notice_link(url: &str) -> Option<(String, NoticeLink)>`, with `pub enum NoticeLink { Open(String), Action(String) }`:
    - `"aether:trust-project"` gives ("Trust this project", `Action`).
    - Any other `"aether:{a}"` gives (`a` humanized, `Action`).
    - `http(s)` gives ("Sign in" for McpAuth, else "Open link", `Open`). The kind is passed in as a second parameter.
  - `pub fn stopped_summary(steps: &[Step]) -> Option<String>`. It counts the `Stopped` rows by kind: command, edit, subagent (Task), then other ("tool call"). Example: "1 command and 2 subagents were interrupted", "1 command was interrupted", "1 command, 1 edit and 2 subagents were interrupted".
  - `pub fn stopped_lead(steps: &[Step]) -> &'static str`. It returns "Interrupted when Aether closed" when every stopped row's error is "Interrupted: Aether closed", else "Stopped by you".
- **Views:**

  | View | Look and content |
  |---|---|
  | `NoticeLine` | One line, 12.5 px. A kind icon (Retry and RateLimit `RefreshCw`, or a spinner while Running; Compaction `Layers`; Hook `Zap`; ModelChange `ArrowLeftRight`; ModeChange `Shield`; Warning `TriangleAlert`; McpAuth `Plug`; Info `Info`; Done Retry `CircleCheck`), the bold `lead`, the text, " · retry in {countdown}" while Running with `until_ms`, then ghost buttons: "View summary" when `detail` is set (toggles `detail` as `Markdown` below the line) and the `notice_link` button. Tone colours the icon and lead: `success_text`, `mango_text` or `danger_text`. |
  | Wake (`NoticeLine`, kind Wake) | A marker row in `info_text`: `RefreshCw`, **"Agent resumed on its own"**, " · {text}", then a 1 px rule filling the rest of the row |
  | `StartupList` | One row per notice: `CircleCheck` (`success_text`), a spinner, or `CircleX` (`danger_text`), then the text, then `detail` in mono, faint |
  | `SteerMarker` | `ArrowUp`, "You, while it worked:" muted, the text, and a "Delivered" pill (success tint) on a raised row with a 1 px dashed `border` |
  | `StoppedMarker` | `Square`, the bold `stopped_lead`, " after {clock(elapsed)}" when known, then " · {stopped_summary}" when there is one |
  | `FatalCard` | Radius 12, a danger border at 0.5 and a danger wash at 0.12. `CircleX` and the bold first line of `error`, then the time on the right. Body: "The turn stopped. Edits that finished are kept." Actions: "Restart agent" (primary, `RefreshCw`, hidden for OpenRouter), "Retry last message" (`RotateCcw`), "Show details"/"Hide details" (toggles the rest of `error` as a mono block), and "Copy" (copies the whole `error`). |

- **App builders in `manage.rs`:**
  - `notice_row` wires the link: `Open` calls `cx.open_url`, and `Action` calls `self.agent.notice_action(&chat.id.to_string(), &action)`.
  - `fatal_row` wires Restart to `restart_agent(active)`, Retry to `retry`, and Copy to the clipboard.
  - The other builders pass their data through.

- [ ] **Step 1: Write the failing tests**

```rust
use super::*;
use aether_core::{NoticeKind as K, NoticeState as S, Step, ToolCall, ToolDetail, ToolStatus};

#[test]
fn notice_leads_and_tones_by_kind_and_state() {
    assert_eq!(lead(K::Retry, S::Running), Some("Retrying"));
    assert_eq!(lead(K::Retry, S::Done), Some("Recovered"));
    assert_eq!(lead(K::Compaction, S::Done), Some("Context compacted"));
    assert_eq!(lead(K::Hook, S::Failed), Some("Hook blocked"));
    assert_eq!(lead(K::Info, S::Done), None);
    assert_eq!(notice_tone(K::Hook, S::Failed), NoticeTone::Bad);
    assert_eq!(notice_tone(K::ModelChange, S::Done), NoticeTone::Warn);
    assert_eq!(notice_tone(K::Retry, S::Done), NoticeTone::Ok);
    assert_eq!(notice_tone(K::Hook, S::Done), NoticeTone::Muted);
}

#[test]
fn countdown_rounds_up_and_ends_at_the_deadline() {
    assert_eq!(countdown(Some(10_000), 1_000).as_deref(), Some("9s"));
    assert_eq!(countdown(Some(10_000), 500).as_deref(), Some("10s"));
    assert_eq!(countdown(Some(10_000), 10_000), None);
    assert_eq!(countdown(None, 0), None);
}

#[test]
fn notice_links_open_urls_or_run_actions() {
    assert_eq!(notice_link("https://figma.test/auth", K::McpAuth), Some(("Sign in".into(), NoticeLink::Open("https://figma.test/auth".into()))));
    assert_eq!(notice_link("aether:trust-project", K::Info), Some(("Trust this project".into(), NoticeLink::Action("trust-project".into()))));
    assert_eq!(notice_link("", K::Info), None);
}

#[test]
fn stopped_marker_says_what_was_cut_off_and_why() {
    let stopped = |id: &str, detail, error: &str| Step::Tool(ToolCall { status: ToolStatus::Stopped,
        error: (!error.is_empty()).then(|| error.to_string()), ..ToolCall::new(id, "T", detail) });
    let cmd = || ToolDetail::Command { command: "pnpm build".into(), cwd: None, output: String::new(), exit_code: None };
    let task = || ToolDetail::Task { description: "d".into(), output: String::new(), agent: "explore".into(),
        prompt: String::new(), tools: 0, tokens: 0, activity: String::new() };
    let steps = [stopped("c", cmd(), ""), stopped("a", task(), ""), stopped("b", task(), "")];
    assert_eq!(stopped_summary(&steps).as_deref(), Some("1 command and 2 subagents were interrupted"));
    assert_eq!(stopped_summary(&steps[..1]).as_deref(), Some("1 command was interrupted"));
    assert_eq!(stopped_summary(&[]), None);
    assert_eq!(stopped_lead(&steps), "Stopped by you");
    assert_eq!(stopped_lead(&[stopped("c", cmd(), "Interrupted: Aether closed")]), "Interrupted when Aether closed");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-ui --lib notice`
Expected: compile errors.

- [ ] **Step 3: Implement the functions, the views and the `manage.rs` builders.**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-ui --lib notice && cargo build -p aether`
Expected: all PASS.

- [ ] **Step 5: Manual check against the prototype chips "Starting", "Retries & notices", "Background", "Errors", "Stopped" and "Steering",** in both themes:
  - the startup checklist with a running step
  - each notice kind on one quiet line
  - the rate-limit line counting down
  - "View summary" expanding the compaction summary
  - "Sign in" opening the URL
  - the Wake marker before the second reply
  - the fatal card's four actions, with details showing the stderr tail
  - the stopped marker text
  - the steer marker with "Delivered"

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/ui/src/chat/notice.rs crates/ui/tests/unit/chat/notice.rs crates/app/src/app/chat/manage.rs
git commit -m "feat(chat): notice lines, wake/steer/stopped markers, fatal card"
```

---

### Task 10: Status line, live timers and the stall card (needs phase 3 Task 1)

**Files:**
- Modify: `crates/ui/src/chat/live.rs`, `crates/app/src/app/chat/live.rs` (`live_row`), `crates/app/src/app/new.rs` (the 1 Hz ticker)
- Test: `crates/ui/tests/unit/chat/live.rs`
- Create: `crates/app/tests/unit/app/chat/live.rs`

**Interfaces:**
- Consumes:
  - `Chat.last_event_ms`, `turn_at`, `health` and `stall_kept_ms` (phase 2, Task 3)
  - `Agent::health` (phase 2)
  - `Message::pending_request` and the phase 3 `Ask` variants
  - `steer_queued` and `Queued.delivering` (phase 2)
  - `Kbd`
- Produces, in `live.rs`:
  - `#[derive(Debug, PartialEq, Eq, Clone, Copy)] pub enum Wait { Approval, Answer, Input }`
  - `#[derive(Debug, PartialEq, Eq, Clone, Copy)] pub enum Phase { Starting, Thinking, Writing, Working, Tools(usize), Subagents(usize), Waiting(Wait), Retrying, Compacting, Stalled }`
  - `pub fn phase(m: &Message, wait: Option<Wait>) -> Phase`. Precedence: Waiting, Retrying (a Running Retry or RateLimit notice), Compacting (a Running Compaction notice), Subagents (Running top-level Task rows), Tools (Running top-level non-Task rows), Thinking (an open thought), Writing (text after the last step), Starting (no text and no steps), else Working. The caller turns the result into `Stalled`.
  - `impl Phase { pub fn label(&self, provider: &str) -> String }`:
    - "Starting {provider}", "Thinking", "Writing", "Working"
    - "Running 1 tool" / "Running {n} tools", "Running {n} subagent(s)"
    - "Waiting for your approval", "Waiting for your answer", "Waiting for you"
    - "Retrying", "Compacting", "No output"
  - `pub fn wait_detail(source: Option<&str>, running_subagents: usize) -> Option<String>`: "· {source} needs approval", then " · {n} subagent(s) still running", in that order. Either part may be absent.
  - `#[derive(Debug, PartialEq, Eq, Clone, Copy)] pub enum Heartbeat { Live, Quiet(u64), Silent(u64) }`, `pub fn heartbeat(idle_ms: u64) -> Heartbeat` (under 4 000 `Live`, under 30 000 `Quiet`, else `Silent`) and `impl Heartbeat { pub fn text(&self) -> String }` ("live", "quiet {clock}", "no output {clock}").
  - `pub const STALL_MS: u64 = 60_000;` and `pub fn stalled(idle_ms: u64, pending: bool) -> bool`.
  - `pub fn last_event(m: &Message) -> String`. It describes the last step in order:
    - a running tool: "{title} `{summary}` started"
    - a finished tool: "{title} `{summary}` finished"
    - a thought: "Thinking"
    - a notice: its text
    - text after the last step: "Writing the reply"
    - nothing: "Session started"
  - `StatusLine::new(key, facts: LiveFacts)`, `.on_stall(StallActions)`, where `LiveFacts` and `StallActions` (callbacks for keep, nudge, tui and stop) are plain structs this task defines.
- Produces, in `crates/app/src/app/chat/live.rs`:
  - `pub(crate) fn tui_command(provider: Provider, session: &str) -> Option<String>`: `opencode --session {session}` for OpenCode with a non-empty session, else `None`. Only OpenCode's TUI attaches to the same live session, because its service is shared. Others would fork it.
  - `pub(crate) const STATUS_NUDGE: &str = "Status check: what are you working on right now, and is anything blocking you?";`
- **The status line** (prototype `.live`): radius 9, a `secondary` wash, a 1 px `border`, 12.5 px. In order:
  1. The lead: a spinner, `Shield` (mango) while waiting, `RefreshCw` (info) while retrying, or `TriangleAlert` (mango) while stalled.
  2. The bold label, then the detail, then " · {since(turn_at)}".
  3. " · " plus an `ArrowDown` icon and "{format_tokens(completion_tokens)} tokens" when non-zero.
  4. " · retry in {countdown}" while retrying.
  5. Right-aligned: the heartbeat (a static dot plus `text()`, in success, muted or mango), hidden while waiting or stalled.
  6. `Kbd("esc")` and "to stop".

  The waiting and stalled variants use a mango border at 0.4 and a mango wash. Retrying uses an info border at 0.35.
- **The stall card**, rendered just above the line when `stalled(now - last_event_ms, pending)` holds and `stall_kept_ms != last_event_ms`:
  - Radius 12, a mango border at 0.4.
  - Header: `TriangleAlert`, "No output for {since(last_event_ms)}", then "checked {since(health.1)} ago" on the right.
  - A key-value list:
    - "Process": "alive · pid {pid}" in `success_text`, "exited · pid {pid}" in `danger_text`, or "checking…"
    - "Last event": `last_event(m)`
    - "Waiting on": "nothing from you, no open approvals or questions"
  - The note "Installs and builds can be quiet. This shows after 60 s without events."
  - Actions:
    - "Keep waiting": sets `stall_kept_ms = last_event_ms`
    - "Ask for a status update": queues `STATUS_NUDGE` as a delivering steer through `steer_queued`
    - "Open in {Provider} TUI" (`ExternalLink`): shown only when `tui_command` is `Some`. It runs the command in the chat's terminal drawer with `TerminalWorkspace::run_command`.
    - "Stop" (danger, `Square`): `stop_chat`
- **The ticker** in `new.rs`: a 1 s loop.
  - When any chat is busy or holds a `Background` row, it calls `cx.notify()`. That redraws every live timer: rows, cards, thoughts, the sidebar and the tray.
  - For each busy chat that is stalled and whose `health` is older than 5 s, it sets `chat.health = agent.health(&key).map(|h| (h, now))`. Mark it `// ponytail: kill -0 spawn on the UI thread every 5 s while stalled, move off-thread if it shows in profiles`.

- [ ] **Step 1: Write the failing tests**

`crates/ui/tests/unit/chat/live.rs` (add):

```rust
use aether_core::{Message, Notice, NoticeKind, NoticeState, ToolCall, ToolDetail, ToolStatus, Update};

#[test]
fn heartbeat_thresholds_match_the_prototype() {
    assert_eq!(heartbeat(3_999), Heartbeat::Live);
    assert_eq!(heartbeat(4_000), Heartbeat::Quiet(4_000));
    assert_eq!(heartbeat(30_000), Heartbeat::Silent(30_000));
    assert_eq!(Heartbeat::Quiet(12_000).text(), "quiet 12s");
    assert_eq!(Heartbeat::Silent(65_000).text(), "no output 1m 05s");
}

#[test]
fn stall_needs_sixty_quiet_seconds_and_nothing_pending() {
    assert!(!stalled(59_999, false));
    assert!(stalled(60_000, false));
    assert!(!stalled(600_000, true));
}

#[test]
fn phase_follows_what_the_reply_is_doing() {
    let running = |id: &str, detail| Update::Tool(ToolCall::new(id, "T", detail));
    let task = || ToolDetail::Task { description: "d".into(), output: String::new(), agent: "explore".into(),
        prompt: String::new(), tools: 0, tokens: 0, activity: String::new() };
    let mut m = Message::assistant();
    assert_eq!(phase(&m, None), Phase::Starting);
    m.apply(Update::Thinking("hmm".into()));
    assert_eq!(phase(&m, None), Phase::Thinking);
    m.apply(Update::Delta("text".into()));
    assert_eq!(phase(&m, None), Phase::Writing);
    m.apply(running("c1", ToolDetail::Plain(String::new())));
    m.apply(running("c2", ToolDetail::Plain(String::new())));
    assert_eq!(phase(&m, None), Phase::Tools(2));
    m.apply(running("t1", task()));
    assert_eq!(phase(&m, None), Phase::Subagents(1));
    m.apply(Update::Notice(Notice { id: "retry-1".into(), kind: NoticeKind::Retry, text: "rate limited (429)".into(),
        state: NoticeState::Running, until_ms: Some(9), url: None, detail: None }));
    assert_eq!(phase(&m, None), Phase::Retrying);
    assert_eq!(phase(&m, Some(Wait::Approval)), Phase::Waiting(Wait::Approval));
}

#[test]
fn labels_details_and_last_events_read_like_the_prototype() {
    assert_eq!(Phase::Starting.label("OpenCode"), "Starting OpenCode");
    assert_eq!(Phase::Tools(1).label(""), "Running 1 tool");
    assert_eq!(Phase::Subagents(2).label(""), "Running 2 subagents");
    assert_eq!(Phase::Waiting(Wait::Answer).label(""), "Waiting for your answer");
    assert_eq!(wait_detail(Some("Designer"), 2).as_deref(), Some("· Designer needs approval · 2 subagents still running"));
    assert_eq!(wait_detail(None, 0), None);
    let mut m = Message::assistant();
    assert_eq!(last_event(&m), "Session started");
    m.apply(Update::Tool(ToolCall::new("c", "Run command", ToolDetail::Command { command: "pnpm install".into(), cwd: None, output: String::new(), exit_code: None })));
    assert_eq!(last_event(&m), "Run command `pnpm install` started");
    m.apply(Update::Tool(ToolCall { status: ToolStatus::Done, ..ToolCall::new("c", "Run command", ToolDetail::Command { command: "pnpm install".into(), cwd: None, output: String::new(), exit_code: Some(0) }) }));
    assert_eq!(last_event(&m), "Run command `pnpm install` finished");
}
```

`crates/app/tests/unit/app/chat/live.rs`:

```rust
use super::*;
use aether_core::Provider;

#[test]
fn only_opencode_offers_its_tui_for_the_live_session() {
    assert_eq!(tui_command(Provider::OpenCode, "ses_1").as_deref(), Some("opencode --session ses_1"));
    assert_eq!(tui_command(Provider::OpenCode, ""), None);
    assert_eq!(tui_command(Provider::Claude, "abc"), None);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-ui --lib chat::live && cargo test -p aether --lib app::chat::live`
Expected: compile errors.

- [ ] **Step 3: Implement** the functions, `StatusLine`, the stall card, `live_row` (mapping `Ask::Approval` to `Wait::Approval`, `Question`/`Questions` to `Answer`, and anything else to `Input`, with `wait_detail` from `Approval.source`), the actions, and the ticker. Add `#[cfg(test)] #[path = "../../../tests/unit/app/chat/live.rs"] mod tests;` to the app file.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-ui --lib && cargo test -p aether --lib && cargo build -p aether`
Expected: all PASS.

- [ ] **Step 5: Manual check against the prototype chips "Live run", "Writing", "Needs approval", "Retries & notices" and "Stalled",** in both themes:
  - The line reads "Thinking", then "Running 2 tools", then "Waiting for your approval", with elapsed time, tokens, and a heartbeat that goes live, then quiet, then no output.
  - Run `sleep 90` through an agent. At 60 s the stall card shows "alive · pid …", the last event and the actions.
  - "Keep waiting" hides the card until the next event.
  - "Ask for a status update" shows a Delivering chip.
  - In an OpenCode chat, "Open in OpenCode TUI" opens the drawer on that session.

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/ui/src/chat/live.rs crates/ui/tests/unit/chat/live.rs crates/app/src/app/chat/live.rs crates/app/tests/unit/app/chat/live.rs crates/app/src/app/new.rs
git commit -m "feat(chat): status line with heartbeat, stall card, 1 Hz live timers"
```

---

### Task 11: Approval, question, form and plan cards (needs phase 3 Task 1)

**Files:**
- Modify: `crates/ui/src/chat/request.rs`, `crates/app/src/hitl.rs`, `crates/app/src/actions.rs`, `crates/ui/src/chat/timeline.rs` (request placement)
- Test: `crates/app/tests/unit/hitl.rs`, `crates/ui/tests/unit/chat/timeline.rs`

**Interfaces:**
- Consumes:
  - phase 3's `Approval`, `ApprovalOption`, `Question`, `Choice`, `Form`, `Field` and `Plan`
  - phase 3's `Answer::{Approve, Deny, Pick, DenyWith, Answers, Form, Plan}`
  - phase 3's `questions_answer`, `form_answer`, `field_visible` and `request_picks`
  - `Request.asked_ms` (Task 1) and `DiffView` (Task 5)
  - `RequestCode` as phase 1 left it: `SelectableText` inside a scroll container with an id, `max_h(260)` and `overflow_y_scroll`. Reuse it unchanged.
  - `focus_ask` (Task 3, completed here) and `Kbd`
- Produces:
  - **Placement** in `reply_blocks`: a request whose `Approval.tool_id` names a step in this reply is placed right after the top-level block holding that step: its `Tool`, or the `Subagent` card that nests it. Other requests stay at `Request.at`.
  - In `hitl.rs`:
    - `pub(crate) fn takes_focus(active_chat: bool, composer_empty: bool) -> bool`
    - `pub(crate) fn queue_note(others: &[&str]) -> Option<String>`: "1 more request queued: {first}", or "{n} more requests queued: {first}"
    - `pub(crate) fn stepper(i: usize, n: usize) -> Option<String>`: "Question {i+1} of {n}", only when `n > 1`
    - `pub(crate) fn pick_hint(multi: bool) -> &'static str`: "Pick any" or "Pick one"
  - Resolved lines gain `RequestStatus::Withdrawn` → "Answered elsewhere · {subject}" (`Outcome::Missed` look), and `Approve { always: false }` reads "Allowed once · {subject}". The rest stay as phase 3 set them.
  - In `actions.rs`: the key context `ASK_CONTEXT = "AskCard"`, with actions `AskPrimary` (enter), `AskDismiss` (escape), `AskPrev` (up), `AskNext` (down) and `AskToggle` (space).
- **Approval card** (prototype `.card.perm`):
  - **Shell.** Radius 12, a `secondary` wash, a mango border at 0.45, and no glow.
  - **Header.** `Shield` (mango) and the bold title, which is `Approval.title` or "Allow {Provider} to {verb}?". Verbs: Command "run a command", Edit "edit a file", Fetch "fetch a URL", Mcp "use a tool", Other "continue". Then " · subagent {source}" muted, and "waiting {since(asked_ms)}" on the right.
  - **Body.** `DiffView` (unified) for `diff`. Otherwise the full `input` in phase 1's `RequestCode`, keyed `"request-{id}-code"`. That block stays selectable and unhighlighted: `SelectableText` takes no highlight runs, and the user must be able to select and copy every line they approve. Then the key-value rows "Directory" (`cwd`) and "Reason".
  - **Actions:** "Allow once" (primary, `Kbd("enter")`), one secondary button per option with its `label` verbatim (sends `Pick(id)`), "Deny" (`Kbd("esc")`), and the inline field "Deny and tell the agent what to do instead…". Enter in that field sends `DenyWith(note)`.
  - Then `queue_note` from the chat's other pending approvals.
  - Non-agent (OpenRouter) chats keep their editable input.
- **Question card** (prototype `.card.ask`): an info border at 0.45. One question shows at a time.
  - **Header.** `CircleQuestionMark` (info), "{Provider} has a question", then `stepper(…)`, plus " · {header}" when set, on the right.
  - **Body.** The question as `Markdown` at 14 px. Then option rows: a round box (single) or square box (multi), the label, and the `description` muted. The selected row gets an info border and an info wash at 0.12.
  - "Something else…" when `other` is set. Selecting it shows a line field, masked when `secret` is set.
  - **Actions:** "Next" or "Submit" on the last question (primary, `Kbd("enter")`, sends `questions_answer`), "Back" from question 2 on, "Let the agent decide" (sends `Deny`), and `pick_hint` on the right.
- **Form card:**
  - The same shell as the question card, with the title and message. When `url` is set, "Open link" (`ExternalLink`).
  - Fields render through `field_visible`. Text and Number get a line field with the default. Bool gets "Yes"/"No" rows. Choice and Multi get option rows plus "Something else…" when `other` is set.
  - A required label ends with " (required)".
  - "Closes in {countdown}" shows when `timeout_ms` is set, counted from `asked_ms`.
  - Actions: "Submit" (`form_answer`, with an `Err` shown as one `danger_text` line) and "Cancel" (`Deny`).
- **Plan card:** `Plan.markdown` renders as `Markdown`. With no markdown, `steps` render as a "- " bullet list. Delete `PlanSteps`, which numbered every markdown line. Options become buttons that send `Pick`. Then "Approve" (primary) and "Send back" with an optional feedback field.
- **Keyboard:**
  - Each pending card tracks a focus handle and sets `key_context(ASK_CONTEXT)`.
  - When an ask arrives and `takes_focus(chat is active, composer is empty)` holds, focus moves to the card. Answering returns focus to the composer.
  - `AskPrimary`: Allow once, or Next/Submit. `AskDismiss`: Deny on an approval. On a question or form it only returns focus to the composer, and sends no answer.
  - Up and Down move the highlighted option or button, and Space toggles the highlighted option.
  - `focus_ask` scrolls to the card's row and focuses it.

- [ ] **Step 1: Write the failing tests**

`crates/app/tests/unit/hitl.rs` (add):

```rust
#[test]
fn ask_takes_focus_only_from_an_empty_composer() {
    assert!(takes_focus(true, true));
    assert!(!takes_focus(true, false), "the user is typing");
    assert!(!takes_focus(false, true), "another chat");
}

#[test]
fn queue_notes_and_steppers_read_like_the_prototype() {
    assert_eq!(queue_note(&[]), None);
    assert_eq!(queue_note(&["Run `pnpm build`"]).as_deref(), Some("1 more request queued: Run `pnpm build`"));
    assert_eq!(queue_note(&["a", "b"]).as_deref(), Some("2 more requests queued: a"));
    assert_eq!(stepper(0, 1), None);
    assert_eq!(stepper(0, 2).as_deref(), Some("Question 1 of 2"));
    assert_eq!((pick_hint(true), pick_hint(false)), ("Pick any", "Pick one"));
}

#[test]
fn withdrawn_and_once_resolutions_say_what_happened() {
    let approval = Ask::Approval(Approval { title: "Run command".into(), input: "pnpm build".into(), ..Approval::default() });
    let req = |status| Request { id: 1, at: 0, asked_ms: None, ask: approval.clone(), status };
    assert_eq!(resolved_text(&req(RequestStatus::Withdrawn)), "Answered elsewhere · Run command “pnpm build”");
    assert_eq!(resolved_text(&req(RequestStatus::Answered(Answer::Approve { input: "pnpm build".into(), always: false }))),
        "Allowed once · Run command “pnpm build”");
}
```

`crates/ui/tests/unit/chat/timeline.rs` (add):

```rust
#[test]
fn approvals_sit_below_their_row_or_subagent_card() {
    use aether_core::{Approval, Ask};
    let mut m = Message::assistant();
    m.apply(row("t1", agent(), ToolStatus::Running));
    m.apply(under("c1", "t1", ToolDetail::Plain(String::new())));
    m.apply(Update::Delta("text".into()));
    m.apply(Update::Ask { id: 7, ask: Ask::Approval(Approval { tool_id: Some("c1".into()), source: Some("Designer".into()), ..Approval::default() }) });
    m.apply(Update::Ask { id: 8, ask: Ask::Approval(Approval::default()) });
    assert_eq!(live(&m), [Block::Subagent { step: 0, children: vec![1] }, Block::Request(0), Block::Text(0..4), Block::Request(1), Block::Live]);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether --lib hitl && cargo test -p aether-ui --lib timeline`
Expected: compile errors (`takes_focus`, `queue_note`, `stepper`, `pick_hint` and `resolved_text`), and a FAIL for placement.

- [ ] **Step 3: Implement.** `resolved_text(&Request) -> String` is the text of today's `resolved()`, split out so it can be tested. `resolved()` wraps it.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether --lib && cargo test -p aether-ui --lib && cargo build -p aether`
Expected: all PASS.

- [ ] **Step 5: Manual check against the prototype chips "Needs approval", "Question" and "Subagents",** in both themes, with the keyboard only:
  - Tab reaches every button.
  - Enter allows once, and Esc denies.
  - An "always" option sends its rule.
  - The note field denies with text.
  - "waiting 41s" ticks, and "1 more request queued" shows.
  - The two-question card steps "Question 1 of 2", then 2. Multi-select toggles with Space. "Something else…" takes text, and "Let the agent decide" skips.
  - A subagent's approval sits under its card as a separate card.
  - Typing in the composer while an ask arrives keeps focus in the composer.

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/ui/src/chat/request.rs crates/ui/src/chat/timeline.rs crates/ui/tests/unit/chat/timeline.rs crates/app/src/hitl.rs crates/app/src/actions.rs crates/app/tests/unit/hitl.rs
git commit -m "feat(chat): keyboard-first approval, question, form and plan cards"
```

---

### Task 12: Composer trays, queue chips and the dead-agent bar

**Files:**
- Modify: `crates/ui/src/chat/composer.rs` (tray views, placeholders), `crates/app/src/app/composer/mod.rs`
- Test: `crates/ui/tests/unit/chat/composer.rs`

**Interfaces:**
- Consumes:
  - `Queued.delivering` and `steer_queued` (phase 2)
  - `Agent::stop_tool` (phase 3 Task 4)
  - `reveal_tool`, `restart_agent` and `Chat.restarted_ms` (Task 3)
  - `since` (Task 2)
  - `PlanItem`/`PlanStatus` (core)
- Produces, in `composer.rs`:
  - `#[derive(Debug, PartialEq, Eq)] pub struct PlanProgress { pub done: usize, pub total: usize, pub current: Option<String> }`, `pub fn plan_progress(items: &[PlanItem]) -> PlanProgress` (cancelled items are left out of `total`) and `impl PlanProgress { pub fn header(&self) -> String }` ("{done} of {total}").
  - `pub fn latest_plan(messages: &[Message]) -> Option<&[PlanItem]>`: the items of the newest `ToolDetail::Plan` row in the chat.
  - `pub fn background_rows(messages: &[Message]) -> Vec<(usize, &ToolCall)>`: the top-level `Background` rows across the chat, as (message ix, row), oldest first.
  - `pub fn chip_copy(delivering: bool, provider: &str) -> (&'static str, String)`: ("Queued", "Sends when this turn ends") or ("Delivering", "Waiting for {provider} to pick it up"). The prototype's "sends when the current step ends" is wrong, because a queued message runs after the turn.
  - `pub fn busy_placeholder(agent: Option<&str>, busy: Busy) -> String`, with `pub enum Busy { Idle, Working, Waiting }`:
    - Idle: `composer_placeholder(agent)`, which keeps its signature so its callers in `new.rs` and `providers.rs` stay untouched.
    - Working: "Type to steer {agent} while it works".
    - Waiting: "Answer above, or type to tell {agent} what to do instead".
  - Views: `PlanTray`, `BackgroundTray`, `QueueChip` and `DeadAgentBar`, each taking plain data plus callbacks.
- **Trays** sit in the `Composer::header` slot, in this order: plan tray, background tray, dead bar, queue chips, then today's header items.
  - **`PlanTray`** (prototype `.tray`): `ListChecks`, bold "Plan", `header()`, a 60 × 4 progress bar (a `success` fill on a `border` track, radius 2), then " · {current}" with ellipsis. Collapsed by default, and the header toggles it. Expanded, each item shows `Check` (success) and struck-through faint text when done, a spinner and foreground text when active, or a circle and muted text when pending. It shows while `latest_plan` has an unfinished item, or belongs to the last reply.
  - **`BackgroundTray`:** a static info dot, bold "Background", "{n} running". Expanded by default. Each item shows a spinner, `Bot` for a Task row or `SquareTerminal` otherwise, the mono name (the command summary or the agent name), "{tools} tools" for an agent, `since(started_ms)`, then "Open" (agent) or "Output" (command), both calling `reveal_tool`, then "Stop" (calls `agent.stop_tool(chat key, id)`). It shows while `background_rows` is non-empty.
  - **`QueueChip`** (prototype `.queued`): `Clock` or a spinner (delivering), the label, the text in the foreground with ellipsis, then the hint muted.
    - Queued: "Send now" (`steer_queued`), "Edit" (moves the text and files into the composer, and is disabled with the tooltip "Clear the composer to edit this" when the composer has text), and Remove (an `X` icon button, accessibility label "Remove").
    - Delivering: no buttons.
  - **`DeadAgentBar`:** `CircleX` (`danger_text`), "{Provider} stopped. Your message will restart it.", then "Restart now" (`RefreshCw`, calls `restart_agent`). It shows when the chat is idle, the last reply has `error`, and `restarted_ms` is older than that reply's `ended_at_ms`.
- **Composer state.** The card border becomes mango at 0.4 while the last reply has a pending request. The placeholder follows `busy_placeholder`, set only when it changed (`set_placeholder` behind an equality check).

- [ ] **Step 1: Write the failing tests**

```rust
use aether_core::{Message, PlanItem, PlanStatus, ToolCall, ToolDetail, ToolStatus, Update};

#[test]
fn plan_progress_counts_done_and_names_the_active_item() {
    let item = |text: &str, status| PlanItem { text: text.into(), status };
    let items = [item("Audit", PlanStatus::Done), item("Draft", PlanStatus::Done), item("Rewrite Hero", PlanStatus::Active),
        item("Restyle", PlanStatus::Pending), item("Drop X", PlanStatus::Cancelled)];
    let p = plan_progress(&items);
    assert_eq!(p, PlanProgress { done: 2, total: 4, current: Some("Rewrite Hero".into()) });
    assert_eq!(p.header(), "2 of 4");
}

#[test]
fn trays_read_the_newest_plan_and_running_background_rows() {
    let plan = |id: &str, text: &str| Update::Tool(ToolCall::new(id, "Plan", ToolDetail::Plan { items: vec![PlanItem { text: text.into(), status: PlanStatus::Pending }] }));
    let cmd = |id: &str, status| Update::Tool(ToolCall { status, ..ToolCall::new(id, "Run", ToolDetail::Command { command: id.into(), cwd: None, output: String::new(), exit_code: None }) });
    let mut first = Message::assistant();
    first.apply(plan("p1", "old"));
    first.apply(cmd("dev", ToolStatus::Background));
    let mut second = Message::assistant();
    second.apply(plan("p2", "new"));
    second.apply(cmd("test", ToolStatus::Done));
    let msgs = [Message::user("a"), first, Message::user("b"), second];
    assert_eq!(latest_plan(&msgs).unwrap()[0].text, "new");
    assert_eq!(background_rows(&msgs).iter().map(|(ix, c)| (*ix, c.id.as_str())).collect::<Vec<_>>(), [(1, "dev")]);
}

#[test]
fn queue_chips_and_placeholders_say_what_happens_next() {
    assert_eq!(chip_copy(false, "Pi"), ("Queued", "Sends when this turn ends".to_string()));
    assert_eq!(chip_copy(true, "Pi"), ("Delivering", "Waiting for Pi to pick it up".to_string()));
    assert_eq!(busy_placeholder(Some("OpenCode"), Busy::Working), "Type to steer OpenCode while it works");
    assert_eq!(busy_placeholder(Some("OpenCode"), Busy::Waiting), "Answer above, or type to tell OpenCode what to do instead");
    assert_eq!(busy_placeholder(Some("OpenCode"), Busy::Idle), composer_placeholder(Some("OpenCode")));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-ui --lib composer`
Expected: compile errors.

- [ ] **Step 3: Implement the functions, the views and the app wiring.** Replace today's queue rows ("Steer now"/"Send"/"Remove") with `QueueChip`. Mark the tray `// ponytail: running background rows only, finished ones leave the tray; their rows keep the result`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-ui --lib && cargo test -p aether --lib && cargo build -p aether`
Expected: all PASS.

- [ ] **Step 5: Manual check against the prototype chips "Running tools", "Background", "Steering" and "Errors",** in both themes:
  - The plan tray is collapsed, reading "2 of 6" plus the current item, and expands.
  - The background tray lists `pnpm dev` and Explore with timers, Output/Open scroll to the rows, and Stop ends `pnpm dev`.
  - A queued chip offers Send now, Edit and Remove. After Send now it reads Delivering with no buttons, and it disappears once the CLI takes it.
  - A fatal turn shows the dead bar, and "Restart now" hides it.

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/ui/src/chat/composer.rs crates/ui/tests/unit/chat/composer.rs crates/app/src/app/composer/mod.rs
git commit -m "feat(composer): plan and background trays, delivering chips, dead-agent bar"
```

---

### Task 13: Finished-turn fold and footer

**Files:**
- Modify: `crates/ui/src/chat/turn.rs`, `crates/app/src/app/chat/turn.rs` (`fold_row`, `footer_row`, `foot_facts`), `crates/app/src/app/format.rs`
- Test: `crates/ui/tests/unit/chat/turn.rs`, `crates/app/tests/unit/app/format.rs`

**Interfaces:**
- Consumes: `Message.cost_micros`, `served_model` and `error` (phase 2), `clock` (Task 2), `format_tokens`, `message_cost` and `format_time` (app), and `open_steps`/`set_steps_open`.
- Produces, in `turn.rs`:
  - `#[derive(Debug, PartialEq, Eq, Clone, Copy)] pub enum Outcome { Worked, Failed, Stopped }`.
  - `pub fn tool_counts(steps: &[Step]) -> (usize, usize)`: (tool calls, subagents). Tool calls are every `Step::Tool` that isn't a Task, nested ones included. Subagents are the Task rows.
  - `pub fn fold_lead(outcome: Outcome, ms: u64) -> String`: "Worked for {clock}", "Failed after {clock}" or "Stopped after {clock}".
  - `pub fn fold_counts(steps: &[Step]) -> String`, over all rows, nested included:
    - " · {n} subagent(s)"
    - " · read {n} file(s)" (unique paths)
    - " · ran {n} command(s)"
    - " · edited {n} file(s)" (unique paths)
    - " · searched {n} time(s)" (`Search` rows and `Step::Search`)
    - " · used {n} tool(s)" (Web, Other and Plain)

    Each part appears only when non-zero, in this order. This replaces the old `summary`, and its moved tests are rewritten to these strings.
  - `pub struct FootFacts { pub outcome: Outcome, pub duration_ms: u64, pub tools: usize, pub subagents: usize, pub model: Option<String>, pub tokens_in: u64, pub tokens_out: u64, pub cached: u64, pub cost: Option<String>, pub at: Option<String> }`.
  - `pub fn footer_parts(f: &FootFacts) -> Vec<String>`. The parts, in order:
    1. The lead: "Worked {clock}", "Failed after {clock}" or "Stopped after {clock}".
    2. "{n} tool call(s)".
    3. "{n} subagent(s)", when non-zero.
    4. The model.
    5. "{in} in · {out} out", plus " · {pct}% cached" when `cached > 0`, using `format_tokens`. Left out when both counts are 0.
    6. The cost.
  - `FoldLine` (prototype `.group`): a chevron (rotated when open), the bold `fold_lead`, then `fold_counts` muted. The whole row is a button with `aria_expanded`, and toggles `set_steps_open`.
  - `TurnFooter` (prototype `.foot`, 12 px muted). The lead icon is `CircleCheck` (success), `CircleX` (danger) or `Square`. Then the parts joined by " · ". The token part has the tooltip "Input · output · cache hits". Right-aligned: Copy and Retry (last reply, not busy) as ghost icon buttons, always visible, then the time.
- Produces, in `format.rs`: `pub(crate) fn cost_text(micros: u64, estimated: bool) -> Option<String>`. Zero gives `None`. Under 10 000 gives "<$0.01". Otherwise "${:.2}". `estimated` prefixes "≈". `foot_facts` uses the CLI's `cost_micros` when it is non-zero, else `message_cost(...)` × 1e6 with `estimated = true`. The model is `served_model` or `model`, through `model_label`.
- Remove the `is_agent` suppression, so CLI chats get the footer too. Delete `TurnUsage`, `turn_usage` and the usage popover. Delete `format_elapsed` if it is left unused.

- [ ] **Step 1: Write the failing tests**

`crates/ui/tests/unit/chat/turn.rs`:

```rust
use super::*;
use aether_core::{Message, ToolCall, ToolDetail, ToolStatus, Update};

fn done(id: &str, detail: ToolDetail) -> Update {
    Update::Tool(ToolCall { status: ToolStatus::Done, ..ToolCall::new(id, "T", detail) })
}

#[test]
fn fold_line_counts_subagents_files_and_commands() {
    let mut m = Message::assistant();
    m.apply(done("t", ToolDetail::Task { description: "d".into(), output: String::new(), agent: "explore".into(),
        prompt: String::new(), tools: 0, tokens: 0, activity: String::new() }));
    m.apply(Update::Tool(ToolCall { parent: Some("t".into()), status: ToolStatus::Done,
        ..ToolCall::new("r1", "Read", ToolDetail::Read { path: "a.rs".into(), range: None }) }));
    m.apply(done("r2", ToolDetail::Read { path: "b.rs".into(), range: None }));
    m.apply(done("r3", ToolDetail::Read { path: "b.rs".into(), range: None }));
    m.apply(done("c", ToolDetail::Command { command: "ls".into(), cwd: None, output: String::new(), exit_code: Some(0) }));
    m.apply(done("e", ToolDetail::Edit { path: "a.rs".into(), diff: String::new(), added: 1, removed: 0 }));
    assert_eq!(fold_counts(&m.steps), " · 1 subagent · read 2 files · ran 1 command · edited 1 file");
    assert_eq!(tool_counts(&m.steps), (5, 1));
    assert_eq!(fold_lead(Outcome::Worked, 401_000), "Worked for 6m 41s");
    assert_eq!(fold_lead(Outcome::Stopped, 72_000), "Stopped after 1m 12s");
}

#[test]
fn footer_parts_match_the_prototype() {
    let f = FootFacts { outcome: Outcome::Worked, duration_ms: 401_000, tools: 31, subagents: 2,
        model: Some("DeepSeek V4.1 Flash".into()), tokens_in: 182_000, tokens_out: 12_400, cached: 165_620,
        cost: Some("$0.08".into()), at: Some("8:20 AM".into()) };
    assert_eq!(footer_parts(&f), ["Worked 6m 41s", "31 tool calls", "2 subagents", "DeepSeek V4.1 Flash",
        "182k in · 12.4k out · 91% cached", "$0.08"]);
    let bare = FootFacts { outcome: Outcome::Failed, duration_ms: 123_000, tools: 1, subagents: 0, model: None,
        tokens_in: 0, tokens_out: 0, cached: 0, cost: None, at: None };
    assert_eq!(footer_parts(&bare), ["Failed after 2m 03s", "1 tool call"]);
}
```

`crates/app/tests/unit/app/format.rs` (add):

```rust
#[test]
fn costs_read_in_cents() {
    assert_eq!(cost_text(0, false), None);
    assert_eq!(cost_text(4_000, false).as_deref(), Some("<$0.01"));
    assert_eq!(cost_text(80_000, false).as_deref(), Some("$0.08"));
    assert_eq!(cost_text(80_000, true).as_deref(), Some("≈$0.08"));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether-ui --lib turn && cargo test -p aether --lib format`
Expected: compile errors.

- [ ] **Step 3: Implement.**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether-ui --lib && cargo test -p aether --lib && cargo build -p aether`
Expected: all PASS.

- [ ] **Step 5: Manual check against the prototype chip "Done",** in both themes:
  - The finished turn reads "Worked for 6m 41s · 2 subagents · read 9 files · ran 4 commands · edited 6 files".
  - The answer, the opened Hero.tsx diff and the changes card stay visible, and clicking the line expands the timeline.
  - The footer reads "Worked 6m 41s · 31 tool calls · 2 subagents · {model} · 182k in · 12.4k out · 91% cached · $0.08", with Copy, Retry and the time.
  - A Claude chat shows the footer too.

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/ui/src/chat/turn.rs crates/ui/tests/unit/chat/turn.rs crates/app/src/app/chat/turn.rs crates/app/src/app/format.rs crates/app/tests/unit/app/format.rs
git commit -m "feat(chat): finished turns fold to one line, footer with model, tokens and cost"
```

---

### Task 14: Sidebar statuses

**Files:**
- Modify: `crates/ui/src/chat/sidebar/mod.rs` (`ChatStatus`), `sidebar/chat_row.rs`, `sidebar/project_row.rs`
- Modify: `crates/app/src/app/mod.rs` (`Chat::status`, `StatusFacts`, `chat_status`), `crates/app/src/app/sidebar.rs` (`chat_line`), `crates/app/src/project_page/model.rs`, `crates/app/src/projects.rs`
- Test: `crates/app/tests/unit/app/mod.rs`, `crates/ui/tests/unit/chat/sidebar/mod.rs`

**Interfaces:**
- Consumes: `Chat.last_event_ms` and `turn_at` (phase 2), `clock` (Task 2), and `ToolStatus::Background`.
- Produces:
  - `ChatStatus` becomes `{ Idle, Starting, Working { since_ms: i64 }, Waiting, Retrying, Quiet { since_ms: i64 }, Background(usize), Done, Failed }`. The `Waiting(&str)` labels go.
  - `impl ChatStatus { pub fn text(&self, now_ms: i64) -> String }`:
    - Working: "Working · {clock}"
    - Starting: "Starting"
    - Waiting: "Needs you"
    - Failed: "Failed"
    - Done: "Done"
    - Background: "{n} in background"
    - Quiet: "Quiet {clock}"
    - Retrying: "Retrying"
    - Idle: ""
  - The mark per status:

    | Status | Mark | Colour |
    |---|---|---|
    | Working, Starting | spinner | muted |
    | Waiting | `CircleDot` | mango |
    | Failed | `CircleX` | danger |
    | Done | `CircleCheck` | info |
    | Background | `Layers` | info |
    | Quiet | `TriangleAlert` | mango |
    | Retrying | `RefreshCw` | info |

    Text always sits beside the mark.
  - In app: `pub(crate) struct StatusFacts { pub busy: bool, pub ask: bool, pub started: bool, pub retrying: bool, pub last_event_ms: i64, pub turn_at: i64, pub background: usize, pub unseen: ChatStatus }` and `pub(crate) fn chat_status(f: &StatusFacts, now_ms: i64) -> ChatStatus`.
    - When busy: ask → Waiting, then not started → Starting, then retrying → Retrying, then idle for 30 s or more → Quiet, else Working.
    - When not busy: Failed (unseen), then Background(n), then Done (unseen), then Idle.
    - `Chat::status(now_ms)` builds the facts:
      - `ask`: a pending request in the last reply
      - `started`: `last_event_ms > 0`
      - `retrying`: a Running Retry or RateLimit notice in the last reply
      - `background`: the count of top-level `Background` rows
  - The agent row's corner shows `text(now)`, replacing `busy_label`. `projects::busy_label` is deleted if it is left unused.

- [ ] **Step 1: Write the failing tests**

`crates/app/tests/unit/app/mod.rs` (add):

```rust
#[test]
fn sidebar_status_precedence() {
    let facts = StatusFacts { busy: true, ask: false, started: true, retrying: false, last_event_ms: 100_000,
        turn_at: 40_000, background: 0, unseen: ChatStatus::Idle };
    let now = 110_000;
    assert_eq!(chat_status(&StatusFacts { ask: true, retrying: true, ..facts }, 900_000), ChatStatus::Waiting);
    assert_eq!(chat_status(&StatusFacts { started: false, ..facts }, now), ChatStatus::Starting);
    assert_eq!(chat_status(&StatusFacts { retrying: true, ..facts }, now), ChatStatus::Retrying);
    assert_eq!(chat_status(&facts, 130_000), ChatStatus::Quiet { since_ms: 100_000 });
    assert_eq!(chat_status(&facts, now), ChatStatus::Working { since_ms: 40_000 });
    let idle = StatusFacts { busy: false, ..facts };
    assert_eq!(chat_status(&StatusFacts { background: 2, ..idle }, now), ChatStatus::Background(2));
    assert_eq!(chat_status(&StatusFacts { background: 2, unseen: ChatStatus::Failed, ..idle }, now), ChatStatus::Failed);
    assert_eq!(chat_status(&StatusFacts { unseen: ChatStatus::Done, ..idle }, now), ChatStatus::Done);
    assert_eq!(chat_status(&idle, now), ChatStatus::Idle);
}
```

`crates/ui/tests/unit/chat/sidebar/mod.rs` (add):

```rust
#[test]
fn status_text_pairs_every_state_with_words() {
    let now = 1_400_000;
    assert_eq!(ChatStatus::Working { since_ms: 80_000 }.text(now), "Working · 22m 00s");
    assert_eq!(ChatStatus::Waiting.text(now), "Needs you");
    assert_eq!(ChatStatus::Background(2).text(now), "2 in background");
    assert_eq!(ChatStatus::Quiet { since_ms: now - 120_000 }.text(now), "Quiet 2m 00s");
    assert_eq!(ChatStatus::Retrying.text(now), "Retrying");
    assert_eq!(ChatStatus::Idle.text(now), "");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether --lib app::tests && cargo test -p aether-ui --lib sidebar`
Expected: compile errors.

- [ ] **Step 3: Implement,** then fix every `ChatStatus` match the compiler flags (`project_page/model.rs`, `projects.rs`, `chat/create.rs` and `send.rs` use `Idle`/`Done`/`Failed` unchanged).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether --lib && cargo test -p aether-ui --lib && cargo build -p aether`
Expected: all PASS.

- [ ] **Step 5: Manual check against the prototype sidebar,** across the chips "Live run", "Needs approval", "Errors", "Done", "Background", "Stalled" and "Retries & notices", in both themes. Each state shows its mark and words, and "Working · 0m 05s" ticks every second.

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/ui/src/chat/sidebar crates/ui/tests/unit/chat/sidebar crates/app/src/app/mod.rs crates/app/src/app/sidebar.rs crates/app/src/project_page crates/app/src/projects.rs crates/app/tests/unit/app/mod.rs
git commit -m "feat(sidebar): statuses for starting, needs you, quiet, retrying and background"
```

---

### Task 15: New-chat health cards and the version chip (needs phase 3 Task 21)

**Files:**
- Modify: `crates/ui/src/chat/hero.rs` (`HealthCard`), `crates/app/src/app/start.rs`, `crates/app/src/app/composer/header.rs`, `crates/app/src/app/providers.rs` (`pick_provider` becomes `pub(in crate::app)`)
- Test: `crates/app/tests/unit/app/start.rs`

**Interfaces:**
- Consumes: phase 3 Task 21's extended `ProviderProbe`. This plan assumes `version: Option<String>`, `status: String`, `degraded: Option<String>` and `capabilities: Vec<(String, bool)>`. Use Task 21's names if they differ. Also consumes `self.probes`, `api_key`, the OpenRouter model list and `provider_icon`.
- Produces, in `start.rs`:
  - `#[derive(Debug, PartialEq, Eq)] pub(crate) enum Health { Checking, Ready(String), Degraded(String), Missing }`
  - `pub(crate) fn health(probe: Option<&ProviderProbe>) -> Health`:
    - `None`: Checking
    - not installed: Missing
    - `degraded` set: Degraded
    - otherwise Ready(status)
  - `pub(crate) fn version_text(provider: Provider, probe: Option<&ProviderProbe>) -> String`: the probe version, "built-in" for OpenRouter, else "".
  - `pub(crate) fn version_chip(label: &str, version: &str) -> String`: "{label} {version}", trimmed.
- **`HealthCard`** (prototype `.ac`): radius 12, a 1 px `border` (the foreground colour when selected), and a `secondary` wash. It is a button, keyboard-focusable, and calls `pick_provider`.
  - The provider icon, the name in semibold, and the version in mono, faint, on the right.
  - The health line:
    - Ready: `CircleCheck` and `success_text`
    - Degraded: `TriangleAlert` and `mango_text`
    - Missing: `CircleX` and `danger_text`, with "Not installed" and an "Install" link when the probe has one
    - Checking: a spinner and "Checking…"
  - Capability chips (10.5 px, radius 4 inside the chip, muted). Unsupported ones are struck through at 0.5 with the tooltip "Not supported".

  The cards sit in a wrapping grid of 220 px cards with 8 px gaps, under the composer on the new-chat screen of agent chats, above "Recent in …".
- **Version chip** in `render_header`, on the right: the provider icon and `version_chip(label, version)`, as a quiet chip. Agent chats only.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn health_reads_the_probe() {
    let ready = ProviderProbe { installed: true, version: Some("2.0.32".into()), status: "Service running on :4096".into(), ..ProviderProbe::default() };
    assert_eq!(health(None), Health::Checking);
    assert_eq!(health(Some(&ProviderProbe::default())), Health::Missing);
    assert_eq!(health(Some(&ready)), Health::Ready("Service running on :4096".into()));
    let degraded = ProviderProbe { degraded: Some("Auto mode unavailable on this plan; runs in Default".into()), ..ready.clone() };
    assert_eq!(health(Some(&degraded)), Health::Degraded("Auto mode unavailable on this plan; runs in Default".into()));
    assert_eq!(version_text(Provider::OpenCode, Some(&ready)), "2.0.32");
    assert_eq!(version_text(Provider::OpenRouter, None), "built-in");
    assert_eq!(version_chip("OpenCode", "2.0.32"), "OpenCode 2.0.32");
    assert_eq!(version_chip("Claude Code", ""), "Claude Code");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p aether --lib app::start`
Expected: compile errors.

- [ ] **Step 3: Implement.**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aether --lib && cargo build -p aether`
Expected: all PASS.

- [ ] **Step 5: Manual check against the prototype chip "New chat",** in both themes, with one CLI uninstalled. There is one card per Provider with its version, health and capability chips. The missing CLI says so. Enter on a focused card picks it. The chat header shows "OpenCode 2.0.32".

- [ ] **Step 6: Commit (after user approval)**

```bash
git add crates/ui/src/chat/hero.rs crates/app/src/app/start.rs crates/app/src/app/composer/header.rs crates/app/src/app/providers.rs crates/app/tests/unit/app/start.rs
git commit -m "feat(chat): provider health cards on new chat, version chip in the header"
```

---

### Task 16: Phase gate

- [ ] **Step 1: Full test and lint run**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: all tests pass, and clippy reports no warnings.

- [ ] **Step 2: Rubric sweep.** Run `rg -n 'rgb\(0x|ShimmerText|"JetBrainsMonoNL|border_l_2|border_l_3' crates/ui/src/chat crates/app/src/app`.
Expected: matches only in `crates/ui/src/theme.rs` and the existing `chat/mod.rs` brand colours.

- [ ] **Step 3: Prototype walkthrough.**
  1. Open the prototype beside the app.
  2. For every chip (New chat, Starting, Thinking, Writing, Running tools, Subagents, Needs approval, Question, Background, Retries & notices, Stalled, Errors, Stopped, Steering, Done, and Live run), reproduce the state with a real Provider: OpenCode for subagents and the stall, Claude for approvals and the footer, Codex for background terminals. Do this in the dark theme, then the light theme.
  3. Review each surface with `docs/ux-rubric.md` section 2. Also check with reduce motion on, and with the keyboard only (Tab, Enter, Esc, arrows).
  4. Record findings as `[Severity] surface, what, impact, fix`. Blockers and Majors are fixed before the phase closes.

- [ ] **Step 4: Performance check.**
  1. Open a chat with 2,000 messages and a reply with 300 tool calls.
  2. Scroll, type in the composer, and stream a new reply.
  3. Watch with Instruments (Time Profiler). Frames stay at 60 Hz, and no `link_inline_code`, `parse_markdown_blocks` or `path.exists` shows on the main thread while streaming.
