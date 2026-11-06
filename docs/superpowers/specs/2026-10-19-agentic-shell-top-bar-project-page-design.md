# Agentic shell: top bar, workspace popover, sidebar entry, project page

Date: 2026-10-19. Visual reference: `.superpowers/brainstorm/3774-1792395948/content/shell-v5.html` (open in a browser; everything there is clickable). Locked decisions log: `.superpowers/brainstorm/3774-1792395948/locked.md`.

## Intent

Aether is becoming the user's agentic OS: the place they spend the day running and steering agents, not an IDE. This slice gives every chat a clear "where am I" (breadcrumb), puts per-workspace quick actions one click away (workspace popover), and adds a project page that shows every agent working in one project at a glance.

## Goals

1. Chat top bar: breadcrumb `project / chat title` on the left; three icon toggles on the right: workspace popover, terminal drawer, side panel.
2. Workspace popover: quick actions for the chat's workspace (machine and path, open in app, project scripts, branch, publish/remote, changes, browse files).
3. Project page: a full page per project with the project's agents as cards, filters, group-by, sort, worktrees and recent activity.
4. Sidebar: a way into the project page; otherwise unchanged.

## Non-goals

- The side panel (tabs, + menu, Changes, Diff, Browser, Files, Device) stays exactly as it is today.
- No Home/dashboard page, no remote machines, no custom window chrome (the native macOS title bar stays).
- No detection of dev-server URLs in script output (the prototype's green `● localhost:3000` chip). Later.

## Anti-goals

- No modal dialogs where a popover or inline view works (docs/ux-rubric.md).
- No I/O reachable from `render`. Every git, gh, filesystem or process call runs on the background executor and the result is stored on the entity.
- No raw hex colours; theme tokens and the existing `theme::mango*` helpers only.
- Never force-push; never push or create a GitHub repository without an explicit click.

## Constraints

- GPUI via gpui-kit 0.7.3; icons must be bundled through `icon_assets!` in `crates/app/src/assets.rs` and listed in an asset test. `LayoutList`, `CloudUpload` and `Code` are not bundled yet.
- Tests live in `crates/<crate>/tests/unit/<path>.rs` behind `#[cfg(test)] #[path = ...] mod tests;` (GLOSSARY.md). Tests are pure-function tests; spoken labels are pulled into pure fns and tested.
- Settings: `Settings` is `#[serde(default)]` with a hand-written `Default`; new fields need a default and a round-trip test in `crates/core/tests/unit/settings.rs`.
- `Page` is `Copy`; project paths are not. Use a unit variant plus a field.
- `cmd-j` is already `FocusTerminal`. Ctrl+` toggles the drawer. Cmd+Alt+B toggles the side panel. Cmd+B toggles the sidebar.
- docs/ux-rubric.md applies: popover radius 12–13, menus 9, controls 6; hit areas ≥ 24 px (28 for primary); focus ring via `focus_visible`; focus returns to the invoker; every control has hover/pressed/focus/disabled states.

## 1. Chat top bar

Replaces the body of `render_header` (`crates/app/src/app/composer/header.rs`) for agent chats on `Page::Chat`. Height stays 40 px.

Left, in order:
- Expand-sidebar button, only while the sidebar is collapsed (exists today; keep).
- Breadcrumb:
  - The project crumb has the project mark (same initials and hue as the sidebar's `project_mark`) and the project name. The name is `folder_label(project_dir)`, disambiguated the same way as the sidebar.
  - Clicking the project crumb opens the project page.
  - Then a muted `/` separator and the chat title.
  - A chat whose project is the home folder (no project) shows only the title.
  - The title truncates first. The project crumb keeps at least its mark.

Right, in order:
- `open_error` text (exists today; keep).
- Workspace toggle. Icon `LayoutList`; it shows the selected fill while the popover is open. Tooltip "Workspace".
- Terminal drawer toggle. Same behaviour as today's `terminal_button`, but the icon becomes `PanelBottom`. Tooltip shows the Ctrl+` binding. Selected fill while the drawer is open. Hidden when `terminal_folder()` is None, as today.
- Side panel toggle: today's `side_button` unchanged, including its changed-files count chip. It gains the selected fill while the panel is open.

Removed from the header, because they move into the popover: the worktree branch label, the "Open in" split button, and the files/explorer chip.

Direct (non-agent) chats keep today's header: back button, title, "Direct Chat" pill.

## 2. Workspace popover

Anchored below the workspace toggle, right-aligned, about 380 px wide. It uses the gpui-kit `Popover`, or `anchored_popup` if keyboard handling needs it (builder's call).

Navigation is drill-in. Rows with a sub-view replace the popover body with that view, which has a back arrow and a title. Esc goes back one level; Esc on the main view closes the popover and returns focus to the toggle. Opening the popover focuses its first row. Opening a sub-view focuses its first input, or else its first row. Up/Down move between rows; Enter or Space activates.

"Workspace folder" below means the chat's folder (`folder_of(chat)`; the worktree when there is one). "Project" means `project_dir(chat)`.

Main view rows, top to bottom:

1. **Machine + folder.** The computer name is read once in the background: `scutil --get ComputerName` on macOS, else `hostname`. Below it is the workspace folder, home-shortened.
   - Opens the **Location** view:
     - worktree path and project root path; clicking either copies it
     - Reveal in Finder (`cx.reveal_path`)
     - Open terminal here (opens the chat's drawer)
2. **Open in `<app>`** split row.
   - The main part opens the workspace folder in the project's app (`open_in::open`). The caret opens the **Apps** view: discovered apps with a check on the current one; picking one sets the project's app and goes back.
   - The app choice becomes per project: new setting `project_open_in: BTreeMap<String, String>` (project path → app id). It falls back to the global `open_in_app`, which becomes "default for projects without a choice".
   - The project page's split button reads and writes the same entry.
3. **Scripts** caption, then one row per project script:
   - a run/stop button, the name, and the command in muted mono
   - a chip "on new worktree" for setup scripts
4. **Add project script** opens a form view:
   - Name and Command fields, plus a checkbox "Run automatically when a new worktree is created".
   - Save is disabled until Command is non-empty; Enter saves.
   - An empty name defaults to the command's last word.
   - Editing and deleting existing scripts: a ⋯ on each script row with Edit and Delete (builder may fold this into the same form).
5. **Branch** row: the current branch with a "worktree" chip when the chat runs in a linked worktree. It opens the **Branch** view:
   - A search field over local branches, then remote branches.
   - Check on the current branch.
   - Branches checked out in another worktree show "in `<short path>`". Choosing one selects the newest chat bound to that worktree, or opens a new chat there (`open_worktree_chat`) if none.
   - Typing a name with no match offers "Create `<name>` from `<current>`".
   - "New worktree…" opens the existing git panel worktree creation flow.
   - Switching or creating is refused while an agent is running in that folder (`agent_in`). The row stays visible, disabled, with the reason as its tooltip.
   - Reuse `pick_rows`, `checked_out_at` and the `GitOp::Checkout/Track/CreateBranch` paths. Widen visibility as needed. Do not duplicate them.
6. **Remote** row, by state of the cached `Status`:
   - **No remote at all:** "Publish repository". Opens the **Publish** view:
     - repository name (defaults to the project folder name)
     - Private (default) / Public as two radio cards
     - a Publish button
     - Runs `gh repo create <name> --private|--public --source <top> --remote origin --push` in the background.
     - When `gh` is not installed or not authenticated (`gh auth status` fails), the row is disabled with the tooltip "Install GitHub CLI and run gh auth login".
   - **Remote but no upstream:** "Publish branch" (`GitOp::Push`, which sets the upstream).
   - **Upstream exists:** "origin · `<owner/repo>`" with an "Up to date" or "↑n ↓m" summary. Opens the **Remote** view:
     - Push ↑n, Pull ↓m, Fetch, enabled per `remote_ops`
     - Create pull request: `gh pr create --web` when gh is available, else open `<https remote>/compare/<branch>?expand=1`
     - Open on GitHub, which opens the https form of the remote URL
     - Needs a new `aether_git` fn to read a remote's URL.
7. **Browse files** opens the in-app explorer for the workspace folder. This is the old header chip.
8. **Changes +A −R**: line totals summed from the cached `Status` of the workspace folder. Untracked files count as added; binary files are ignored. It opens the side panel on the Changes tab.

Feedback: git and gh results reuse the git panel's `Feedback` style where it is visible. Otherwise a short line appears at the bottom of the popover with the error text and stays until the next action.

### Project scripts

- New setting `project_scripts: BTreeMap<String, Vec<ProjectScript>>`, keyed by project path. `ProjectScript { name: String, command: String, on_new_worktree: bool }`. Scripts belong to the project and show in every chat of that project, including chats in its worktrees.
- **Run** opens the chat's terminal drawer and starts the command in a new terminal tab titled with the script name, in the workspace folder.
  - The command runs as the tab's process (`$SHELL -lc '<command>'`), not typed into an interactive shell.
  - While it runs, the row shows Stop. Stop kills the process.
- **When the command ends** (on its own or via Stop), the tab stays open with its output plus a final line "Exited with code N" (or "Stopped"), and the row goes back to Run.
  - Running again reuses that tab: it clears it and starts the command over.
  - Closing the tab is the user's choice.
  - This needs a terminal-crate change: today a pane closes itself on `TerminalPaneEvent::Exited`. Script panes get a "hold on exit" flag, and the exit code is reported to the app.
- **Run state is keyed by (workspace folder, script name)**, not by chat. Two chats in the same folder see the same Running/Stop state. Stop from either kills the one process. Run never starts a second copy in the same folder. The process's tab lives in the drawer of the chat that started it.
- The terminal crate gains a small public API on `TerminalWorkspace`:
  - run a command in a new (or reused) held group
  - kill it
  - report exit
  - The exact shape is the builder's call.
- **Setup scripts** (`on_new_worktree`) run when a chat first binds to a worktree aether just created:
  - the success arm in `send_in_new_worktree`
  - the first send of a draft armed by the git panel's "Open in new chat" (`arm_existing_worktree`)
  - Choosing "Not now" in the git panel runs nothing.
  - Setup scripts run in that chat's drawer, using the same held tabs. See Q1 for whether the first prompt waits.

### Folders that aren't ready for git

- **Folder is not a git repository:** the Branch, Remote and Changes rows are replaced by one row, "Initialize git repository" (`git init`).
- **Repository with no commits:** Publish repository is disabled with the tooltip "Make a first commit to publish". Publish never commits on the user's behalf.
- The project page's facts row follows the same rules: no branch/+−/publish facts for a non-repo; it shows "not a git repository" instead.

## 3. Sidebar

Unchanged except:

- The project row's ⋯ menu (not worktree rows) gains "Open project page" as its first item, followed by a separator. Clicking the project row still folds it.
- No other sidebar change. The header already shows the expand button only while the sidebar is collapsed.

## 4. Project page

- New `Page::Project` plus `project_page: Option<PathBuf>` on `ChatApp`, holding the project folder.
- `open_project_page(folder)` sets both and dismisses popups like `open_page` does. Selecting a chat returns to `Page::Chat` as today.
- Not persisted across restarts.

Top bar: expand-sidebar button while the sidebar is collapsed, then the breadcrumb with only the project crumb. No toggles.

Body, max width about 980 px, centred, scrolls:

1. **Header**
   - A 44 px project mark, the project name (20 px semibold), and a facts row:
     - home-shortened path
     - current branch · "N worktrees"
     - uncommitted "+A −R" for the project root (same summing as the popover)
     - publish state: "not published", or `owner/repo`
   - Actions on the right:
     - the "Open in `<app>`" split button, sharing the per-project app with the popover
     - a primary "New agent" button (`new_chat_in(folder)`)
2. **Filter tabs:** All / Running / Needs you / Done, each with a live count.
   - Running = `ChatStatus::Running`.
   - Needs you = `Waiting(_)`, or failed.
   - Done = everything else.
   - "Failed" means `status() == Failed`, or idle with the last message `failed`.
3. **Group** dropdown: None / Worktree (default) / Status / Provider. Captions show the group name and count.
   - Worktree groups are keyed by branch (folder name when there is no branch).
   - Status groups are ordered Needs you (waiting), Failed, Running, Done.
4. **Sort** dropdown: Recent activity (default, `activity()` desc) / Newest (`id` desc) / Name / Status (same order as above).
   - Filter, group and sort live on `ChatApp` in memory, shared by all project pages, and are not persisted.
5. **Agent cards** in a responsive grid (min card width about 290 px).
   - Each card has:
     - provider icon (muted tint), title, status badge
     - two lines of last-activity text
     - a meta row: branch or folder, +added −removed, model, age
   - Status badge: "Working · 2m" (info tint), the waiting label in mango, "Failed" (danger), "Done" (success) for an unseen new reply, and nothing for idle (the age shows instead).
   - Last-activity text:
     - running: the live tool label (`live_label`, made public)
     - waiting: the pending request's title
     - failed: the error
     - otherwise: the first line of the last assistant message
   - +/− is the agent-reported edit total over the whole chat (`reply_edits` summed across messages, with Denied/Failed edits already excluded). It is hidden when zero.
   - Click or Enter selects the chat. Each card is a `Role::Button` with an aria label: "title, status, branch, provider model, age".
   - Empty state: "No agents here." in a dashed box.
6. **Worktrees** list from `repo_worktrees`: the main worktree first, then linked ones.
   - Each row: icon (House for main, GitBranch for linked), branch, short path, and badges "N agents", "N running", "N waiting".
   - A hover "+" starts a new agent chat there: `new_chat_in` for main, `open_worktree_chat` for linked.
   - Missing worktrees show "missing" and no "+".
7. **Recent activity:** the newest 8 entries, merged by time.
   - Chat entries: "`<title>` finished / failed / needs approval / is working", at `activity()`.
   - Commit entries: the newest 5 commits from `aether_git::history(top)` on the HEAD log, as "Committed `<short>` `<subject>`".
   - Each entry shows its relative time. Chat entries are clickable and select the chat.

All data loads in the background when the page opens. The page re-reads it on the existing 30 s branch refresh tick and after each reply, reusing the `statuses` cache where it is fresh.

## Accessibility

- Every control is reachable by keyboard and has a visible focus ring.
- Popover and dropdowns trap focus while open and return it to their trigger on close.
- Cards, rows and toggles carry aria labels built by pure functions, and those functions are unit-tested.
- Toggles expose their state: `aria_toggled` for the three header toggles and `aria_expanded` for the popover trigger.
- Colour is never the only cue: each status has its own word or glyph.

## Testing

Pure-function tests for:
- breadcrumb label and aria
- line-total summing over `Status`
- remote URL to https/owner-repo conversion
- `gh` command argument building
- script default name
- filter, group and sort functions
- card status mapping and last-activity text selection
- recent-activity merge
- settings round-trips for `project_open_in` and `project_scripts`
- the asset test for new icons
- keybinding source-grep tests if bindings change

Run `cargo build && cargo test --workspace` after each phase.

## Phasing

- A: top bar and popover shell (Location, Open in, Browse files, Changes).
- B: popover Branch and Remote/Publish.
- C: scripts (settings, terminal API, run/stop, setup on new worktree).
- D: sidebar entry and project page.

Each phase builds and passes tests on its own.

## Calls made by me (check these)

- The explorer chip leaves the header and becomes the popover's "Browse files" row. The sidebar project hover still has Files.
- The "Open in" app becomes per project, falling back to the global setting.
- Scripts are stored in the user's settings, not in a file inside the repo.
- Card +/− is agent-reported edits, not git. The project header +/− is git (uncommitted).
- "Needs you" includes failed chats. Idle chats count as Done.
- Filter, group and sort are not persisted.
- The side panel toggle keeps its changed-files count chip.
- Publishing a repository uses the `gh` CLI. Without gh, Publish repository is disabled and explains why.
- Script tabs stay open after the command ends. Run/Stop state is per folder, not per chat.
- Non-repo folders get "Initialize git repository". Repos with no commits can't be published until they have a commit.
- The project page shows aether chats only. Agents running outside aether stay in the sidebar's Outside aether view.

## Left to the builder

- Popover component choice.
- The exact terminal API shape and how liveness is observed.
- The module layout: suggested `crates/app/src/app/top_bar.rs`, `crates/app/src/workspace_popover/`, `crates/app/src/project_page/`, with pure helpers in their own files for testing.
- Card grid breakpoints.
- Caching details.

## Decided by the user

- Q1: The first prompt in a new worktree **waits for its setup scripts**.
  - Setup scripts run one after another, in settings order, each in its own held tab in the chat's drawer.
  - While they run, the composer shows "Setting up worktree… (`<script name>`)" and a Cancel action. Cancel kills the running script and returns the prompt to the draft.
  - Then the prompt is sent.
  - When a script exits non-zero, the prompt is held and the composer shows "Setup failed: `<script name>` exited with code N" with three actions:
    - Retry, which reruns from the failed script
    - Send anyway
    - Show output, which focuses that tab
  - The queued prompt survives switching chats. It does not survive an app restart; the draft text stays.
