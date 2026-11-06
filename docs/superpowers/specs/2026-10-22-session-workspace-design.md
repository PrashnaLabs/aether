# Session workspace design

## Intent and approval status

Developers need to watch and interact with several agent sessions at once,
including across monitors. Agent sessions will support recursive splits,
movable floating panes inside Aether, and separate native desktop windows.
Each session keeps its own input, transcript, approvals, and running agent
when its presentation changes.

The user approved this scope and reviewed and approved this written spec on
2026-10-22. Implementation begins after implementation-plan review and selection
of the execution method.

## Existing constraints

- `crates/app/src/app/mod.rs` owns chats and agent access in `ChatApp`, but
  keeps one composer, transcript scroller, and row cache for the active chat.
- `crates/app/src/app/render.rs` renders that active chat. Many callbacks
  address `self.active`, so rendering the same app in several places would
  send some actions to the wrong session.
- `crates/terminal/src/workspace/` already implements terminal split trees,
  focus, headers, and divider dragging. Reuse interaction conventions while
  leaving its PTY ownership intact.
- `crates/app/src/main.rs` quits on main-window close to avoid the documented
  Touch Bar window-drop crash. A secondary window must not use that handler.
- Chat persistence, driver reuse, cancellation, and resume already exist.
  Layout changes must preserve their semantics.

## Approach

Introduce a session workspace that places session views independently of the
runtime owning their conversations. Keep one application-owned runtime and
store. A window or pane references a chat by its persisted ID.

Duplicating `ChatApp` for each pane would also duplicate settings, persistence,
and active-chat assumptions. Extending only the terminal workspace would leave
agent composers and approvals coupled to one active view. A dedicated session
workspace gives agent views their own layout while retaining the terminal
workspace as a component.

## Ownership and responsibilities

### Session runtime

Extract shared chat ownership and mutation operations into an application-owned
GPUI entity. It owns conversations, agent streams, queues, persistence, and
request state. Application lifetime holds a strong reference independently of
all session windows. Stream handling and background activity observe this owner.

Commands accept a stable chat ID. Commands for a deleted chat are rejected;
they never fall back to the focused chat. Async replies retain existing run
and epoch checks. Deleting a chat removes all its placements and releases its
view state through the coordinator before disposing of its runtime state.

### Session view

Create a session view for each open chat. It owns its composer, draft editing,
attachment presentation, input history, suggestions, transcript row cache,
scroll position, and focus handles. Model/access selectors and approval input
widgets belong to that session view. Draft content and attachments stay tied
to the chat when its view is hidden or recreated.

Runtime notifications update visible views of the affected chat. Transcript
virtualization remains per session. Moving a view between native windows
recreates window-bound widgets and subscriptions from saved view state,
including unsent text, attachments, edit state, and scroll anchor. It does not
reuse focus handles belonging to another window.

### Workspace coordinator

The coordinator owns placement, focused pane per window, native window handles,
and layout persistence. The main application shell owns navigation and settings.
Detached windows render session content and pane controls, without duplicating
the application sidebar or settings state.

Use separate IDs for chats, panes, splits, and windows. A pane references one
chat ID. Each chat has at most one visible placement across the application.
Opening an already visible chat focuses its pane and raises its native window.
Opening a hidden chat replaces the focused docked pane; explicit split actions
add a placement. A replaced chat continues running in the background.

## Layout and interactions

Each workspace window has a binary dock tree and an ordered list of floating
panes. A dock tree is empty, a pane leaf, or a split with axis, ratio, and two
children. Splitting right divides left/right; splitting down divides top/bottom.
Ratios default to 0.5 and clamp to 0.15 through 0.85, with minimum child sizes
taking precedence. Closing or moving a leaf promotes its sibling.

- **Split right/down:** create a new session in the source session's project
  using existing new-chat defaults. The new pane receives focus. An existing
  session can instead be chosen through the session picker.
- **Float:** remove a docked leaf and place it over the current workspace.
  Its title bar moves it, its edges resize it, and clicking raises it.
- **Dock:** offer left/right/top/bottom docking against a selected pane.
  When the dock tree is empty, docking creates its first leaf.
- **Detach:** open a native window and transfer the placement only after the
  window and session view have been created successfully.
- **Return to main window:** move a detached pane into the main workspace,
  splitting to the right of its focused pane or becoming its first leaf.
- **Close pane:** remove the placement and preserve its session and draft.
  Stop remains an explicit session action. Closing the last main pane shows
  the session picker/start view.
- **Maximize:** temporarily show one pane across its window's content area.
  Restore recovers its earlier dock or float bounds. This does not change
  native window maximization.

Every pane has a compact header with session title, provider, live status,
focus indication, close control, and an overflow menu for layout operations.
All layout actions are also available through the command palette. Dragging
works within a native window; cross-window transfer uses Detach and Return.

Detached windows support the same splits and in-window floats. Their close
control returns all their sessions to the main workspace in display order,
then hides the native window. Hidden secondary windows are retained and reused
until application exit to avoid the existing native window-drop hazard.
Closing a secondary window never quits Aether. Closing the main window retains
the existing application-quit behavior.

## Focus and session context

Keyboard actions bubble from the focused session view to its workspace.
Send, Stop, retry, queue, steering, and approval actions target that view's
chat ID. Focusing another pane preserves unfinished input and pending forms.
Pending approval status stays visible in its pane header and the session list.

Next/previous pane actions cycle through dock leaves, then floating panes in
their saved order. Directional focus actions use visible pane bounds. Shortcut
bindings must avoid existing terminal bindings and platform window shortcuts;
the implementation plan records the chosen bindings after auditing actions.

Git, explorer, browser, and the terminal drawer in the main shell follow its
focused session. Detached windows initially contain the session workspace only;
they offer Return to main window for access to those project tools. Existing
terminal split behavior remains available in the main shell.

## Persistence and recovery

Store a versioned layout file under Aether's config directory, separate from
chat records. Persist pane/chat mappings, dock topology, split ratios, floating
bounds and order, window bounds, focused panes, and maximized pane IDs.
Write through a temporary file and atomic replacement after debounced changes.
Persist session drafts and pending attachment references through the shared
session owner so application shutdown can save them from every window.

Load conversations before resolving layout references. Remove missing or
archived chat references, collapse empty branches, and keep only the first
placement of a duplicated chat. Invalid ratios or bounds use defaults. Clamp
restored native windows to an available display and floating panes to the
workspace. Older installations start with the current single-pane experience.
An unreadable or unsupported layout falls back to one pane without changing
stored conversations. An empty valid layout opens the start view.

Restore native windows only for saved nonempty layouts. Layout restoration
does not itself start an agent turn; existing session resume rules apply.
Hidden retained windows are not restored. Layout save errors keep the in-memory
layout usable and show one recoverable error in the main shell.

## Failure and size handling

Window creation or transfer failure leaves the source placement and draft
intact and reports the failure there. Movement is transactional: prepare the
destination, commit one placement, then release the old presentation. Closing
or moving panes never cancels an agent or destroys a pending approval.

Minimum pane size is 320 by 240 logical pixels. Refuse splits that cannot fit
two minimum-size children and a divider, with a disabled action and explanatory
tooltip. If resizing a window makes its existing layout smaller than those
limits, retain the tree inside a scrollable workspace. Floating panes retain
minimum dimensions and reachable title bars. Layout and focus changes do not
save an unrelated chat's draft.

## Acceptance and verification

1. Two visible sessions stream concurrently. Sending, stopping, queueing,
   steering, retrying, and answering approvals affect only the intended chat,
   including after focus changes and deletion of another chat.
2. Recursive splits resize, collapse on removal, maximize, and restore.
   Too-small splits are rejected without creating a new chat.
3. Floating panes move, resize, raise, dock, and remain reachable after resize.
4. Detach and return preserve running session identity, unsent drafts,
   attachments, edit state, transcript position, and pending approval forms.
5. Closing panes leaves agents running. Closing a detached window returns its
   panes and leaves the app alive. Closing the main window performs existing
   quit cleanup once. Verify retained-window reuse on macOS, including the
   documented Touch Bar close path, and secondary-window behavior on Linux.
6. Layouts survive restart, missing chats, malformed files, and removed displays.
   Drafts restore without submitting messages or automatically starting turns.
7. Existing single-chat workflows, project tools, terminal splits, and keyboard
   input continue to work.

Use pure model tests for tree operations, unique placement, focus fallback,
geometry validation, layout migration, and transfer rollback. Use fake agent
streams for session command isolation and lifecycle tests. Add UI integration
tests where the existing harness supports them and perform native smoke checks
for window creation, input, focus, transfer, and closing. Run workspace tests,
format checking, and a build before reporting implementation complete. Report
unavailable platform smoke checks explicitly.

## Scope boundary

This delivery covers agent-session layouts and the focus and lifecycle changes
they require. Terminal, browser, Git, and file views keep their existing hosting
arrangements. Dragging between native windows, mirrored views of the same chat,
named layout presets, and general-purpose tool docking are future work.


## Implementation notes

The shared owner remains the existing `ChatApp` entity. `SessionHandle` binds
callbacks to chat ID and epoch, and `SessionState` keeps each session's widgets
and presentation independent. This preserves the existing driver, store,
settings, and worktree ownership while introducing the view boundary.

The app harness uses deterministic reply channels to exercise concurrent
streams, scoped Stop, and queued delivery. Real driver-worker tests remain in
the agent crate because the GPUI test scheduler rejects foreign-thread wakes.

Secondary windows are retained and reused. macOS hides them through the live
NSWindow; Linux minimizes them because the pinned GPUI API lacks a per-window
hide method. Physical Linux and Touch Bar hardware checks remain unverified.
Headless GPUI checks cover detach, return, retained-window reuse, text selection,
independent composers, request input, pane drawing, and pointer dragging.
