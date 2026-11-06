# Aether UX/UI rubric

For implementers (self-check before review) and reviewers (run per surface). Mode: Operate. The user is mid-task, so the UI should disappear into the work. Brand lives in precise details: exact spacing, consistent radii, restrained color. Tokens live in `docs/wayfinder/research/waku-composer-design.md`. Performance and accessibility rules live in `reference/AGENTS.md`. This doc does not repeat them, it checks them.

## 1. Quality floor and bans

Floor (every surface, both themes, before review):

- Text contrast at least 4.5:1 (placeholder included), large text and icons at least 3:1. Check light and dark separately.
- Every control has default, hover, pressed, selected (if applicable), keyboard focus, disabled and loading/error where relevant. Half-built control = not done.
- Real content tested: long names, 10k-line output, 500 chats, empty, offline, CLI missing.
- Same action looks and behaves the same everywhere. Two different "Save" or "Open" buttons means one is wrong.
- Colors come from theme tokens, never raw hex in a view. Light/dark parity is by token, not by inversion.
- Hit areas at least 24 px tall for dense desktop rows and chips, 28+ for primary controls. Extend the hit region instead of shrinking to the glyph.

Bans (reject on sight):

- Nested cards, or same-size icon+heading+text card grids as structure.
- Modal for anything that does not need interruption or protected focus. Prefer inline, popover, drawer, panel.
- Colored side stripes above 1 px on rows, callouts or alerts. Gradient text. Decorative glass/blur.
- Hard offset shadows. Elevation is declared once: border or soft shadow, never a 1 px border under a wide shadow.
- Emoji or unicode glyphs as icons. One icon set, one stroke weight (see `crates/app/assets/icons`).
- Monospace as costume. Mono is for code, paths, hashes, command output, diffs only.
- Eyebrow/kicker labels above headings, section numbering, hero-metric tiles.
- Invented affordances for standard tasks: custom scrollbars with odd behaviour, nonstandard text selection, custom window drag regions that eat clicks.
- Meaning by color, hover or motion alone.
- Decorative motion, page-load choreography, spinners over existing content.
- Pill shapes on large containers. Radius scale is small controls 6, menus 9, cards/popovers 12-13. Pick from the scale, do not invent a new radius.

## 2. Per-surface checklist

Run each row. A "no" becomes a finding (severity in section 4).

### Hierarchy and scanability
- One primary thing per surface, findable in under 2 seconds. Secondary info is tertiary-colored, not smaller than 12 px.
- Rows scan on one axis: icon, primary label, then metadata right-aligned. No row needs reading twice.
- Accent only for primary action, current selection, focus and live state. Never decoration.
- A second neutral layer separates chrome (sidebar, toolbar, drawer) from content. No borders on everything.

### Spacing, rhythm, alignment
- All gaps and paddings on the 4/8 px grid (2 allowed only inside chips). Tight inside groups, generous between groups.
- Icons, labels and text baselines align across rows in a list. Left edges of sibling surfaces line up (sidebar rows, composer, transcript column).
- Reserve a transparent 1 px border on rows so hover and selection never shift layout.
- Same radius for same-role things. Nested radius = outer radius minus padding.

### Typography
- One family. Fixed scale with 1.125 to 1.2 steps, authored at the 14 px UI font: 12.5 secondary and menu rows, 13 labels, 14 body, 16+ only for empty-state titles.
- Weight steps are obvious (regular, medium, semibold). No weight below regular, no tracking tighter than -0.02em.
- Tabular numerals for counts, durations, diff stats, line numbers.
- Prose measure 65-75ch in the transcript. Code, diffs and tables may run wide.
- Text scales with the UI font setting. No fixed-height row that clips at larger sizes.

### Color
- Status always pairs color with icon or text: error, warning, success, running, changed, untracked.
- Diff add/remove use tinted backgrounds plus `+`/`-` gutter marks, never green/red text alone.
- Provider colors are identity marks only (12-18 px icon), never backgrounds for content.
- Check every surface in light and dark with real content. Washes and borders keep the same alpha across themes.

### States
- Empty: says what goes here and offers the next action ("Open a folder to start an agent chat"). Never "Nothing here".
- Loading: skeleton or placeholder rows in the final layout. Spinner only for an action in flight, in the control that started it.
- Error: what failed, why if known, and one recovery action (Retry, Open settings, Install CLI). Stays until resolved or dismissed. Never only a toast.
- Disabled: dimmed and also explains why (tooltip or inline text): "Install or switch on a provider to send".
- Overflow: single-line labels truncate with ellipsis, middle-truncate paths, full text on tooltip and in context menu "Copy". Multi-line fields cap height then scroll.
- Long names: 200-char chat titles, deep paths, branch names with slashes, unicode and RTL do not break the row.
- Huge lists: 1k chats, 50k lines of output, 500 changed files stay at frame rate (virtualized, section 2 Performance).
- Narrow window (the minimum window width): sidebar collapses or overlays, panels stack or hide behind a toggle, composer stays usable, no horizontal scroll on the main column.

### Interaction
- Hover fill, pressed fill (slightly darker, ~85% opacity), selected fill: three distinct, none shifts layout.
- Keyboard focus ring (`focus_visible`) appears for keyboard only, 1 px accent, not clipped by overflow.
- Full keyboard path: every flow completes without the mouse. Tab order follows visual order. Arrows move within a list or menu, Home/End jump, Enter/Space activate.
- Esc closes the topmost layer only, one layer per press. Enter confirms the primary action. Cmd+Enter sends where Enter inserts a newline.
- Focus restore: closing a popover, drawer or dialog returns focus to the invoker (composer field after a picker).
- Hover-only content (copy buttons, row actions) must also appear on keyboard focus and on row selection.
- Destructive actions: undo if possible, else confirm with the verb named ("Delete worktree"), never "OK".
- Popovers anchor to their trigger, flip when no room, never clip inside `overflow` parents, close on outside click.

### Motion
- 100-200 ms, ease-out, only for state change: open/close, selection, appearing rows. Nothing loops except a real in-progress indicator.
- Honor reduce-motion: skip decorative animation, replace slides with instant or crossfade.
- Streaming text never animates per token. Respect stream cadence limits in AGENTS.md.
- Layout never jumps while content streams. If the user scrolled up, do not yank them down.

### Copy
- Verb first, sentence case, specific: "Commit 4 files", "Open in terminal", not "Submit", "OK", "Click here".
- No internal jargon in UI text (session id, worktree ref, pty, SSE). Say what the user sees.
- Errors: what happened + what to do. "Could not start Claude Code. Check that `claude` is installed, then retry." Include the raw CLI message behind a "Details" disclosure.
- Same word for same thing everywhere (chat vs session, folder vs project, agent vs provider). Pick one in the glossary.
- Placeholder is an example, not an instruction. Tooltips add info, they do not repeat the label.

### Performance (design-facing)
- Render reads only in-memory state. No I/O, no spawn, no `git` call in a row builder. Unknown yet = skeleton or "..." and fill in later.
- Long lists use `list()` virtualization. Row height is cheap to measure. No per-frame rebuild of whole-session state.
- Typing, scrolling, resizing a panel and streaming output each hold frame rate with a long transcript open. Test with a 2k-message chat.
- Open/close of popover, drawer, panel is visible in the next frame. Heavy content loads after the shell appears.

### Native macOS conventions
- Cmd shortcuts for primary actions, shown in menus and tooltips: Cmd+N new chat, Cmd+K palette, Cmd+, settings, Cmd+W close, Cmd+[ and ] navigate, Cmd+Enter send. Never reuse a system shortcut for another meaning.
- Application menu bar exists and mirrors actions (About, Settings, Quit, Edit with working copy/paste, Window, Help).
- Right-click context menu on every row and object: chats, files, diff hunks, tool rows, terminal. Contents match the visible row actions plus Copy.
- Window chrome: traffic lights placed and padded correctly, drag region is the titlebar only and never covers a control, double-click titlebar zooms, fullscreen works, restores size and position.
- Scrolling: native momentum and overlay scrollbars, no scroll hijacking, scroll position retained when switching chats, trackpad horizontal scroll works in code blocks and diffs.
- Text: selectable in transcript and tool output, copy preserves content, Cmd+A, Cmd+F where there is content. IME composition not broken by shortcuts.
- Drag and drop files onto the composer, drop highlights per the composer spec. Finder "Reveal" and "Open in" actions exist for paths.
- System appearance follows OS light/dark live. Respect system accent only if the design says so (it currently does not).

## 3. Surface notes

### Sidebar: project groups vs plain chats
Excellent: two clearly different zones. Agent chats grouped under a folder header (folder name, middle-truncated path on tooltip, collapse state remembered), plain OpenRouter chats in a flat list below or above. Group header is quieter than chat rows. Active chat is one selected fill. Each row shows title, a live status mark (running, needs input, error, unread) and relative time. New chat per group via a header "+" and Cmd+N respecting the selected group. Context menu: rename, pin, archive, delete, reveal folder, copy path.
Failure modes: groups so heavy they read as cards; status by color dot only; header actions invisible to keyboard; reordering on every update; search that ignores group names; 200-char titles pushing the time; collapsing a group losing the selection; 500 rows not virtualized; deleted folder leaving a ghost group with no explanation or recovery.

### Agent transcript with rich tool rows
Excellent: prose reads at a comfortable measure; tool rows are compact one-line summaries (icon, verb, target, status, duration) that expand to detail. Commands show `$ cmd` and output in mono with a cap (~12 lines) and "Show all". File edits show path, `+N -M`, and an inline diff with gutter marks. Reads and searches collapse to a single line with count ("Read 3 files", "Searched 12 matches"), expandable. Running rows are visibly live, finished rows go quiet. Approvals (HITL) are inline, keyboard-answerable, and name the exact command or file. Errors in tools show red mark + text and stay expanded. Auto-scroll follows the tail until the user scrolls up, then shows a "Jump to latest" button.
Failure modes: every tool call as a full-size card; output dumped unbounded; diff lines wrapping mid-token; expand state lost on re-render or scroll; mono used for prose; streaming causing layout jump; unreadable wide lines without horizontal scroll; approvals hidden behind collapsed rows; thousands of rows rebuilt per frame; copy buttons only on hover.

### Git side panel (status, diffs, commit)
Excellent: a changes list grouped Staged / Unstaged / Untracked with per-file status letter plus color, path with filename emphasized and directory tertiary, `+N -M`. Click a file opens its diff in the panel (unified by default, side by side when wide). Stage/unstage per file and per hunk, with keyboard. Commit area: message field, primary "Commit N files" disabled with a reason when nothing is staged or message is empty. Branch name and ahead/behind visible. State changes (after agent edits) update in place without losing selection or scroll.
Failure modes: git called from render; whole panel flicker on refresh; status by color only; commit button enabled with nothing staged; conflicted and binary files unhandled; huge diffs freezing the UI (cap and offer "Load full diff"); no empty state for a clean tree or a non-repo folder; destructive discard without confirm; panel pushing the transcript below a usable width.

### Terminal drawer
Excellent: drawer opens from the bottom with a shortcut (Ctrl+` or Cmd+J), keeps its height and sessions, resizes by drag with a clear handle, and starts in the chat's working directory. Mono at readable size, full color and selection, copy/paste, scrollback, links clickable. Focus moves into the terminal on open and returns to the composer on close. Exit state is explicit ("Process exited with code 1" with a Restart action). Multiple tabs only if needed, labeled by command.
Failure modes: terminal swallowing app shortcuts (Cmd+W, Cmd+K) without a documented rule; focus trapped; resize jank; unbounded scrollback memory; wrong cwd; dead terminal with no message; theme mismatch (terminal black in light mode); sizing only on window resize, not drawer resize; drag handle too thin to grab.

### Git-worktree chat flows
Excellent: starting an agent chat offers "Run in new worktree" as a plain option near the folder choice, with a default branch name that is editable and a one-line explanation ("Isolated copy of the repo so the agent cannot touch your working tree"). The chat header and sidebar row show worktree and branch with an icon and text. Finishing offers clear next steps: Open diff, Commit, Merge or create PR, Remove worktree, each naming what will happen. Failures (dirty tree, branch exists, detached HEAD) explain themselves with a recovery.
Failure modes: jargon ("worktree ref"); silent worktree creation users cannot find on disk; no way to see the path; delete removing unmerged work without warning; a chat that looks identical to a normal one; slow `git worktree add` blocking the UI with no progress; orphaned worktrees after a crash with no cleanup path.

### Notifications
Excellent: reserve for events the user is not looking at: agent needs approval, turn finished in a background chat, error. Native macOS notifications only when the app is unfocused; in-app signal otherwise (sidebar status mark, dock badge count). Click opens the exact chat and focuses the answer control. Copy names the chat and action ("Fix auth bug needs your approval"). Settings let users mute per kind. Toasts auto-dismiss only for success, never for errors that need action.
Failure modes: notifying for every tool call; notifying while the app is focused on that chat; click goes to the wrong chat or only activates the app; stacked toasts covering the composer; error toast vanishing before it is read; no keyboard route to dismiss or act; sound by default; no permission-denied state handling.

## 4. Severity guide

- **Blocker**: prevents completing a task or loses data. Examples: approval prompt unreachable by keyboard; discard of uncommitted changes with no confirm; UI freezes on a large diff; composer unusable at minimum width; send fails silently.
- **Major**: real friction or a convention break, fix before release. Examples: status shown by color only; no empty or error state; hover-only action; focus not restored after a popover; I/O in render causing stutter; Esc closes the wrong layer; contrast below 4.5:1 in one theme.
- **Minor**: annoyance with a workaround. Examples: missing tooltip on a truncated title; inconsistent radius between two panels; no Cmd shortcut for a secondary action; copy says "Submit"; context menu missing Copy.
- **Polish**: no user impact beyond craft. Examples: 1 px misalignment of icon and label; a hover fill 2% off the token; easing curve slightly abrupt; weight step ambiguous between two labels.

Review report: list findings as `[Severity] surface, what is wrong, impact, fix`. Call out systemic patterns (same issue in 3+ places) once. Note what works so it is kept.
