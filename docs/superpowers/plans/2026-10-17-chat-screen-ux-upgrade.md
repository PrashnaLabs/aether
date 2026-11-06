# Agent Chat Screen UX Upgrade Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade the agent chat screen with chronological inline streaming (thinking and tool actions interleaved naturally in the message flow where they happen), enhanced code blocks with language headers and clipboard copy feedback, and polished auto-scroll streaming UX.

**Architecture:**
1. Extend `Message` in `crates/core` to record `step_positions: Vec<usize>` so every thought, tool call, and search step preserves its exact chronological character offset in `Message::text`.
2. Introduce a `CodeBlockView` in `crates/ui` that extracts fenced code blocks from markdown segments, rendering them with a language badge, one-click clipboard copy button with checkmark feedback, and monospaced styled container.
3. Overhaul `AssistantMessage` in `crates/ui/src/chat/message.rs` to interleave thinking blocks, tool execution cards, requests, and markdown/code blocks in strict chronological order instead of hoisting all steps into a panel at the top.
4. Auto-collapse finished tool cards into sleek compact pills while keeping active/streaming tools expanded with a live progress indicator.

**Tech Stack:** Rust, GPUI, `aether-core`, `aether-ui`, `gpui-kit`.

---

## Global Constraints

- Preserve complete backward compatibility with existing saved chats in SQLite (`aether.db`). All new fields must use `#[serde(default)]`.
- Maintain test coverage: all existing 215 tests in `cargo test` must continue to pass, plus new tests for chronological interleaving and code block extraction.
- Dark and light theme consistency through GPUI theme helpers (`cx.theme()`).

---

## Review Focus

1. **Interleaving order**: If an assistant thinks, produces text, calls a tool, and writes final text, the screen must display: Thought -> Text 1 -> Tool Card -> Text 2, in that exact order.
2. **Backward compatibility**: Old chats without `step_positions` loaded from disk must render cleanly without panic or out-of-bounds indexing.
3. **Live streaming responsiveness**: While streaming, the active tool or latest text chunk must show live progress and pin the scroller to the bottom unless the user scrolled up.
4. **Code block copy**: Clicking copy must write the exact raw code to the system clipboard and temporarily toggle to "Copied" feedback.
5. **Tool card toggle**: Tool cards must auto-collapse when done, but remain clickable to expand full output/diff.

---

## Tasks

### Task 1: Record Step Offsets in `aether-core`

**Files:**
- Modify: `crates/core/src/lib.rs`
- Test: `crates/core/src/lib.rs` (unit tests)

**Steps:**
- [ ] Add `#[serde(default)] pub step_positions: Vec<usize>` to `Message` struct in `crates/core/src/lib.rs`.
- [ ] In `Message::apply`, when a new `Step::Thought`, `Step::Tool`, `Step::Search`, or `Step::Skill` is added, record `self.text.len()` into `self.step_positions`.
- [ ] Add helper `Message::step_at(&self, index: usize) -> usize` that returns the step's character offset (defaulting to 0 if missing for backward compatibility).
- [ ] Add unit test `test_step_positions_track_interleaved_events` verifying that steps record their exact text offset as deltas and tools arrive.
- [ ] Run `cargo test -p aether-core` to verify all tests pass.

---

### Task 2: Build `CodeBlockView` and Fenced Markdown Splitter in `aether-ui`

**Files:**
- Create: `crates/ui/src/chat/code_block.rs`
- Modify: `crates/ui/src/chat/mod.rs`
- Test: `crates/ui/src/chat/code_block.rs` (unit tests)

**Steps:**
- [ ] Implement `parse_markdown_blocks(text: &str) -> Vec<ContentBlock>` where `ContentBlock` is either `Prose(String)` or `Code { language: String, code: String }`.
- [ ] Implement `CodeBlockView` component:
  - Header with language tag/label (e.g. `RUST`, `PYTHON`, `BASH`, `JSON`, etc.) in subtle uppercase font.
  - Copy button with clipboard icon, writing code to system clipboard via `cx.write_to_clipboard(ClipboardItem::new_string(...))`.
  - State tracking for "Copied" feedback with checkmark icon for 2 seconds.
  - Formatted code body with monospaced font, subtle background (`theme.secondary`), border (`theme.border`), rounded corners (`rounded_lg`), and padding.
- [ ] Add unit tests verifying:
  - Markdown with mixed prose and fenced code blocks parses into correct blocks.
  - Code blocks preserve exact indents, linebreaks, and language identifiers.
- [ ] Run `cargo test -p aether-ui` to verify tests pass.

---

### Task 3: Overhaul `AssistantMessage` for Chronological Interleaving

**Files:**
- Modify: `crates/ui/src/chat/message.rs`
- Modify: `crates/ui/src/chat/thinking.rs`
- Test: `crates/ui/src/chat/message.rs` (unit tests)

**Steps:**
- [ ] Refactor `AssistantMessage` to construct a unified chronological timeline of items sorted by `at`:
  - `TimelineItem::Step(usize, Step)` at its `step_position`.
  - `TimelineItem::Request(usize, AnyElement)` at its request `at`.
  - `TimelineItem::Text(Range<usize>)` between consecutive step/request boundaries.
- [ ] Render reasoning/thinking inline:
  - Dedicated collapsible thought pill ("Thinking..." when live, "Thought for Xs" / "Reasoning" when finished).
  - Expandable to view model's chain of thought in an indented, styled reasoning container.
- [ ] Render tool actions inline:
  - Embed `ToolRow` inline at its exact chronological position.
  - While running / streaming: keep expanded with live status / spinner.
  - When finished: default to compact collapsed pill showing tool icon, target summary, and diff stats (e.g. `+12 -3`), expandable on click.
- [ ] Render text blocks using `parse_markdown_blocks`, routing code blocks to `CodeBlockView` and prose to `TextView`.
- [ ] Show a subtle streaming pulse indicator at the tail while `self.streaming` is true.
- [ ] Update unit tests in `crates/ui/src/chat/message.rs` to assert chronological rendering.
- [ ] Run `cargo test -p aether-ui` and `cargo test` across workspace.

---

### Task 4: End-to-End Verification and UX Polish

**Files:**
- Modify: `crates/ui/src/chat/message.rs`
- Modify: `crates/ui/src/chat/scroller.rs`
- Modify: `crates/app/src/app.rs`

**Steps:**
- [ ] Verify light and dark theme styling across tool cards, code blocks, and reasoning pills.
- [ ] Verify scrolling behavior: auto-scroll follows tail during streaming unless user manually scrolled up.
- [ ] Run `cargo check` and `cargo test` to ensure full pass with 0 failures or warnings.
