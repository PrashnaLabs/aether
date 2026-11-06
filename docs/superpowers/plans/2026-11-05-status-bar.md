# Status bar redesign: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:**
- The status bar always sits at the bottom of the window and can't be hidden.
- It takes over the sidebar's bottom nav row, with a Back button in Direct chats and Archive.
- It gains a usage strip with popovers, copying the reference design the user supplied.
- It shows the machine's CPU and RAM.

**Decisions (from the user, 2026-11-05):**
- Remove the "position" and "hide" status bar options. The bar is always at the bottom.
- Move the sidebar's bottom row into the status bar: Direct chats with its count badge, Archive, Terminal, Notes, Hacker News, Usage, Settings.
- In Direct chats or Archive, the Back button goes into that row, and the sidebar header drops its back arrow.
- Copy the reference usage UI:
  - Strip: `Used ✳ 66% 3d 21h · <codex> 0% 4h 59m · <agy> 1% Gemini wk ⟳`.
  - Clicking it opens a "Usage" popover with an "all agents" label, a refresh button, and a Detailed / Compact segmented toggle. It lists one row per provider (icon, name, reset, % used, chevron). Below the list are "Usage details & history ›" and "Manage Accounts… ›".
  - Clicking a provider row opens a provider card. The header shows the icon, the name and "Updated <ago>". An inner card shows the plan name and one block per limit: label, bar, "N% used", "Resets in …".
- System resources: the whole machine's CPU % and RAM used/total. No disk.
- Order, left to right: nav (Back when in Direct/Archive, then Direct chats, Archive, Terminal, Notes, HN, Usage, Settings), then usage strip, then CPU/RAM, then the existing clock on the right.

**Architecture:** `render_global_status_bar` (`crates/app/src/app/status_bar.rs`) becomes the one bottom bar. It is rendered once at the bottom of the root layout in `crates/app/src/app/render.rs`, never inside the sidebar footer. The sidebar's `.nav(...)` items (`crates/app/src/app/sidebar.rs` ~700-740, plus Direct chats / Archive ~506) move into it as small icon buttons using the same listeners. Usage data already exists: `ChatApp.provider_plan_usages: HashMap<Provider, PlanUsage>` (`crates/agent/src/usage.rs`: `PlanUsage { plan_name, limits: Vec<PlanLimit { label, percent_left, reset_label, .. }> }`), filled by `refresh_all_provider_usages` (`crates/app/src/app/providers.rs:83`).

**Tech Stack:** Rust 2024, gpui via gpui-kit. Unsafe code is denied workspace-wide.

## Global Constraints

- No new crates unless Task 4 proves one is unavoidable (see there).
- Keep the time/date format and seconds settings. Delete only `status_bar_visible` and `status_bar_position`, plus `StatusBarPosition`, `status_bar_at`, their settings rows, settings-search entries and tests. Old settings files that still contain those keys must load fine; check for `deny_unknown_fields`.
- Nav buttons keep their current behavior and listeners. Moving them only changes where they render. Each one needs a tooltip and an aria label, and an "active" look for the current page, matching how `.nav` marks it today.
- Popovers reuse the app's existing popover/menu components and theme tokens. Don't hand-roll a new popover system. Find the nearest existing popover, for example the access or model menu or the workspace popover, and copy its pattern.
- "Manage Accounts…" is in the reference, but Aether has no accounts concept. Render it disabled with the tooltip "Not available yet", so the copied layout stays and no fake feature ships.
- Percent colors follow the existing usage page thresholds if there are any (grep `crates/app/src/usage_view`). Otherwise warn at ≥ 60% used and alert at ≥ 85%, using theme warning/danger tokens.
- Tests live in `crates/<crate>/tests/unit/...` via `#[cfg(test)] #[path]`. UI draw tests need a keyless OpenRouter active chat, because drawing a CLI chat trips gpui's determinism check. No test spawns a CLI or makes network calls.
- No comments unless genuinely non-obvious. `cargo fmt --check` is already red on main, so don't reformat unrelated code. `hackernews::client` is flaky; run with `--skip hackernews::client`. Disk is tight: no extra profiles, no `cargo clean`.
- Commits: Conventional Commits, no `Co-Authored-By` or AI attribution.

## Review Focus

1. **The sidebar is collapsed or hidden.** Expected: the status bar and its nav stay visible and usable, since that is now the only way to reach Terminal, Settings and the rest. Test in Task 2.
2. **No provider usage yet** (no credentials, still fetching). Expected: the strip shows only `Used` and the refresh icon, or a muted "No usage", with no crash and no empty separators. The popover lists only providers that have data. Test in Task 3.
3. **A narrow window.** Expected: the bar doesn't overflow. Items drop in a fixed priority as width shrinks: usage detail text first, then CPU/RAM, then the clock, while the nav always stays. Test in Task 2 or 4.
4. **A CPU/RAM sampler failure** (command missing, bad parse). Expected: the item is hidden, nothing panics, and the sampler doesn't spin. Test in Task 4.
5. **Old settings with `status_bar_position = "sidebar"` or `status_bar_visible = false`.** Expected: they load, and the bar shows at the bottom. Test in Task 1.

---

### Task 1: The bar is always at the bottom, and the position/hide settings are gone

**Files:** `crates/core/src/settings.rs` (~859-871, ~1018-1022), `crates/app/src/app/status_bar.rs`, `crates/app/src/app/render.rs`, `crates/app/src/app/sidebar.rs` (~665 `sidebar.footer(self.render_global_status_bar(..))`), `crates/app/src/settings_view.rs`, `crates/app/src/settings_search.rs`, plus any other `status_bar_at` / `status_bar_visible` users (`grep -rn "status_bar_visible\|status_bar_position\|status_bar_at\|StatusBarPosition" crates`), and their tests.

- [ ] **Step 1: Write the tests.**
  - `old_settings_with_status_bar_keys_still_load`: a settings JSON/TOML (whatever format Settings persists in) containing both old keys parses, with the other fields intact.
  - Status bar placement: the app draws exactly one status bar, at the bottom of the root, whether or not the sidebar is collapsed. Use a `debug_selector("status-bar")` with `debug_bounds`, and assert its top is near the window bottom.
  - The settings view no longer offers the position/visibility rows. Assert through the existing settings_view and settings_search tests.
- [ ] **Step 2:** Run the tests; they should fail (RED).
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p aether-core` (check the package name) and `cargo test -p aether -- --skip hackernews::client`; they should pass (GREEN).
- [ ] **Step 5:** Commit `refactor(app): keep the status bar at the bottom and drop its position settings`.

### Task 2: Move the sidebar nav row into the status bar, with Back in Direct/Archive

**Files:** `crates/app/src/app/sidebar.rs` (nav items ~700-740, Direct chats / Archive ~506, back headers ~522/755/771), `crates/app/src/app/status_bar.rs`, `crates/ui/src/chat/sidebar/mod.rs` (`.nav` / footer API, if the footer becomes empty), tests.

- [ ] **Step 1: Write the tests.**
  - `status_bar_has_nav_buttons`: draw, then each of these is found by `debug_bounds`: `nav-direct`, `nav-archive`, `nav-terminal`, `nav-usage`, `nav-settings`. Notes and HN appear only when their settings are on.
  - `nav_click_opens_page`: clicking `nav-settings` sets `page == Settings`. Pick one more page too.
  - `direct_count_badge_shows`: N unread or total direct chats, using the same count the sidebar badge uses today.
  - `back_button_only_in_direct_and_archive`: `nav-back` is present on the Direct chats / Archive views and absent otherwise. Clicking it does what the sidebar back arrow did.
  - `sidebar_has_no_nav_footer`: the nav items are gone from the sidebar footer, and the back arrow is gone from the Direct/Archive sidebar header.
  - `nav_works_with_sidebar_collapsed`.
- [ ] **Step 2:** RED.
- [ ] **Step 3:** Implement.
  - Reuse the existing listeners.
  - Icon buttons are sized like the current sidebar nav icons, with a tooltip and an aria label each.
  - The active page gets the active style.
  - Narrow windows: the nav never collapses. Lower-priority status items, added in Tasks 3 and 4, drop first.
- [ ] **Step 4:** GREEN.
- [ ] **Step 5:** Commit `feat(app): move sidebar nav into the status bar`.

### Task 3: Usage strip and popovers

**Files:** `crates/app/src/app/status_bar.rs`, a new `crates/app/src/app/status_bar/usage.rs` (or a module beside it) if `status_bar.rs` grows past about 300 lines, `crates/app/src/app/providers.rs` (record when each usage arrived: `plan_usage_at: HashMap<Provider, i64>` in ms, set where `provider_plan_usages.insert` happens), possibly `crates/ui` for a bar or segmented control if one exists to reuse, tests.

**Interfaces:**
- Pure `fn headline(u: &PlanUsage) -> Option<(f32 /*used %*/, String /*reset*/, Option<String> /*short label when not session/weekly*/)>`. It picks the limit with the highest used % (`100 - percent_left`); on a tie, the earliest reset wins.
  - The short label is the window's own short name when it's neither the session nor the weekly window. The reference shows "Gemini wk", so derive it from `PlanLimit.label` (e.g. "Gemini Models · Weekly Limit" becomes "Gemini wk"). Keep the mapping small and tested.
- **Strip:** `Used`, then for each provider in `[Claude, Codex, Antigravity, OpenRouter]` that has usage: provider icon, `{pct}%`, the reset (and the short label if any). Then the refresh icon, which calls `refresh_all_provider_usages`.
  - Percent color follows the thresholds in Global Constraints.
  - Clicking anywhere on the strip except refresh opens the Usage popover.
- **Usage popover:**
  - Header: `Usage`, a muted `all agents`, and a refresh button.
  - A segmented toggle, `Detailed` / `Compact`. It is session state and doesn't need to persist.
  - Compact: one row per provider, showing icon, name (truncated), headline reset, headline %, and a chevron.
  - Detailed: each row also shows every limit's label, % and reset inline. The Antigravity row in the reference ("Gemini Models · Weekly Limit Remaining") is what Detailed looks like.
  - Footer: `Usage details & history ›` opens the existing Usage page. `Manage Accounts… ›` is disabled (see Global Constraints).
- **Provider card:** clicking a provider row opens it.
  - Header: icon, name, `Updated {ago}` (from `plan_usage_at`, e.g. "just now", "5m ago").
  - Inner rounded card: `plan_name` (e.g. "System default"), then for each limit: the label, a bar filled to the used %, `{used}% used` on the left and `Resets in {reset}` on the right.
  - Below it: `Manage Accounts…`, disabled. The reference's "Claude Account ✓ System default" picker is out of scope.
  - Back or Escape returns to the list. Clicking outside closes.

- [ ] **Step 1: Write the tests.**
  - `headline_picks_highest_used_then_soonest_reset`.
  - `short_label_for_non_session_weekly` (e.g. Gemini weekly becomes "Gemini wk"; session and weekly windows give None).
  - `strip_lists_only_providers_with_usage`.
  - `strip_with_no_usage_draws_without_separators`.
  - `popover_toggles_detailed_compact`.
  - `provider_row_opens_card_with_all_limits`.
  - `usage_details_opens_usage_page`.
  - `updated_ago_formats` (pure).
- [ ] **Step 2:** RED.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** GREEN.
- [ ] **Step 5:** Commit `feat(app): show plan usage in the status bar with detail popovers`.

### Task 4: Machine CPU and RAM

**Files:** a new `crates/app/src/sysinfo.rs` (or wherever background samplers live; grep for an existing periodic task such as the clock tick), `crates/app/src/app/status_bar.rs`, tests.

**Interfaces:**
- `struct Sample { cpu_percent: f32, mem_used: u64, mem_total: u64 }`.
- A background sampler on the background executor updates `ChatApp.sys_sample: Option<Sample>` every 3 s, a constant with a `ponytail:` comment. The UI thread never runs it.
- **macOS, without unsafe:**
  - CPU % comes from `top -l 2 -n 0 -s 1`. The second sample's `CPU usage: X% user, Y% sys, Z% idle` line gives `100 - idle`.
  - Memory comes from `sysctl -n hw.memsize` (total) plus `vm_stat` (pages active + wired + compressed, times page size), the same way Activity Monitor computes "Memory Used".
- **Linux:** two `/proc/stat` reads for CPU, and `/proc/meminfo` `MemTotal - MemAvailable` for memory.
- Parsers are pure functions that take the command or file text, so tests feed fixtures.
- If these commands prove unreliable on this macOS version (verify by running them once and recording the output in the report), report DONE_WITH_CONCERNS and suggest the `sysinfo` crate instead of adding it unasked.
- **Display:**
  - A small CPU icon, then `{cpu:.0}%`.
  - A memory icon, then `{used:.1}/{total:.0} GB`.
  - Each gets the warn/alert color at ≥ 80% / ≥ 95%.
  - A tooltip shows the exact numbers.
  - If there is no sample, the item is hidden.
- **Failure handling:** a failed command or parse gives no sample. Back off to 30 s after 3 consecutive failures.

- [ ] **Step 1: Write the tests.**
  - `parses_top_cpu_line` and `parses_vm_stat_and_memsize` (real fixture text captured from this machine).
  - `parses_proc_stat_delta` and `parses_proc_meminfo`.
  - `bad_input_gives_none`.
  - `status_bar_hides_resources_without_sample`.
  - `status_bar_shows_cpu_ram_with_sample`.
- [ ] **Step 2:** RED.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** GREEN.
- [ ] **Step 5:** Commit `feat(app): show machine CPU and RAM in the status bar`.

### Task 5: Desktop check (user)

- [ ] The bar is at the bottom in every layout, including with the sidebar collapsed. The old position and hide settings are gone.
- [ ] The nav row works. Back shows in Direct chats and Archive. The direct chat badge shows.
- [ ] The usage strip matches the reference. The popover's Detailed/Compact toggle, the provider card and "Usage details & history" all work.
- [ ] CPU/RAM numbers roughly match Activity Monitor.
