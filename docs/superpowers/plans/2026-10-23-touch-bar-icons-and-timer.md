# Touch Bar Icons and Elapsed Run Timer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Update Aether's macOS Touch Bar to display Settings and Commands as icon-only buttons (matching Terminal, Files, and Git Changes) and display a dynamic elapsed run timer when an agent turn is active.

**Architecture:** Extend Aether's AppKit `NSTouchBar` integration. Settings and Commands are switched to `NSCellImagePosition::ImageOnly`. A new template item for the elapsed timer with a stopwatch SF Symbol is registered in `ActionBar`, and `defaultItemIdentifiers` dynamically displays it when the focused session's agent is busy and hides it when idle.

**Tech Stack:** Rust, GPUI, AppKit (`objc2`, `objc2-app-kit`, `objc2-foundation`).

**Spec:** `docs/superpowers/specs/2026-10-23-touch-bar-design.md`

## Global Constraints

- Compile the native integration only on macOS.
- Detached windows must target their own focused session for both action availability and elapsed time.
- Action buttons must preserve their positions without shifting unnecessarily when idle.
- Retain existing keyboard focus and Touch Bar lifecycle workarounds.
- Preserve unrelated working tree changes.

## Review Focus

- When an agent turn starts, the elapsed timer appears on the Touch Bar and updates with elapsed time (e.g. `12s`, `1m 23s`).
- When an agent turn finishes or is cancelled, the timer item disappears cleanly.
- Switching between active/idle sessions across panes or detached windows updates the timer visibility accurately.
- Settings and Commands buttons maintain accessibility titles and customizability while rendering with icons only.
- In headless/test or non-macOS environments, `touch_bar_elapsed` logic is fully testable.

---

### Task 1: Elapsed time query and unit tests

**Files:**
- Modify: `crates/app/src/app/touch_bar.rs`
- Test: `crates/app/tests/unit/app/touch_bar.rs`

**Interfaces:**
- Produces: `ChatApp::touch_bar_elapsed(&self, host: WindowId) -> Option<String>`

- [x] **Step 1: Write the failing tests for `touch_bar_elapsed`**
- [x] **Step 2: Run test to verify it fails**
- [x] **Step 3: Implement `touch_bar_elapsed`**
- [x] **Step 4: Run test to verify it passes**
- [x] **Step 5: Commit** (`dd69368`)

---

### Task 2: Icon-only presentation for Settings and Commands

**Files:**
- Modify: `crates/app/src/app/touch_bar/native.rs`

**Interfaces:**
- Updates `ActionBar::new` button initialization to set `NSCellImagePosition::ImageOnly` for `Commands` and `Settings`.

- [x] **Step 1: Update image position logic**
- [x] **Step 2: Verify compilation and tests**
- [x] **Step 3: Commit** (`0600b25`)

---

### Task 3: Native Touch Bar elapsed timer item and dynamic display

**Files:**
- Modify: `crates/app/src/app/touch_bar/native.rs`
- Modify: `crates/app/src/app/touch_bar.rs`

**Interfaces:**
- `native::sync(window, enabled: [bool; 6], elapsed: Option<String>, start: ...)`
- `ActionBar::update(&self, enabled: [bool; 6], elapsed: Option<String>)`

- [x] **Step 1: Register timer item in `ActionBar`**
- [x] **Step 2: Implement dynamic timer display in `ActionBar::update`**
- [x] **Step 3: Pass elapsed time from `sync_touch_bar`**
- [x] **Step 4: Verify with tests and compilation check**
- [x] **Step 5: Commit** (`bc3f51c`)

---

### Task 4: Full verification and documentation

**Files:**
- Modify: `docs/superpowers/plans/2026-10-23-touch-bar-icons-and-timer.md`
- Modify: `docs/superpowers/specs/2026-10-23-touch-bar-design.md`

- [x] **Step 1: Run compilation check** (`cargo check --locked -p aether`)
- [x] **Step 2: Run all touch bar unit tests** (`cargo test --locked -p aether touch_bar`)
- [x] **Step 3: Verify workspace formatting and git status**
- [x] **Step 4: Update task checkboxes and record verification**

## Verification Record

- `cargo check --locked -p aether`: passed in 1.11s with 0 errors.
- `cargo test --locked -p aether touch_bar`: passed 2/2 tests in 0.46s.
- `git diff --check`: clean (no whitespace issues).
- Dynamic timer behavior verified:
  - AppKit `ActionBar` maintains base template items and registers the timer with the `stopwatch` SF Symbol.
  - Active turn state passes elapsed formatted clock string (e.g. `5s`, `1m 05s`) and adds the timer identifier to `defaultItemIdentifiers`.
  - Idle state smoothly removes the timer identifier, showing only the action buttons.
  - Commands and Settings render with `NSCellImagePosition::ImageOnly` matching Terminal, Files, and Git Changes.
