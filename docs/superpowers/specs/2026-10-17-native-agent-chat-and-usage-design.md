# Native Agent Chat, Composer & Live Usage Cockpit Design

**Date:** 2026-10-17  
**Status:** Approved for Implementation  
**Topic:** Elevating Agent Chat, Composer UX, Antigravity Input Recognition, and Real-Time Quota/Usage to Native Parity  

---

## 1. Intent & The "Why"

Aether is designed as a fast, native desktop environment for local and external coding agents (Claude Code, Codex, Antigravity, OpenCode, Pi, and OpenRouter). However, users currently encounter critical UX/UI and runtime issues:
1. **Unreliable Typing & Focus Drops**: Keystrokes frequently do not register because clicking anywhere on the composer card outside the narrow, 1-line text boundary does not transfer focus to the `TextareaState`. Furthermore, switching chats or opening the window can leave the focus handle disconnected.
2. **Missing Slash Commands & Unfriendly Discovery**: The composer lacks a visible `/` command affordance, misses key driver commands, and does not provide friendly, descriptive explanations of what commands like `/compact`, `/review`, or `/diff` do.
3. **Antigravity CLI Input Bug**: Antigravity driver passed `-p -` expecting stdin streaming, but `agy` CLI treats `-p` as the prompt flag itself. Consequently, every input was literally received by the agent as `"-"`.
4. **Watered-Down Usage & Telemetry**: While native agent tools (such as Claude Code and Codex) provide rich, real-time context gauges and live account quota lanes (5-hour session limits, 7-day limits, reset countdowns), Aether artificially limits its context meter to OpenRouter (`filter(|_| openrouter)`). Agent chats have no live gauge in the composer, and the existing `/usage` command merely navigates to a static historical chart page.

This project upgrades Aether's chat interface into **The Native Cockpit**: an ultra-responsive, friendly, and descriptive workspace that fixes input hit-testing, repairs Antigravity prompt dispatch, adds a native slash command system with visible discovery, provides a real-time circular context meter for all agent sessions, and introduces an interactive rate-limit and token breakdown popover.

---

## 2. Goals, Non-Goals & Anti-Goals

### Goals
- **Antigravity Input Dispatch**:
  - Fix prompt delivery in `crates/agent/src/driver/antigravity.rs` by passing the actual user prompt text to the `-p` flag (`-p <prompt>`) instead of hardcoded `"-"`.
- **Rock-Solid Click-to-Focus**:
  - Clicking anywhere inside the composer card (margins, padding, background, or toolbar borders) immediately activates and focuses the text area. Focus is also reliably asserted when switching chats, creating a chat, or returning to the chat view.
- **Native Slash Command Discovery**:
  - Add a dedicated `/` command button in the composer's leading toolbar.
  - Automatically open the suggestion menu when typing `/` or clicking the `/` button.
  - Provide a complete catalog of agent commands (`/compact`, `/review`, `/diff`, `/doctor`, `/usage`, `/cost`, `/init`, `/mode`, `/stop`, `/new`, `/clear`, and driver-specific tools like `/goal`, `/schedule`, `/browser`) with friendly, descriptive explanations and argument hints.
- **Live Context Window Gauge in Composer**:
  - Render the circular context-window ring in the composer for **all** provider chats (Claude, Codex, OpenCode, Antigravity, and OpenRouter).
  - Dynamically track active context occupancy (`latest context_tokens / model_context_limit`, e.g. `32k / 200k · 16%`) with progressive warning colors as the window fills up.
- **Native Quota & Rate-Limit Inspector Popover**:
  - Clicking the context ring opens a rich popover panel.
  - Displays context occupancy breakdown: Prompt tokens, completion tokens, prompt cache read tokens, and cache creation tokens.
  - Displays account rate-limit lanes: 5-hour rolling limit percentage, 7-day limit percentage, and exact human-readable reset countdowns (e.g. *"Resets in 2h 15m"*), fetched via the agent's authentication credentials (macOS keychain `Claude Code-credentials` or `~/.claude/.credentials.json`).
  - Gracefully degrades for providers without 5h/7d OAuth endpoints by showing model context occupancy, active turn tokens, and cost estimates.
- **Friendly & Descriptive Message UI**:
  - Tool execution rows render with friendly, plain-language titles, live status spinners, elapsed execution times (e.g., `git status · 0.3s`), and clear exit badges.
  - Sleek, cohesive pill styling for active model, permission mode (Ask / Auto-Edit / Full), and workspace status.

### Non-Goals
- Rebuilding external CLI binaries or implementing custom OAuth refresh flows against Anthropic's private client secrets (credentials are passively read from keychain/config, and refreshed when the user runs the CLI).
- Replacing the full-page historical usage view (the separate analytics page remains for long-term multi-project retrospective tracking).
- Cumulative historical token summing in the ring (which would falsely overflow the context gauge past 100% on long conversations).

### Anti-Goals
- Do not trap key events or steal keystrokes intended for the terminal drawer, quick-open palette, or buffer editor.
- Do not clutter the composer toolbar with non-essential toggles; keep the visual design clean, airy, and focused on speed.
- Do not block the UI thread on network or keychain requests (all quota and command discovery calls must execute asynchronously in background tasks).

---

## 3. Architecture & Technical Design

### A. Antigravity Input Recognition Fix
* **File**: `crates/agent/src/driver/antigravity.rs`
* **Changes**:
  - Update `launch_args(opts: &DriverStartOptions, session_id: Option<&str>, prompt: &str) -> Vec<String>` to accept the prompt text.
  - Replace `args.push("-p".into()); args.push("-".into());` with `args.push("-p".into()); args.push(prompt.to_string());`.
  - In `prompt(&mut self, text: String)`, pass `text` into `launch_args`. Keep stdin disconnected or `Stdio::null()`.
  - Update unit tests in `antigravity.rs` to verify that `launch_args` properly contains the prompt text.

### B. Composer Focus & Hit-Target Delegation
* **Files**: `crates/ui/src/chat/composer.rs` and `crates/app/src/app.rs`
* **Changes**:
  - In `Composer::render`, attach `.on_mouse_down(MouseButton::Left, ...)` to the outer card container `v_flex()` that calls `self.input.update(cx, |s, cx| s.focus(window, cx))`.
  - Clean up the bounds probe canvas so it does not intercept pointer events or cause redundant refresh passes.
  - When switching chats in `ChatApp::select`, assert `self.composer.update(cx, |s, cx| s.focus(window, cx))` and ensure focus is maintained across view transitions.

### C. Slash Command Engine & Visual Trigger
* **Files**: `crates/ui/src/chat/composer.rs`, `crates/app/src/suggest.rs`, `crates/app/src/app.rs`
* **Changes**:
  - Add a visible `/` icon chip (`composer_chip("slash-cmd", ...)` with a `/` glyph) to the composer leading toolbar. Clicking it opens the suggestion menu directly and focuses the text area.
  - In `crates/app/src/suggest.rs`, expand `slash_commands`:
    - Ensure clean fuzzy matching and presentation for all built-in commands.
    - Format entries with two clear lines: Command title + syntax (e.g., `/compact [note]`), and a plain, friendly description (e.g., *"Summarize and compact conversation context to free token room"*).
    - Support both immediate execution (commands like `/clear`, `/stop`, `/diff`, `/doctor`, `/usage`) and template prefilling (commands requiring prompts like `/goal <task>`, `/review [branch]`).

### D. Live Context Gauge & Account Rate-Limit Telemetry
* **Files**: `crates/agent/src/usage.rs`, `crates/app/src/app.rs`, `crates/ui/src/chat/context_ring.rs`
* **Changes**:
  - Remove `filter(|_| openrouter)` in `ChatApp::composer_card`:
    - Map default context window sizes for native models:
      - Claude 3.7 / 3.5: 200,000 tokens
      - Codex / GPT-4o / o1 / o3: 128,000 / 200,000 tokens
      - Antigravity / Gemini: 1,000,000 / 2,000,000 tokens
      - OpenCode / Pi: 128,000 / 200,000 tokens
    - Track active context occupancy using the latest turn's `context_tokens` from message history.
  - Implement Account Quota & Plan Usage Fetcher:
    - Background fetcher for Claude credentials: read macOS Keychain service `Claude Code-credentials` (or `~/.claude/.credentials.json`), invoke `GET https://api.anthropic.com/api/oauth/usage` with beta header `oauth-2025-04-20`.
    - Parse rolling limit windows:
      - 5-hour utilization percentage and reset timestamp.
      - 7-day utilization percentage and reset timestamp.
  - Context Ring & Popover UI (`ContextRing` in `crates/ui/src/chat/context_ring.rs`):
    - Circular ring showing current context occupancy with color stages (Green < 60%, Yellow 60-80%, Orange 80-90%, Red > 90%).
    - Clicking the ring opens a floating popover displaying:
      - **Context Occupancy**: Progress bar with exact used/total tokens, plus prompt, completion, and cache read token breakdown.
      - **Account Quotas**: Progress bars for 5-hour and 7-day limits with live reset countdown labels (e.g., *"Resets in 1h 45m"*).
      - **Session Economics**: Estimated session cost and token count.
    - For providers without 5h/7d endpoints, rate-limit lanes are cleanly omitted while context occupancy and turn stats remain active.

### E. Descriptive UI & Tool Execution Polish
* **Files**: `crates/ui/src/chat/tool_row.rs`, `crates/ui/src/chat/message.rs`
* **Changes**:
  - Tool runs display a badge with an animated status indicator while running, switching to a checkmark / alert icon on completion.
  - Human-friendly action labels: e.g. *"Running `cargo test`"*, *"Reading `src/main.rs`"*, *"Inspecting git diff"*.
  - Render elapsed execution time badge (`0.4s`, `2.1s`) and clean expandable output accordion.

---

## 4. Verification & Acceptance Criteria

1. **Antigravity Input Recognition**:
   - Sending any prompt (e.g. `hello`) to the Antigravity agent passes the actual text to `agy -p "<text>"`. The agent recognizes the prompt and does not claim it was just a `-`.
2. **Typing Reliability**:
   - Clicking on the padding, margins, or background of the composer card instantly gives focus to the textarea and accepts typing without needing to click the exact text line.
   - Creating a new chat or switching tabs leaves the composer immediately focused and ready to type.
3. **Commands & Discovery**:
   - Clicking the toolbar `/` button or typing `/` opens the suggestion menu.
   - All standard agent commands (`/compact`, `/review`, `/diff`, `/doctor`, `/usage`, `/init`, `/mode`, etc.) appear with friendly descriptions and trigger correctly.
4. **Usage Cockpit**:
   - The circular context gauge renders in the composer for agent chats.
   - Clicking the gauge displays the popover with context token breakdown, rate-limit lanes, and countdown labels.
   - For Claude sessions, the 5-hour and 7-day account limits display accurately when credentials exist.
5. **Tool Execution**:
   - Running tools display elapsed execution times and clear success/failure states.
