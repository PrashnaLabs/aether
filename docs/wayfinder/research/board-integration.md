# Kanban Tasks Board Integration Research

Integration mapping for a native kanban "Tasks board" tracking AI agent work and human-made cards in Aether.

---

## 1. Pages & Navigation

### Page Registration & Lifecycle
- **Enum definition**: `Page` enum in `crates/app/src/app/mod.rs:66-77`. Add variant `Board` (or `Tasks`).
- **Active page state**: `ChatApp.page: Page` in `crates/app/src/app/mod.rs:163`.
- **Page transition**: `ChatApp::open_page(&mut self, page: Page, cx: &mut Context<Self>)` in `crates/app/src/app/mod.rs:698-756`. It clears errors, dismisses open dropdown menus and popovers (`mod.rs:741-754`), and requests focus/scans.
- **Root rendering**: `ChatApp::render` in `crates/app/src/app/render.rs:14-140`. The page body switches in `render.rs:84-101`:
  ```rust
  let body = if self.page == Page::Settings { ... }
  // Insert: else if self.page == Page::Board { self.render_board_page(window, cx) }
  ```
- **Back navigation (Esc)**: `BackToChat` action handler in `crates/app/src/app/render.rs:191-203`. Add `Page::Board` to the `matches!(this.page, ...)` check so pressing Escape returns to `Page::Chat`.

### Opening Mechanisms
- **Command Palette**: In `crates/app/src/palette.rs:511-562`, add an action row to `groups`:
  `action_row("Open tasks board", IconName::Kanban, Box::new(crate::actions::OpenBoard))` (similar to `OpenNotes` at `palette.rs:515-519` and `OpenUsage` at `palette.rs:553`).
- **Actions & Keybindings**:
  - Define action: In `crates/app/src/actions.rs:40-60` inside the `actions!(aether, [...])` macro invocation (e.g. `OpenBoard`).
  - Key binding: In `crates/app/src/actions.rs:95-150` inside `bind_keys()` via `KeyBinding::new("alt-b", OpenBoard, None)`.
  - App menu bar: In `crates/app/src/menus.rs:64-74` under `View` menu via `MenuItem::action("Tasks Board", OpenBoard)`.
  - Handler binding: In `crates/app/src/app/render.rs:170-254` via `.on_action(cx.listener(|this, _: &OpenBoard, window, cx| this.open_board(window, cx)))`.
- **Sidebar Integration**:
  - `ChatSidebar` is rendered by `crates/app/src/app/sidebar.rs:318-400` with chrome built in `sidebar_chrome` (`sidebar.rs:696-739`).
  - A top-level button or view toggle (similar to `SidebarView` at `mod.rs:90-96` or a sidebar header icon button) can call `this.open_page(Page::Board, cx)`.

### Template Comparison
- **Watch List** (`docs/superpowers/specs/2026-10-29-watch-list-design.md`, `crates/app/src/app/watch/`): Not a `Page`. It is a floating overlay FAB + draggable peek card modal mounted at `crates/app/src/app/render.rs:312`.
- **Smallest standalone page**: `crates/app/src/agents_view.rs` (310 lines), but it wraps in `SettingsPage`.
- **Smallest full-canvas page**: `crates/app/src/notes/view.rs` (376 lines) and `crates/app/src/notes/mod.rs`, providing a custom layout, sidebar, and canvas.
- **Best visual/structural reference for cards**: `crates/app/src/project_page/` (`view.rs:656 lines`, `model.rs:419 lines`). It already models cards, card statuses, groupings, filters, and rendered card elements.

---

## 2. Persistence: The Store (sqlx SQLite)

### Database Location & Connection
- **DB File**: Stored at `<config_dir>/aether.db` via `open_store` in `crates/app/src/app/new.rs:828-834`.
- **Store struct**: Defined in `crates/agent/src/store.rs:70-74`. Wraps `SqlitePool`, a tokio `Handle`, and an unbounded `mpsc::UnboundedSender<Write>`.

### Table Creation & Migration (ALTER TABLE Pattern)
- Initial table creation in `crates/agent/src/store.rs:11-30` (`const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS chats (...)";`).
- Executed on startup in `Store::open` (`store.rs:98-99`).
- **ALTER TABLE pattern**: Inspects columns via `PRAGMA table_info(chats)` (`store.rs:100-105`), checks `cols.iter().any(...)`, and executes `ALTER TABLE chats ADD COLUMN ...` for missing columns (`store.rs:106-165`).
- **Adding a board table**:
  In `Store::open` (`crates/agent/src/store.rs:99`), execute:
  ```sql
  CREATE TABLE IF NOT EXISTS board_cards (
      id INTEGER PRIMARY KEY,
      chat_id INTEGER,
      title TEXT NOT NULL,
      description TEXT NOT NULL DEFAULT '',
      column TEXT NOT NULL DEFAULT 'backlog',
      position INTEGER NOT NULL DEFAULT 0,
      created_at INTEGER NOT NULL,
      updated_at INTEGER NOT NULL
  );
  ```
  Check `PRAGMA table_info(board_cards)` for future column additions using the existing loop pattern (`store.rs:143-165`).

### Save & Load Pipelines
- **Chat save**:
  - `ChatApp::persist(&self, ix: usize)` in `crates/app/src/app/chat/mod.rs:410-444` extracts `SavedChat` (`crates/agent/src/store.rs:35-58`).
  - Calls `store.save(chat)` (`store.rs:232-237`), queuing `Write::Save(Box<SavedChat>)` to an async worker thread (`store.rs:170-183`).
  - `async fn save(pool: &SqlitePool, chat: &SavedChat)` (`store.rs:274-325`) serializes messages to JSON and executes an `INSERT ... ON CONFLICT(id) DO UPDATE SET ...` upsert.
  - On quit, `store.flush()` (`crates/app/src/app/new.rs:449`) drains pending writes synchronously.
- **Chat load**:
  - `ChatApp::restore_chats(&mut self)` in `crates/app/src/app/chat/create.rs:370-430` calls `store.load()` (`store.rs:188-223`).
  - Deserializes rows into `SavedChat`s, parses JSON messages via `parse_messages` (`store.rs:264-272`), recreates runtime `Chat` instances, and restores agent sessions via `self.agent.fork_session(...)`.

---

## 3. Agent State & Orchestration Children

### Per-Chat Status Location
- Defined in `struct Chat` (`crates/app/src/app/mod.rs:109-181`):
  - `cancel: Option<Cancel>` (`mod.rs:119`): `Some` while a turn is actively executing; `None` when idle.
  - `unseen: ChatStatus` (`mod.rs:126`): tracks whether the last background turn ended in `Done` or `Failed`.
  - `turn_at: i64` (`mod.rs:139`): start timestamp of the current turn in ms.
  - `last_event_ms: i64` (`mod.rs:178`): timestamp of the most recent streaming event.
  - `messages: Vec<Message>` (`mod.rs:114`): holds turn messages. Last message's `pending_request` (`mod.rs:330-332`) reveals pending user inputs (`Ask::Question`, `Ask::Plan`, or tool approvals).
- **Status evaluation**:
  - `Chat::status(&self, now_ms: i64) -> ChatStatus` in `crates/app/src/app/mod.rs:322-341`.
  - Maps `StatusFacts` via `chat_status` (`mod.rs:196-221`) to `ChatStatus` (`crates/ui/src/chat/sidebar/mod.rs:36-57`):
    - `Starting`: turn started, awaiting first event.
    - `Working { since_ms }`: turn actively streaming.
    - `Waiting`: paused on user question or approval (`ask: true`).
    - `Quiet`: running but silent for $\ge$ 15s (`SILENT_MS`).
    - `Retrying`: provider rate-limit or retry.
    - `Background(n)`: turn ended, but $n$ background subprocesses still running.
    - `Done` / `Failed` / `Idle`: finished states.
  - `CardStatus` in `crates/app/src/project_page/model.rs:101-108`: maps `ChatStatus` to `Working`, `Waiting(&'static str)`, `Failed`, `Done`, `Idle` via `card_status()` (`model.rs:142-155`).

### UI Subscription
- `ChatApp` is a GPUI entity (`Entity<ChatApp>`).
- Active turns stream via `cx.spawn_in(window, ...)` in `crates/app/src/app/chat/send.rs`, calling `cx.notify()` as tokens/steps arrive.
- Background turn events arrive over `app.agent.side_updates()` in `crates/app/src/app/new.rs:781-806`, applying updates and notifying `cx`.
- Clocks and timers update on frame renders with `now_ms()`.

### Lead and Child Orchestration Relationships
- **Fields**: Lead has `chat.lead == true` (`mod.rs:174`); child has `chat.parent == Some(lead_id)` (`mod.rs:172`). Persisted in SQLite `parent` and `lead` columns (`crates/agent/src/store.rs:26-27`).
- **Sidebar nesting**: `nest_children(&self.chats, &g.chats)` in `crates/app/src/app/sidebar.rs:382-386` clusters children directly beneath their lead.
- **Automatic result delivery**: When a child finishes in `finish_run` (`crates/app/src/app/chat/manage.rs:23-55`), its final reply is queued into the lead's `queue: Vec<Queued>` with `from_children: true` (`crates/app/src/app/mod.rs:223-240`). Results are pushed without the lead polling.

---

## 4. Orchestration MCP

### Architecture
- `aether mcp` runs as a stdio MCP server (`crates/app/src/main.rs:40-42`), delegating to `aether_agent::orchestrator::mcp::run_stdio()`.
- MCP stdio calls are forwarded over local Unix socket `<config_dir>/orchestrator.sock` (`crates/agent/src/orchestrator/mod.rs:52-54`, `server.rs:64-106`).
- The running Aether GUI window listens on the socket and processes incoming tool calls via GPUI's async executor (`crates/app/src/app/new.rs:808-820`).

### Tool Definitions & Dispatch
- **Tool Schemas**: Defined in `pub fn tool_specs() -> Value` in `crates/agent/src/orchestrator/mod.rs:56-116`. Lists `list_agents`, `spawn_agent`, `read_agent`, `message_agent`, `stop_agent`.
- **MCP Protocol Handler**: `handle()` in `crates/agent/src/orchestrator/mcp.rs:7-39` returns `tool_specs()` on `tools/list` and dispatches `tools/call`.
- **Native OpenRouter Leads**: `crates/agent/src/tools/lead.rs:65-87` loops over `tool_specs()`, exposing them as in-process `LeadTool`s automatically.
- **GUI Dispatch**: `ChatApp::orchestrate(&mut self, call: OrchCall, window: &mut Window, cx: &mut Context<Self>)` in `crates/app/src/app/orchestrate.rs:244-263`:
  ```rust
  match call.tool.as_str() {
      "list_agents" => Ok(self.list_agents(ix)),
      "spawn_agent" => self.spawn_agent(ix, &call.args, window, cx),
      "read_agent" => self.read_agent(ix, &call.args),
      "message_agent" => self.message_agent(ix, &call.args, window, cx),
      "stop_agent" => self.stop_agent(ix, &call.args, cx),
      tool => Err(format!(r#"unknown tool "{tool}""#)),
  }
  ```

### Adding Board Tools (`board_add`, `board_move`, `board_list`)
Only three touches needed:
1. **Schema**: Add JSON schema to `tool_specs()` in `crates/agent/src/orchestrator/mod.rs:56-116`.
2. **Dispatch**: Add match arms to `ChatApp::orchestrate` in `crates/app/src/app/orchestrate.rs:252-259`.
3. **Execution**: Implement helper methods on `ChatApp` in `crates/app/src/app/orchestrate.rs` (or a dedicated board submodule) that manipulate the board SQLite table / in-memory state and return `Ok(json!(...))`. All agent harnesses inherit the tools automatically.

---

## 5. Drag and Drop Patterns

### Existing State in Codebase
- **Tab reordering**: Not implemented (tabs in `crates/ui/src/components/tabs.rs` are static click targets).
- **Sidebar reordering**: Not implemented (ordered by project groups and activity).
- **Panes, splits, and floating windows**: Extensively implemented using GPUI's drag APIs.

### Reusable GPUI Drag & Drop Patterns
1. **Continuous Divider Drag (Splits)**:
   - Pattern in `crates/app/src/session_workspace/render.rs:391-430` (`DividerDrag`):
     - Source: `.on_drag(DividerDrag(id, host, axis), |d, _, _, cx| cx.new(|_| *d))`.
     - Container: `.on_drag_move(cx.listener(|app, e: &DragMoveEvent<DividerDrag>, _, cx| ...))`.
     - See also explorer split in `crates/app/src/explorer/mod.rs:368-373` (`TreeDrag`).
2. **Floating Element Drag & Resize**:
   - `crates/app/src/session_workspace/pane.rs:327-332` and `render.rs:182-204` (`PaneDrag`).
   - `crates/app/src/app/watch/view.rs:442-515` (`PeekDrag` with `on_drag`, `on_drag_move`, and `end_peek_drag`).
3. **Typed Drop Target (`drag_over` & `on_drop`)**:
   - Pattern in `crates/ui/src/chat/composer.rs:1243-1249`:
     ```rust
     d.drag_over::<Payload>(|style, _, _, cx| {
         style.border_color(cx.theme().accent)
     })
     .on_drop::<Payload>(move |payload, window, cx| {
         // handle drop
     })
     ```

### Kanban Implementation Pattern
- Define drag payload: `#[derive(Clone)] struct CardDrag { card_id: i64, source_col: String }`.
- **Card**: Add `.on_drag(CardDrag { ... }, |d, _, _, cx| cx.new(|_| CardGhostView))`.
- **Column**: Add `.drag_over::<CardDrag>(|style, _, _, cx| style.bg(overlay(cx)).border_color(cx.theme().primary))` and `.on_drop::<CardDrag>(move |drag, window, cx| { ... })`.

---

## 6. Theme & UI Primitives

### Card Primitives
- **Existing Agent Card**: `project_card` in `crates/app/src/project_page/view.rs:411-513`. Perfect ready-made template:
  - Container: `v_flex().min_w(px(290.)).h(px(120.)).p(px(12.)).corner(px(12.)).border_1().border_color(theme.border)`.
  - Hover & Pressed fills: `.hover(|s| s.bg(fill)).active(|s| s.bg(pressed))` using `overlay(cx)` and `overlay_strong(cx)`.
  - Header: Provider icon (`provider_icon(card.provider)`), medium title (`truncate()`), and status badge pill.
  - Body: 2-line clamped preview (`line_clamp(2)`, `text_xs()`, `muted_foreground`).
  - Meta footer: Branch icon + branch name, diff tokens (`+added` / `-removed`), model name, and relative time (`relative_time`).
- **GlowCard**: `crates/ui/src/components/glow_card.rs` (mango gradient glow ring on focus).
- **ChoiceCard**: `crates/ui/src/components/choice_cards.rs`.

### Columns
- Built with `v_flex()` and `h_flex()` from `gpui_kit::component::{h_flex, v_flex}`.
- Column headers: Follow `project_page/view.rs:388-399` using `text_xs()`, `text_color(muted)`, and item counts.
- (Note: `crates/ui/src/components/column.rs` is a centered prose container at `CONTENT_WIDTH: 760px`, not a board column).

### Status Chips & Badges
- **Pill badge**: `pill(text, bg, fg)` in `crates/app/src/project_page/view.rs:30-40`:
  `div().flex_none().px(px(7.)).py(px(2.)).corner(px(6.)).text_xs().bg(bg).text_color(fg).child(text)`.
- **Composer / Control chips**: `composer_chip` in `crates/ui/src/components/composer_chip.rs`.
- **Count chips**: `count_chip` in `crates/ui/src/chat/sidebar/rows.rs`.

### Color & Contrast Tokens
- Semantic status colors from `cx.theme()` (`crates/app/src/project_page/view.rs:421-428` and `crates/ui/src/theme.rs:119-139`):
  - **Running (`Working`)**: `theme.info.opacity(0.12)` bg, `theme::info_text(cx)` fg.
  - **Needs Input (`Waiting`)**: `theme::mango().opacity(0.14)` bg, `theme::mango_text(cx)` fg.
  - **Error (`Failed`)**: `theme.danger.opacity(0.12)` bg, `theme::danger_text(cx)` fg.
  - **Done / Idle**: `theme.success.opacity(0.12)` bg, `theme::success_text(cx)` fg.
- Surface and border tokens:
  - `theme.border`, `theme.ring` for focus rings.
  - Overlays: `overlay(cx)` and `overlay_strong(cx)` from `crates/ui/src/components/composer_chip.rs`.
  - Scaled corners: `Corners` trait in `crates/ui/src/theme.rs:25-39` via `.corner(px(12.))`.
