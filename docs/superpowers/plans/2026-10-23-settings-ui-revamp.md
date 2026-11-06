# Settings UI Revamp Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Revamp Aether's settings interface into an accessible, unconstrained two-pane master-detail view with vertical sidebar navigation, category grouping, and cross-category search.

**Architecture:** Update `SettingsPage` in `crates/ui/src/settings.rs` to support an optional sidebar slot and content canvas header while preserving single-column rendering for existing pages. Refactor `crates/app/src/settings_view.rs` to build a 240px vertical sidebar with grouped categories, search input, and keyboard arrow navigation, paired with an 860px max-width content canvas with live search matching.

**Tech Stack:** Rust (GPUI, `gpui-kit`, `aether-ui`, `aether-core`).

**Spec:** `docs/superpowers/specs/2026-10-23-settings-revamp-design.md`

## Global Constraints

- **Layout Structure:** 240px fixed sidebar on the left, unconstrained scrollable canvas with 860px centered max-width on the right.
- **Backward Compatibility:** `SettingsPage` consumers without sidebars (`render_agents`, `render_skills`) must continue rendering correctly without visual breakage.
- **Keyboard Navigation:** $\uparrow$/$\downarrow$ arrow keys navigate sidebar items; `Tab` moves focus into active controls; `Escape` returns to chat or clears active search.
- **Search Scope:** Instant matching over titles, descriptions, and keywords across all 6 setting categories with category match badges and filtered result views.
- **Existing Settings Preserved:** All existing configurations in `SettingsTab::ALL` (`Appearance`, `Model`, `Providers`, `Worktrees`, `Tools`, `Account`) must retain their current behavior, options, and actions.

## Review Focus

1. **Focus retention and escape key handling:** When typing in the search input, pressing `Escape` clears the query first; pressing `Escape` with an empty query returns to chat.
2. **Tab-level action buttons on split layout:** Category-specific actions (e.g. "Refresh" on `Providers` and `Worktrees`) must render cleanly in the right content canvas header and execute their handlers.
3. **Empty search results state:** When a user types a query matching no settings, display a friendly "No settings found" empty state with a "Clear search" button.
4. **Single-column fallback integrity:** `SettingsPage` without `.sidebar(...)` must preserve the existing `Column` layout and not introduce unwanted horizontal splits or padding issues.
5. **Arrow-key wrap-around:** Arrow-down from the last sidebar item wraps to the first item; arrow-up from the first wraps to the last.

---

### Task 1: Enhance `SettingsPage` in `aether_ui` for Two-Pane Master-Detail

**Files:**
- Modify: `crates/ui/src/settings.rs:10-95`
- Test: `crates/ui/src/settings.rs:198` (unit tests module)

**Interfaces:**
- Produces:
  - `SettingsPage::sidebar(mut self, sidebar: impl IntoElement) -> Self`
  - `SettingsPage::subtitle(mut self, subtitle: impl Into<SharedString>) -> Self`
  - Two-pane rendering in `RenderOnce for SettingsPage` when `sidebar` is `Some`.

- [x] **Step 1: Write unit tests for `SettingsPage` with and without sidebar**

Add unit tests in `crates/ui/src/settings.rs`:
- `test_settings_page_single_column_preserves_structure`: asserts single-column layout when `sidebar` is `None`.
- `test_settings_page_master_detail_creates_split`: asserts sidebar and content split when `sidebar` is `Some`.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::AppContext;

    #[gpui_kit::test]
    fn settings_page_supports_sidebar_slot(cx: &mut gpui_kit::TestAppContext) {
        let page_without_sidebar = SettingsPage::new("Settings");
        assert!(page_without_sidebar.sidebar.is_none());

        let page_with_sidebar = SettingsPage::new("Settings")
            .sidebar(div().id("test-sidebar"))
            .subtitle("Manage preferences");
        assert!(page_with_sidebar.sidebar.is_some());
        assert_eq!(page_with_sidebar.subtitle.as_deref(), Some("Manage preferences"));
    }
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether-ui settings_page_supports_sidebar_slot`
Expected: FAIL with missing fields/methods `sidebar` and `subtitle`.

- [x] **Step 3: Implement `sidebar` and `subtitle` on `SettingsPage` in `crates/ui/src/settings.rs`**

Update `SettingsPage` struct:
```rust
pub struct SettingsPage {
    title: SharedString,
    subtitle: Option<SharedString>,
    on_back: Option<ClickHandler>,
    sidebar: Option<AnyElement>,
    actions: Vec<AnyElement>,
    children: Vec<AnyElement>,
    fill: bool,
}
```
In `RenderOnce for SettingsPage`:
- If `sidebar` is `Some(sidebar)`:
  Render `h_flex().id("settings-page").size_full()` containing `div().w(px(240.)).flex_none().child(sidebar)` on the left, and a scrollable content area on the right with centered container `max_w(px(860.))`, header row with `title`, `subtitle`, and `actions`, followed by `children`.
- If `sidebar` is `None`:
  Preserve existing `Column::new().py_8().flex().flex_col().gap_8()` layout.

- [x] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether-ui settings_page_supports_sidebar_slot`
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add crates/ui/src/settings.rs
git commit -m "feat(ui): add sidebar and master-detail support to SettingsPage"
```

---

### Task 2: Implement Settings Search Catalog and Index

**Files:**
- Create: `crates/app/src/settings_search.rs`
- Modify: `crates/app/src/main.rs:1-50` (or `crates/app/src/app/mod.rs` to register module)
- Test: `crates/app/tests/unit/settings_search.rs`

**Interfaces:**
- Produces:
  - `pub(crate) struct SettingItem { pub id: &'static str, pub tab: SettingsTab, pub title: &'static str, pub description: &'static str, pub keywords: &'static [&'static str] }`
  - `pub(crate) fn all_setting_items() -> &'static [SettingItem]`
  - `pub(crate) fn search_settings(query: &str) -> Vec<&'static SettingItem>`
  - `pub(crate) fn tab_match_count(tab: SettingsTab, matches: &[&'static SettingItem]) -> usize`

- [x] **Step 1: Write failing tests for search catalog**

Create `crates/app/tests/unit/settings_search.rs`:
```rust
use crate::settings_search::*;
use crate::settings_view::SettingsTab;

#[test]
fn search_settings_matches_title_description_and_keywords() {
    assert!(!all_setting_items().is_empty());

    let theme_matches = search_settings("theme");
    assert!(theme_matches.iter().any(|m| m.tab == SettingsTab::Appearance));

    let key_matches = search_settings("keychain");
    assert!(key_matches.iter().any(|m| m.tab == SettingsTab::Account));

    let agent_matches = search_settings("claude");
    assert!(agent_matches.iter().any(|m| m.tab == SettingsTab::Providers));

    assert!(search_settings("nonexistentxyz123").is_empty());
}

#[test]
fn tab_match_count_aggregates_per_category() {
    let matches = search_settings("font");
    let count = tab_match_count(SettingsTab::Appearance, &matches);
    assert!(count >= 2); // UI Font size and Terminal Font
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether search_settings`
Expected: FAIL (module/functions not found).

- [x] **Step 3: Implement `settings_search.rs`**

Define static `SETTING_CATALOG` containing metadata for all Aether settings across the 6 tabs. Implement case-insensitive token filtering in `search_settings` and per-tab counting in `tab_match_count`.

- [x] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether search_settings`
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add crates/app/src/settings_search.rs crates/app/tests/unit/settings_search.rs crates/app/src/main.rs
git commit -m "feat(app): add settings search catalog and index"
```

---

### Task 3: Build Settings Sidebar Component with Arrow-Key Navigation

**Files:**
- Modify: `crates/app/src/app/mod.rs` (add `settings_search_input: Entity<InputState>`)
- Modify: `crates/app/src/app/new.rs` (initialize `settings_search_input`)
- Modify: `crates/app/src/settings_view.rs`
- Test: `crates/app/tests/unit/settings_view.rs`

**Interfaces:**
- Consumes:
  - `SettingItem`, `search_settings`, `tab_match_count` from `settings_search`
  - `SettingsTab` from `settings_view`
- Produces:
  - `ChatApp::cycle_settings_tab(&mut self, next: bool, cx: &mut Context<Self>)`
  - `ChatApp::render_settings_sidebar(&self, matches: &[&SettingItem], cx: &mut Context<Self>) -> impl IntoElement`

- [x] **Step 1: Write failing test for sidebar tab cycling**

In `crates/app/tests/unit/settings_view.rs`:
```rust
#[test]
fn settings_tabs_cycle_with_wrap_around() {
    use SettingsTab::*;
    assert_eq!(SettingsTab::ALL[0], Appearance);
    assert_eq!(next_settings_tab(Appearance, false), Account);
    assert_eq!(next_settings_tab(Appearance, true), Model);
    assert_eq!(next_settings_tab(Account, true), Appearance);
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether settings_tabs_cycle_with_wrap_around`
Expected: FAIL (`next_settings_tab` undefined).

- [x] **Step 3: Implement `next_settings_tab` and `render_settings_sidebar`**

1. Implement `next_settings_tab(current: SettingsTab, next: bool) -> SettingsTab`.
2. Add `settings_search_input: Entity<InputState>` to `ChatApp`.
3. In `crates/app/src/settings_view.rs`, implement `render_settings_sidebar`:
   - Top bar: Back to chat button (`settings-back`).
   - Search row: `field(&self.settings_search_input)` with search icon.
   - Grouped categories:
     - Group "General": Appearance, Model, Providers
     - Group "Developer": Worktrees, Tools
     - Group "Account": Account
   - Each item renders Lucide icon, label, active selection styling, and optional match count badge when searching.
   - Handle clicks to switch `self.settings_tab`.

- [x] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether settings_tabs_cycle_with_wrap_around`
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add crates/app/src/settings_view.rs crates/app/src/app/mod.rs crates/app/src/app/new.rs crates/app/tests/unit/settings_view.rs
git commit -m "feat(app): add settings sidebar navigation and tab cycling"
```

---

### Task 4: Connect Two-Pane Content Canvas and Search Results View

**Files:**
- Modify: `crates/app/src/settings_view.rs:50-528`
- Test: `crates/app/tests/unit/settings_view.rs`

**Interfaces:**
- Consumes:
  - `SettingsPage::new(...).sidebar(...)` from `crates/ui/src/settings.rs`
  - `search_settings` from `crates/app/src/settings_search.rs`
- Produces:
  - Integrated `ChatApp::render_settings` returning `SettingsPage` with master-detail layout.

- [x] **Step 1: Write test for search results rendering & empty query state**

In `crates/app/tests/unit/settings_view.rs`:
```rust
#[gpui_kit::test]
fn settings_view_switches_between_category_and_search_results(cx: &mut gpui_kit::TestAppContext) {
    // Assert settings page construction with sidebar produces split layout
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test -p aether settings_view_switches_between_category_and_search_results`
Expected: FAIL

- [x] **Step 3: Implement content canvas & search view in `render_settings`**

In `crates/app/src/settings_view.rs`:
1. Read `query = self.settings_search_input.read(cx).value()`.
2. If `query` is empty:
   - Construct `sidebar = self.render_settings_sidebar(&[], cx)`.
   - Set content header: active tab title + description + category actions (e.g. Refresh on Providers / Worktrees).
   - Render active tab's card contents.
3. If `query` is non-empty:
   - Run `matches = search_settings(&query)`.
   - Construct `sidebar = self.render_settings_sidebar(&matches, cx)`.
   - Content header: `"Search Results"` + `format!("{} matches for \"{}\"", matches.len(), query)` + `"Clear"` action.
   - Render matching rows grouped under category headers, or an empty state message if `matches.is_empty()`.
4. Wrap in `SettingsPage::new("Settings").sidebar(sidebar).on_back(...)`.

- [x] **Step 4: Run test to verify it passes**

Run: `cargo test -p aether settings_view_switches_between_category_and_search_results`
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add crates/app/src/settings_view.rs crates/app/tests/unit/settings_view.rs
git commit -m "feat(app): integrate two-pane master-detail settings and search results"
```

---

### Task 5: Accessibility Polish, Keybindings, and Full Verification

**Files:**
- Modify: `crates/app/src/actions.rs`
- Modify: `crates/app/src/settings_view.rs`
- Test: `crates/app/tests/unit/settings_view.rs`

**Interfaces:**
- Produces:
  - Keyboard shortcut `⌘,` opens settings directly.
  - `Escape` in settings search clears search or returns to chat.
  - Focus rings and ARIA accessibility labels verified across all controls.

- [x] **Step 1: Write tests for settings key actions**

In `crates/app/tests/unit/settings_view.rs`:
Test that pressing escape with an active search query clears the search input, and when empty closes settings.

- [x] **Step 2: Run tests to verify failure**

Run: `cargo test -p aether settings_escape_handling`
Expected: FAIL

- [x] **Step 3: Implement key event listeners & accessibility tags**

1. Register `on_key_down` on the settings page:
   - `ArrowDown` / `ArrowUp` triggers `cycle_settings_tab`.
   - `Escape` clears search if query non-empty, otherwise calls `open_page(Page::Chat, cx)`.
2. Ensure all buttons, switches, and inputs carry accessibility labels (`accessibility_label`).

- [x] **Step 4: Run full test suite to verify everything passes**

Run: `cargo test -p aether` and `cargo test -p aether-ui`
Expected: All tests pass with 0 failures.

- [x] **Step 5: Commit**

```bash
git add crates/app/src/actions.rs crates/app/src/settings_view.rs crates/app/tests/unit/settings_view.rs
git commit -m "feat(app): add keyboard navigation and accessibility polish to settings"
```


## Implementation record

Implemented on 2026-10-23. Search results reuse the interactive category controls and filter by stable catalog IDs. Keyboard actions are scoped to Settings, the sidebar, and text inputs. Native AccessKit tab/tabpanel roles provide accessibility metadata.

Verification included the full workspace suite and a final app/UI run, with 403 app tests and 194 UI tests passing. Headless tests cover category/search/empty transitions, clearing search, 240px sidebar sizing, arrow wrap-around, text-input arrow isolation, Tab/Shift-Tab traversal, sidebar search shortcuts, Escape focus retention, and fresh category entry.

The app tasks were committed together after verifying their shared behavior. Placeholder rendering tests were replaced with real GPUI window tests. A final independent review found focus-traversal, selected-tab metadata, and stale-query issues; all were addressed. Manual visual and screen-reader checks were not performed.
