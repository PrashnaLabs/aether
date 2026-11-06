# Agentic Shell: Top Bar, Workspace Popover, Project Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the chat header with a breadcrumb top bar, add a drill-in workspace popover (location, open-in, scripts, branch, publish/remote, changes), run project scripts (including setup scripts that gate a new worktree's first prompt), and add a project page reachable from the breadcrumb and the sidebar.

**Architecture:**
- Pure helpers carry every decision that can be tested: totals, labels, remote parsing, gh args, filter/group/sort, card status, the setup queue. They live in `model.rs` files with unit tests.
- GPUI state lives on `ChatApp`:
  - `WorkspacePopover` for the popover
  - `ProjectPage` for the page
  - `script_runs` for running scripts
- All git, gh and process calls go through the background executor.
- The terminal crate gains "command panes", which run one command and are held open on exit, with an event reporting the exit.

**Tech Stack:** Rust 2024, GPUI via gpui-kit 0.7.3 (gpui-component), Lucide icons via `icon_assets!`, alacritty_terminal (terminal crate), git CLI, GitHub CLI `gh`.

**Spec:** `docs/superpowers/specs/2026-10-19-agentic-shell-top-bar-project-page-design.md`. Executors read it alongside this plan; its section numbers are cited below. Visual reference: `.superpowers/brainstorm/3774-1792395948/content/shell-v5.html`.

## Global Constraints

**Git workflow**
- Never commit, stage, amend or push. Each task ends at a checkpoint (tests green, changes left in the working tree). The controller asks the user before any commit.
- Never add `Co-Authored-By` lines.

**Tests**
- Tests live in `crates/<crate>/tests/unit/<path>.rs`, attached with `#[cfg(test)] #[path = "<rel>"] mod tests;` at the end of the source file. The test file starts with `use super::*;`. Never inline tests, and never add `tests/unit/main.rs` (GLOSSARY.md).
- Tests are pure-function tests. No GPUI test contexts.
- Run commands:
  - per task: `cargo test -p aether <filter>` (app crate), `cargo test -p aether-ui`, `cargo test -p aether-core`, `cargo test -p aether-git`, `cargo test -p aether-terminal`
  - per phase: `cargo build && cargo test --workspace`

**No I/O in render**
- No I/O reachable from `render`. Git, gh, `scutil`/`hostname` and filesystem calls run via `cx.background_executor().spawn` or the `Agent` wrappers, and results are stored on the entity.

**Styling**
- Theme tokens only (no raw hex); waiting colour via `aether_ui::theme::mango()` / `mango_text(cx)`.
- Radius: popover 12, menus 9, controls 6.
- Hit areas ≥ 24 px; primary buttons ≥ 28 px.
- Every interactive element has hover, pressed, `focus_visible` ring, and disabled states.
- Text sizes 12.5 / 13 / 14 (docs/ux-rubric.md).

**Icons**
- Every new Lucide icon goes into `icon_assets!` in `crates/app/src/assets.rs` and into an asset test in `crates/app/tests/unit/assets.rs`. Not yet bundled: `LayoutList`, `CloudUpload`, `Code`, `GitPullRequest`.

**Accessibility**
- Spoken labels are built by pure fns and unit-tested.
- Toggles use `aria_toggled`; the popover trigger uses `aria_expanded`.
- Focus returns to the invoker when a popup closes.

**Formatting**
- `rustfmt.toml`: `max_width = 100`, `use_small_heuristics = "Max"`. Run `cargo fmt` on touched files.

**Code style**
- No new comments unless the why is non-obvious (user rule). Match surrounding style.

## Review Focus

1. **Missing folder.** The chat's folder or worktree was deleted (`worktree_missing`, `worktree_removed`, or `missing_folders`). Expect:
   - the popover's git and script rows disabled with "Folder not found"
   - the breadcrumb still renders
   - the project page shows "Folder not found" in its facts row
   - Test: Task 4 `pop_rows_for_missing_folder_disable_git_and_scripts`.
2. **Git locked or failed** (`StatusError::Locked` / `Failed`) or git not installed. Expect the Changes row to show "—" (not "+0 −0"), and the Branch and Remote rows to stay usable after the next refresh. Test: Task 4 `changes_label_unknown_when_status_missing`.
3. **Script commands with quotes, `&&`, pipes or `$VARS`.** Expect the command to reach the shell exactly as typed, as one `-lc` argument. Test: Task 8 `command_args_pass_command_verbatim`.
4. **A chat archived, deleted or closed while its script runs**, or the app quitting. Expect the process to be killed, and the run state to clear so other chats in that folder show Run. Test: Task 9 `runs_for_closed_chat_are_dropped`.
5. **gh installed but not logged in, or `gh repo create` failing** (name taken). Expect Publish disabled with "Install GitHub CLI and run gh auth login" when logged out. A failure shows gh's stderr line in the popover footer and leaves the Publish view open with the inputs intact. Test: Task 7 `gh_state_from_auth_status`, `publish_error_keeps_first_stderr_line`.

---

## Phase A: top bar and popover shell

### Task 1: Per-project settings and app choice

**Files:**
- Modify: `crates/core/src/settings.rs` (struct ~:233-304, `Default` ~:351-388)
- Modify: `crates/core/src/lib.rs:12-15` (re-export `ProjectScript`)
- Modify: `crates/app/src/open_in.rs` (add `project_app`)
- Test: `crates/core/tests/unit/settings.rs`, `crates/app/tests/unit/open_in.rs`

**Interfaces:**
- Produces:
  - `pub struct ProjectScript { pub name: String, pub command: String, #[serde(default)] pub on_new_worktree: bool }`, deriving `Clone, Debug, PartialEq, Serialize, Deserialize`.
  - On `Settings`:
    - `pub project_open_in: BTreeMap<String, String>` (project path → app id)
    - `pub project_scripts: BTreeMap<String, Vec<ProjectScript>>` (project path → scripts in run order)
    - Both `#[serde(default)]` and empty in `Default`.
  - `pub(crate) fn project_app(apps: &[OpenApp], settings: &Settings, project: &Path) -> Option<usize>`: uses the `project_open_in` entry when its app is discovered, else `preferred(apps, &settings.open_in_app)`.
  - `pub(crate) fn project_key(project: &Path) -> String`, which is `project.display().to_string()`. It is the one key used for both maps.

- [ ] **Step 1: Write failing tests**

```rust
// crates/core/tests/unit/settings.rs
#[test]
fn project_maps_default_empty_and_round_trip() {
    let old: Settings = serde_json::from_str("{}").unwrap();
    assert!(old.project_open_in.is_empty() && old.project_scripts.is_empty());
    let s = Settings {
        project_open_in: [("/p".into(), "cursor".into())].into(),
        project_scripts: [("/p".into(), vec![ProjectScript {
            name: "dev".into(), command: "pnpm dev".into(), on_new_worktree: false,
        }])].into(),
        ..Settings::default()
    };
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back, s);
}

#[test]
fn project_script_on_new_worktree_defaults_false() {
    let s: ProjectScript = serde_json::from_str(r#"{"name":"a","command":"b"}"#).unwrap();
    assert!(!s.on_new_worktree);
}
```

```rust
// crates/app/tests/unit/open_in.rs
#[test]
fn project_app_prefers_project_entry_then_global() { /* apps [finder, cursor, zed]; settings.open_in_app = "zed";
   project_open_in {"/p": "cursor"} → Some(1) for /p; Some(2) for /q; entry naming an undiscovered app → Some(2) */ }
```

- [ ] **Step 2: Run them and confirm they fail.** `cargo test -p aether-core project_ && cargo test -p aether project_app`. Expected: compile errors (fields and fn missing).
- [ ] **Step 3: Implement the fields, `ProjectScript`, the re-export, `project_key` and `project_app`.**
- [ ] **Step 4: Run them and confirm they pass.** Same commands. Expected: PASS.
- [ ] **Step 5: Checkpoint.** Leave the changes uncommitted.

### Task 2: Top bar (breadcrumb and three toggles)

**Files:**
- Modify: `crates/ui/src/chat/sidebar/chat_row.rs` (make the project mark public), `crates/ui/src/chat/sidebar/mod.rs` (re-export)
- Modify: `crates/app/src/app/composer/header.rs` (`render_header` body)
- Modify: `crates/app/src/app/sidebar.rs` (extract `project_names`)
- Modify: `crates/app/src/terminal.rs:304-319` (`terminal_button` icon `PanelBottom`)
- Modify: `crates/app/src/assets.rs`, `crates/app/tests/unit/assets.rs` (`LayoutList`)
- Test: `crates/app/tests/unit/app/composer/header.rs` (create; add the `#[path]` mod to header.rs), `crates/ui/tests/unit/chat/sidebar/chat_row.rs`

**Interfaces:**
- Consumes: Task 1 (none directly).
- Produces:
  - `pub fn project_mark(project: Option<&SharedString>, cx: &App) -> AnyElement` (now `pub`, re-exported as `aether_ui::project_mark`).
  - `pub fn project_mark_sized(name: &str, size: Pixels, cx: &App) -> AnyElement`: same tint and initials; font is `size * 0.5625`; radius is `size * 0.25`.
  - `pub(crate) fn project_names(&self) -> HashMap<PathBuf, String>` on `ChatApp`: the disambiguated folder labels for every project group. The sidebar switches to it.
  - `pub(crate) fn breadcrumb_aria(project: Option<&str>, title: &str) -> String`: "auto-clicker / Project Purpose", or just "Project Purpose" when `project` is None.
  - `pub(crate) fn chat_project(&self, ix: usize) -> Option<PathBuf>`: `project_dir`, or None when that is the home dir.
  - `ChatApp.workspace_pop: WorkspacePopover` exists from Task 3. Here the toggle only calls `toggle_workspace_pop(window, cx)`; stub it as a no-op until Task 3.

Behaviour (spec §1):
- The left side renders `expand_btn` (unchanged), then the crumb. The crumb is `project_mark` plus the project name (`text_sm`, muted), then a `/` separator (muted), then the title (`text_sm`, medium, foreground, truncates first).
- The project crumb is a `Role::Link`-style `div` with id `crumb-project`. Its click is wired in Task 11; until then it is not clickable.
- The right side keeps `open_error`, then for agent chats:
  - the workspace toggle: `icon_chip("workspace-toggle", pop_open, cx)`, `IconName::LayoutList`, `aria_expanded(pop_open)`, tooltip "Workspace"
  - `terminal_button` (icon `PanelBottom`)
  - `side_button` (unchanged)
- Delete the worktree label, the Open-in split and `files_btn` from the header. `open_project` and `load_open_apps` stay; Task 4 uses them.
- Direct chats render exactly as today.

- [ ] **Step 1: Write failing tests**

```rust
// crates/app/tests/unit/app/composer/header.rs
#[test]
fn breadcrumb_aria_joins_project_and_title() {
    assert_eq!(breadcrumb_aria(Some("auto-clicker"), "Project Purpose"), "auto-clicker / Project Purpose");
    assert_eq!(breadcrumb_aria(None, "Scratch"), "Scratch");
}
```
Add `I::LayoutList, I::PanelBottom, I::PanelRight` to a new `serves_top_bar_icons` test in `crates/app/tests/unit/assets.rs`.

- [ ] **Step 2: Run them and confirm they fail.** `cargo test -p aether breadcrumb_aria serves_top_bar_icons`. Expected: FAIL (fn missing; `LayoutList` not served).
- [ ] **Step 3: Implement.** Make the mark fns public, add `project_names` and switch the sidebar to it, build the header and swap the terminal icon.
- [ ] **Step 4: Run and confirm they pass.** `cargo test -p aether && cargo test -p aether-ui`. Then `cargo run`: an agent chat shows "mark project / title" and three toggles; a direct chat is unchanged.
- [ ] **Step 5: Checkpoint.**

### Task 3: Popover state, navigation and shell rendering

**Files:**
- Create: `crates/app/src/workspace_popover/mod.rs` (state, open/close/back, key handling, render dispatch)
- Create: `crates/app/src/workspace_popover/model.rs` (pure helpers)
- Modify: `crates/app/src/main.rs` (declare the module), `crates/app/src/app/mod.rs` (`ChatApp.workspace_pop` field), `crates/app/src/app/new.rs` (construct), `crates/app/src/app/mod.rs` `open_page` and `crates/app/src/app/chat/mod.rs` `select` (close the popover)
- Test: `crates/app/tests/unit/workspace_popover/model.rs`

**Interfaces:**
- Produces:
  - `pub(crate) enum PopView { Main, Location, Apps, Script(Option<usize>), Branch, Publish, Remote }`. The `Script` index is `Some` when editing an existing script.
  - `pub(crate) fn back_of(view: &PopView) -> Option<PopView>`: `Main` → None; every other view → `Some(Main)`.
  - `pub(crate) struct WorkspacePopover`:
    - `open: bool`
    - `view: PopView`
    - `focus: FocusHandle`
    - `restore: Option<FocusHandle>`
    - `machine: Option<SharedString>`
    - `error: Option<SharedString>`
    - plus the inputs created in `new`: `script_name`, `script_cmd`, `branch_query`, `publish_name` (all `Entity<InputState>`), `script_setup: bool`, `publish_private: bool` (default true)
  - On `ChatApp`:
    - `pub(crate) fn toggle_workspace_pop(&mut self, window: &mut Window, cx: &mut Context<Self>)`
    - `pub(crate) fn pop_view(&mut self, view: PopView, window: &mut Window, cx: &mut Context<Self>)`, which focuses the first input of the view if any, else the first row
    - `pub(crate) fn close_workspace_pop(&mut self, window: &mut Window, cx: &mut Context<Self>)`, which restores focus to the toggle
    - `pub(crate) fn render_workspace_pop(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement>`
  - `pub(crate) fn machine_name(scutil: Option<&str>, hostname: Option<&str>) -> Option<String>`: picks the trimmed, non-empty `scutil` output, else `hostname` with any `.local` suffix removed.

Behaviour (spec §2):
- Render: about 380 px wide, radius 12, anchored below and right-aligned to the toggle. Use `aether_ui`'s `anchored_popup` (see `crates/ui/src/components/anchored.rs`), as `ChoiceMenu` does; the trigger is the header toggle.
- Main view rows come from Task 4. This task renders the frame and a placeholder body.
- Keys inside the popover:
  - `escape` → `back_of`, else close
  - `up`/`down` → move between rows via per-row focus handles (the same pattern as `crates/ui/src/chat/sidebar/state.rs`)
  - `enter`/`space` → activate
- Clicking outside closes the popover.
- `toggle_workspace_pop` reads the machine name once in the background (`scutil --get ComputerName` on macOS, else `hostname`).

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn back_goes_to_main_then_closes() {
    assert_eq!(back_of(&PopView::Branch), Some(PopView::Main));
    assert_eq!(back_of(&PopView::Script(Some(2))), Some(PopView::Main));
    assert_eq!(back_of(&PopView::Main), None);
}
#[test] fn machine_name_prefers_scutil_and_trims_local() {
    assert_eq!(machine_name(Some("Iris's MacBook Pro\n"), Some("iris-mbp.local")).as_deref(), Some("Iris's MacBook Pro"));
    assert_eq!(machine_name(Some("  "), Some("iris-mbp.local")).as_deref(), Some("iris-mbp"));
    assert_eq!(machine_name(None, None), None);
}
```

- [ ] **Step 2: Run and confirm they fail.** `cargo test -p aether workspace_popover`. Expected: FAIL.
- [ ] **Step 3: Implement the state, the helpers, the field wiring and the shell render.** Mount `render_workspace_pop` as a child of the header so it can anchor.
- [ ] **Step 4: Run and confirm they pass.** Also check by hand: clicking the toggle opens the frame; Esc closes it and focus returns to the toggle.
- [ ] **Step 5: Checkpoint.**

### Task 4: Main view: Location, Open in / Apps, Browse files, Changes

**Files:**
- Create: `crates/app/src/workspace_popover/rows.rs` (main view rows, Location and Apps views)
- Modify: `crates/app/src/workspace_popover/model.rs`
- Modify: `crates/app/src/app/composer/header.rs` (`open_project` takes the folder and app index)
- Modify: `crates/app/src/assets.rs`, `crates/app/tests/unit/assets.rs`
- Test: `crates/app/tests/unit/workspace_popover/model.rs`

**Interfaces:**
- Consumes:
  - Task 1 `project_app`, `project_key`
  - Task 3 `PopView`, `pop_view`, `close_workspace_pop`
  - `open_in::open`, `chat_folder`
  - `GitPanel` cached `Status` via `ChatApp::git_status()` (`crates/app/src/git_panel/status.rs:28`)
  - `toggle_changes`, `open_terminal`, `toggle_explorer_page`
- Produces:
  - `pub(crate) fn line_totals(changes: &[aether_git::Change]) -> (u32, u32)`: sums `Stat::Lines(a, r)`; `Unknown` and `Binary` add nothing.
  - `pub(crate) fn changes_label(status: Option<&aether_git::Status>) -> String`: "+6,005 −2" with thousands separators, or "—" when None.
  - `pub(crate) fn home_short(path: &Path, home: &Path) -> String`: "~/…" when under home.
  - `pub(crate) struct PopRows { git: bool, scripts: bool, reason: Option<&'static str> }` and `pub(crate) fn pop_rows(folder_ok: bool, repo: bool) -> PopRows`. A missing folder disables git and scripts with the reason "Folder not found"; a non-repo disables git only (Task 7 swaps in the Init row).
  - `open_project(&mut self, folder: PathBuf, app_ix: usize, cx)`: the signature changes from today's `(app_ix, cx)`.

Behaviour (spec §2 rows 1, 2, 7, 8):
- **Machine row:** `Laptop` icon, the machine name, and below it `home_short(workspace folder)`. Opens `PopView::Location`.
- **Location view:**
  - worktree path and project root, each copying on click
  - Reveal in Finder (`cx.reveal_path`)
  - Open terminal here (`open_terminal`, then close the popover)
- **Open in split row:** the main part runs `open_project(folder, project_app(...))`; the caret runs `pop_view(Apps)`.
- **Apps view:** the discovered apps with their icon and a check on the current one. Picking an app writes `project_open_in[project_key(project)]` through `update_settings`, then goes back.
- **Browse files** (`Folder` icon): `toggle_explorer_page`, then close.
- **Changes** (`FileDiff` icon): shows `changes_label`. Runs `toggle_changes(true, …)`, then close.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn line_totals_sum_lines_and_skip_binary_unknown() { /* [Lines(10,2), Binary, Unknown, Lines(5995,0)] → (6005, 2) */ }
#[test] fn changes_label_formats_thousands() { /* status with (6005,2) → "+6,005 −2" */ }
#[test] fn changes_label_unknown_when_status_missing() { assert_eq!(changes_label(None), "—"); }
#[test] fn home_short_tildes_home_paths() { /* /Users/i/Code/x with home /Users/i → "~/Code/x"; /opt/x → "/opt/x" */ }
#[test] fn pop_rows_for_missing_folder_disable_git_and_scripts() {
    let r = pop_rows(false, true);
    assert!(!r.git && !r.scripts && r.reason == Some("Folder not found"));
    let r = pop_rows(true, false);
    assert!(!r.git && r.scripts);
}
```
Extend `serves_top_bar_icons` with `Laptop, Folder, FileDiff, Copy, Check, ChevronRight, ChevronDown, ArrowLeft, House, SquareTerminal`.

- [ ] **Step 2: Run and confirm they fail.** `cargo test -p aether workspace_popover serves_top_bar_icons`.
- [ ] **Step 3: Implement the rows, the views, the helpers and the `open_project` signature change** (update its one caller).
- [ ] **Step 4: Run and confirm they pass.** Then run the phase check: `cargo build && cargo test --workspace`.
- [ ] **Step 5: Checkpoint (end of Phase A).** The controller asks the user about committing Phase A.

---

## Phase B: branch and publish

### Task 5: git crate: remotes, URL helpers, init

**Files:**
- Modify: `crates/git/src/lib.rs` (new pub fns and re-exports)
- Modify: `crates/agent/src/agent.rs` (wrappers)
- Test: `crates/git/tests/unit/lib.rs`

**Interfaces:**
- Produces:
  - `pub struct Remote { pub name: String, pub url: String }` (Clone, Debug, PartialEq, Eq)
  - `pub fn parse_remotes(out: &str) -> Vec<Remote>`: parses `git remote -v`, fetch lines only, deduped by name, in input order.
  - `pub fn remotes(top: &Path) -> Result<Vec<Remote>, String>`
  - `pub fn https_url(url: &str) -> Option<String>`:
    - `git@github.com:o/r.git` → `https://github.com/o/r`
    - `ssh://git@host/o/r.git` → `https://host/o/r`
    - `https://host/o/r.git` → `https://host/o/r`
    - Local paths → None.
  - `pub fn owner_repo(url: &str) -> Option<String>`: "o/r" from any of the URL forms above.
  - `pub fn init(folder: &Path) -> Result<(), String>`: runs `git init` through the crate's `run`.
  - Agent wrappers following the `git_status` pattern (`spawn_blocking`, return a `'static` future):
    - `pub fn git_remotes(&self, top: PathBuf) -> impl Future<Output = Result<Vec<git::Remote>, String>> + 'static`
    - `pub fn git_init(&self, folder: PathBuf) -> impl Future<Output = Result<(), String>> + 'static`

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn parse_remotes_keeps_fetch_lines_once() {
    let out = "origin\tgit@github.com:iris/ac.git (fetch)\norigin\tgit@github.com:iris/ac.git (push)\nup\thttps://github.com/x/y (fetch)\n";
    assert_eq!(parse_remotes(out), vec![
        Remote { name: "origin".into(), url: "git@github.com:iris/ac.git".into() },
        Remote { name: "up".into(), url: "https://github.com/x/y".into() }]);
}
#[test] fn https_url_converts_ssh_forms() {
    assert_eq!(https_url("git@github.com:iris/ac.git").as_deref(), Some("https://github.com/iris/ac"));
    assert_eq!(https_url("ssh://git@gitlab.com/a/b.git").as_deref(), Some("https://gitlab.com/a/b"));
    assert_eq!(https_url("https://github.com/a/b.git").as_deref(), Some("https://github.com/a/b"));
    assert_eq!(https_url("/srv/repos/x.git"), None);
}
#[test] fn owner_repo_from_any_form() { /* each of the three → "iris/ac" / "a/b" */ }
```

- [ ] **Step 2: Run and confirm they fail.** `cargo test -p aether-git remote`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run and confirm they pass.**
- [ ] **Step 5: Checkpoint.**

### Task 6: Branch view

**Files:**
- Create: `crates/app/src/workspace_popover/branch.rs`
- Modify:
  - `crates/app/src/git_panel/picker.rs` (`pick_rows`, `PickRow`, `PickMode` → `pub(crate)`)
  - `crates/app/src/git_panel/worktrees.rs` (`checked_out_at`, `trees` → `pub(crate)`)
  - `crates/app/src/git_panel/commit.rs` (`run_git_in` → `pub(crate)`)
- Modify: `crates/app/src/workspace_popover/model.rs`
- Test: `crates/app/tests/unit/workspace_popover/model.rs`

**Interfaces:**
- Consumes:
  - `pick_rows(&Branches, &PickMode::Switch, query)`
  - `checked_out_at(trees, top, branch)`
  - `run_git_in(folder, top, GitOp, None, window, cx)`
  - `agent_in(folder)` (`crates/app/src/app/worktree/branch.rs:11`)
  - `open_worktree_chat(path, false, window, cx)`
  - `aether_git::branches` (background)
- Produces:
  - `pub(crate) fn newest_chat_in(chats: &[(usize, PathBuf, i64)], path: &Path) -> Option<usize>`: the chat index with max activity whose cwd is `path`, else None.
  - `pub(crate) fn branch_block_reason(agent_running: bool, folder_ok: bool) -> Option<&'static str>`: "An agent is working in this folder" or "Folder not found".

Behaviour (spec §2 row 5):
- The main row shows `status.branch` (or "detached"). It adds a "worktree" chip when `chat.worktree_branch` is non-empty.
- The view has a `branch_query` field, then:
  - `Create "<q>" from <current>` (from the `PickRow::Create` row)
  - local branches with a check on the current one
  - "Remote branches"
  - then "New worktree…", which calls `open_picker_as(true)` on the git panel and opens the side panel.
- A branch that `checked_out_at` reports as `in …` shows that suffix and an `ExternalLink` icon. Picking it selects `newest_chat_in`, else calls `open_worktree_chat`.
- Picking any other row runs `GitOp::Checkout` / `Track` / `CreateBranch` via `run_git_in`, then goes back to Main.
- When `branch_block_reason` is Some, every action row is disabled and its tooltip shows the reason.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn newest_chat_in_picks_latest_activity_in_path() { /* [(0,/a,5),(1,/b,9),(2,/a,7)] /a → Some(2); /c → None */ }
#[test] fn branch_block_reason_prefers_missing_folder() {
    assert_eq!(branch_block_reason(true, false), Some("Folder not found"));
    assert_eq!(branch_block_reason(true, true), Some("An agent is working in this folder"));
    assert_eq!(branch_block_reason(false, true), None);
}
```

- [ ] **Step 2: Run and confirm they fail.**
- [ ] **Step 3: Implement the view and widen the visibilities.**
- [ ] **Step 4: Run and confirm they pass.** Check by hand: switch, create, a branch in another worktree, and blocked while running.
- [ ] **Step 5: Checkpoint.**

### Task 7: Remote row, Publish and Remote views, gh

**Files:**
- Create: `crates/app/src/workspace_popover/remote.rs` (views)
- Create: `crates/app/src/workspace_popover/gh.rs` (process calls; background only)
- Modify: `crates/app/src/workspace_popover/model.rs`; `crates/app/src/git_panel/sync.rs` (`push_state`, `remote_ops`, `Push` → `pub(crate)`)
- Modify: `crates/app/src/assets.rs`, `crates/app/tests/unit/assets.rs` (`Cloud`, `CloudUpload`, `ArrowUp`, `ArrowDown`, `RotateCw`, `GitPullRequest`, `ExternalLink`)
- Test: `crates/app/tests/unit/workspace_popover/model.rs`

**Interfaces:**
- Consumes:
  - Task 5 `Remote`, `remotes`, `https_url`, `owner_repo`, `git_remotes`, `git_init`
  - `push_state`, `remote_ops`, `run_git_in`
  - `cx.open_url`
- Produces:
  - `pub(crate) enum GhState { Missing, LoggedOut, Ready }`
  - `pub(crate) fn gh_state(found: bool, auth_ok: bool) -> GhState`
  - `pub(crate) enum RemoteRow { Init, Publish { blocked: Option<&'static str> }, PublishBranch, Remote { name: String, slug: Option<String>, ahead: u32, behind: u32 } }`
  - `pub(crate) fn remote_row(repo: bool, status: Option<&Status>, remotes: &[Remote], gh: GhState) -> RemoteRow`:
    - not a repo → `Init`
    - no remotes → `Publish`, blocked by "Make a first commit to publish" when `status.commit` is None, and by "Install GitHub CLI and run gh auth login" when gh is not `Ready` (the commit reason wins)
    - remotes but no upstream → `PublishBranch`
    - otherwise → `Remote`, with `ahead_behind`
  - `pub(crate) fn publish_args(name: &str, private: bool, top: &Path) -> Vec<String>`: `["repo","create",name,"--private"|"--public","--source",top,"--remote","origin","--push"]`
  - `pub(crate) fn compare_url(https: &str, branch: &str) -> String`: `"{https}/compare/{branch}?expand=1"`
  - `pub(crate) fn first_stderr_line(stderr: &str) -> String`: the first non-empty trimmed line, or "gh failed".
  - `ChatApp.remotes: HashMap<PathBuf, Vec<Remote>>` and `ChatApp.gh: Option<GhState>`: both loaded in the background when the popover opens; gh is checked once per app run.

Behaviour (spec §2 row 6, "Folders that aren't ready for git"):
- **Init row:** "Initialize git repository" runs `git_init`, then `refresh_git`.
- **Publish view:**
  - a name input defaulting to the project folder name
  - Private/Public radio cards (`role Radio`, `aria_selected`)
  - a primary Publish button (≥ 28 px)
  - Publish runs `gh` with `publish_args` in the background. On error it sets `workspace_pop.error = first_stderr_line(...)` and stays on the view. On success it reloads remotes and status, then goes back.
- **PublishBranch row:** "Publish branch" runs `run_git_in(GitOp::Push)`.
- **Remote view:**
  - Push ↑n, Pull ↓m and Fetch, enabled per `remote_ops`
  - Create pull request: `gh pr create --web` (cwd = top) when gh is `Ready`, else `cx.open_url(compare_url(https, branch))`
  - Open on GitHub: `cx.open_url(https)`
- The popover footer shows `workspace_pop.error` in danger text until the next action.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn gh_state_from_auth_status() {
    assert_eq!(gh_state(false, false), GhState::Missing);
    assert_eq!(gh_state(true, false), GhState::LoggedOut);
    assert_eq!(gh_state(true, true), GhState::Ready);
}
#[test] fn remote_row_states() { /* non-repo → Init; no remotes + no commit → Publish{blocked: Some("Make a first commit to publish")};
   no remotes + commit + LoggedOut → Publish{blocked: Some("Install GitHub CLI and run gh auth login")};
   remotes + upstream None → PublishBranch; upstream + (2,1) → Remote{ahead:2, behind:1, slug: Some("iris/ac")} */ }
#[test] fn publish_args_shape() { /* ("ac", true, /p) → [..., "--private", "--source", "/p", "--remote", "origin", "--push"] */ }
#[test] fn compare_url_builds_github_compare() { assert_eq!(compare_url("https://github.com/o/r", "feat/x"), "https://github.com/o/r/compare/feat/x?expand=1"); }
#[test] fn publish_error_keeps_first_stderr_line() { assert_eq!(first_stderr_line("\n  GraphQL: Name already exists\nmore"), "GraphQL: Name already exists"); assert_eq!(first_stderr_line(""), "gh failed"); }
```

- [ ] **Step 2: Run and confirm they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run and confirm they pass.** Then run the phase check: `cargo build && cargo test --workspace`.
- [ ] **Step 5: Checkpoint (end of Phase B).** Ask the user about committing.

---

## Phase C: scripts and worktree setup

### Task 8: Terminal command panes

**Files:**
- Modify: `crates/terminal/src/session.rs` (`Session::spawn_command`)
- Modify: `crates/terminal/src/view/mod.rs` (`TerminalView.command`, `hold`, exit footer, restart)
- Modify: `crates/terminal/src/workspace/mod.rs` (public run/kill API, held exit handling, event)
- Test: `crates/terminal/tests/unit/session.rs`, `crates/terminal/tests/unit/view/mod.rs` (or the existing view test file), `crates/terminal/tests/unit/workspace/model.rs`

**Interfaces:**
- Produces:
  - `pub(crate) fn command_args(command: &str) -> Vec<String>`: `vec!["-lc".into(), command.to_string()]`
  - `Session::spawn_command(folder: &Path, command: &str, size: WindowSize, palette: Palette) -> Result<Self, String>`: same as `spawn`, but with `command_args`.
  - `pub(crate) fn exit_label(code: Option<i32>, stopped: bool) -> String`: "Stopped", "Exited with code N", or "Exited" when the code is None.
  - On `TerminalView`:
    - `pub(crate) command: Option<String>`. When Some, `fit()` uses `spawn_command`.
    - `pub(crate) hold: bool`. When true, the view does not request close on exit, and renders a one-line footer with `exit_label`.
    - `pub(crate) key: Option<SharedString>`
  - `TerminalEvent::CommandEnded { key: SharedString, code: Option<i32> }` (new variant).
  - On `TerminalWorkspace`:
    - `pub fn run_command(&mut self, key: SharedString, title: SharedString, command: String, window: &mut Window, cx: &mut Context<Self>)`. It reuses the held group whose pane has the same `key` (restarting it fresh), else creates a new group with a held command pane titled `title`. Either way that group becomes active, and the workspace opens.
    - `pub fn stop_command(&mut self, key: &str, cx: &mut Context<Self>)`. It kills that pane's process; the pane stays, showing "Stopped".
    - `pub fn command_running(&self, key: &str, cx: &App) -> bool`
  - `WorkspaceModel::group_of_key` is not needed. Track `key → PaneId` in a `HashMap<SharedString, PaneId>` on `TerminalWorkspace`.
  - Emitting:
    - The workspace emits `CommandEnded` when a held pane exits, with `ExitStatus::code()`.
    - If the user closes a held pane while it runs, it emits `CommandEnded { code: None }` before shutdown.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn command_args_pass_command_verbatim() {
    let cmd = r#"echo 'a b' && FOO="$HOME" ls | wc -l"#;
    assert_eq!(command_args(cmd), vec!["-lc".to_string(), cmd.to_string()]);
}
#[test] fn exit_label_words() {
    assert_eq!(exit_label(Some(0), false), "Exited with code 0");
    assert_eq!(exit_label(Some(1), true), "Stopped");
    assert_eq!(exit_label(None, false), "Exited");
}
```

- [ ] **Step 2: Run and confirm they fail.** `cargo test -p aether-terminal command_args exit_label`.
- [ ] **Step 3: Implement.**
  - Kill on stop: send `SIGTERM` to the session pid (unix), else drop the session.
  - Restart for re-run: reset `session = None`, `status = Starting`, so the next `fit()` respawns.
- [ ] **Step 4: Run and confirm they pass**, along with the existing terminal tests.
- [ ] **Step 5: Checkpoint.**

### Task 9: Scripts in the popover

**Files:**
- Create: `crates/app/src/workspace_popover/scripts.rs` (rows, form view, run/stop)
- Modify: `crates/app/src/workspace_popover/model.rs`
- Modify: `crates/app/src/terminal.rs` (`drawer_for` and the `CommandEnded` subscription in `terminal_view`; `close_terminal` clears runs)
- Modify: `crates/app/src/app/mod.rs` (`ChatApp.script_runs`)
- Modify: `crates/app/src/assets.rs`, `crates/app/tests/unit/assets.rs` (`Play`, `Square`, `Ellipsis`, `Pencil`, `Trash2`)
- Test: `crates/app/tests/unit/workspace_popover/model.rs`

**Interfaces:**
- Consumes:
  - Task 1 `ProjectScript`, `project_key`
  - Task 8 `run_command`, `stop_command`, `CommandEnded`
- Produces:
  - `pub(crate) struct ScriptRuns(HashMap<(PathBuf, String), i64>)`, mapping (folder, script name) → the chat id whose drawer runs it. Methods:
    - `start(&mut self, folder, name, chat) -> bool` (false if already running)
    - `end(&mut self, folder, name)`
    - `running(&self, folder, name) -> Option<i64>`
    - `drop_chat(&mut self, chat: i64)`
  - `pub(crate) fn script_default_name(command: &str) -> String`: take the text before the first `&&`, `;` or `|`, drop words starting with `-`, and use the last remaining word. If nothing is left, use "script".
  - `pub(crate) fn drawer_for(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> Option<Entity<TerminalWorkspace>>`: like `terminal_view`, but for any chat index (its folder via `folder_of`).
  - `pub(crate) fn run_script(&mut self, ix: usize, script: &ProjectScript, window, cx)` and `pub(crate) fn stop_script(&mut self, folder: &Path, name: &str, cx)`.

Behaviour (spec §2 rows 3–4, "Project scripts"):
- **Script rows:** one per `project_scripts[project_key(project)]`. Each row has:
  - a play/stop `icon_chip`, labelled "Run dev" / "Stop dev"
  - the name, and the command in muted mono
  - an "on new worktree" chip for setup scripts
  - a hover ⋯ with Edit (`pop_view(Script(Some(i)))`) and Delete
- **Run state:** a row shows Stop while `script_runs.running(folder, name)` is Some, for any chat in that folder.
- **Running:** `run_script` calls `drawer_for(ix)`, then `run_command(key = name, title = name, command)`, records the run, and closes the popover. On `CommandEnded` the run ends.
- **Script form:**
  - Name, Command (mono) and a checkbox "Run automatically when a new worktree is created"
  - Save is disabled while Command is empty; Enter in either field saves
  - an empty Name becomes `script_default_name`
  - editing replaces the script at its index
- **Cleanup:** `close_terminal(chat_id)` and archive/delete both call `script_runs.drop_chat(chat_id)` after their existing shutdown, which kills the processes.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn script_default_name_skips_flags() {
    assert_eq!(script_default_name("pnpm dev"), "dev");
    assert_eq!(script_default_name("cargo run --release"), "run");
    assert_eq!(script_default_name("make && ./bin/serve -p 3"), "make");
    assert_eq!(script_default_name("   "), "script");
}
#[test] fn script_runs_one_per_folder_and_name() { /* start(/a,dev,1)=true; start(/a,dev,2)=false; running(/a,dev)=Some(1); start(/b,dev,2)=true */ }
#[test] fn runs_for_closed_chat_are_dropped() { /* start(/a,dev,1), start(/a,test,2); drop_chat(1) → running(/a,dev)=None, running(/a,test)=Some(2) */ }
```

- [ ] **Step 2: Run and confirm they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run and confirm they pass.** Check by hand: run `sleep 2; echo hi`; the row goes Stop → Run, and the tab shows "Exited with code 0".
- [ ] **Step 5: Checkpoint.**

### Task 10: Setup scripts gate a new worktree's first prompt

**Files:**
- Create: `crates/app/src/app/worktree/setup.rs` (SetupJob flow)
- Modify:
  - `crates/app/src/app/worktree/send.rs` (the Ok arm of `send_in_new_worktree`; the `send_in_existing_worktree` first send)
  - `crates/app/src/app/mod.rs` (`Chat.setup`, `Chat.fresh_worktree`)
  - `crates/app/src/git_panel/worktrees.rs` (`offer_tree_chat`: after its existing `open_worktree_chat` call, set `fresh_worktree = true` on the now-active chat; `open_worktree_chat`'s signature is unchanged)
  - `crates/app/src/app/composer/mod.rs` (the setup banner, next to the existing `WorktreeJob::Creating` UI at :172)
  - `crates/app/src/terminal.rs` (route `CommandEnded` to setup)
- Test: `crates/app/tests/unit/app/worktree/setup.rs`

**Interfaces:**
- Consumes: Task 9 `run_script`, `ScriptRuns`; Task 8 `CommandEnded`.
- Produces:
  - `pub(crate) struct SetupJob { pub(crate) queue: Vec<ProjectScript>, pub(crate) current: usize, pub(crate) text: String, pub(crate) failed: Option<Option<i32>> }`. `queue` holds the setup scripts in settings order; `text` is the held prompt.
  - `pub(crate) fn setup_queue(scripts: &[ProjectScript]) -> Vec<ProjectScript>`: the `on_new_worktree` scripts, order kept.
  - `pub(crate) enum SetupStep { Next(usize), Send, Failed(Option<i32>) }`
  - `pub(crate) fn after_exit(job: &SetupJob, code: Option<i32>) -> SetupStep`: `Some(0)` → `Next(current + 1)`, or `Send` when it was the last; anything else → `Failed(code)`.
  - `pub(crate) fn setup_banner(job: &SetupJob) -> String`:
    - while running: "Setting up worktree… (install)"
    - when failed: "Setup failed: install exited with code 1", or "Setup failed: install stopped" when the code is None
  - On `ChatApp`:
    - `pub(crate) fn begin_setup_or_submit(&mut self, ix: usize, text: String, window, cx)`
    - `pub(crate) fn setup_retry(&mut self, ix, window, cx)`
    - `pub(crate) fn setup_send_anyway(&mut self, ix, window, cx)`
    - `pub(crate) fn setup_cancel(&mut self, ix, window, cx)`

Behaviour (spec "Decided by the user"):
- **Starting:** in the Ok arm, replace `this.submit(ix, text, …)` with `begin_setup_or_submit`. With an empty queue it calls `submit` as before; otherwise it stores a `SetupJob` and runs `queue[0]`.
- **Existing-worktree drafts:** `send_in_existing_worktree` does the same only when `chat.fresh_worktree`, then clears the flag.
- **Composer banner:** shows `setup_banner`.
  - While running: a Cancel action. Cancel stops the script and restores `text` into the composer when the composer is empty.
  - When failed: Retry (rerun `queue[current]`), Send anyway (`submit` with `text`), and Show output (focus the drawer and that tab).
  - Sending is blocked while `chat.setup.is_some()`; extend `worktree_blocks_send`.
- **Lifetime:** the job survives chat switches. It is not persisted.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn setup_queue_keeps_only_on_new_worktree_in_order() { /* [a(setup), b, c(setup)] → [a, c] */ }
#[test] fn after_exit_advances_sends_or_fails() { /* queue len 2, current 0, Some(0) → Next(1); current 1, Some(0) → Send; Some(2) → Failed(Some(2)); None → Failed(None) */ }
#[test] fn setup_banner_texts() { /* running "install" → "Setting up worktree… (install)"; failed Some(1) → "Setup failed: install exited with code 1"; failed None → "Setup failed: install stopped" */ }
```

- [ ] **Step 2: Run and confirm they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run and confirm they pass.** Then run the phase check: `cargo build && cargo test --workspace`. Check by hand: a setup script `sleep 2 && true` delays the first prompt; `false` shows the failed banner and its actions.
- [ ] **Step 5: Checkpoint (end of Phase C).** Ask the user about committing.

---

## Phase D: sidebar entry and project page

### Task 11: Page::Project, entry points, project top bar

**Files:**
- Modify:
  - `crates/app/src/app/mod.rs` (`Page::Project`; `ChatApp.project_page: Option<PathBuf>`)
  - `crates/app/src/app/render.rs` (body dispatch; header for the project page)
  - `crates/app/src/app/composer/header.rs` (crumb click; `render_project_top_bar`)
- Modify:
  - `crates/ui/src/chat/sidebar/mod.rs` (`ProjectItem.on_page`; `ChatSidebar::project` gains `on_page`)
  - `crates/ui/src/chat/sidebar/rows.rs` and `project_row.rs` (pass it through)
  - `crates/ui/src/chat/sidebar/menu.rs` (`project_menu` gains `on_page: Option<RowHandler>`, rendered first, then a separator)
- Modify: `crates/app/src/app/sidebar.rs` (pass `open_project_page`)
- Test: `crates/app/tests/unit/app/mod.rs`, `crates/ui/tests/unit/chat/sidebar/rows.rs`

**Interfaces:**
- Produces:
  - `pub(crate) fn open_project_page(&mut self, folder: PathBuf, cx: &mut Context<Self>)`: sets `project_page`, calls `open_page(Page::Project, cx)`, and closes the workspace popover.
  - `fn render_project_top_bar(&self, cx) -> impl IntoElement`: the expand button while collapsed, then the project crumb only.
  - `ChatSidebar::project(header, on_collapse, on_new, on_files, on_hide, on_page)`: `on_page` is the new last parameter. Worktree rows pass None.

Behaviour (spec §3, §4 top):
- The project crumb becomes clickable (`cursor_pointer`, `Role::Link`, aria "Open project page for auto-clicker") and calls `open_project_page(project)`.
- The project row ⋯ menu starts with "Open project page" (`LayoutDashboard` icon), then a separator, then the existing items.
- `select(ix)` already returns to `Page::Chat`.
- The `Page::Project` body is a placeholder until Task 13.

- [ ] **Step 1: Write failing tests:** add a sidebar test that `project_menu` lists "Open project page" first when `on_page` is Some. Use the rows.rs test pattern; if PopupMenu items can't be inspected, test a pure `project_menu_labels(has_page, has_files, has_hide) -> Vec<&'static str>` extracted from `project_menu`.
- [ ] **Step 2: Run and confirm they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run and confirm they pass.** Check by hand: the crumb and the ⋯ item both open the placeholder page; clicking a chat returns.
- [ ] **Step 5: Checkpoint.**

### Task 12: Project page model (pure)

**Files:**
- Create: `crates/app/src/project_page/model.rs`, `crates/app/src/project_page/mod.rs` (declares model; render comes in Task 13)
- Modify: `crates/app/src/main.rs`; `crates/ui/src/chat/thinking.rs` (`live_label` → `pub`, re-exported)
- Test: `crates/app/tests/unit/project_page/model.rs`

**Interfaces:**
- Produces:
  - `pub(crate) enum Filter { All, Running, NeedsYou, Done }`
  - `pub(crate) enum GroupBy { None, Worktree, Status, Provider }` (default Worktree)
  - `pub(crate) enum SortBy { Activity, Newest, Name, Status }` (default Activity)
  - `pub(crate) enum CardStatus { Working, Waiting(&'static str), Failed, Done, Idle }`
  - `pub(crate) fn card_status(status: ChatStatus, last_failed: bool) -> CardStatus`:
    - `Running` → `Working`
    - `Waiting(l)` → `Waiting(l)`
    - `Failed` → `Failed`
    - `Done` → `Done`
    - `Idle` → `Failed` if `last_failed`, else `Idle`
  - `pub(crate) fn status_rank(s: CardStatus) -> u8`: Waiting 0, Failed 1, Working 2, Done 3, Idle 4.
  - `pub(crate) fn in_filter(f: Filter, s: CardStatus) -> bool`:
    - Running = Working
    - NeedsYou = Waiting or Failed
    - Done = Done or Idle
  - `pub(crate) struct Card`:
    - `ix: usize, id: i64, title: String`
    - `provider: Provider, model: String`
    - `status: CardStatus, since_ms: i64`
    - `place: String` (branch, or folder name)
    - `added: u32, removed: u32`
    - `activity_ms: i64, text: String`
  - `pub(crate) fn group_key(by: GroupBy, c: &Card) -> String`:
    - Worktree → `place`
    - Status → "Needs you" (Waiting), "Failed", "Running", "Done" (Done or Idle)
    - Provider → `provider.label()`
    - None → ""
  - `pub(crate) fn arrange(cards: Vec<Card>, f: Filter, g: GroupBy, s: SortBy) -> Vec<(String, Vec<Card>)>`:
    - Filters, then sorts. Ordering by `SortBy`:
      - Activity: `activity_ms` desc
      - Newest: `id` desc
      - Name: case-insensitive title
      - Status: `status_rank`, then activity
    - Groups in first-appearance order, except Status grouping, which orders groups by rank.
    - With `GroupBy::None`, returns a single group with an empty key.
  - `pub(crate) fn last_text(msg: Option<&Message>, status: CardStatus) -> String`:
    - Working → `live_label(&msg.steps)`, else "Working…"
    - Waiting → the pending request's title
    - Failed → the message error text, else "Failed"
    - otherwise → the first non-empty line of `msg.text`, truncated to 140 chars
  - `pub(crate) fn chat_edits(messages: &[Message], cwd: Option<&str>) -> (u32, u32)`: sums `aether_ui::reply_edits` over all messages' steps.
  - `pub(crate) fn card_aria(c: &Card, age: &str) -> String`: "Project Purpose, Working, feat/autoclicker, Claude Opus 5.5, 2m" (skips empty parts).
  - `pub(crate) enum Activity { Chat { ix: usize, title: String, verb: &'static str, at_ms: i64 }, Commit { short: String, subject: String, at_ms: i64 } }`
  - `pub(crate) fn recent(chats: Vec<Activity>, commits: Vec<Activity>, limit: usize) -> Vec<Activity>`: merges by `at_ms` desc, keeps at most 5 commits, then truncates to `limit` (8).
  - `pub(crate) fn verb(s: CardStatus) -> &'static str`: "finished", "failed", "needs approval" (Waiting labels map to "needs approval", "needs an answer" or "needs review"), "is working".

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn idle_with_failed_last_message_is_failed() { assert_eq!(card_status(ChatStatus::Idle, true), CardStatus::Failed); }
#[test] fn filters_partition_statuses() { /* NeedsYou: Waiting, Failed; Done: Done, Idle; Running: Working; All: all */ }
#[test] fn arrange_groups_status_by_rank() { /* cards Done, Waiting, Working → keys ["Needs you","Running","Done"] */ }
#[test] fn arrange_sorts_by_activity_then_groups_by_first_appearance() { /* ... */ }
#[test] fn arrange_none_is_single_unkeyed_group() { /* ... */ }
#[test] fn sort_by_name_is_case_insensitive() { /* "b","A","c" → A,b,c */ }
#[test] fn recent_merges_and_caps_commits() { /* 6 commits + 4 chats interleaved; limit 8 → 8 items, ≤5 commits, desc by time */ }
#[test] fn card_aria_skips_empty_parts() { /* model "" → no double commas */ }
#[test] fn last_text_uses_first_nonempty_line() { /* "\n\nDone. All good\nmore" → "Done. All good" */ }
```

- [ ] **Step 2: Run and confirm they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run and confirm they pass.**
- [ ] **Step 5: Checkpoint.**

### Task 13: Project page render and data

**Files:**
- Create: `crates/app/src/project_page/view.rs` (render), `crates/app/src/project_page/data.rs` (loading)
- Modify: `crates/app/src/project_page/mod.rs`; `crates/app/src/app/mod.rs` (`ChatApp.project_view: ProjectPageState`); `crates/app/src/app/render.rs` (body)
- Modify: `crates/app/src/assets.rs`, `crates/app/tests/unit/assets.rs` (`LayoutDashboard`, `House`, `GitBranch`, `Plus`, `CircleAlert`)
- Test: `crates/app/tests/unit/project_page/model.rs` (aria helpers added here if any)

**Interfaces:**
- Consumes:
  - Task 12 (all)
  - Task 1 `project_app`
  - Task 4 `line_totals`, `changes_label`, `home_short`
  - Task 5 `git_remotes`, `owner_repo`
  - `agent.repo_worktrees`, `agent.git_status`, `aether_git::history` (background)
  - `projects::{group_projects, relative_time, busy_label}`
  - `new_chat_in`, `open_worktree_chat(path, false, …)`
- Produces:
  - `pub(crate) struct ProjectPageState { filter: Filter, group: GroupBy, sort: SortBy, data: Option<ProjectData> }` (the first three default to `All` / `Worktree` / `Activity`)
  - `pub(crate) struct ProjectData { folder: PathBuf, status: Option<Status>, repo: bool, worktrees: Vec<Listed>, remotes: Vec<Remote>, commits: Vec<Commit> }`
  - `pub(crate) fn load_project_data(&mut self, folder: PathBuf, cx)`: called by `open_project_page`, the 30 s branch tick, and `send.rs:320`'s after-reply refresh while `page == Project`.

Behaviour (spec §4 body):
- Centred column, max width 980, scrolling.
- **Header:**
  - a 44 px `project_mark_sized`
  - the name, 20 px semibold
  - the facts row (`text_xs`, muted, icons 12 px): path, branch · N worktrees, `changes_label` + " uncommitted", and publish state (`owner_repo` or "not published"). A non-repo shows "not a git repository"; a missing folder shows "Folder not found".
  - right side: the "Open in" split (same chips as the popover; the caret opens a `ChoiceMenu` that writes `project_open_in`) and a primary "New agent" button
- **Filter tabs:** `Role::Tab` inside `Role::TabList` with `aria_selected` and counts.
- **Group and Sort:** `DropdownMenu`s with checked items.
- **Cards:**
  - a grid of `flex_wrap` rows with min width 290
  - each card is `Role::Button` with `card_aria`, `tab_index(0)`, `focus_visible` ring, radius 12
  - click or Enter → `select(ix)`
  - badges:
    - Working → info tint, `busy_label("Working", since)`
    - Waiting → mango, with its label
    - Failed → danger
    - Done → success
    - Idle → no badge, the age in the meta row instead
- **Empty state:** "No agents here." in a dashed-border box.
- **Worktrees:** rows with `House` or `GitBranch` icon, branch, `home_short(path)`, and badges for agents / running / waiting counted from cards by cwd. A hover "+" calls `new_chat_in(folder)` for main and `open_worktree_chat(path, false)` for linked. Missing worktrees show "missing" and no "+".
- **Recent activity:** `recent(...)`. Each row shows the verb sentence and `relative_time`; chat rows are clickable.

- [ ] **Step 1: Write failing tests** for any new pure helper introduced here (e.g. `facts_line(...)` if extracted); add the icons to a `serves_project_page_icons` asset test.
- [ ] **Step 2: Run and confirm they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run and confirm they pass.** Then run the phase check: `cargo build && cargo test --workspace`. Check by hand against `shell-v5.html`: filters, group, sort, card click, worktree "+", empty state, non-repo project.
- [ ] **Step 5: Checkpoint (end of Phase D).** Ask the user about committing; update `features.md` only if the user asks.
