# Agent Context Handoff: CLI Seeding Research

Investigation into seeding target CLI agents with handed-off chat context across Aether's drivers (`claude`, `codex`, `pi`, `opencode`, `antigravity`).

## 1. Injection Options per CLI

| CLI | CLI / RPC Injection Mechanism | Size Limits | Target Agent Interpretation |
| :--- | :--- | :--- | :--- |
| **Claude** | 1. `--append-system-prompt <text>` or `--append-system-prompt-file <path>` at startup.<br>2. First user turn via stdin stream-json (`crates/driver/src/claude/spawn.rs:400`). | Arg max for flag; file variant unlimited; stream-json payload practically unbounded. | System prompt is treated as persistent persona/context (waits for prompt). User message is treated as the active task to execute immediately. |
| **Codex** | 1. `turn/start` setting `collaborationMode.settings.developer_instructions` (`crates/driver/src/codex/spawn.rs:397`) or `-c` flag.<br>2. First user turn in `turn/start` `input` array (`crates/driver/src/codex/spawn.rs:349`). | JSON-RPC stdio unbounded; CLI `-c` limited by OS `ARG_MAX`. | `developer_instructions` acts as developer rules (not a turn). `input` text is treated as an active task. |
| **Pi** | 1. `--append-system-prompt <text\|file>` at launch (`crates/driver/src/pi.rs:648`).<br>2. First user prompt via RPC `message_cmd` (`crates/driver/src/pi.rs:732`). | Flag takes file path without limits; RPC payload unbounded. | System prompt is background guidelines. First user message triggers response. |
| **OpenCode** | 1. No CLI `--append-system-prompt` flag; configured per agent (`opencode.json`).<br>2. First user prompt via `POST /api/session/{id}/prompt` (`crates/driver/src/opencode/session.rs:748`). | HTTP POST body unbounded. | OpenCode treats `prompt` as the user turn to solve. |
| **Antigravity** | 1. No system prompt flag; `--settings` overrides settings JSON.<br>2. First user prompt via stream-json on stdin (`crates/driver/src/antigravity.rs:250`). | Stdio stream unbounded. | User event immediately triggers agent turn. |

### Reliability & Semantic Recommendation
- **First user message injection is the only universal, reliable mechanism** across all 5 drivers.
- **Asymmetry problem**: Claude and Pi support system prompt appending via CLI flags at launch, Codex supports `developer_instructions` in RPC turn settings, but OpenCode and Antigravity lack runtime system prompt overrides. Furthermore, system prompt injection does not trigger an agent turn—the model waits idly for user input.
- **Verdict**: Seed the new session with a **structured first user message**. Frame the handed-off history explicitly inside `<handed_off_context>` XML tags so the model treats previous state as background knowledge, followed by an explicit task continuation directive (`## Next Task`).

---

## 2. Brief Generation Strategy & Templates

### Generation Approaches Evaluated
1. **Source Agent Summarization** (asking the source session with a summary prompt):
   - *Pros*: Highest semantic understanding of prior decisions and internal reasoning.
   - *Cons*: Fails when source is in an unrecoverable state (rate limit 429, quota exhausted, context window filled, process crashed).
2. **Cheap Standalone Summarizer** (reusing `/btw` side-run in `crates/app/src/app/chat/side.rs` with OpenRouter / flash):
   - *Pros*: Completely decoupled from source CLI process; does not pollute source context.
   - *Cons*: Requires external API credentials, incurs additional token billing and network roundtrip.
3. **Mechanical Transcript Compression**:
   - *Pros*: Zero latency, zero cost, completely offline, immune to rate/quota limits.
   - *Cons*: Raw transcript can be verbose or include noisy tool outputs.

### Recommendation: Two-Tier Cascade
- **Primary**: If source agent is healthy and user manually triggers handoff, ask the source agent.
- **Fallback**: If handoff is triggered by an error, rate limit, quota exhaustion, or dead driver, immediately use **deterministic mechanical compression**.

### Handoff Prompt Template (Source Agent)
```text
You are preparing a context handoff brief for another AI coding agent taking over this task in this repository.
Summarize the conversation succinctly in this exact markdown format:

## Handoff Context
### 1. Goal
<What was requested and what the user is trying to accomplish>
### 2. Work Completed & Architectural Decisions
<Key files edited, libraries chosen, and design patterns followed>
### 3. Current State & Blockers
<What was being attempted when the session stopped, and any encountered errors or limits>
### 4. Modified Files
<List all files modified or created during this session>
### 5. Next Immediate Step
<The exact next code edit, test run, or verification command the new agent should execute>
```

### Mechanical Fallback Extraction (Out-of-Quota / Dead Driver)
When the source agent is unreachable, construct the handoff payload directly from Aether's `Message` history:
1. **Goal**: First `Role::User` message text in `chat.messages`.
2. **Files Touched**: Deduplicated list of `path` strings from all `Step::Tool(ToolCall)` entries where `detail` matches `ToolDetail::Edit { path, .. }` or `ToolDetail::Read { path, .. }`.
3. **Recent Conversation**: Last 2-3 turns of `(Role::User, Role::Assistant)` text (truncated to 400 chars each).
4. **Last Error / Limit**: The `error` string from the final `Message` or last `Update::Error` event.
5. **Next Step**: Prompt the target agent: "Review the modified files and error above, then continue resolving the task."

---

## 3. Limit Signals per Driver (Exact Updates & file:line)

| Driver | Exact Update Variant / Error Text | Event Trigger | File:Line |
| :--- | :--- | :--- | :--- |
| **Claude** | `Update::Notice(Notice { kind: RateLimit, state: Failed, text: "Claude usage limit reached" })` | `rate_limit_event` with `status == "rejected"` | `crates/driver/src/claude/events.rs:857` |
| **Claude** | `Update::Notice(Notice { kind: RateLimit, state: Running, text: "Claude usage at {pct}% of the {kind} limit" })` | `rate_limit_event` with `status == "allowed_warning"` | `crates/driver/src/claude/events.rs:853` |
| **Claude** | `Update::Error("Claude stopped: budget reached")` or `"Claude stopped: max turns reached"` | `result` with `subtype: "error_max_budget_usd"` / `"error_max_turns"` | `crates/driver/src/claude/events.rs:777` |
| **Claude** | `Update::Notice(Notice { kind: Retry, text: "Retrying ({attempt}/{max}): {why}" })` | `v["event"] == "api_retry"` (HTTP 429/529) | `crates/driver/src/claude/events.rs:402` |
| **Codex** | `Update::Notice(Notice { kind: RateLimit, state: Failed, text: error })` & `Update::Error(error)` | `error` event where `codexErrorInfo` is `"usageLimitExceeded"` or `"rateLimitExceeded"` | `crates/driver/src/codex/events.rs:163-172` |
| **Codex** | `Update::Notice(Notice { kind: RateLimit, state: Failed, text: "Codex usage limit reached ({kind})" })` | `account/rateLimits/updated` with `rateLimitReachedType` | `crates/driver/src/codex/events.rs:280-281` |
| **Codex** | `Update::Error(error)` | `turn/completed` with `turn["status"] == "failed"` | `crates/driver/src/codex/events.rs:428-430` |
| **Pi** | `Update::Warning("The reply hit the model's output limit")` | `m["stopReason"] == "length"` | `crates/driver/src/pi.rs:1008` |
| **Pi** | `Update::Error(e)` (where `e` = `m["errorMessage"]`) | `m["stopReason"] == "error"` (e.g. 429 rate limit or context exhausted) | `crates/driver/src/pi.rs:983`, `850` |
| **Pi** | `Update::Error(e)` | `v["success"] == false` on prompt response | `crates/driver/src/pi.rs:903`, `919` |
| **Pi** | `NoticeKind::Retry` ("Retrying: ...") & failure notice on `auto_retry_end` | `auto_retry_start` / `auto_retry_end` | `crates/driver/src/pi.rs:1154`, `1176` |
| **OpenCode** | `Update::Warning("The reply hit the model's output limit")` | `data["finish"] == "length"` | `crates/driver/src/opencode/events.rs:217` |
| **OpenCode** | `Update::Error(why)` (where `why` = `data["error"]["message"]`) | `session.execution.failed` (e.g. rate limit, quota exhausted) | `crates/driver/src/opencode/events.rs:311-317` |
| **OpenCode** | `s.step_error = Some(error_message(&data["error"]))` | `session.step.failed` | `crates/driver/src/opencode/events.rs:230` |
| **OpenCode** | `NoticeKind::Retry` ("Retrying (attempt N): ...") | `session.retry.scheduled` | `crates/driver/src/opencode/events.rs:235-242` |
| **Antigravity** | `Update::Error(error)` | `result` with `status == "ERROR"` | `crates/driver/src/antigravity.rs:1205-1207` |
| **Antigravity** | `Update::Error(why)` | Stderr stream `AGY_ERROR:` parsed and emitted on process termination | `crates/driver/src/antigravity.rs:546`, `607` |

---

## 4. Working Tree & Project Folder Constraints

All drivers instantiate child CLI processes using `DriverStartOptions.cwd`:
- **Claude**: Executes in `opts.cwd`. Project storage is keyed by directory path in `~/.claude/projects/`.
- **Codex**: Executes in `opts.cwd` and explicitly passes `"cwd": opts.cwd` in `thread_params` (`crates/driver/src/codex/spawn.rs:330`).
- **Pi**: Executes in `opts.cwd`. Scopes sessions under `~/.pi/agent/sessions/--<sanitized-cwd>--/`.
- **OpenCode**: Canonicalizes path via `std::fs::canonicalize(&opts.cwd)` and posts `location: {"directory": ...}` (`crates/driver/src/opencode/session.rs:827-834`). **Caution**: OpenCode matches session directories by exact string comparison; symlinks or trailing slashes cause session mismatches.
- **Antigravity**: Executes in `opts.cwd` and resolves git root/project configs.

### Handoff Isolation Rule
Because all drivers operate on disk files directly in `opts.cwd`, uncommitted file edits created by the source agent are already on disk. The target agent should be spawned with the **exact same canonical cwd** without transferring session files or transcript stores between different CLI engines.
