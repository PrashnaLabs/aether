# Agent orchestration, phase 4: worktree per child, implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `spawn_agent{worktree: true}` gives the child its own git branch and folder off the lead's current branch. Parallel children then never edit the same checkout.

**Architecture:** Reuse the composer's existing first-send path, `ChatApp::send_in_new_worktree(ix, base, prompt, text, attachments, window, cx)` in `crates/app/src/app/worktree/send.rs`. It creates the worktree off the UI thread, binds the chat's `cwd`, `project`, `worktree_path` and `worktree_branch`, runs setup scripts, then submits. `spawn_agent` creates the child as it does now. When `worktree` is true, it hands the child to that path instead of calling `submit` directly. The running-children cap (phase 4, part 1) has already landed.

**Tech Stack:** Rust 2024, gpui, aether_git worktrees. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-04-agent-orchestration-design.md` (phase 4; Tools table `worktree?`)

## Global Constraints

- No new crates.
- `worktree` defaults to false, and false keeps today's behavior exactly.
- The base is the lead's current branch. Uncommitted changes in the lead's folder are not carried over, and the tool description says so.
- Spawning never steals focus and never touches the lead's composer. That includes the composer text, the worktree name input and sent-copy state. Any composer side effect in `send_in_new_worktree` must apply only when `ix == self.active`. Gate or parameterize it minimally.
- A worktree that fails to create, or a lead folder that isn't a git repo, must reach the lead. A folder that isn't a repo is caught at spawn time and returned as an `Err` from `spawn_agent`, with no child created. A creation failure after spawn is delivered to the lead as an `[Aether]` delivery with outcome `failed` and the error as the body, through the existing hold/submit delivery rules (`deliver_to_lead` / `enqueue_delivery`). The child chat keeps its `worktree_error`.
- Merging back is not automated. The lead merges with git itself.
- Tests live in `crates/<crate>/tests/unit/...` via `#[cfg(test)] #[path]`. No test reaches `submit` on a CLI chat. No test creates real worktrees in the user's `~/.aether/worktrees`. Use a temp repo, or test the pure decision parts.
- No comments unless genuinely non-obvious. `cargo fmt --check` is red on main, so don't reformat unrelated code. Known flake: `--skip hackernews::client`. Disk space is tight, so don't build extra profiles.
- Commits: Conventional Commits, no `Co-Authored-By` or AI attribution.

## Review Focus

1. **The lead's folder isn't a git repo.** Expected: `Err("the lead's folder isn't a git repository; spawn without worktree")`, and no chat is created. Test in Task 1.
2. **Worktree creation fails after spawn** (git error, cancel). Expected: one `failed` delivery to the lead, the child shows its `worktree_error`, and nothing ends up in the lead's composer. Test in Task 1.
3. **The lead is itself in a worktree.** Expected: the child branches off the lead's worktree branch, not the main checkout's branch. Test in Task 1.
4. **Two worktree children spawned back to back.** Expected: distinct branch names and folders (the existing `aether/<slug>` naming plus dedupe). Test in Task 1, or note in the report if naming already guarantees it.
5. **Crew strip and sidebar for worktree children.** Expected: they still nest and show under the lead. A child's `project` should equal the lead's project folder so grouping holds. Test in Task 1.

---

### Task 1: `spawn_agent{worktree: true}`

**Files:**
- `crates/agent/src/orchestrator/mod.rs` (`tool_specs`: add `worktree: boolean` to `spawn_agent`, with a terse description: "own git branch + folder off the lead's current branch; lead's uncommitted changes aren't included")
- `crates/app/src/app/orchestrate.rs` (`SpawnArgs.worktree: bool`, `spawn_agent`)
- `crates/app/src/app/worktree/send.rs` (gate composer side effects on `ix == self.active`; route a failure for a child to the lead)
- tests in `crates/app/tests/unit/app/orchestrate.rs` and wherever worktree send tests live

**Interfaces:**
- `SpawnArgs::parse` reads `worktree` as a bool, default false. A non-bool value gives `Err(r#""worktree" must be true or false"#)`.
- `spawn_agent` with `worktree`:
  - Resolve the lead's folder. Use `chat_cwd`; when the lead has a `worktree_path`, that is its cwd.
  - Check the folder is a git repo with the existing aether_git helper. If it isn't, return the Review Focus 1 error before `create_child`.
  - Base is the lead's current branch: the lead's `worktree_branch` if set, otherwise the folder's current branch via the existing git helper that `load_bases` / `worktree_bases` uses to pick the default.
  - After `create_child`, `persist`, then `send_in_new_worktree(child_ix, base, prompt, child_prompt(&task), vec![], window, cx)` instead of `submit`. For `prompt`, which names the worktree, use the child's title.
  - Reply `{"id","title","provider","model","worktree":"creating"}`.
- On creation failure for a chat with a live lead parent, call `self.deliver_failure_to_lead(child_ix, &error)`. Add it next to `deliver_to_lead`. It builds `[Aether] Agent {id} "{title}" ({provider}/{model}) failed.\n\nCouldn't create its worktree: {error}`, reusing `delivery_text`'s header logic if it fits, and follows the same hold/submit rules.

- [ ] **Step 1: Tests**
  - `spawn_worktree_rejects_non_git_folder`: a lead cwd in a temp dir with no `.git` gives the exact Err, and `chats.len()` is unchanged.
  - `parse_reads_worktree_flag`: absent gives false, `true` gives true, `"yes"` gives Err.
  - `worktree_base_is_leads_branch`: test the pure base-choosing helper. A lead with a `worktree_branch` gives that branch. Otherwise it gives the folder's current branch, checked against a temp git repo created with `git init` plus a commit on a named branch, if that's cheap. If it isn't cheap, inject the branch lookup.
  - `worktree_failure_is_delivered_to_lead`: call the failure handler directly with a busy lead (`Cancel::inert()`). It produces one queued `from_children` item containing `failed` and the error. The lead's composer text is unchanged.
  - `child_send_in_worktree_leaves_lead_composer_alone`: as far as the harness allows, show that composer side effects are skipped when `ix != active`.
  - Keep worktree children grouped with the lead: `project` is set to the lead's project folder, as `send_in_new_worktree` does. Add an assertion if the existing code already does this.
- [ ] **Step 2:** RED.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** GREEN for `cargo test -p aether-agent` and `cargo test -p aether -- --skip hackernews::client`.
- [ ] **Step 5: Commit** `feat(app): let leads spawn child agents in their own worktree`.

### Task 2: Desktop check (user)

- [ ] A lead in a git repo spawns two Codex children with `worktree: true`. Each gets its own `aether/<slug>` branch and folder, and both nest under the lead. Edits in one don't show up in the other. The lead can merge them with git.
- [ ] A lead in a folder that isn't a git repo, asking for a worktree child, gets a clear error.
