# Tasks Board Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A native kanban page in Aether whose cards track agent chats and human work, editable by humans and by lead agents through the orchestration MCP.

**Architecture:** Cards persist in a new `cards` table in the existing Store (`crates/agent/src/store.rs`). Pure board rules live in `crates/app/src/board/model.rs`; `ChatApp` holds `Vec<BoardCard>` and exposes board methods in `crates/app/src/board/mod.rs`; the page renders from `crates/app/src/board/view.rs`. Card status pills read live from linked `Chat`s, never stored.

**Tech Stack:** Rust 2024, gpui + gpui-kit, sqlx sqlite, serde_json.

**Spec:** `docs/superpowers/specs/2026-11-06-tasks-board-design.md`

## Global Constraints

- Columns fixed, keys `backlog|todo|doing|review|done`, labels `Backlog, Todo, Doing, Review, Done`.
- Done is human-only: leads get the error `only a human can move a card to done`.
- Status pill reuses `project_page::model::card_status` and `pill` colors from `project_page/view.rs`. No stored status.
- SQLite writes go through the Store's existing `Write` queue; never block the UI thread except `load_cards` at startup (same as `load`).
- Orchestration replies for board tools are plain strings (`Value::String`).
- Never `git commit`, `git add`, or push. The user commits.
- No new dependencies. No comments unless the why is non-obvious. Match existing style.
- Keybinding `alt-b` for `OpenBoard` (verified free).
- Verify each task with `cargo test -p <crate>` and `cargo clippy --workspace --all-targets -- -D warnings`.

## Review Focus

- Card linked to a chat that was deleted or archived: pill hidden, card still renders, Open chat button hidden, Run button stays hidden (still linked). Test in Task 2 (`pill_absent_when_chat_missing` via `linked_status` helper taking `Option`).
- `move_card` / `add_card` with unknown column or missing id: clear error string, no state change. Test in Task 2.
- Repeated midpoint drops in the same gap: positions stay strictly ordered for at least 40 inserts. Test in Task 2.
- Child spawned with `card` id that does not exist: spawn fails before creating the child with `card #<id> not found`. Covered in Task 3 by validating before `create_child`.
- New card left with empty title: deleted on blur, not persisted as blank. Task 4 manual check.

---

### Task 1: Store `cards` table

**Files:**
- Modify: `crates/agent/src/store.rs` (schema, `Store::open`, `Write` enum, worker loop, new pub fns)
- Test: the existing store test module (`mod tests;` at end of `store.rs`)

**Interfaces:**
- Produces:
  ```rust
  pub struct SavedCard { pub id: i64, pub title: String, pub body: String, pub col: String,
      pub position: f64, pub chat: Option<i64>, pub project: String, pub updated_at: i64 }
  impl Store {
      pub fn load_cards(&self) -> sqlx::Result<Vec<SavedCard>>; // ordered by col, position
      pub fn save_card(&self, card: SavedCard);                 // queued upsert
      pub fn delete_card(&self, id: i64);                       // queued
  }
  ```
  `SavedCard` derives `Clone, Debug, PartialEq`; re-export from `aether_agent` next to `SavedChat`.

- [ ] **Step 1: Write failing tests** `cards_round_trip` (save two cards, `flush`, `load_cards` returns both with equal fields), `card_delete` (save, delete, flush, empty), `old_db_gains_cards_table` (create db with only the old `chats` schema, open Store, `load_cards` is `Ok(vec![])`). Follow the existing tests' temp-file setup.
- [ ] **Step 2:** `cargo test -p aether-agent --lib store` fails (missing items).
- [ ] **Step 3:** Add `CARDS_SCHEMA` const with the spec's SQL, run it in `Store::open` after `SCHEMA`. Add `Write::SaveCard(SavedCard)` and `Write::DeleteCard(i64)`; worker executes `INSERT ... ON CONFLICT(id) DO UPDATE` and `DELETE FROM cards WHERE id = ?`, logging errors like chat saves.
- [ ] **Step 4:** Tests pass; clippy clean.
- [ ] **Step 5:** Do not commit. Leave changes uncommitted for the user to review.

### Task 2: Pure board model

**Files:**
- Create: `crates/app/src/board/mod.rs` (only `pub(crate) mod model;` for now), `crates/app/src/board/model.rs`
- Modify: `crates/app/src/main.rs` or wherever top-level `mod` lines live (`mod board;`)

**Interfaces:**
- Consumes: `SavedCard` (Task 1), `CardStatus`, `card_status` from `crate::project_page::model`.
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub(crate) enum Col { Backlog, Todo, Doing, Review, Done }
  impl Col {
      pub(crate) const ALL: [Col; 5];
      pub(crate) fn key(self) -> &'static str;
      pub(crate) fn label(self) -> &'static str;
      pub(crate) fn parse(s: &str) -> Option<Col>; // trims, lowercase
  }
  #[derive(Clone, Debug, PartialEq)]
  pub(crate) struct BoardCard { pub id: i64, pub title: String, pub body: String, pub col: Col,
      pub position: f64, pub chat: Option<i64>, pub project: String, pub updated_at: i64 }
  impl From<SavedCard> for BoardCard;  // unknown col -> Todo
  impl From<&BoardCard> for SavedCard;
  pub(crate) enum Turn { Started, Finished { failed: bool } }
  pub(crate) fn auto_move(col: Col, turn: Turn) -> Option<Col>;
  pub(crate) fn position_between(prev: Option<f64>, next: Option<f64>) -> f64;
  pub(crate) fn list_line(card: &BoardCard) -> String;
  pub(crate) fn lead_move(card: &mut BoardCard, to: &str) -> Result<String, String>;
  ```

- [ ] **Step 1: Write failing tests** in `model.rs` `#[cfg(test)] mod tests`:
  - `parse_keys`: `Col::parse(" Doing ") == Some(Col::Doing)`, `Col::parse("blocked") == None`, all `ALL` round-trip through `key`.
  - `auto_move_rules`: `(Backlog, Started) -> Some(Doing)`, `(Todo, Started) -> Some(Doing)`, `(Review, Started) -> None`, `(Doing, Finished{failed:false}) -> Some(Review)`, `(Doing, Finished{failed:true}) -> None`, `(Done, Started) -> None`.
  - `positions`: `position_between(None, None) == 1.0`, `(Some(3.0), None) == 4.0`, `(None, Some(2.0)) == 1.0`, `(Some(1.0), Some(2.0)) == 1.5`; 40 repeated inserts between `1.0` and the last result all strictly between their neighbours.
  - `list_line_format`: card id 7, col Doing, title "Fix parser", chat Some(42) gives `#7 [doing] Fix parser (agent 42)`; chat None gives `#7 [doing] Fix parser`.
  - `lead_move_rules`: Todo to "review" returns `Ok("Moved #7 todo -> review: Fix parser")` and mutates col; to "done" returns `Err("only a human can move a card to done")` unchanged; to "nope" returns `Err(r#"unknown column "nope"; use backlog, todo, doing or review"#)` unchanged.
- [ ] **Step 2:** `cargo test -p aether board` fails (crate name per `crates/app/Cargo.toml`).
- [ ] **Step 3:** Implement. `position_between`: both: midpoint; prev only: prev+1; next only: next-1; none: 1.0.
- [ ] **Step 4:** Tests pass. Skip clippy here: model items stay unused outside tests until Task 3, which runs clippy.
- [ ] **Step 5:** Do not commit. Leave changes uncommitted for the user to review.

### Task 3: Board state, lead tools, lifecycle hooks

**Files:**
- Modify: `crates/app/src/board/mod.rs` (ChatApp board methods)
- Modify: `crates/app/src/app/mod.rs` (fields `pub(crate) cards: Vec<BoardCard>`, `pub(crate) board_selected: Option<i64>` on `ChatApp`)
- Modify: `crates/app/src/app/new.rs` (init `cards` from `store.load_cards()` next to chat restore; empty on error or no store)
- Modify: `crates/agent/src/orchestrator/mod.rs` (`tool_specs`: three tools + `card` on `spawn_agent`)
- Modify: `crates/app/src/app/orchestrate.rs` (dispatch arms; `SpawnArgs.card: Option<i64>`; link after spawn)
- Modify: `crates/app/src/app/chat/send.rs` (turn start at the two `chat.cancel = Some(..)` sites ~L352, ~L415; `finish_run` ~L529)

**Interfaces:**
- Consumes: Task 1 Store fns, Task 2 model.
- Produces (on `impl ChatApp` in `board/mod.rs`):
  ```rust
  pub(crate) fn board_add(&mut self, title: String, body: String, col: Col, project: String, chat: Option<i64>) -> i64; // appends at column end, persists, returns id (now_ms, bumped if taken)
  pub(crate) fn board_save(&self, id: i64);                     // persist one card
  pub(crate) fn board_move(&mut self, id: i64, col: Col, before: Option<i64>); // human drag; before = card to insert above
  pub(crate) fn board_delete(&mut self, id: i64);
  pub(crate) fn board_link(&mut self, id: i64, chat: i64);      // sets chat, col = Doing, persists
  pub(crate) fn board_on_turn(&mut self, chat: i64, turn: Turn); // applies auto_move to every card linked to chat
  pub(crate) fn board_status(&self, card: &BoardCard, now_ms: i64) -> Option<CardStatus>; // None when unlinked or chat gone
  ```
  Tool names: `list_cards {}`, `add_card {title, body?, col?}`, `move_card {id, col}`; `spawn_agent` optional `card: integer`, reply gains `"card": <id>`.

- [ ] **Step 1:** Add tool specs; descriptions one sentence each. `list_cards`: "List open task-board cards, one line each." `add_card`: "Add a card to the task board (default column todo)." `move_card`: "Move a task-board card to backlog, todo, doing or review." `spawn_agent.card`: "Optional board card id to link; omitted adds a card for this agent".
- [ ] **Step 2:** Implement `board/mod.rs` methods. `board_status` finds the chat by id (skip archived), then calls `card_status(chat.status(now), last_failed, wait)` the same way `project_page` builds its cards (copy that call site's arguments).
- [ ] **Step 3:** Dispatch arms: `list_cards` joins `list_line` of non-Done cards in column order then position, empty board returns `"No open cards."`; `add_card` validates title non-empty and col via `Col::parse` (error `unknown column "<x>"; use backlog, todo, doing or review`), project = lead's `chat_cwd` display string, returns `Created #<id> in <col>: <title>`; `move_card` uses `lead_move` then persists. Wrap strings with `Value::String`.
- [ ] **Step 4:** `spawn_agent`: parse `card`; if Some and missing, return `Err(format!("card #{id} not found"))` before `create_child`. After the child exists: link given card, or `board_add(a.title, a.task, Col::Doing, lead folder, Some(child_id))`. Put card id in reply.
- [ ] **Step 5:** Hooks: after each `chat.cancel = Some(..)` in `send.rs` call `self.board_on_turn(chat_id, Turn::Started)` (capture id before borrow ends); in `finish_run` after `failed` is known and `chat.epoch == epoch`, call `self.board_on_turn(id, Turn::Finished { failed })`.
- [ ] **Step 6:** `cargo test --workspace` passes; clippy clean; `cargo build -p aether` OK.
- [ ] **Step 7:** Do not commit. Leave changes uncommitted for the user to review.

### Task 4: Board page UI

**Files:**
- Create: `crates/app/src/board/view.rs`
- Modify: `crates/app/src/board/mod.rs` (`mod view;`, `open_board`, `board_new_card`, `board_run`)
- Modify: `crates/app/src/app/mod.rs` (`Page::Board`; title/body `Entity<InputState>`/`Entity<TextareaState>` fields for the detail panel)
- Modify: `crates/app/src/app/new.rs` (create the two inputs like `rename_input` / auto-grow textareas)
- Modify: `crates/app/src/app/render.rs` (body branch `Page::Board => self.render_board(window, cx)`, `OpenBoard` listener, add `Page::Board` to the `BackToChat` matches!)
- Modify: `crates/app/src/actions.rs` (`OpenBoard` in `actions!`, `KeyBinding::new("alt-b", OpenBoard, None)`)
- Modify: `crates/app/src/palette.rs` (row "Tasks board" next to Open Notes, icon: pick an existing grid/list icon from `IconName`)
- Modify: `crates/app/src/menus.rs` (View menu item "Tasks Board")

**Interfaces:**
- Consumes: Task 3 methods.
- Produces: `pub(crate) fn open_board(&mut self, window: &mut Window, cx: &mut Context<Self>)` (toggle like `toggle_usage_page`), `pub(crate) fn render_board(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement`, `#[derive(Clone)] struct CardDrag { id: i64, title: SharedString }` with a small ghost view rendering the title.

- [ ] **Step 1:** Page plumbing (enum, action, key, palette, menu, render branch, Esc). Build: page opens and shows an empty five-column board.
- [ ] **Step 2:** Columns: header row (`Col::label` + count, styled like `project_page/view.rs` captions), "New card" button in page header. Each column `v_flex` with own scroll, cards sorted by position.
- [ ] **Step 3:** Card element modeled on `project_card` (`project_page/view.rs:411`): title truncated, `line_clamp(2)` body, project folder name (last path component), status pill from `board_status` using the same color mapping as `project_card`. Selected card gets `theme.ring` border. Click sets `board_selected` and loads title/body into the inputs.
- [ ] **Step 4:** Drag and drop: card `.on_drag(CardDrag{..}, ..)`; column `.drag_over::<CardDrag>` highlight + `.on_drop` calling `board_move(id, col, None)`; card `.on_drop` calling `board_move(id, card.col, Some(card.id))`.
- [ ] **Step 5:** Detail panel (right side, ~320px, shown when `board_selected` is Some): title input, body textarea, column label, buttons Run with agent (unlinked), Open chat (linked and chat present; selects chat and opens `Page::Chat`), Delete. Save title/body on input blur/Enter via `board_save`. Empty title on blur deletes the card.
- [ ] **Step 6:** `board_new_card`: `board_add("New card", "", Col::Todo, current project folder or "", None)`, select it, focus title with text selected. `board_run(id)`: `new_chat_in(folder)` (folder = card.project, else workspace root), `submit(ix, text, vec![], ..)` with text = title + "\n\n" + body, then `board_link(id, chat_id)`.
- [ ] **Step 7:** `cargo build`, clippy clean, `cargo test --workspace`. Manual (use the `run` skill): open with alt-b, add card, drag Todo to Review, reorder, run a card and watch it move Doing then Review, spawn a child from a lead and see its card appear.
- [ ] **Step 8:** Do not commit. Leave changes uncommitted for the user to review.
