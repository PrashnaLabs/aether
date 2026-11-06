# Watch-list Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A global bottom-right FAB listing watched agent chats, direct chats and agent-chat terminals, with an interactive peek and a jump-to button.

**Architecture:** The list is a `Vec<Watch>` in core `Settings`. `ChatApp` owns the transient UI state (list open, peeked item) in a new `app::watch` module, prunes the list on archive, delete, terminal close and load, and draws the FAB, list and peek as a root overlay in the main window. Chat peeks reuse the session-pane path (`ensure_session_ui` + `in_session`), and terminal peeks borrow the chat's terminal drawer, which the usual spots skip while it's peeked.

**Tech Stack:** Rust 2024, GPUI (gpui-pre 0.3.9), gpui-kit/gpui-component 0.7.3, serde.

**Spec:** `docs/superpowers/specs/2026-10-29-watch-list-design.md`

## Global Constraints

- Code names avoid `watched` for the existing `session_is_watched` meaning; the new type is `Watch`, state lives in `ChatApp.watch_open` / `ChatApp.watch_peek`.
- FAB only in the main window (root render, host 0). No FAB in detached session windows.
- One peek at a time. Esc never closes the peek. Clicking outside never closes the peek. Only ✕, the FAB, Open ↗, or the gives-way rule close it.
- Rows appear in the order added. The FAB stays visible when the list is empty.
- Closures in menus and rows capture chat ids, never indices.
- Tests must never write the user's real `settings.json` (Task 2 makes `settings::save` a no-op under `cfg(test)`).
- No code comments unless a non-obvious why, max 2 terse lines. Only run rustfmt on files that were rustfmt-clean at HEAD (`crates/app/src/app/mod.rs` and `crates/app/src/main.rs` are not).
- Icons: `IconName::Eye` and `IconName::EyeOff` are already available; anything else must be registered in `crates/app/src/assets.rs` `icon_assets!`.
- Test commands: `cargo test -p aether-core`, `cargo test -p aether-ui --lib`, `cargo test -p aether --bin aether -- --skip hackernews::client` (4 hackernews tests fail pre-existing without the skip).
- Commits: Conventional Commits, terse, never a `Co-Authored-By` line, and only after the user approves each one. Never push.

## Review Focus

- A watched chat is archived or deleted while its peek is open: the peek closes and nothing indexes a stale chat (Task 2 test `archiving_a_watched_chat_drops_it_and_closes_its_peek`).
- The peeked chat comes on screen another way (sidebar click, palette, tab): the peek closes on the next render (Task 5 test `peek_gives_way_when_its_chat_is_on_screen`).
- Esc in a chat peek stops the peeked chat, not the active one, and never triggers `BackToChat` on the page underneath (Task 5 test `stop_in_peek_targets_the_peeked_chat`).
- A peeked terminal keeps polling and isn't also drawn under the chat, in the side panel's Terminal tab, or on the Terminal page (Task 5, GUI check; exemption logic tested by `peeked_terminal_is_skipped_by_the_drawer`).
- The Browser and Hacker News web views come back after the list or peek closes (Task 4 test `web_views_uncover_when_the_watch_overlay_closes`).

---

### Task 1: `Watch` in core settings

**Files:**
- Modify: `crates/core/src/settings.rs` (new enum, new field in `Settings` and its `Default`)
- Modify: `crates/core/src/lib.rs:17-20` (re-export `Watch`, `toggle_watch` beside `TileLayout`)
- Test: `crates/core/tests/unit/settings.rs`

**Interfaces:**
- Produces: `pub enum Watch { Chat(i64), Terminal(i64) }` (`Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize`), `impl Watch { pub fn chat(self) -> i64 }`, `Settings.watchlist: Vec<Watch>`, `pub fn toggle_watch(list: &mut Vec<Watch>, item: Watch) -> bool` (true when now watched).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn toggle_watch_adds_once_then_removes() {
    let mut list = vec![Watch::Chat(1)];
    assert!(toggle_watch(&mut list, Watch::Terminal(1)));
    assert_eq!(list, [Watch::Chat(1), Watch::Terminal(1)]);
    assert!(!toggle_watch(&mut list, Watch::Chat(1)));
    assert_eq!(list, [Watch::Terminal(1)]);
    assert_eq!(Watch::Terminal(9).chat(), 9);
}

#[test]
fn old_settings_load_with_an_empty_watchlist() {
    let s: Settings = serde_json::from_str("{}").unwrap();
    assert!(s.watchlist.is_empty());
}

#[test]
fn watchlist_round_trips() {
    let s = Settings { watchlist: vec![Watch::Chat(7), Watch::Terminal(7)], ..Settings::default() };
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.watchlist, s.watchlist);
}
```

- [ ] **Step 2: Run `cargo test -p aether-core` and confirm they fail to compile (`Watch` not found)**

- [ ] **Step 3: Add `Watch`, `Watch::chat`, `toggle_watch` and `Settings.watchlist` (doc: "Watch-list items, in the order added."), defaulting to empty**

- [ ] **Step 4: Run `cargo test -p aether-core`; expect all pass**

- [ ] **Step 5: Commit after user approval: `feat(core): watch-list setting`**

---

### Task 2: Watch state and lifecycle in the app

**Files:**
- Create: `crates/app/src/app/watch/mod.rs`
- Modify: `crates/app/src/app/mod.rs` (`mod watch;`; fields `pub(crate) watch_open: bool`, `pub(crate) watch_peek: Option<Watch>` on `ChatApp`, initialised in `crates/app/src/app/new.rs`)
- Modify: `crates/app/src/app/chat/manage.rs:151` (`archive_chat`) and `:95` (`delete_chat`)
- Modify: `crates/app/src/terminal.rs:525` (`close_terminal`)
- Modify: `crates/app/src/app/chat/send.rs:417` (`session_is_watched`)
- Modify: `crates/app/src/app/new.rs:572` (after `app.restore_chats()`)
- Modify: `crates/app/src/settings.rs` (`save` returns early under `cfg(test)`)
- Test: `crates/app/tests/unit/app/watch.rs` (wired with `#[cfg(test)] #[path = "../../../tests/unit/app/watch.rs"] mod tests;`)

**Interfaces:**
- Consumes: `Watch`, `toggle_watch`, `Settings.watchlist` (Task 1).
- Produces, in `app::watch`:
  - `pub(crate) fn watchable(item: Watch, chat: Option<&Chat>) -> bool`: the chat exists and isn't archived; `Watch::Terminal` also needs `chat.is_agent()`.
  - `ChatApp::is_watched(&self, item: Watch) -> bool`
  - `ChatApp::toggle_watch(&mut self, item: Watch, cx: &mut Context<Self>)`: no-op unless `watchable`; clears `watch_peek` when it removes the peeked item.
  - `ChatApp::unwatch_chat(&mut self, id: i64, cx: &mut Context<Self>)`: drops both kinds for `id`, clears the peek if it's on that chat; saves only when something changed.
  - `ChatApp::unwatch_terminal(&mut self, id: i64, cx: &mut Context<Self>)`: same, `Watch::Terminal(id)` only.
  - `ChatApp::prune_watchlist(&mut self, cx: &mut Context<Self>)`: keeps only `watchable` items; saves only when something changed.
  - `ChatApp::sync_terminal_watch(&self, cx: &mut Context<Self>)`: no-op for now; Task 3 fills it in. Called after every list change above.
  - `ChatApp::session_visible(&self, id: i64) -> bool`: extracted from `session_is_watched` lines 418-426 (has a pane, host 0 needs `Page::Chat`, not hidden behind another maximized pane). `session_is_watched` calls it.
- `session_is_watched` also returns true when `self.watch_peek == Some(Watch::Chat(id))` and the main window (`self.workspace.windows.get(&0)`) is `cx.active_window()`.
- Hooks: `archive_chat` and `delete_chat` call `unwatch_chat`; `close_terminal` calls `unwatch_terminal`; `new.rs` calls `prune_watchlist` after `restore_chats`. `shutdown_terminals` (quit) must not prune.

- [ ] **Step 1: Write the failing tests** (`chat_with` from `crates/app/src/app/mod.rs:689` builds a `Chat`)

```rust
#[test]
fn only_live_chats_are_watchable() {
    let mut chat = chat_with("a", &[]);
    assert!(watchable(Watch::Chat(chat.id), Some(&chat)));
    assert!(!watchable(Watch::Chat(2), None));
    chat.archived = true;
    assert!(!watchable(Watch::Chat(chat.id), Some(&chat)));
}

#[test]
fn only_agent_chats_have_a_watchable_terminal() {
    let mut chat = chat_with("a", &[]);
    chat.provider = Provider::OpenRouter;
    assert!(!watchable(Watch::Terminal(chat.id), Some(&chat)));
    chat.provider = Provider::Claude;
    assert!(watchable(Watch::Terminal(chat.id), Some(&chat)));
}
```

Plus a `#[gpui_kit::test] archiving_a_watched_chat_drops_it_and_closes_its_peek`, using the `ChatApp::with_state` harness from `crates/app/tests/unit/session/view.rs:4-30`: set `settings.watchlist = vec![Watch::Chat(id), Watch::Terminal(id)]` and `watch_peek = Some(Watch::Chat(id))` for a second, non-active chat; call `archive_chat(ix, window, cx)`; assert `settings.watchlist.is_empty()` and `watch_peek == None`.

- [ ] **Step 2: Run `cargo test -p aether --bin aether watch -- --skip hackernews::client`; expect compile failure**
- [ ] **Step 3: Add the `cfg(test)` early return to `settings::save`, then implement the interfaces and hooks above**
- [ ] **Step 4: Run the full app suite; expect all pass**
- [ ] **Step 5: Commit after user approval: `feat(watch): watch-list state and pruning`**

---

### Task 3: Watch / Unwatch in menus and the eye toggle

**Files:**
- Modify: `crates/ui/src/chat/sidebar/mod.rs` (`ChatSidebar` field + builder; `ChatItem` fields; `chat` at :671 and `chat_in_section` at :845 fill them)
- Modify: `crates/ui/src/chat/sidebar/menu.rs:122` (`chat_menu`)
- Modify: `crates/app/src/app/sidebar.rs:638` (`sidebar_chrome`)
- Modify: `crates/app/src/session_workspace/pane.rs:217` (`session_menu`)
- Modify: `crates/app/src/side_panel.rs:366-377` (Agent tab) and `:405-441` (`side_tab_bar` actions)
- Modify: `crates/app/src/session_workspace/render.rs:277-312` (monocle tabs)
- Modify: `crates/app/src/app/composer/header.rs` (header tools, beside `background_toggle`)
- Modify: `crates/terminal/src/workspace/mod.rs:51` (field), `crates/terminal/src/workspace/header.rs:68-121` (group tab menu), `crates/terminal/src/view/mod.rs:47` (`TerminalEvent`)
- Modify: `crates/app/src/terminal.rs:194` (subscription) and `app/watch/mod.rs` (`sync_terminal_watch`, `watch_toggle`)

**Interfaces:**
- Consumes: `toggle_watch`, `is_watched`, `sync_terminal_watch` (Task 2).
- Produces:
  - `ChatSidebar::watching(self, watched: Vec<i64>, on_watch: impl Fn(i64, &mut Window, &mut App) + 'static) -> Self`. `ChatItem` gains `watched: bool` and `on_watch: Option<Rc<dyn Fn(i64, &mut Window, &mut App)>>`. `chat_menu` adds "Watch" (`IconName::Eye`) or "Unwatch" (`IconName::EyeOff`) after Pin, only when `on_watch` is set and `!chat.archived`.
  - `sidebar_chrome` calls `.watching(..)` with the `Watch::Chat` ids, so every sidebar mode and Direct Chats get it; the Archive view's rows are archived, so they don't.
  - `session_menu`, the side-panel Agent tab and the monocle tabs get the same item ("Watch session" / "Unwatch session" in `session_menu`, matching "Pin session"), toggling `Watch::Chat(chat id)`.
  - `ChatApp::watch_toggle(&self, cx: &mut Context<Self>) -> AnyElement`: `icon_chip("watch-toggle", watched, cx)` with `IconName::Eye`, selected when the active chat is watched, tooltip "Watch" / "Unwatch". Goes in the header tools and the `side_tab_bar` actions, just before `background_toggle`.
  - `TerminalWorkspace.pub watched: Option<bool>`: `None` (the global terminal) hides the item. `TerminalEvent::ToggleWatch`. The group tab menu adds "Watch" / "Unwatch" before the separator when `Some`. The app subscription maps it to `toggle_watch(Watch::Terminal(id), cx)`.
  - `sync_terminal_watch` sets `watched = Some(self.is_watched(Watch::Terminal(id)))` on every drawer in `self.terminals`; `drawer_for` also sets it when it creates a drawer.

- [ ] **Step 1: Implement the interfaces above**
- [ ] **Step 2: Run all three test suites; expect all pass (the existing `crates/ui/tests/unit/chat/sidebar` tests cover the builders' other fields)**
- [ ] **Step 3: Commit after user approval: `feat(watch): watch from menus, tabs and header`**

---

### Task 4: FAB, list and web-view handling

**Files:**
- Create: `crates/app/src/app/watch/view.rs`
- Modify: `crates/app/src/app/render.rs:305-308` (append `.children(self.render_watch(window, cx))` after `notes_search`)
- Modify: `crates/app/src/browser.rs` (`covered` field, `set_covered`, `native_visible`, bottom strip in `render`)
- Test: `crates/app/tests/unit/app/watch.rs`

**Interfaces:**
- Consumes: Task 2 state and `toggle_watch`.
- Produces:
  - `ChatApp::render_watch(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement>`: the FAB (absolute, bottom-right, above the bottom status bar when it's shown, never over the composer's send button; `deferred`/`occlude` so it sits above content), badge = `settings.watchlist.len()` (hidden at 0), and the list when `watch_open`. The peek slot stays empty until Task 5.
  - Rows, in list order: kind icon (`aether_ui::provider_icon(chat.provider)` for agent chats, a direct-chat mark for `!is_agent()`, `Lucide::SquareTerminal` or similar `>_` icon for terminals), title, status line, coloured dot, ✕ (`toggle_watch`). Chat status: `Chat::status(now)` and `ChatStatus::text(now)`; dot colour from `is_waiting` / `is_busy` / Done / Failed. Terminal status: `terminal_job(id)` or "Idle". Empty list: "Right-click a chat or terminal tab and choose Watch."
  - Clicking the FAB toggles `watch_open`; if a peek is open, it closes the peek instead. Clicking a row calls `open_watch_item` (Task 5); until then it sets `watch_peek`.
  - `Browser::set_covered(&mut self, covered: bool, cx: &mut Context<Self>)`: no-op when unchanged. `native_visible` also needs `!self.covered`. The web-view content area leaves a bottom strip the FAB's height plus its margin. `render_watch` calls `set_covered(self.watch_open || self.watch_peek.is_some())` on `self.browser` and `self.hn_browser`.

- [ ] **Step 1: Write the failing test `web_views_uncover_when_the_watch_overlay_closes`** (`with_state` harness): set `watch_open = true`, render once, assert `!browser.native_visible(cx)` given `shown`; set `watch_open = false`, render, assert `browser.read(cx).covered == false`.
- [ ] **Step 2: Run it; expect failure**
- [ ] **Step 3: Implement the interfaces above**
- [ ] **Step 4: Run all suites; expect all pass**
- [ ] **Step 5: Commit after user approval: `feat(watch): global FAB and watch-list`**

---

### Task 5: Peek and Open ↗

**Files:**
- Modify: `crates/app/src/app/watch/view.rs` and `watch/mod.rs`
- Modify: `crates/app/src/session_workspace/pane.rs:29-108` (extract the transcript + composer body)
- Modify: `crates/app/src/terminal.rs:400-500` (`terminal_drawer`, `render_terminal_page`)
- Modify: `crates/app/src/side_panel.rs:470-480` (Terminal tab)
- Test: `crates/app/tests/unit/app/watch.rs`

**Interfaces:**
- Consumes: Tasks 2 and 4.
- Produces:
  - `ChatApp::session_body(&mut self, scroller: ElementId, window: &mut Window, cx: &mut Context<Self>) -> AnyElement`: lines 29-108 of `render_session_pane` (transcript `ChatScroller`, selection bar, composer, or `render_start` for an empty chat), with the scroller id passed in. `render_session_pane` uses it with `("session-messages", pane as usize)`.
  - `ChatApp::open_watch_item(&mut self, item: Watch, window: &mut Window, cx: &mut Context<Self>)`: closes the list. For a chat already on screen (`session_visible`), focus its pane and composer instead of peeking. Otherwise set `watch_peek`; for a terminal, get the drawer via `drawer_for(ix, ..)` and make sure its shell is running the way `show_terminal` (`terminal.rs:251`) does, without opening the drawer under the chat.
  - `ChatApp::jump_to_watched(&mut self, item: Watch, window: &mut Window, cx: &mut Context<Self>)`: closes the peek, switches to `Page::Chat`, `select(ix, ..)`; for a terminal also `open_terminal` and focuses it.
  - `ChatApp::close_peek(&mut self, cx: &mut Context<Self>)`.
  - `ChatApp::settle_peek(&mut self)`: closes a chat peek whose chat is `session_visible`, or whose chat is gone. `render_watch` calls it first.
  - `ChatApp::peeked_terminal(&self) -> Option<i64>`.
  - Chat peek body: `ensure_session_ui(id, window, cx)`, `acknowledge_session(id, cx)` when the main window is active, then `in_session(id, cx, |app, cx| app.session_body(("watch-peek", 0).into(), window, cx))`. The card root sets `GENERATING_CONTEXT` (`crates/app/src/actions.rs:296`) as its key context while the peeked chat is busy, and handles `StopGenerating` by stopping the peeked chat without propagating. It also handles `BackToChat` (Esc bound in the root `"Aether"` context) as a no-op, so Esc in the peek never changes the page underneath.
  - Terminal peek body: `dark(view.cached(StyleRefinement::default().size_full()), cx)`, with `set_shown(true)`.
  - Exemptions: `terminal_drawer` and `render_terminal_page` skip the peeked id in their `set_shown(false)` loops; when the active chat's terminal is peeked, `terminal_drawer` returns `None`, and the side panel's Terminal tab draws nothing.
  - Card header: kind icon, title, Open ↗ (`jump_to_watched`), ✕ (`close_peek`). No Esc binding, no click-outside dismissal.

- [ ] **Step 1: Write the failing tests** (`with_state` harness, two chats, the second non-active):
  - `peek_gives_way_when_its_chat_is_on_screen`: `watch_peek = Some(Watch::Chat(second))`; `select(second_ix, ..)`; `settle_peek()`; assert `watch_peek == None`.
  - `stop_in_peek_targets_the_peeked_chat`: mark both chats busy, peek the second, press Esc with focus in the peek's composer, assert only the second chat stopped and `page` didn't change.
  - `peeked_terminal_is_skipped_by_the_drawer`: with a drawer for the active chat and `watch_peek = Some(Watch::Terminal(active_id))`, `terminal_drawer(..)` returns `None` and the drawer's `shown` isn't forced to false.
- [ ] **Step 2: Run them; expect failure**
- [ ] **Step 3: Implement the interfaces above**
- [ ] **Step 4: Run all suites; expect all pass**
- [ ] **Step 5: Commit after user approval: `feat(watch): interactive peek with open`**
- [ ] **Step 6: Hand the GUI checks from the spec's Verification section to the user**

---

## Additions (spec section "Additions", approved 2026-10-29)

### Task 6: Aether icon on the FAB and a corner setting

**Files:**
- Modify: `crates/core/src/settings.rs` (+ re-export in `crates/core/src/lib.rs`), test in `crates/core/tests/unit/settings.rs`
- Modify: `crates/app/src/app/watch/view.rs` (FAB icon, corner-aware placement of FAB, list and peek)
- Modify: `crates/app/src/settings_view.rs` (Appearance section control) and `crates/app/src/settings_search.rs` (search entry)
- Test: `crates/app/tests/unit/app/watch.rs`

**Interfaces:**
- Produces: `pub enum WatchCorner { BottomRight (default), BottomLeft, TopRight, TopLeft }` with `ALL` and `label()` ("Bottom right", …), like `TileLayout`; `Settings.watch_corner: WatchCorner`.
- The FAB draws the Aether icon from `crates/app/assets/app-icons/aether.svg` (or a PNG size of it) at the FAB size, rounded like the app tile; badge unchanged.
- The FAB, list and peek anchor to `settings.watch_corner`. The lift-above-composer rule only applies to bottom corners; top corners sit below the window's top chrome (title/tab bar) so they never cover tabs.
- Settings: a "Watch-list button" row in Settings → General → Appearance with a four-way choice, saved via `update_settings`; searchable as "watch-list button position".

- [ ] Write failing tests: core `watch_corner_defaults_to_bottom_right_and_round_trips`; app test that each corner places the FAB on that side (assert on a pure helper, e.g. `fab_edges(corner) -> (left: bool, top: bool)`).
- [ ] Implement; run all suites.

### Task 7: The peek is a draggable, resizable floating window

**Files:**
- Modify: `crates/core/src/settings.rs` (persisted bounds), `crates/app/src/app/watch/view.rs`, `crates/app/src/app/watch/mod.rs`
- Test: `crates/app/tests/unit/app/watch.rs`, `crates/core/tests/unit/settings.rs`

**Interfaces:**
- Produces: `Settings.watch_peek_bounds: Option<[f32; 4]>` (x, y, width, height in window px; `None` = default spot beside the FAB at the current default size).
- `pub(crate) fn clamp_peek(bounds: [f32; 4], viewport: (f32, f32), min: (f32, f32)) -> [f32; 4]`: keeps the card fully inside the window and at least `min` size (pick ~320×240).
- Drag by the header and resize from all four edges and corners. Reuse the floating session pane's drag/resize approach (`session_workspace` floats, `FloatingPane`, pane.rs drag handling) rather than inventing a new one. Save bounds to settings on drag/resize end only.
- Peek content (chat body, terminal) fills the card; no layout change to the page underneath.

- [ ] Write failing tests: `clamp_peek_keeps_the_card_inside_and_above_min_size` (oversized, off-screen and tiny inputs); core round-trip for `watch_peek_bounds`.
- [ ] Implement; run all suites.

### Task 8: Pop out to a desktop window

**Files:**
- Modify: `crates/app/src/app/watch/mod.rs`, `crates/app/src/app/watch/view.rs`, possibly `crates/app/src/session_workspace/windows.rs`
- Test: `crates/app/tests/unit/app/watch.rs`

**Interfaces:**
- Produces: `ChatApp::pop_out_watched(&mut self, item: Watch, window: &mut Window, cx: &mut Context<Self>)`: closes the peek; for `Watch::Chat(id)` makes sure the chat has a pane (open one the way the workspace normally opens a chat) and calls `detach_session(pane, window, cx)`; for `Watch::Terminal(id)` does the same for the chat and then shows that chat's terminal tab in the new window. A chat already in a detached window just gets that window activated.
- A pop-out button (icon, tooltip "Open in new window") in the peek header beside Open ↗.

- [ ] Write a failing GPUI test: popping out a watched chat with no pane closes the peek and leaves the chat assigned to a non-main window in `workspace.model` (or, if opening an OS window is impossible in the test harness, asserts the transfer was prepared for a host ≠ 0; say which in the report).
- [ ] Implement; run all suites.

### Task 9: Per-window side tabs; terminal pop-out opens on Terminal

**Files:** `crates/app/src/side_panel.rs`, `crates/app/src/session_workspace/{pane.rs,windows.rs,model.rs}`, `crates/app/src/terminal.rs`, `crates/app/src/app/watch/mod.rs`, and any other `side_tab` reader (25 uses across `side_panel.rs`, `terminal.rs`, `browser.rs`, `app/chat/{mod,create}.rs`, `app/render.rs`, `app/new.rs`, `app/draft_editor.rs`, `explorer/mod.rs`, `session_workspace/pane.rs`, `watch/view.rs`).
**Test:** `crates/app/tests/unit/app/watch.rs` (or a session-workspace test file if that fits better).

**Interfaces:**
- The side tab becomes per host window: main (host 0) keeps `ChatApp.side_tab`; each detached host stores its own (e.g. a `HashMap<WindowId, SideTab>` on `ChatApp`, or a field on the window layout), defaulting to Agent. Pick the smaller change; one accessor pair like `side_tab_for(host) -> SideTab` / `set_side_tab(host, tab)` is how callers reach it.
- Tab clicks, tab cycling (`PreviousSideTab`/`NextSideTab`), `show_terminal`, `select_tab`, `focus_composer`'s Agent switch and the terminal drawer exemptions all act on the window they run in, never another one.
- `pop_out_watched(Watch::Terminal(id))` sets the new window's tab to Terminal once the detach commits, so the popped-out chat window opens on its terminal.

- [ ] Failing tests: (1) switching the side tab in a detached window leaves the main window's `side_tab` unchanged and vice versa; (2) popping out a watched terminal leaves the new host on `SideTab::Terminal` and main's tab unchanged.
- [ ] Implement; run all suites (`CARGO_INCREMENTAL=0`).
