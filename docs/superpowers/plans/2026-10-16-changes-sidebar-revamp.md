# Changes Sidebar UI & Layout Revamp (VS Code-style) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Revamp the Changes sidebar drawer into a sleek, uncluttered VS Code-style Source Control interface with a unified 38px header, top-loaded commit composer with auto-staging, collapsible accordion file sections, and a dedicated 28px bottom sync footer.

**Architecture:** Consolidate the duplicate top headers between `side_panel.rs` and `git_panel.rs` into a single 38px tab bar. Relocate the commit composer directly beneath the header at the top of `git_panel.rs` with auto-staging logic, and render the changes list as collapsible accordion sections with right-aligned status chips. Place branch sync metadata and the Push button into a slender bottom footer.

**Tech Stack:** Rust (2021 edition), GPUI Kit, `crates/app`, `aether_ui`, `aether_agent::git`.

**Spec:** [`docs/superpowers/specs/2026-10-16-changes-sidebar-revamp-design.md`](../specs/2026-10-16-changes-sidebar-revamp-design.md)

## Global Constraints

- Must compile with existing workspace dependencies; no new external crates.
- Must run cleanly at `MIN_WIDTH` (280px) without text clipping or layout overflow.
- Git operations remain non-destructive (no discard commands).
- Preserve all existing keyboard navigation (`Cmd+Enter` to commit, arrow keys/Home/End to navigate, Space to toggle stage, Enter to open diff, Esc to close diff or panel).
- Keep all unit tests in `crates/app/src/git_panel.rs` passing (`cargo test --package aether git_panel`).

## Review Focus

1. **Commit with 0 staged changes**: Pressing Commit or `Cmd+Enter` when files are modified but none are staged must automatically stage all and commit rather than silently failing or remaining permanently disabled.
2. **Narrow panel layout at 280px**: Long branch names or long filenames must middle-truncate without pushing action buttons off-screen.
3. **Empty / clean repository state**: When no files are changed and nothing is ready to push, the commit composer must cleanly hide and show an uncluttered "No changes" state.
4. **Diff view interaction**: Opening a diff must consume the full body below the header with an accessible back button and keyboard `Esc` handler returning to the file list.
5. **Section folding / collapse state**: Collapsing "Changes" or "Staged" must not cause keyboard navigation (Up/Down arrow keys) to index into hidden rows.

---

### Task 1: Accordion Collapsible Sections State & Row Generation

**Files:**
- Modify: `crates/app/src/git_panel.rs:55-95`
- Test: `crates/app/src/git_panel.rs:1810-1845`

**Interfaces:**
- Consumes: `Change`, `Section`, `Row`
- Produces:
  ```rust
  pub(crate) struct GitUiState {
      // existing fields...
      pub(crate) staged_collapsed: bool,
      pub(crate) changes_collapsed: bool,
  }

  fn rows(changes: &[Change], staged_collapsed: bool, changes_collapsed: bool) -> Vec<Row>;
  ```

- [ ] **Step 1: Write the failing test for accordion row folding**

Add `test_accordion_collapses_sections` in `crates/app/src/git_panel.rs`:
```rust
#[test]
fn accordion_collapses_sections() {
    let changes = vec![
        change("a.rs", ChangeKind::Modified, true),
        change("b.rs", ChangeKind::Modified, false),
    ];
    // Both expanded
    assert_eq!(
        rows(&changes, false, false),
        vec![
            Row::Header(Section::Staged, 1),
            Row::File(0),
            Row::Header(Section::Changes, 1),
            Row::File(1),
        ]
    );
    // Staged collapsed
    assert_eq!(
        rows(&changes, true, false),
        vec![
            Row::Header(Section::Staged, 1),
            Row::Header(Section::Changes, 1),
            Row::File(1),
        ]
    );
    // Both collapsed
    assert_eq!(
        rows(&changes, true, true),
        vec![
            Row::Header(Section::Staged, 1),
            Row::Header(Section::Changes, 1),
        ]
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --package aether accordion_collapses_sections`
Expected: FAIL due to `rows` taking 1 argument instead of 3.

- [ ] **Step 3: Implement `rows` with collapse flags and update `GitUiState`**

In `crates/app/src/git_panel.rs`:
1. Add `pub(crate) staged_collapsed: bool` and `pub(crate) changes_collapsed: bool` to `GitUiState::new`.
2. Update `rows(changes: &[Change], staged_collapsed: bool, changes_collapsed: bool) -> Vec<Row>`:
```rust
fn rows(changes: &[Change], staged_collapsed: bool, changes_collapsed: bool) -> Vec<Row> {
    let mut rows = Vec::new();
    for section in [Section::Staged, Section::Changes] {
        let collapsed = match section {
            Section::Staged => staged_collapsed,
            Section::Changes => changes_collapsed,
        };
        let files: Vec<Row> = changes
            .iter()
            .enumerate()
            .filter(|(_, c)| c.staged == (section == Section::Staged))
            .map(|(ix, _)| Row::File(ix))
            .collect();
        if !files.is_empty() {
            rows.push(Row::Header(section, files.len()));
            if !collapsed {
                rows.extend(files);
            }
        }
    }
    rows
}
```
3. Update callers of `rows` across `git_panel.rs` to pass `self.git.staged_collapsed, self.git.changes_collapsed` (or `false, false` in existing tests).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --package aether git_panel`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/git_panel.rs
git commit -m "feat(git_panel): support collapsible accordion sections in rows generation"
```

---

### Task 2: VS Code-Style Top Commit Composer with Auto-Staging

**Files:**
- Modify: `crates/app/src/git_panel.rs:769-800, 1533-1628`
- Test: `crates/app/src/git_panel.rs:1871-1900`

**Interfaces:**
- Consumes: `GitUiState`, `GitOp`, `commit_state`
- Produces:
  ```rust
  fn commit_state(staged: usize, changed: usize, message: &str) -> Result<String, &'static str>;
  fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>);
  fn git_commit_area(&self, cx: &mut Context<Self>) -> Option<AnyElement>;
  ```

- [ ] **Step 1: Write the failing test for auto-staging commit state**

Update `commit_says_why_it_is_off` in `crates/app/src/git_panel.rs`:
```rust
#[test]
fn commit_allows_auto_staging_when_unstaged_only() {
    // With unstaged changes and a message, commit is ready (auto-stages all)
    assert_eq!(
        commit_state(0, 3, "feat: auto stage"),
        Ok("Commit 3 changes".to_string())
    );
    // With staged changes, commits staged count
    assert_eq!(
        commit_state(2, 5, "feat: staged"),
        Ok("Commit 2 staged changes".to_string())
    );
    // Without message, blocked
    assert_eq!(
        commit_state(1, 1, ""),
        Err("Write a commit message first")
    );
    // Without any changes, blocked
    assert_eq!(
        commit_state(0, 0, "feat: empty"),
        Err("No changes to commit")
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --package aether commit_allows_auto_staging_when_unstaged_only`
Expected: FAIL due to old `commit_state` expecting `stage_all: bool` and rejecting 0 staged changes.

- [ ] **Step 3: Update `commit_state` and `commit()` logic for auto-staging**

In `crates/app/src/git_panel.rs`:
1. Simplify `commit_state`:
```rust
fn commit_state(staged: usize, changed: usize, message: &str) -> Result<String, &'static str> {
    if changed == 0 {
        return Err("No changes to commit");
    }
    if message.trim().is_empty() {
        return Err("Write a commit message first");
    }
    if staged > 0 {
        Ok(format!("Commit {}", count(staged, "staged change", "staged changes")))
    } else {
        Ok(format!("Commit {}", count(changed, "change", "changes")))
    }
}
```
2. In `commit(&mut self, window: &mut Window, cx: &mut Context<Self>)`:
   If `staged == 0 && changed > 0`, queue `GitOp::StageAll` before running `GitOp::Commit { message }`.
3. In `git_commit_area(&self, cx: &mut Context<Self>) -> Option<AnyElement>`:
   Style as a top-docked composer placed above the changes list:
   - Compact textarea (2 lines) with placeholder `"Message (Cmd+Enter to commit)"`.
   - Full-width primary `Button::new("git-commit").primary().w_full().icon(Lucide::GitCommitHorizontal)`.
   - Remove the old `"Stage all changes"` checkbox and bottom Push button from this area.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --package aether commit`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/git_panel.rs
git commit -m "feat(git_panel): implement top-loaded commit composer with auto-staging"
```

---

### Task 3: Unified Top Header & Browser Tab Integration

**Files:**
- Modify: `crates/app/src/side_panel.rs:71-137`
- Modify: `crates/app/src/git_panel.rs:1008-1115`
- Test: `crates/app/src/git_panel.rs:1805-1820`

**Interfaces:**
- Consumes: `SideTab`, `ChatApp`, `GitUiState`
- Produces:
  ```rust
  // side_panel.rs
  fn side_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement;
  ```

- [ ] **Step 1: Write test verifying panel layout & height rules**

Verify `panel_keeps_the_transcript_usable_or_floats` passes in `crates/app/src/git_panel.rs`.

- [ ] **Step 2: Unify `side_tab_bar` to 38px and eliminate redundant `git_header`**

In `crates/app/src/side_panel.rs`:
1. Change `side_tab_bar` height to `px(38.)`.
2. Add the refresh button `git-refresh` inside `side_tab_bar` when `self.side_tab == SideTab::Changes`, displaying loading state and invoking `refresh_git(true, cx)`.
3. Keep the tab pills (`Changes`, `Browser`), `+` dropdown menu, and `side-close` `✕` button.
In `crates/app/src/git_panel.rs`:
4. Remove the standalone `git_header` (the 40px bar with branch and refresh), as the tab bar in `side_panel.rs` now provides the unified header and the footer provides branch status.

- [ ] **Step 3: Run tests to verify compilation and behavior**

Run: `cargo test --package aether git_panel`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/app/src/side_panel.rs crates/app/src/git_panel.rs
git commit -m "refactor(side_panel): unify top header bar to 38px and eliminate duplicate git header"
```

---

### Task 4: VS Code-Style File Rows & Accordion Header UI

**Files:**
- Modify: `crates/app/src/git_panel.rs:1207-1382`
- Test: `crates/app/src/git_panel.rs:1864-1870, 1944-1954`

**Interfaces:**
- Consumes: `Row::Header(Section, usize)`, `Row::File(usize)`
- Produces:
  ```rust
  fn git_row(&self, ix: usize, view: &Entity<Self>, window: &Window, cx: &App) -> AnyElement;
  fn toggle_section(&mut self, section: Section, cx: &mut Context<Self>);
  ```

- [ ] **Step 1: Write failing test for toggling section collapse**

Add test in `crates/app/src/git_panel.rs`:
```rust
#[test]
fn section_collapse_toggles_state() {
    let mut state = GitUiState::default();
    assert!(!state.staged_collapsed);
    state.staged_collapsed = !state.staged_collapsed;
    assert!(state.staged_collapsed);
}
```

- [ ] **Step 2: Run test to verify it passes/compiles**

Run: `cargo test --package aether section_collapse_toggles_state`
Expected: PASS

- [ ] **Step 3: Update `git_row` for VS Code layout**

In `crates/app/src/git_panel.rs`:
1. **Header rows (`Row::Header(section, files)`)**:
   - Chevron indicator: `Lucide::ChevronDown` if expanded, `Lucide::ChevronRight` if collapsed.
   - Title: `"Staged Changes"` or `"Changes"`.
   - File count badge pill: `tabular(files)`.
   - Hover action button: `[-]` Unstage all for Staged; `[+]` Stage all for Changes.
   - Clicking header toggles `self.git.staged_collapsed` or `self.git.changes_collapsed` and syncs rows.
2. **File rows (`Row::File(ix)`)**:
   - Layout:
     - Left: filename (medium font), followed by dimmed middle-truncated directory path (`dir`).
     - Right: diff stat badge (`+N -M`), right-aligned letter chip (`M`, `A`, `D`), and hover stage/unstage button (`+` or `-`).
   - Clicking file row opens `open_diff`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --package aether git_panel`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/git_panel.rs
git commit -m "feat(git_panel): style accordion headers and file rows following VS Code layout"
```

---

### Task 5: Bottom Sync Footer & Full-Height Diff View

**Files:**
- Modify: `crates/app/src/git_panel.rs:1394-1526, 1533-1628`
- Test: `crates/app/src/git_panel.rs:1881-1905`

**Interfaces:**
- Consumes: `Status`, `push_state`, `Push`
- Produces:
  ```rust
  fn git_sync_footer(&self, cx: &mut Context<Self>) -> Option<AnyElement>;
  ```

- [ ] **Step 1: Write test for sync footer push state**

Verify existing `push_shows_only_with_something_to_send` test in `crates/app/src/git_panel.rs` correctly verifies push state detection.

- [ ] **Step 2: Implement `git_sync_footer` and wire into panel**

In `crates/app/src/git_panel.rs`:
1. Create `fn git_sync_footer(&self, cx: &mut Context<Self>) -> Option<AnyElement>`:
   - 28px height, `border_t_1()`, `sidebar_border`.
   - Left side: Branch icon + branch name (middle-truncated if long), or detached commit SHA.
   - Right side: If `ahead > 0`, shows `"↑ {ahead} to push"` and a compact `Push` button running `GitOp::Push`.
2. Update `git_panel`:
   - If diff is open (`self.git.diff.is_some()`), render `git_diff_view` taking full height below the header (with back button).
   - If diff is closed, render:
     1. Top commit composer (`git_commit_area`)
     2. File accordion list (`git_body`)
     3. Bottom sync footer (`git_sync_footer`)

- [ ] **Step 3: Run full git_panel test suite**

Run: `cargo test --package aether git_panel`
Expected: All 13+ tests PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/app/src/git_panel.rs
git commit -m "feat(git_panel): add bottom sync footer and full-height diff view"
```

---

## Self-Review Checklist

1. **Spec coverage**:
   - Single unified 38px header: Covered in Task 3.
   - Top-loaded commit composer with auto-staging: Covered in Task 2.
   - Collapsible accordion sections: Covered in Task 1 & Task 4.
   - VS Code file rows (name, path, status, hover stage): Covered in Task 4.
   - Bottom sync footer: Covered in Task 5.
   - Full-height diff view: Covered in Task 5.
2. **Type consistency**: `rows(&changes, staged_collapsed, changes_collapsed)` used consistently across Tasks 1, 4, and tests.
3. **Review Focus**:
   - Auto-staging when unstaged only tested in Task 2.
   - Narrow panel truncation handled in Tasks 3, 4, 5.
   - Section collapse keyboard navigation safety handled in Task 1.
