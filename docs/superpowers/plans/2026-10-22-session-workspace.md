# Session workspace implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Let independent agent sessions run in split panes, floating panes, and native desktop windows while preserving their input and live runs.

**Architecture:** One application-owned session runtime owns conversations and streams. Per-session views own window-bound widgets and transcript presentation. A workspace coordinator maintains one visible placement per chat and transfers view snapshots between windows transactionally.

**Tech stack:** Rust, existing GPUI Kit, serde/serde_json, existing agent/store APIs. Add `serde.workspace = true` to the app crate for layout DTO derives. Retain the pinned toolchain and locked dependencies.

**Spec:** `docs/superpowers/specs/2026-10-22-session-workspace-design.md`, approved 2026-10-22.

## Execution record

Native implementation uses the existing `ChatApp` as the single runtime owner,
with ID/epoch-bound handles and scoped per-session presentation state. The
implementation notes in the spec record this adjustment and the Linux window
retention behavior. Widget state lives in `app/session_ui/`; layout, pane
rendering, commands, windows, and persistence live in `session_workspace/`.

The code for Tasks 1 through 8 is present. Whole-branch review and its final
validation are recorded with the feature commits. Physical Linux and Touch Bar
checks are unavailable in this environment; headless native-window tests run
with the pinned GPUI test scheduler.

## Global constraints

- Each chat has at most one visible placement across the application.
- Commands accept a stable chat ID.
- Ratios default to 0.5 and clamp to 0.15 through 0.85, with minimum child sizes taking precedence.
- Minimum pane size is 320 by 240 logical pixels.
- Closing a pane hides its session; Stop remains an explicit session action.
- Moving a view between native windows recreates window-bound widgets and subscriptions from saved view state.
- Hidden secondary windows are retained and reused until application exit.
- Closing the main window retains the existing application-quit behavior.
- Layout restoration does not itself start an agent turn; existing session resume rules apply.
- Native windows contain session workspaces; project tools retain their existing main-shell hosting.

## Review focus

- A queued approval callback outlives deletion or rewind: reject its old chat/run identity, never answer another session. Task 1.
- An attachment copy completes during detach: record it exactly once on its chat, not on whichever view is now focused. Tasks 2 and 6.
- A composer is composing IME text when moved: finish or cancel composition through the input widget, snapshot committed text, and preserve the resulting draft. Tasks 3 and 6.
- Closing a detached window races with a pending transfer or chat deletion: cancel stale reservations and leave exactly one valid placement for every surviving session. Task 6.
- A display disappears and restored bounds contain non-finite or huge numbers: sanitize geometry before opening any window and preserve reachable title bars. Tasks 4 and 7.

## File structure and migration rules

Create these modules under `crates/app/src/`:

- `session/mod.rs`: shared identities, runtime/view exports, runtime events.
- `session/runtime.rs`: chat ownership, stable-ID command dispatch, shared settings snapshot and persistence ownership.
- `session/commands.rs`: send, stop, retry, edit, queue, steer, approval and delete operations.
- `session/stream.rs`: runtime-owned subscriptions, run/epoch validation and update fanout.
- `session/view.rs`: one session's composer and transcript widgets and subscriptions.
- `session/snapshot.rs`: portable draft/edit/approval/scroll presentation state.
- `session_workspace/mod.rs`: GPUI workspace controller and view registry.
- `session_workspace/model.rs`: pure placement, dock tree, focus and ID allocation.
- `session_workspace/geometry.rs`: split constraints, floating bounds and directional focus.
- `session_workspace/render.rs`: split tree, headers, overlay stack, drag handlers.
- `session_workspace/windows.rs`: secondary window registry and transfer transactions.
- `session_workspace/persistence.rs`: versioned layout DTOs, repair and atomic storage.

Modify `main.rs`, `app/{mod,new,render,sidebar,start,providers,draft_editor}.rs`,
`app/chat/{mod,create,send,edit,manage,live,turn}.rs`, `app/composer/{mod,attach,chips,header,message,usage}.rs`,
`app/worktree/{mod,setup,send,remove}.rs`, `hitl.rs`, `suggest/mod.rs`,
`side_panel.rs`, `terminal.rs`, `agents_view.rs`, `palette.rs`, `actions.rs`,
`notify.rs`, and `Cargo.toml` only where their chat ownership or routing changes.
Audit other readers of `ChatApp.chats` with `rg` before removing that field.
Keep existing unit tests at their paths and update imports as ownership moves.
New tests live under `crates/app/tests/unit/session/` and
`crates/app/tests/unit/session_workspace/`, included with the repository's
`#[cfg(test)] #[path = ...] mod tests;` convention.

Use `ChatId(pub i64)` for the persisted ID, and `PaneId(pub u64)`,
`SplitId(pub u64)`, `WorkspaceWindowId(pub u64)` for layout identities.
All are Copy/Eq/Hash. `WorkspaceWindowId(0)` is the main window.
Avoid introducing a second driver runtime or a second chat/settings store.

## Task 1: Route session operations by stable identity

**Files:** Create `session/{mod,runtime,commands,stream}.rs`; modify `main.rs`, `app/{mod,new}.rs`, `app/chat/{mod,create,send,edit,manage,live}.rs`, `hitl.rs`, and affected shared-state callers from the file map. Modify `crates/agent/{Cargo.toml,src/agent.rs,src/lib.rs}` and app `Cargo.toml` for fake-driver test access. Create `tests/unit/session/runtime.rs` and `tests/unit/session/support.rs`.

**Interfaces:** `SessionRuntime` owns `Vec<Chat>`, `Agent`, `Option<Store>`, the current `Settings` snapshot, and shared lifecycle tasks. `CommandTarget { chat: ChatId, epoch: u64 }` identifies a mutation; stream updates also carry the existing run number. Produce `SessionRuntime::chat(id: ChatId) -> Option<&Chat>`, `dispatch(target: CommandTarget, command: SessionCommand, cx: &mut Context<Self>) -> Result<(), SessionError>`, and `SessionEvent::{Changed(ChatId), Deleted(ChatId)}`. `SessionCommand` carries Send text/attachments, Stop, Retry, Steer/RemoveQueued index, Answer request ID/Answer, Edit message index/text, and Delete. `SessionError` distinguishes missing chat, stale target, invalid request, and blocked operation.

- [ ] Run `cargo test --locked -p aether` and `cargo check --locked --workspace` before edits; record pre-existing failures. Audit all direct `self.active` mutations and all chat-index captures in delayed callbacks.
- [ ] Add a `test-support` feature to `aether-agent`. Under that feature export the existing factory alias as `DriverFactory` and add `Agent::with_driver_factory(factory: DriverFactory, idle: Duration) -> std::io::Result<Self>` forwarding to `with_factory`. Enable it through the app's dev-dependency on `aether-agent`; keep normal production construction unchanged. In `session/support.rs`, adapt the existing fake driver from `crates/agent/tests/unit/agent.rs` to expose a shared command log and a channel for manually emitted updates. Use this same support in Tasks 3 and 8.
- [ ] Add `runtime::commands_keep_their_chat_after_focus_and_delete` using two chats and recorded fake driver commands. Submit to B, remove A, then Stop B and answer B's request. Assert every recorded command targets B; submit through the deleted A target and assert `MissingChat` with no driver call. Add `runtime::stale_approval_after_rewind_is_rejected`, asserting no answer is forwarded after epoch changes.
- [ ] Run `cargo test --locked -p aether session::runtime`; expect failure for absent identity-based dispatch before implementing it.
- [ ] Move stream subscriptions and mutation logic to `SessionRuntime`. Separate UI preparation of send/edit payloads from runtime dispatch; worktree setup and queue delivery execute on the runtime by chat ID. Keep existing run/epoch validation and provider locking. Use one runtime created and retained by the application bootstrap; `ChatApp` holds its entity. Retain a single quit cleanup subscription.
- [ ] Keep the single-pane UI working through temporary ID-based shell adapters. Runtime events drive its existing row refresh. Replace index-bound request callbacks, attachment completion, worktree completion, and notification actions with ID resolution at invocation. Settings changes update the runtime snapshot through the one existing settings writer.
- [ ] Run the new runtime tests and existing `app::chat`, `hitl`, and `app::worktree` tests; expect PASS. Run `cargo check --locked --workspace`; expect success. Commit as `refactor: isolate session runtime by chat id`.

## Task 2: Preserve portable session drafts and view snapshots

**Files:** Create `session/snapshot.rs`, `tests/unit/session/snapshot.rs`; modify `session/runtime.rs`, `app/draft_editor.rs`, `app/composer/attach.rs`, and `hitl.rs`.

**Interfaces:** `SessionSnapshot` contains composer text/selection, pending attachment references, editing message ID/index plus stashed draft, composer history cursor/stash, open tool keys, transcript anchor/offset, follow-tail flag, approval form text/picks, and draft-editor text/selection. Produce `SessionRuntime::snapshot(id: ChatId) -> Option<&SessionSnapshot>` and `save_snapshot(id: ChatId, snapshot: SessionSnapshot) -> Result<(), SessionError>`. Widget focus handles and GPUI subscriptions never enter snapshots.

- [ ] Add `snapshot::round_trip_keeps_both_drafts_and_pending_forms`, creating distinct A/B snapshots with attachments, edit stashes, scroll anchors and form answers. Assert equality after serialization and reload. Add `snapshot::late_attachment_updates_its_chat_once`, recording a completion after A's presentation is released and B is focused; assert only A's pending list gains that attachment, once.
- [ ] Run `cargo test --locked -p aether session::snapshot`; expect FAIL before implementation.
- [ ] Implement snapshots and runtime ownership. Treat pending request answers as presentation data; preserve runtime request IDs and validate them on submission. Mark snapshot changes for the persistence writer from Task 7. Deduplicate attachment completions with their existing operation identity, assigning a per-copy identity if none exists.
- [ ] Run snapshot tests and existing composer attachment/draft-editor tests; expect PASS. Commit as `refactor: retain portable session view state`.

## Task 3: Render independent session views

**Files:** Create `session/view.rs`, `tests/unit/session/view.rs`; modify `app/{mod,new,render,start,providers,draft_editor}.rs`, `app/composer/{mod,chips,header,message,usage,attach}.rs`, `app/chat/{turn,edit,live}.rs`, `hitl.rs`, and `suggest/mod.rs`.

**Interfaces:** Produce `SessionView::new(chat: ChatId, runtime: Entity<SessionRuntime>, window: &mut Window, cx: &mut Context<Self>) -> Self`, `snapshot(&self, cx: &App) -> SessionSnapshot`, and `prepare_transfer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> SessionSnapshot`. The last method settles IME composition and dismisses transient menus before capturing state. The view implements Render and Focusable and holds one chat ID for its lifetime.

- [ ] Add `view::two_views_keep_independent_composers_scroll_and_requests` with two test session entities: type distinct drafts, scroll A upward, update B's reply, and answer A's form. Assert B's composer/scroll is unchanged and the runtime receives A's answer. Use the GPUI test context already exposed by the dependency if available; retain pure snapshot/dispatch tests and record a native smoke procedure if window creation is unavailable headlessly.
- [ ] Run `cargo test --locked -p aether session::view`; expect FAIL before implementation.
- [ ] Move composer/selectors, rows/scroller, request input widgets, suggestion state, edit state, and their subscriptions from the shell into `SessionView`. Runtime changes refresh only the corresponding view. Replace transcript reflow callbacks that retain row indices with chat/message/block identity and resolve the current row on invocation. Keep offscreen sessions receiving runtime updates.
- [ ] Have the main shell render one `SessionView` initially. Remove temporary active-chat dispatch adapters when no callers remain. Retain main-shell project tools and sidebar selection through explicit session-focus events. Restore snapshot state into freshly created widgets; missing scroll anchors fall back to the nearest surviving message, then tail.
- [ ] Run session tests plus existing composer, transcript, approval, edit and suggestion tests; expect PASS. Smoke type with an IME, switch sessions, and reopen each draft. Commit as `refactor: render sessions with independent view state`.

## Task 4: Model placements, recursive splits and focus

**Files:** Create `session_workspace/{mod,model,geometry}.rs`, `tests/unit/session_workspace/{model,geometry}.rs`; register the module in `main.rs`.

**Interfaces:** Produce `WorkspaceModel` with pane/chat mappings, per-window optional dock root, floating list, focused/maximized pane IDs, and monotonic layout IDs. Define `SplitAxis::{Horizontal, Vertical}`, `DockSide::{Left, Right, Top, Bottom}`, and `LayoutError`. Methods: `open(chat: ChatId, window: WorkspaceWindowId) -> PaneId`, `split(target: PaneId, chat: ChatId, axis: SplitAxis, available: Size<f32>) -> Result<PaneId, LayoutError>`, `close(pane: PaneId) -> Result<(), LayoutError>`, `float(pane: PaneId, bounds: Bounds<f32>) -> Result<(), LayoutError>`, `dock(pane: PaneId, target: Option<PaneId>, side: DockSide) -> Result<(), LayoutError>`, `focus(pane: PaneId) -> Result<(), LayoutError>`, `cycle_focus(window: WorkspaceWindowId, backwards: bool) -> Option<PaneId>`, and `forget_chat(chat: ChatId)`. Produce geometry helpers `split_sizes(total: f32, first_min: f32, second_min: f32, ratio: f32) -> Option<(f32, f32)>` and `sanitize_bounds(bounds: Bounds<f32>, available: Bounds<f32>, minimum: Size<f32>) -> Bounds<f32>`.

- [ ] Add model tests for nested splits, sibling promotion, first-leaf fallback, closing the last leaf, unique visible placement, and opening an existing chat without creating a second pane. Assert focus cycles dock leaves then floats in saved order. Assert forgetting a deleted chat removes its pane and stale focus/maximize references.
- [ ] Add geometry tests: 0.5 default, ratios clamped to 0.15/0.85, nested minimum sizes, refusal below two 320 by 240 panes plus divider, and NaN/infinite/huge restored coordinates producing finite reachable bounds. Use a 4 logical pixel divider, consistent with terminal conventions.

  Pin the split helper's divider accounting with these assertions. `total` includes the divider:

  ```rust
  assert_eq!(split_sizes(644.0, 320.0, 320.0, 0.5), Some((320.0, 320.0)));
  assert_eq!(split_sizes(643.0, 320.0, 320.0, 0.5), None);
  assert_eq!(split_sizes(484.0, 240.0, 240.0, 0.5), Some((240.0, 240.0)));
  assert_eq!(split_sizes(1004.0, 100.0, 100.0, -1.0), Some((150.0, 850.0)));
  ```

- [ ] Run `cargo test --locked -p aether session_workspace`; expect FAIL before implementation.
- [ ] Implement pure model operations with validation before mutation. Provide an admission check before new-chat creation so refused splits leave no orphan chat. Preserve the empty dock tree when the final docked pane floats. Keep stable floating order while storing a separate stacking order for click-to-raise behavior.
- [ ] Run model/geometry tests; expect PASS. Commit as `feat: model agent session pane layouts`.

## Task 5: Add split and floating pane controls

**Files:** Create `session_workspace/render.rs`, `tests/unit/session_workspace/render.rs`; modify `session_workspace/mod.rs`, `app/{render,sidebar}.rs`, `actions.rs`, `palette.rs`, `side_panel.rs`, and `terminal.rs`.

**Interfaces:** `SessionWorkspace` holds runtime/coordinator references and its `WorkspaceWindowId`, renders placements from Task 4, and resolves views by `PaneId`. Its event `SessionFocusChanged(ChatId)` updates main-shell tool context only for the main workspace. Produce actions `SplitSessionRight`, `SplitSessionDown`, `FloatSession`, `DockSession`, `DetachSession`, `ReturnSessionToMain`, `CloseSessionPane`, `MaximizeSessionPane`, `NextSessionPane`, `PreviousSessionPane`, and directional pane-focus actions.

- [ ] Add controller tests asserting insufficient room disables splitting before new-chat creation, close leaves the runtime session busy, maximize/restore preserves tree and float geometry, and sidebar open focuses an existing placement. Add a two-pane render/input smoke scenario with independently streaming fake replies.
- [ ] Run `cargo test --locked -p aether session_workspace::render`; expect FAIL before implementation.
- [ ] Render dock trees with 4-pixel draggable dividers and double-click ratio reset. Render floats in a clipped overlay with move/resize handles, title bars, and click-to-raise stacking. Use 320 by 240 minimum bounds and a scrollable workspace when the native window is smaller than the layout minimum.
- [ ] Add compact headers with session title, provider, status, focus border, close control and overflow menu. Implement docking as a menu choosing target and side; drag title bars moves floats within a window. Splits create chats using existing project/provider defaults. Focus movement changes neither drafts nor runtime ownership. Main shell tools track the focused main pane; detached panes leave that context intact.
- [ ] Bind `secondary-alt-]` / `secondary-alt-[` for next/previous pane and `secondary-alt-left`, `secondary-alt-right`, `secondary-alt-up`, `secondary-alt-down` for directional pane focus in `SessionWorkspace` context. Keep split/float/detach/maximize accessible through pane menus and palette. Audit app, terminal and editor bindings before adding keys; exclude Terminal, Editor, and modal input descendants from session navigation bindings. Add keymap tests that pin this isolation, especially macOS `cmd-alt-left/right` already used by terminals.
- [ ] Run new layout tests, existing action/palette/side-panel/terminal tests, and `cargo check --locked --workspace`; expect PASS. Smoke split, drag, dock, focus, resize, maximize and independent input. Commit as `feat: add split and floating agent panes`.

## Task 6: Transfer sessions into native windows safely

**Files:** Create `session_workspace/windows.rs`, `tests/unit/session_workspace/windows.rs`; modify `main.rs`, `session_workspace/{mod,model,render}.rs`, `session/{view,runtime}.rs`, and `menus.rs`.

**Interfaces:** Define `TransferId(pub u64)` and `TransferDestination::{NewWindow, MainWindow}`. Coordinator methods: `prepare_transfer(pane: PaneId, destination: TransferDestination) -> Result<TransferId, LayoutError>`, `commit_transfer(id: TransferId) -> Result<(), LayoutError>`, `abort_transfer(id: TransferId)`, and `return_window(window: WorkspaceWindowId) -> Result<(), LayoutError>`. Reservations record the source placement and chat generation. Commit validates source existence and generation again. Native window records have active/hidden/initializing state and retain their handles.

- [ ] Add tests for destination-creation failure leaving source intact, deletion during initialization rejecting commit, duplicate transfer requests rejecting the second reservation, and closing a window with pending transfers resolving each surviving chat to exactly one main placement. Assert no transfer command calls Cancel or closes a driver.
- [ ] Add snapshot transfer tests with committed IME text, pending attachment completion, draft edit stash and partially completed approval forms. Assert restored widget state matches the snapshot while runtime stream identity remains unchanged.
- [ ] Run `cargo test --locked -p aether session_workspace::windows`; expect FAIL before implementation.
- [ ] Inspect the pinned GPUI window APIs in the local Cargo source before adding platform code. Implement prepare/snapshot, destination view creation, placement commit, then old-view release. Queue runtime events during destination initialization and refresh from current runtime state on commit. Restore original presentation if preparation fails.
- [ ] Create native windows with the existing Aether identity/icon/theme and a session workspace root. Apply per-window typography and shared theme changes. Secondary close returns panes in dock traversal then floating saved order, splitting right of the main focused pane, hides the native window, and vetoes destruction. Reuse a hidden window before creating another. Keep main close as app quit and flush/cleanup exactly once across all windows.
- [ ] Test keyboard/input focus and menus in two windows, next/previous pane per window, repeated detach/close/reuse, and app quit with running sessions. Inspect whether the native API can hide one window without hiding the app; preserve the source placement and report a concrete blocker if it cannot. Never replace secondary close with app quit.
- [ ] Run session/window tests and `cargo check --locked --workspace`; expect PASS. Record native macOS and Linux smoke results separately. Commit as `feat: detach agent sessions into native windows`.

## Task 7: Save and restore layouts and drafts

**Files:** Create `session_workspace/persistence.rs`, `tests/unit/session_workspace/persistence.rs`; modify app `Cargo.toml`, `main.rs`, `session_workspace/{mod,windows}.rs`, and `session/runtime.rs`.

**Interfaces:** Define serde DTO `SavedWorkspace { version: u32, windows: Vec<SavedWindow>, panes: Vec<SavedPane> }` at version 1 and a separately versioned `SavedSessionDrafts` keyed by `ChatId`. `load_layout(path: &Path, known: &HashSet<ChatId>, displays: &[Bounds<f32>]) -> Result<WorkspaceModel, LayoutLoadError>` repairs known version 1 data. `save_layout(path: &Path, saved: &SavedWorkspace) -> io::Result<()>` performs temporary-file replacement. Draft writer uses the same atomic-write helper. Save files as `session-layout.json` and `session-drafts.json` under `aether_core::config_dir()`.

- [ ] Add tests for JSON round-trip of nested splits, floats, stacking order, focused/maximized IDs, multiple windows, and session snapshots. Assert missing/archived chats disappear, duplicate chat placement keeps the first in saved window/pane traversal order, empty branches collapse, and empty windows are omitted.
- [ ] Add tests for unsupported versions, truncated JSON, removed displays, non-finite/huge geometry, stale focus IDs, ID allocation after reload, and temp-write failure leaving the previous saved file intact. Assert malformed layout fallback does not discard a valid drafts file or modify chat records.
- [ ] Run `cargo test --locked -p aether session_workspace::persistence`; expect FAIL before implementation.
- [ ] Load chats first, then draft snapshots, then repaired layout. Use a main-pane fallback for invalid/missing layouts and the start view for a valid empty layout. Restore only nonempty native windows. Clamp window positions to connected displays and floats to their workspace. Reconstruct session views without dispatching Send; retain existing attach/resume policy.
- [ ] Debounce saves by 300 ms, serialize snapshots on the UI thread, write on a background task, and serialize writes so an older generation cannot overwrite a newer one. Track mutations during a write and flush latest snapshots/layout at quit before store cleanup. Surface one recoverable save error in the main shell, clearing it after a successful save. Never serialize credentials or live GPUI handles into these files.
- [ ] Run persistence tests plus existing settings/store tests; expect PASS. Smoke restart with unsent attachments, several pane types, and a removed display. Commit as `feat: restore session layouts and drafts`.

## Task 8: Prove the complete session lifecycle

**Files:** Create `tests/unit/session_workspace/lifecycle.rs`, register it from `session_workspace/mod.rs`; add verification notes to `docs/superpowers/specs/2026-10-22-session-workspace-design.md` and user-facing controls to `README.md`.

**Interfaces:** Consume Tasks 1 through 7. Add no new production abstractions unless a lifecycle failure requires one.

- [ ] Add a fake-agent lifecycle regression that creates A/B, starts both streams, queues on B, floats A, detaches B, answers A's approval, closes B's window, completes A, and sends the next queued B message. Assert command/session IDs throughout, one stream consumer per chat, one visible placement each, and no cancel caused by movement or closing.
- [ ] Add a delete/rewind regression while streams, snapshot saves and destination creation are pending. Assert stale updates/requests are ignored and surviving sessions retain drafts and placements. Run `cargo test --locked -p aether session_workspace::lifecycle`; expect PASS after correcting any demonstrated failures.
- [ ] Run `cargo fmt --all -- --check`, `cargo test --locked --workspace`, and `cargo build --locked`; require successful completion and record exact outcomes. Broaden testing only for a new failure or unresolved concern.
- [ ] Perform native smoke checks for macOS main/secondary close paths, retained secondary-window reuse, Linux input/focus/close, IME transfer, resizing below minimum, tool context, and layout restore on monitor changes. Report unavailable platforms or hardware as unverified, including the Touch Bar-specific behavior if that hardware is unavailable.
- [ ] Update README with pane menu actions, focus keys and close-versus-Stop behavior. Record validation results, commit as `test: verify session workspace lifecycle`, and obtain a whole-branch review using the selected execution workflow. Address actionable findings and rerun affected checks.

## Completion criteria

All eight tasks and their checks are complete, all three presentations are usable,
and spec acceptance conditions 1 through 7 have evidence. A plan, a model-only
implementation, or splits without native windows does not complete this feature.
The final report states shipped behavior, validation, and any platform checks
that remain unavailable.

### Final review corrections

The independent whole-branch review found session scoping defects in mouse
suggestions and retained stall/restart callbacks, widget rebinding on hidden
session reopen, a duplicate shell editor overlay, maximize/focus cycling, and
detached notification/unread targeting. The correction pass scopes callbacks
through stable session handles, rebinds existing UI when its native window
changes, leaves editor rendering to the pane, and uses current window placement
for attention state. Regressions reproduced suggestion misrouting, callback
index invalidation, hidden-session rebinding, maximized focus, and detached
attention targeting before passing with the fixes. The shell overlay removal
was checked directly against the pane-owned render path.

The runtime remains in ChatApp with per-session UI state. UI stream tests use
deterministic channels, with worker coverage in the agent suite. Retained
secondary windows hide on macOS and minimize on Linux. Physical Linux, macOS
IME transfer, and Touch Bar-specific checks remain unavailable.

Final verification: `cargo test --locked --workspace` passed 1,376 tests with
8 ignored and no failures. The app suite passed 370 tests. Formatting and
whitespace checks passed. No review findings are deferred.
`cargo build --locked` also passed after the review fixes.
