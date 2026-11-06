# Changes Sidebar UI & Layout Revamp (VS Code-style) — Design

Date: 2026-10-16
Status: approved design

## Intent and why

The current Changes drawer (`crates/app/src/git_panel.rs` and `crates/app/src/side_panel.rs`) feels cluttered and vertically constrained:
1. **Stacked Headers**: The side tab bar (42px) sits directly above a second Git header (40px with branch name, ahead/behind badges, and refresh button), consuming over 80px before any changes or diffs are visible.
2. **Heavy Bottom Commit Pane**: The commit message textarea, "Stage all changes" checkbox, Commit button, Push button, and error/status text occupy a large, permanent chunk of height at the bottom of the drawer.
3. **Cramped File Rows**: File rows pack the change status badge, filename, middle-truncated path, diff line stats (`+N -M`), and hover stage buttons into a cramped 28px strip.

The user selected a complete revamp following the familiar **VS Code Source Control** layout to achieve a cleaner, more spacious, and intuitive UX.

## Goals

- **Unified Single Header (38px)**:
  - Consolidates the side tab bar and git drawer header into a single sleek bar.
  - Houses the tab pills (`Changes (N)`, and `Browser` when open), the `+` tab addition dropdown menu, a refresh button (`↻`), and the drawer close button (`✕`).
  - Solves the double-header vertical bloat and avoids cramping at minimum width (280px).
- **Top-Loaded Commit Input (VS Code style)**:
  - Moves the commit composer to the top, directly beneath the unified header.
  - Clean textarea with placeholder: `Message (Cmd+Enter to commit)`.
  - Prominent primary `Commit` action button right below the message box.
  - **Auto-staging workflow**: If staged files exist, commits staged files. If 0 files are staged but changes exist, clicking Commit or pressing `Cmd+Enter` automatically stages all and commits (mirroring VS Code's seamless default).
- **Collapsible Accordion File Sections**:
  - `▼ STAGED CHANGES (count)` with hover bulk action `[-]` (Unstage all).
  - `▼ CHANGES (count)` with hover bulk action `[+]` (Stage all).
  - Clicking section headers toggles folding/collapsing.
- **VS Code Style File Rows**:
  - Filename in primary text, dimmed directory path next to it.
  - Right-aligned status badges (`M` yellow/blue, `U`/`A` green, `D` red).
  - Hover action buttons on each row (`+` to stage, `-` to unstage).
  - Clicking a row opens the diff view with full back-navigation support.
- **Dedicated Diff View**:
  - When opening a file's diff, it displays full-height below the header with a back button, maximizing line visibility on narrow widths.
- **Bottom Sync Status Bar (28px)**:
  - Slender footer showing repository sync state: current branch (`🌿 main`), ahead/behind counts (`↑ 1 to push`), and a `Push` button (active only when commits are ready to push).
  - Keeps push actions cleanly separated from local commit flows.
- **Maintain Full Keyboard & A11y Support**:
  - `Cmd+Enter`: Commit (focused on message or within drawer).
  - `Up` / `Down` / `Home` / `End`: Navigate changes list.
  - `Space`: Toggle staging for selected row.
  - `Enter`: Open diff for selected row.
  - `Esc`: Close diff view if open, or close panel.
  - Full screen reader / aria labels preserved and updated.

## Non-goals

- No changes to underlying Git CLI execution (`aether_agent::git`), locking retries, or repository state tracking.
- No destructive Discard operations (`git checkout --` / `git restore` / `git clean`); `aether_agent::git` intentionally remains safe and non-destructive.
- No pull/rebase operations from the UI; the footer button is strictly `Push`, pointing users to the terminal if pulling is needed.
- No changes to the diff parsing engine (`aether_ui::parse_diff`).

## Anti-goals

- Do not hide the commit box inside an obscure popover or separate screen—VS Code keeps it visible and readily accessible at the top.
- Do not duplicate branch/sync status between both header and footer where it crowds the 280px minimum width.
- Do not break existing keyboard shortcuts or accessibility labels.

## Constraints

- Rust + GPUI Kit (`gpui_kit`, `aether_ui`, `crates/app`).
- Maintain and update existing tests in `crates/app/src/git_panel.rs`.

## Decided how (from conversation & review)

- **UX Model**: Modeled closely after VS Code's Source Control panel.
- **Header**: Single unified bar containing tabs (`Changes`, `Browser`), `+` menu, refresh, and close.
- **Commit area position**: Top of the sidebar below header.
- **Commit staging logic**: If staged files exist, commit staged. If 0 staged but changes exist, stage all and commit.
- **Sections**: Accordion style with collapsible staged / unstaged sections.
- **File rows**: Filename + path on left, status chip and hover stage button on right.
- **Footer**: Dedicated slim sync bar at the base showing branch, ahead/behind, and push button.
- **Diff view**: Full drawer height below header with back button.

## Left to builder

- In-memory accordion collapse state toggles for Staged and Changes sections.
- Exact padding and styling adjustments using `gpui_kit` tokens (`px(38.)`, `theme.sidebar`, `theme.border`).
