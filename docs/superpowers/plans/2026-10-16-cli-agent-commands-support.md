# CLI Agent Commands & Ecosystem Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand Aether's CLI agent capabilities across all providers (Claude, Codex, Pi, OpenCode, Antigravity) with comprehensive slash commands, external process management, dedicated review/workflow handlers, and transparent command pass-through.

**Architecture:** Extend `crates/app/src/suggest.rs` with full slash command autocompletion and execution (both UI-level shortcuts and agent-level commands). Upgrade `crates/agent/src/driver/codex.rs` and sibling drivers to handle specialized commands like `/review` and command pass-through. Enhance `crates/agent/src/external.rs` to support stopping and tracking external Codex and Antigravity agent processes.

**Tech Stack:** Rust, GPUI, tokio, serde_json, std::process::Command.

**Spec:** Direct user requirements to support all commands and features from all CLI agents (Claude, Codex, Pi, OpenCode, Antigravity).

## Global Constraints

- Preserve all existing comments and docstrings unrelated to code changes.
- All clickable file links in user communication must use `[filename](file:///path)` format.
- Ensure 100% test pass rate across the workspace without introducing compiler warnings.

## Review Focus

- Typing `/clear` empties the active chat's message history and resets session state.
- Typing `/compact` or picking it from the suggestion menu sends the compaction command directly to the active CLI agent.
- In a Codex chat, `/review` launches `codex exec review` with `--uncommitted` and custom instructions.
- External processes running `codex` or `agy` can be stopped via `stop_external` without returning "unsupported provider".
- Provider-specific slash commands (`/goal`, `/schedule`, `/browser`, `/undo`, `/redo`, `/tokens`, `/fork`) appear contextually based on the active provider.

---

### Task 1: Composer Slash Command Expansion

**Files:**
- Modify: `crates/app/src/suggest.rs`
- Modify: `crates/app/src/app.rs`
- Test: `crates/app/src/suggest.rs` (unit tests)

- [ ] **Step 1: Write the failing test for new slash commands**
  Add unit tests in `crates/app/src/suggest.rs` asserting that `slash_commands` generates `/clear`, `/compact`, `/review`, `/cost`, `/mode`, `/help`, `/doctor`, and provider-specific commands (`/goal`, `/browser` for Antigravity; `/undo`, `/redo` for OpenCode; `/tokens` for Pi; `/fork` for Codex).

- [ ] **Step 2: Run tests to verify failure**
  Run `cargo test -p aether` to ensure the new tests fail before implementation.

- [ ] **Step 3: Implement expanded `SlashCommand` variants and autocompletion**
  Update `SlashCommand` in `crates/app/src/suggest.rs`:
  - `ClearChat`
  - `Doctor`
  - `Access(AccessMode)`
  - `AgentPrompt(String)`
  Add command definitions to `slash_commands`:
  - `/clear`: "Clear chat history"
  - `/compact`: "Summarize and compact conversation context"
  - `/cost`: "View token and cost usage across agents" (alias to usage page)
  - `/review`: "Review repository changes against base branch"
  - `/mode <ask|autoedit|full|readonly>`: "Set access / permission mode"
  - `/doctor`: "Check agent environment and binary diagnostics"
  - `/help`: "Show available commands and assistance"
  - Contextual provider commands:
    - If `Provider::Antigravity`: `/goal`, `/schedule`, `/browser`, `/learn`, `/boost`
    - If `Provider::OpenCode`: `/undo`, `/redo`
    - If `Provider::Pi`: `/tokens`
    - If `Provider::Codex`: `/fork`

- [ ] **Step 4: Implement execution handlers in `crates/app/src/suggest.rs` and `crates/app/src/app.rs`**
  - Implement `SlashCommand::ClearChat`: clears `chat.messages`, resets `chat.cli_session`, and persists.
  - Implement `SlashCommand::Access(mode)`: updates `chat.access` and notifies.
  - Implement `SlashCommand::Doctor`: shows a diagnostic system message with installed binary paths and statuses.
  - Implement `SlashCommand::AgentPrompt(text)`: submits the prompt text to the active chat session.

- [ ] **Step 5: Run tests and verify**
  Run `cargo test -p aether` and ensure all tests pass.

---

### Task 2: Dedicated CLI Handlers & Slash Command Pass-Through in Drivers

**Files:**
- Modify: `crates/agent/src/driver/codex.rs`
- Modify: `crates/agent/src/driver/antigravity.rs`
- Test: `crates/agent/src/driver/codex.rs`
- Test: `crates/agent/src/driver/antigravity.rs`

- [ ] **Step 1: Write failing tests for Codex review command execution**
  Add unit tests in `crates/agent/src/driver/codex.rs` verifying that when a prompt starts with `/review`, `launch_args` outputs `["exec", "review", "--json", "--skip-git-repo-check", "--uncommitted", "-"]` instead of standard prompt arguments.

- [ ] **Step 2: Run tests to verify failure**
  Run `cargo test -p aether-agent -- driver::codex::tests` to verify failure.

- [ ] **Step 3: Implement `/review` support in `CodexDriver`**
  In `crates/agent/src/driver/codex.rs`, detect `/review` prompts:
  - If prompt begins with `/review`: invoke `exec review` with `--uncommitted` (and any trailing review instructions stripped and passed via stdin).
  - Normal prompts continue to use `exec` or `exec resume`.

- [ ] **Step 4: Ensure command pass-through across all drivers**
  Ensure that prompts starting with `/` (such as `/compact`, `/goal`, `/undo`) are passed through without modification to CLI processes that process slash commands via stdin.

- [ ] **Step 5: Run tests and verify**
  Run `cargo test -p aether-agent` and verify all tests pass.

---

### Task 3: External Process Management for Codex and Antigravity

**Files:**
- Modify: `crates/agent/src/external.rs`
- Test: `crates/agent/src/external.rs`

- [ ] **Step 1: Write failing tests for external process termination of Codex and Antigravity**
  Add unit test in `crates/agent/src/external.rs` checking `signal_plan` for `Provider::Codex` and `Provider::Antigravity`, verifying it returns `Ok(Some("INT"))` or `Ok(Some("TERM"))` instead of `Err("unsupported provider")`.

- [ ] **Step 2: Run tests to verify failure**
  Run `cargo test -p aether-agent -- external::tests::signal_plan` to verify failure.

- [ ] **Step 3: Update `signal_plan` and external session discovery in `crates/agent/src/external.rs`**
  - Update `signal_plan` to allow `Provider::Codex` and `Provider::Antigravity`.
  - Add session discovery heuristics for Codex (parsing session ids/titles from `~/.codex/` if available).
  - Add session discovery heuristics for Antigravity (parsing conversation ids from `~/.gemini/antigravity-cli/`).

- [ ] **Step 4: Run tests and verify**
  Run `cargo test -p aether-agent` and ensure all tests pass.

---

### Task 4: Full Workspace Verification

**Files:**
- Workspace-wide verification

- [ ] **Step 1: Run full test suite across workspace**
  Execute `cargo test --workspace`.
- [ ] **Step 2: Check for compiler warnings**
  Execute `cargo check --workspace`.
- [ ] **Step 3: Verify git status is clean and report results to user.**
