# Sidebar with Git + Worktree Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Sidebar shows git status per chat, groups chats under nested worktree containers, and creates/removes worktrees opt-in.

**Architecture:** Extend existing `ChatSidebar` rows + `render_sidebar` grouping with a per-folder git cache; reuse `git::status`, `worktree::create/check_remove/remove`, `GitPanel` drawer for actions.

**Tech Stack:** Rust 2024 workspace, GPUI Kit, `aether-agent` git/worktree, `aether-ui` sidebar, `app` ChatApp.

**Spec:** `docs/superpowers/specs/2026-10-15-sidebar-git-worktree-design.md`

## Global Constraints

- Blocking git/worktree calls stay off UI thread (agent blocking pool + `cx.spawn`), panel paints last result.
- Sidebar reuses per-folder status cache: refresh active plus visible rows throttled and after git ops, dedup by folder so N chats in one worktree is 1 git call.
- Keep keyboard: Up/Down/Home/End, F2 rename, Esc closes, focus ring without layout shift.
- Keep a11y labels: row aria includes title + status + place + time + removed.
- Shortest diff: extend row second line and header, add worktree-container rows, add composer toggle + remove-with-branch option. No new deps.
- Branch names past 22 chars keep both ends `feature/…login-fix` via `middle_truncate`.
- Never `prune` other worktree records; missing slots only drop own record.

## Review Focus

- Detached HEAD shows short SHA with detached icon, not empty branch.
- Unborn branch (no commits) shows branch name + "no commits yet", push hidden unless remote exists.
- Missing worktree folder dims row, says removed, reveal disabled, cannot run.
- 100 worktrees sharing one repo folder still issues 1 git status per folder per refresh, not N.
- Rename collision `aether/slug` already taken retries once then errors without stray folder.

---

### Task 1: Per-folder sidebar git cache

**Files:**
- Modify: `crates/app/src/app.rs:2544-2600` (`refresh_branches`, add `sidebar_git` cache)
- Modify: `crates/app/src/git_panel.rs:337-386` (reuse `statuses: HashMap<PathBuf, Loaded>`)
- Test: `crates/app/src/projects.rs` tests + new `sidebar_git` unit test in `app.rs`

**Interfaces:**
- Consumes: `Agent::git_status(folder: PathBuf) -> Task<Result<Status, StatusError>>`, `Status { changes, branch, upstream, ahead_behind, commit }`
- Produces: `fn sidebar_git_summary(folder: &Path) -> Option<(usize changed, Option<(u32,u32)> ahead_behind)>` used by Task 2.

- [ ] **Step 1: Write failing test for dedup + counts**

```rust
#[test]
fn sidebar_git_dedups_by_folder_and_counts_unique_paths() { }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p aether-app sidebar_git -- --nocapture`
Expected: FAIL with function not defined

- [ ] **Step 3: Implement `sidebar_git_summary` + `refresh_sidebar_git(folders)` in `crates/app/src/app.rs`**

Dedup folders, reuse `self.git.statuses`, throttle to visible + active, call `refresh_git(false)` after ops.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether-app sidebar_git`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/app.rs
git commit -m "feat(sidebar): cache per-folder git status for rows"
```

### Task 2: Sidebar rows show git + worktree containers

**Files:**
- Modify: `crates/ui/src/chat/sidebar.rs:145-192` (`RowPlace`, `ChatLine`), `ProjectHeader`, `ChatSidebar::project` + new `worktree` container rows
- Modify: `crates/app/src/app.rs:2326-2436` (`render_sidebar`), `2475-2502` (`chat_line`)
- Modify: `crates/app/src/projects.rs:18-32` (group by worktree container within project)
- Test: `crates/app/src/projects.rs` grouping test, `crates/ui` sidebar truncate test

**Interfaces:**
- Consumes: `sidebar_git_summary` from Task 1, `BranchInfo { branch, detached, worktree }`, `Chat { worktree_path, worktree_branch }`
- Produces: `ChatLine { place, time, path, removed, changed: usize, ahead_behind }` shape for renderer; container key `worktree:<path>`.

- [ ] **Step 1: Write failing grouping test + detached/unborn row cases**

```rust
#[test]
fn groups_worktree_containers_under_project_by_canonical_prefix() { }

#[test]
fn detached_shows_short_sha_and_unborn_shows_no_commits_yet() { }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p aether-app groups_worktree`
Expected: FAIL

- [ ] **Step 3: Implement grouping + `chat_line` with counts**

Canonicalized prefix match, main chats at root, empty worktrees hidden, sort containers by newest chat, collapsed project/worktree still leaks active chat, reuse `middle_truncate`, `changed_files`, `push_state` hide rules.

- [ ] **Step 4: Run tests**

Run: `cargo test -p aether-app -p aether-ui`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/app.rs crates/app/src/projects.rs crates/ui/src/chat/sidebar.rs
git commit -m "feat(sidebar): nest worktree containers with git counts"
```

### Task 3: Click status opens drawer, removed stays safe

**Files:**
- Modify: `crates/app/src/app.rs:2441-2470` (`chat_row` on_click), `crates/app/src/git_panel.rs:510-527` (`toggle_changes`)
- Test: manual via `cargo test -p aether-app chat_row` + existing `worktree_removed` path

**Interfaces:**
- Consumes: container keys from Task 2
- Produces: click behavior: select chat + open Changes for that chat's folder.

- [ ] **Step 1: Write failing test for removed guard**

```rust
#[test]
fn removed_worktree_row_disables_reveal_and_run() { }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p aether-app removed_worktree`
Expected: FAIL

- [ ] **Step 3: Implement `select(ix)` then `toggle_changes(false)` on status click; keep dim + `removed` + reveal disabled when `worktree_removed() || worktree_missing(ix)`**

- [ ] **Step 4: Run tests**

Run: `cargo test -p aether-app`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/app.rs crates/app/src/git_panel.rs
git commit -m "feat(sidebar): open changes drawer from git status"
```

### Task 4: Composer toggle new-or-existing worktree

**Files:**
- Modify: `crates/app/src/app.rs:2657-2710` (`choose_worktree`, `send_in_new_worktree`, `active_bases`, `load_bases`)
- Modify: `crates/app/src/palette.rs:244-270` (picker items)
- Modify: composer area near `crates/app/src/app.rs:3059` work-local toggle
- Test: `crates/agent/src/worktree.rs` existing round-trip; new app test for `new_worktree` state machine

**Interfaces:**
- Consumes: `Bases { branches, default }`, `worktree::create(root, folder, prompt, base, cancel)`
- Produces: `chat.new_worktree: Option<String>` + `chat.new_worktree_existing: Option<PathBuf>` (or reuse path) read by send path.

- [ ] **Step 1: Write failing state test**

```rust
#[test]
fn composer_toggle_arms_create_on_send_with_existing_picker() { }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p aether-app composer_worktree`
Expected: FAIL

- [ ] **Step 3: Implement composer chip opening an alert dialog with base branch + name inputs (not inline field) plus existing worktree picker; create runs on first send, error to `worktree_error` above composer, cancel safe, slug never reused. Surface worktree settings in Settings at workspace level.**

- [ ] **Step 4: Run tests**

Run: `cargo test -p aether-app -p aether-agent worktree`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/app.rs crates/app/src/palette.rs
git commit -m "feat(worktree): composer toggle new or existing"
```

### Task 5: Remove worktree with optional branch delete

**Files:**
- Modify: `crates/app/src/app.rs:2749-2880` (`remove_worktree`, `check_remove` dialog)
- Modify: `crates/agent/src/worktree.rs:175-238` (`check_remove`, `remove` — add branch delete helper, reuse `managed`)
- Test: `crates/agent/src/worktree.rs` existing `worktrees_round_trip_in_a_temp_repo`

**Interfaces:**
- Consumes: `RemoveCheck { missing, changes, branch, unmerged }`, `RemoveError::NeedsForce`
- Produces: `fn remove_worktree_with_branch(path, project, force, delete_branch: bool)` used by dialog.

- [ ] **Step 1: Write failing test for branch-delete flag**

```rust
#[test]
fn remove_offers_branch_delete_but_keeps_branch_by_default() { }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p aether-agent remove_offers`
Expected: FAIL

- [ ] **Step 3: Implement checkbox in remove dialog (default keep branch), run `worktree::remove` then optional `git branch -d` only on success, keep chats dimmed removed, never prune others**

- [ ] **Step 4: Run tests**

Run: `cargo test -p aether-agent -p aether-app`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/agent/src/worktree.rs crates/app/src/app.rs
git commit -m "feat(worktree): remove with optional branch delete"
```
