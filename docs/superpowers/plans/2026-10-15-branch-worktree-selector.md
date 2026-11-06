# Branch and worktree selector implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Replace the mixed composer menu with explicit branch switching and worktree selection.

**Architecture:** A dedicated selector owns tabs, search, focus, and typed selection events. ChatApp dispatches those events to the existing worktree flow or a background Git switch operation. Existing base-branch dialog edits remain intact.

**Tech Stack:** Rust, GPUI Kit, existing Git subprocess runner and blocking executor.

**Spec:** `docs/superpowers/specs/2026-10-15-branch-worktree-selector-design.md`

## Global constraints

- No new dependencies.
- Never force checkout, discard changes, or silently stash files.
- Preserve existing uncommitted work, including the base-branch selector and worktree creation dialog.
- Git calls run off the UI thread.
- Keep first-send worktree creation and sharing semantics.

## Review focus

- Dirty checkout: refused switch leaves files and branch unchanged.
- Branch checked out elsewhere: report refusal and retain draft selection.
- Search with no results: navigation and confirmation do nothing safely.
- Active chat changes during background work: completion updates the initiating chat only.
- Worktrees without chats and detached worktrees: list by Git records with truthful labels.

### Task 1: Safe branch switching

**Files:** `crates/agent/src/git.rs`, `crates/agent/src/agent.rs`

**Interfaces:** Add `git::switch_branch(cwd: &Path, branch: &str) -> Result<(), String>` and an Agent async wrapper accepting owned `PathBuf` and `String`, using the existing blocking executor.

- [ ] Add repository-backed tests for switching an existing local branch, conflicting dirty files, a branch checked out in another worktree, and invalid branch input. Assert HEAD and file contents remain unchanged on refusal.
- [ ] Run the focused tests and confirm they fail before implementation.
- [ ] Validate the target as a local branch and execute non-forced `git switch` through the existing runner. Return existing Git error formatting.
- [ ] Run agent tests and confirm they pass.

### Task 2: Selector and application integration

**Files:** Create `crates/ui/src/components/workspace_select.rs`; modify `crates/ui/src/components/mod.rs`, `crates/app/src/app.rs`. Extend `crates/agent/src/worktree.rs` only if existing listing excludes ordinary Git worktrees.

**Interfaces:** `WorkspaceSelectEvent::{SwitchBranch(String), NewWorktree(String), UseWorktree(PathBuf), Local}`. Selector state accepts branch rows, worktree rows, current selection, and disabled state. ChatApp consumes typed events instead of reconstructing row indexes.

- [ ] Add filtering and navigation tests covering empty results, branch/path searches, tab changes, and selected state independent of highlight. Add a listing regression for worktrees without chats and detached worktrees.
- [ ] Run focused tests and confirm failure before implementation.
- [ ] Build searchable Branches and Worktrees tabs with explicit branch action buttons. Reuse popup anchoring, input, theme, and focus patterns. Leave the existing base-branch selector untouched.
- [ ] Replace only the composer worktree menu integration. Reuse the creation dialog, first-send binding, and composer error display. Guard background completion by initiating chat ID and block draft edits/send while switching. Refresh branch and Git caches after switching.
- [ ] Run `cargo test -p aether-agent -p aether-ui` and `cargo check --workspace`. Confirm success.
- [ ] Manually verify keyboard search, tabs, both branch actions, worktree selection, error display, popup dismissal, and trigger labels. Record any unavailable GUI check honestly.
- [ ] Review the final diff against the initial dirty state. Commit only task-owned changes, without staging unrelated user edits.

### Follow-up: Explicit branch-switch confirmation

- [x] Reuse the existing GPUI alert for `Switch here`, naming the target branch and warning that other chats and tools share checkout changes. Do not start Git, set a switch job, or alter pending workspace selection on cancel or dismissal.
- [x] Capture the initiating chat ID and resolve it again on affirmative confirmation. Recheck existing safe-switch guards before starting Git; never redirect to a different active chat.
- [x] Add a focused confirmation-wiring regression, observe failure before implementation and success afterwards, then run `cargo test --workspace` and `cargo check --workspace`.
- [x] Record evidence and GUI verification limits in `.superpowers/sdd/2026-10-15-branch-worktree-selector/confirmation-report.md`.
