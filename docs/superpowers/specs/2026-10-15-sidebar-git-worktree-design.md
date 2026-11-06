# Sidebar with Git + Worktree + Agent-in-Worktree — Design

Date: 2026-10-15
Status: draft for review

## Intent and why

Agent chats already run in git repos and in linked worktrees (`crates/agent/src/worktree.rs`, `crates/agent/src/git.rs`), but the UI splits that truth across three places: sidebar rows show branch/worktree as a second line (`crates/ui/src/chat/sidebar.rs` `RowPlace`, `ChatLine`), git stage/commit/push lives in a right drawer for the active chat only (`crates/app/src/git_panel.rs` `GitPanel`), and outside agents live on another page (`crates/app/src/agents_view.rs`).

Users lose track of which chat is dirty, ahead, or in a gone worktree. Goal is one sidebar that tells that story at a glance and lets a worktree own its chats, without losing the drawer that already works.

## Goals

- Sidebar rows surface git: branch/worktree/detached name (middle-truncated past 22 chars, full in tooltip), changed-file count, ahead/behind chips, removed-worktree dim + "removed" note.
- Project headers roll up: count + highest-priority status while collapsed (`Waiting > Running > Failed > Done > Idle` per `crates/app/src/projects.rs::rollup`), same as today.
- Worktree is a nested container under its Project group: guide line, folder icon, holds chats whose canonicalized `cwd` has the worktree path as prefix. Main-checkout chats stay at project root. Empty worktrees hidden. Sort containers by newest chat inside. Collapsed project and collapsed worktree both still show the active chat (builder call, keeps selection never vanishes).
- Click on sidebar git status selects that chat and opens its Changes view (current `toggle_changes` path, same chat switch + drawer context). Status itself is display-only at first.
- New worktree is opt-in: composer toggle arms it (offers New plus existing worktree picker for sharing), create runs on first send from picked base, failure shows above composer until dismissed, cancel leaves no stray branch reuse. Plus New-from-sidebar-project-header and chat-menu entry. Many chats may share one worktree. Default stays run-in-place.
- Create uses existing `worktree::create`: `aether/<slug>` from local base, never reuse branch/folder, slug from prompt, `MAX_SLUG` 48. Show `missing_subdir` note when base lacks subfolder.
- Remove offers folder delete plus optional branch delete (branch stays today; this adds checkbox). Keep safety: `check_remove` for changes/unmerged/locked/missing, `NeedsForce` wording, missing slots only drop git record, never `prune` others. Chats in a removed worktree stay, dimmed with "removed" note, reveal disabled, cannot run there (existing `worktree_removed()` behavior).
- Right Changes drawer stays for now; merge into sidebar is direction. First cut keeps stage/unstage, diff, commit with per-folder drafts, push. Slim later once rows prove enough. Counts reuse drawer helpers: unique-path `changed_files`, `push_state` hide rules (hidden with no commit or no remote, blocked text when detached), branch header branch vs detached-SHA vs unborn "no commits yet".

## Non-goals

- No auto-delete of worktrees when last chat archives. Manual only + prompt-when-empty is future.
- No terminal-in-sidebar, no multi-select bulk git ops.
- No change to driver/spawn, provider CLIs, transcript/store format.
- No new git binary handling; reuse `git::run` timeouts, `index.lock` retry, `safe_path` checks.

## Anti-goals

- Fail if sidebar becomes a second git panel with stage buttons on every row.
- Fail if 1:1 agent==worktree forced; sharing must keep working.
- Fail if collapsed groups hide the open chat or list jumps to top on reorder (keep `splice_range` + anchor logic).

## Constraints

- Rust + GPUI Kit, existing crates: `aether-core`, `aether-agent`, `aether-ui`, `app`. Reuse `ChatSidebar`, `ProjectHeader`, `ChatLine`, `RowPlace`, `GitPanel`, `group_projects`, `disambiguate`.
- Blocking git/worktree calls stay off UI thread (agent blocking pool + `cx.spawn`), panel paints last result. Sidebar reuses per-folder status cache: refresh active plus visible rows throttled and after git ops, dedup by folder so N chats in one worktree is 1 git call.
- Keep keyboard: Up/Down/Home/End, F2 rename, Esc closes, focus ring without layout shift.
- Keep a11y labels: row aria includes title + status + place + time + removed.
- Shortest diff: extend row second line and header, add worktree-container rows, add composer toggle + remove-with-branch option. No new deps.

## Decided how (from conversation)

- Sidebar git density: all — inline status + project rollup, tooltips for full path.
- Worktree shape: nested container, not row badge only, not top-level group.
- Agent binding: opt-in isolate, shared pool allowed.
- Sidebar click: open drawer for that chat.
- Worktree removal: delete folder + optional delete branch.
- Start flow: composer chip opens an alert dialog with base branch + name inputs (not an inline field), before first send; sidebar entry stays.
- Worktree settings are workspace-level, edited in Settings rather than per chat.
- Panel fate: merge into sidebar over time, keep drawer short term.

## Left to builder

- Exact second-line layout when branch + counts + time collide at 255px width; truncate order builder's call.
- Whether worktree container header reuses `ProjectRow` or new `WorktreeRow`; keep guide alignment under folder icon either way.
- Composer entry is a chip (not an inline field) that opens an alert dialog with base branch and name inputs; create still runs on first send.
- Worktree settings (base branch defaults, naming, cleanup) live in Settings at workspace level, rather than per chat.
- Remove dialog copy for branch-delete option; keep "Chats and files are kept" note pattern.
- Migration: chats with `worktree_removed()` already dim; no data migration.
