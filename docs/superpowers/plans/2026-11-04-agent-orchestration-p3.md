# Agent orchestration, phase 3: UI, implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make leads and children visible and one click apart:
- a Lead chip in the composer
- a crew strip of child chips above a lead's composer
- children nested under their lead in the sidebar
- a "Delegated by" link in a child's header
- spawn tool rows that open the child
- transcript rows for the native lead's tool calls

**Architecture:** This is UI only, on data that already exists. `Chat.lead`, `Chat.parent`, `Chat::status(now)`, `toggle_lead`, `select` and the workspace pane model are all in place. Each piece copies the nearest existing pattern; the file:line references below come from a code map made on 2026-11-04.

**Tech Stack:** Rust 2024, gpui via gpui-kit, crates/app + crates/ui. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-04-agent-orchestration-design.md` (UI section)

## Global Constraints

- No new crates. No settings flag; leads are already opt-in per chat.
- Children never become leads, so every lead control is hidden when `chat.parent.is_some()`.
- Every click handler uses `self.session_listener(cx, ..)`, the pattern used around it.
- Reuse existing components (`control_chip`/`resting_chip`, `provider_icon`, `ChatStatus` badge, `QueueChip` look, `OnOpen` builders). Add no new component when an existing one fits with a small `pub` change.
- Spawning or delivering never steals focus (unchanged). Clicking a child chip or link is an explicit user action and may focus the child.
- Tests live in `crates/<crate>/tests/unit/...` and are included with `#[cfg(test)] #[path]`. UI tests use the existing `VisualTestContext` / `window.draw(cx)` harness (`crates/app/tests/unit/app/composer/mod.rs:57-80`) and assert on state or element presence. No test reaches `submit` on a CLI chat.
- No comments unless the code is genuinely non-obvious.
- Baseline: `cargo fmt --check` is red on main; `hackernews::client` is flaky (`--skip hackernews::client`).
- Commits: Conventional Commits, no `Co-Authored-By` or AI attribution.

## Review Focus

1. **A lead with zero children.** Expected: no strip and no empty header gap, and the Lead chip still shows. Test in Task 2.
2. **A child whose lead was deleted, or no longer has lead set.** Expected: the "Delegated by" link is hidden, and the sidebar falls back to a normal un-nested row. Tests in Tasks 3 and 4.
3. **A child in a different project group than its lead** (another cwd). Expected: it stays in its own group, un-nested. Nesting happens only inside the same group. Test in Task 3.
4. **A spawn row whose output isn't JSON with an `id`, or names a deleted chat.** Expected: no link, and no panic. Test in Task 5.
5. **Ten or more children.** Expected: the strip wraps or scrolls and doesn't push the composer off-screen. The cap is 8 running, but there's no cap on total children. Test in Task 2 (with 12 children it renders without panic, at bounded height).

---

### Task 1: Lead chip in the composer

**Files:** `crates/app/src/app/composer/mod.rs` (control row ~409-442), test in `crates/app/tests/unit/app/composer/`.

**Interfaces:** Consumes `toggle_lead(ix, window, cx)` (`chat/manage.rs:155`) and the Search/Plan chip pattern: `resting_chip(control_chip(id, active, cx), resting).icon(..).label(..).on_click(self.session_listener(cx, ..))`.
- Add a `Lead` chip after Plan, with `active = chat.lead`, `IconName::Users`, or the closest existing icon if Users is missing.
- Show it only when `parent.is_none()` and the provider is lead-capable. Reuse `lead_capable` from `palette.rs`, moving it somewhere shared if it is private.
- Clicking calls `toggle_lead`, so the Antigravity confirm still runs. Leave the palette command in place.

- [ ] **Step 1:** Write the tests.
  - `lead_chip_shows_for_top_level_chats_only`: a draw with a top-level Claude chat shows the chip, and a child chat shows none. Assert through element-id presence (`debug_bounds("lead-chip")`, or however the harness queries ids) or through a pure `fn shows_lead_chip(&Chat) -> bool`.
  - `lead_chip_toggles_lead`.
- [ ] **Step 2:** Run them; they should fail (RED).
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p aether -- --skip hackernews::client`; it should pass (GREEN).
- [ ] **Step 5:** Commit `feat(app): add lead chip to the composer`.

### Task 2: Crew strip above a lead's composer

**Files:** `crates/app/src/app/composer/mod.rs` (banner block ~443-478), `crates/ui/src/chat/sidebar/mod.rs:122` (make `ChatStatus::badge` `pub` if needed), plus an optional small `crew_chip` in `crates/ui/src/chat/composer.rs` next to `QueueChip`, tests.

**Interfaces:**
- `crew: Option<AnyElement>` is built when `chat.lead` and some chat has `parent == Some(chat.id)`. It goes into the existing `.when(any banner, ..)` header right after `.children(plan)`, and the `.when` condition must include it.
- Each chip is `provider_icon(child.provider)`, then the title truncated to about 24 chars, then `child.status(now).badge(..)`.
- Children are ordered by id, oldest first.
- Clicking opens the child. Reuse whatever the sidebar uses to open a chat in a pane: use `workspace.model.open(child.id, window_id)` (`session_workspace/model.rs:352`) when the current layout has panes, and `select(ix, ..)` otherwise. Find the function the sidebar's "open in new pane" or middle-click path calls and use that; if no such path exists, use `select`.
- The strip is a wrapping `h_flex` with gap, capped at two rows of height, with overflow scroll.

- [ ] **Step 1:** Write the tests.
  - `crew_strip_lists_own_children_in_order`: a pure `fn crew(&self, lead_ix) -> Vec<usize>` returns the right ixs, and excludes another lead's children.
  - `no_crew_strip_without_children`: a lead with zero children draws with no `crew` element.
  - `crew_strip_with_many_children_draws`: 12 children, a draw, no panic.
- [ ] **Step 2:** RED.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** GREEN.
- [ ] **Step 5:** Commit `feat(app): show a crew strip of child agents above the lead's composer`.

### Task 3: Nest children under their lead in the sidebar

**Files:** `crates/app/src/app/sidebar.rs` (`project_groups` ~147-163, `chat_row` ~850, `chat_line` ~889), and `crates/ui/src/chat/sidebar/{mod.rs,rows.rs}` (`ChatRow::Chat { nested }`, `NEST` ~361), tests in `crates/app/tests/unit/app/sidebar.rs`.

**Interfaces:**
- Inside each project group, put a lead's live children directly after the lead, sorted by id. Everything else keeps its current order, and the lead's own position is unchanged. A child is "live" when `parent` names a chat in the same group that has `lead == true`.
- Render children with the existing `nested` inset. The unused worktree container is not needed; use `nested` on `ChatRow::Chat`, or the smallest equivalent.
- Orphans keep their normal position, un-nested. That covers a child whose parent is missing, not a lead, or in another group.
- Pinned children stay in Pinned.
- No collapse control. Leave a `ponytail:` comment that collapse can be added if crews grow large.
- Flat sidebar styles (`render_flat_sidebar`) don't change.

- [ ] **Step 1:** Write the tests.
  - `children_follow_their_lead_in_group_order`, using a pure ordering fn over the group's chat ixs.
  - `orphan_child_keeps_normal_position`, covering a deleted lead and a lead with `lead == false`.
  - `child_in_other_group_is_not_nested`.
- [ ] **Step 2:** RED.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** GREEN.
- [ ] **Step 5:** Commit `feat(app): nest child agents under their lead in the sidebar`.

### Task 4: "Delegated by" link in a child's header

**Files:** `crates/app/src/app/composer/header.rs` (`render_header_content` ~215, breadcrumb ~311-323), tests.

**Interfaces:**
- When `chat.parent` names an existing chat with `lead == true`, a muted `Delegated by {lead title}` element follows the title in the breadcrumb. Clicking it calls `select(lead_ix, ..)`.
- In any other case nothing is shown.
- Add a pure `fn delegated_by(&self, ix) -> Option<usize>` for the test.

- [ ] **Step 1:** Write the tests.
  - `delegated_by_names_live_lead`.
  - `delegated_by_hidden_for_orphan_or_non_lead`.
- [ ] **Step 2:** RED.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** GREEN.
- [ ] **Step 5:** Commit `feat(app): link a child agent back to its lead`.

### Task 5: Spawn rows open the child; native lead tool rows

**Files:**
- `crates/ui/src/chat/tool_row.rs`: add `on_child: Option<OnOpen>` and a builder copying `on_diff` (`:173`). Render it as an `Open agent` link or a menu entry, the same way `on_diff` is rendered.
- `crates/app/src/app/composer/message.rs` ~145-165: the arm that wires it.
- `crates/agent/src/events.rs` ~39-60: native lead rows.
- Tests in crates/ui, crates/app and crates/agent.

**Interfaces:**
- A pure `fn spawned_child(detail: &ToolDetail) -> Option<i64>` in the app. It matches `ToolDetail::Other { name, output, .. }` whose name leaf is `spawn_agent`. The leaf is what follows the last `__`, or the name minus an `aether_` prefix, or the bare name. It parses `output` as JSON and reads `id` as an i64.
  - When the id names an existing chat, set `on_child` to open it, using Task 2's open path.
- Native lead rows: in `events.rs`, a lead tool name (any name in `orchestrator::tool_specs()`) emits the same `Update::Tool` / `ToolCall` shape other tools use. That means `ToolDetail::Other { name, input: pretty args, output: result text }` with running/done status, matched the way `web_search` emits its rows. The UI then renders native lead calls like CLI MCP calls, and `spawned_child` works for them too.

- [ ] **Step 1:** Write the tests.
  - `spawned_child_reads_id_from_each_harness_name`, covering `mcp__aether__spawn_agent`, `aether_spawn_agent` and `spawn_agent`. Non-spawn names give None.
  - `spawned_child_ignores_bad_output`: non-JSON, missing `id`, and a string `id` all give None.
  - `native_lead_tool_emits_a_tool_row`, in crates/agent events tests.
  - A tool-row test showing the link only when `on_child` is set, if crates/ui tests already cover `on_diff` the same way.
- [ ] **Step 2:** RED.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** GREEN for `cargo test -p aether-agent`, `cargo test -p aether-ui` (check the package name) and `cargo test -p aether -- --skip hackernews::client`.
- [ ] **Step 5:** Commit `feat: open child agents from spawn rows and show native lead tool rows`.

### Task 6: Desktop check (user)

- [ ] Make a lead. The Lead chip turns mango. Spawn two children: chips appear above the composer with live status, and clicking one opens that child.
- [ ] Sidebar: the children sit indented under the lead. Deleting the lead un-nests them.
- [ ] In a child, "Delegated by …" goes back to the lead.
- [ ] On the lead's spawn row, "Open agent" opens the child. A native lead shows tool rows.
