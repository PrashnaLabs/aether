# Sidebar: projects, agent chats, plain chats

Design research only. Waku is GPL-3.0 (read for design, nothing copied). T3 Code is MIT (design notes, no code).
Sources: Waku `src/app/sidebar.rs`, `task_switcher.rs`, `project_picker.rs`; T3 `apps/web/src/components/Sidebar.tsx`, `Sidebar.logic.ts`, `BranchToolbar.tsx` (partly truncated by fetch, gaps noted).

## 1. How the references structure the sidebar

| Aspect | Waku | T3 Code |
|---|---|---|
| Grouping | User toggle: by Project (default) or by Updated date (Today/Yesterday/...). One shared collapse-state set so switching views keeps disclosure state. | Shelves: pinned, active, working, snoozed, settled. Project is shown on the row (favicon + name) and as a group in project views. |
| Project order | Order of first appearance in the recency-sorted session list, so the project with the newest session floats up. A trailing "no project" group collects sessions whose folder is the scratch workspace root. | Projects sort by most recent thread activity, ties by title then id. A "manual" order exists. Archived threads do not count. |
| Thread order | Newest/Oldest toggle. Recency = last reply, else created. Renaming does not bump. Sending a turn bumps immediately. | Pinned and active keep manual order (drag). Settled is time-ordered. |
| Truncation | Per project: everything from the last 3 days is always shown, older sessions hidden behind "Show more" in batches of 30. | Settled tail shows 10, then 25 per "Show more". Working/Snoozed/Settled headers collapsible, persisted, new users start collapsed. |
| Project header | Folder icon (open/closed by state), display name, hover-only compose button (new task in this project), chevron only in date mode. A thin vertical guide line runs down the children. | Favicon or monogram, name, project-level status dot = highest-priority thread status among its threads. |
| Project actions | New task here (hover button). Header row holds filter menu (grouping/ordering), new project (folder-new icon). Project picker is a separate fuzzy-filter popup. | Context menu per project (contents not visible in fetched slice). Add-project flow lives elsewhere (palette / hero when no projects). |
| Collapse | Click, Enter/Space toggle; Left collapses, Right expands. Collapse-all command. Collapsed group keeps header only. | Chevron per shelf, persisted. |
| Empty state | If no history, the first project header still renders so the header actions (compose, new project) stay reachable. Full empty page in main area. | Empty shelves take no space; dashed drop target only during drag. "No projects" hero in main pane. |
| Keyboard | Headers and rows are tab stops. Enter/Space select, Shift+F10 context menu, F2-style rename inline. Ctrl+Tab task switcher: MRU grid of last 10 tasks, cycles while modifier held. | Enter/Space activate, Shift+F10 menu, hold jump modifier to show numbered jump hints on rows, search input keeps focus while arrows move a listbox highlight. |
| Row menu | Rename, Remove. Inline rename with accent border, commit on blur. | Rename (double-click), pin, snooze, settle, bulk actions with counts that only include rows affected. |

## 2. Row anatomy

Waku session row (51px, two lines):
- Line 1: title (13.5px, single line, ellipsis) plus a 12px status icon on the right. Spinner for working/connecting, hourglass for waiting on user, alert for error, x for failed. Idle shows nothing.
- Line 2 (12.5px, tertiary color): git-branch icon + branch name when grouped by project and the session is on a worktree (or the project's current branch cached in the background for local sessions); folder icon + project name when grouped by date. Right side: time. Working shows elapsed ("working 2m"), background shows a label, else "5m / 3h / 420d" since last reply. Never replied = blank.
- Children are indented under the project guide line.

T3 thread row (card):
- Top: project favicon+name, pin, status slot. Status slot shows a label with live elapsed while working, otherwise relative time. On hover/focus it swaps to action buttons (snooze, settle).
- Title line, double-click to rename.
- Bottom: worktree icon + branch (middle truncated), running-terminal dot, PR badge, provider icon stack (previous providers kept after a handoff).
- Diff stats: a placeholder that renders nothing. Neither reference shows +/- on rows. Skip it.
- Slim variant: one 36px line, favicon, title, time. Good model for a dense mode.

T3 status model (7 states): approval (amber), input (indigo), working (sky pulse), waiting (gray, background work pending), failed, limited, ready (unlabeled). Color only for things needing action, motion, or failure. Unread = completion newer than last visit; shown as "Done". Project dot rolls up the highest priority.

## 3. Chat kinds

Neither separates "plain chat" from "agent". Both are agent-only apps; provider shows as a small icon (T3) or not at all (Waku). Waku's only "projectless" concept: sessions started without a folder land in a scratch workspace folder and are shown in one trailing group called "no project". That is the closest analogue to aether's OpenRouter chats and it confirms the pattern: a distinct trailing group, no folder semantics.

## 4. Worktrees and branches

- Waku models a session workspace as Local, NewWorktree{base branch}, or Worktree{path, branch}. Row label comes from the persisted branch first (so a pending worktree already shows its base branch), then falls back to the project's live branch for Local. Branch labels resolve in one background pass keyed by a cheap fingerprint, never in render. A worktree session is not a separate project group; it stays under its parent project with the branch label as the only differentiator.
- T3 chooses mode at new-thread time in the composer context strip: Local, New worktree, or Previous worktree (reuses the most recent one, shows its branch). Once a thread starts, the mode is locked. Branch picker is separate, strip labels collapse to icons when narrow. Row shows worktree icon + branch; hover card shows full path and a branch-mismatch warning.
- A new worktree thread simply appears as a normal row under the same project, top by recency, with the worktree/branch line. No nested group.

## 5. Recommendation for aether

Aether constraint: agent chats have a fixed `cwd`; OpenRouter chats have none. That maps cleanly onto Waku's "projects + trailing projectless group", with T3's status vocabulary.

### Section layout (top to bottom)
1. Header actions: new chat, new project (open folder), search/filter. Keep always visible even when empty.
2. **Agents** live strip: only chats that are working or need attention (approval/input/error). Hide the section when empty. Rows are the same component, with the project name shown on line 2 instead of branch. This is the existing AGENTS section; keep it, but define membership as "status needs eyes", not "has a live process".
3. **Pinned**: cross-kind, flat, rows show a kind icon so mixing stays readable.
4. **Projects**: one group per distinct agent-chat `cwd`. Header: folder icon (open/closed), folder basename, tooltip with full path, rollup status dot when collapsed, hover compose button. Children indented with a guide line. Order projects by newest chat activity; chats newest first.
5. **Chats**: OpenRouter chats, flat, newest first, header with a chat-bubble icon (not a folder). This is the single trailing group; no folder, no compose-in-folder, its hover button is plain "new chat".

Default all groups expanded except show-more truncation: show chats active within 3 days, plus first 5 older, "Show N more" reveals 30. Persist collapse state per project path.

### Row anatomy (two lines, ~44-50px)
- Line 1: provider icon (12px, existing provider_icon) + title (ellipsis) + trailing status glyph (spinner / hourglass for HITL / alert / none). Provider icon lives here because within a project the chats can mix Claude/Pi/OpenCode. In Chats section the OpenRouter icon repeats; drop it there and let the section header carry the kind.
- Line 2 (tertiary): branch icon + branch when the cwd is a git worktree or has a known branch (resolved on a background task, cached by cwd, never in render); else nothing. Right: relative time, or "working 2m" elapsed while busy.
- Skip diff stats (neither reference ships them) and skip PR badges for v1.
- Status color rule from T3: color only for working, waiting-on-user, failed. Idle/ready is unstyled. Add an unread dot only if cheap (last-seen timestamp per chat).
- Selected row: background fill, no border. Focus ring on keyboard focus.

### Interactions
- Click header or Enter/Space toggles; Left/Right collapse/expand; "collapse all" in the filter menu.
- Project header menu (right-click and Shift+F10): New chat here, Reveal in Finder, Copy path, Remove project from sidebar (only hides the group and leaves chat history; confirm if it has chats, otherwise delete them via per-row remove). Do not delete the folder, ever.
- Hover compose on a project header starts an agent chat with that cwd; provider defaults to the last-used agent provider for that project.
- Row menu: Rename (inline), Pin/Unpin, Reveal folder (agent chats), Remove. Remove on a running agent asks first.
- New project: native folder picker, creates an empty group and immediately opens a new-chat draft there; an empty project group shows one muted line "No chats yet. + to start one" so it is not invisible.
- Empty sidebar: keep header actions plus one hint line in the Chats section; main pane carries the hero.
- Worktrees (when aether adds them): same project group, branch label distinguishes; no nested groups. Pick Local/New worktree in the composer strip, lock after the first send (T3 pattern).

### Keyboard
- Tab stops on every header and row; Enter/Space open; Left/Right fold; Shift+F10 menu; F2 rename.
- Up/Down within the list moves focus across visible rows, skipping hidden ones.
- Ctrl+Tab MRU switcher (Waku pattern) is a later add; the command palette already covers jump-to-chat.

### Skipped on purpose
Date grouping toggle, drag reorder, snooze/settle shelves, hover cards, diff stats. Add the date toggle only if project groups prove too long; add drag reorder only with user request.

### Open questions
- Should Pinned stay cross-kind, or live inside each group? Recommendation above is cross-kind, flat.
- Should the AGENTS strip duplicate rows that also appear under Projects? Both references duplicate nothing; recommend showing in the strip only and hiding from the group while attention-worthy, or keep duplicates with a dimmed second copy. Needs a decision before build.
