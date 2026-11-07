# `/btw` Research: OpenCode and Antigravity (`agy`)

Read-only investigation of native `/btw` mechanics in OpenCode and Google Antigravity (`agy`), assessing how Aether can integrate or emulate side questions without derailing active agent runs.

---

## 1. Semantics Comparison

| Dimension | OpenCode `/btw` | Antigravity (`agy`) `/btw` |
| :--- | :--- | :--- |
| **Native Mechanism** | Builtin TUI plugin (`opencode.btw`, command `session.aside`) invoking transient LLM completion. | Builtin TUI command (`third_party/jetski/cli/commands/btw.go`) spawning a branched side-question fork (`fork.checkSideQuestionForkPolicy`). |
| **Main History Mutation** | **None.** Operates on a transient context snapshot; no messages or steps are added to the session database or event log. | **None in parent.** Runs in a branched thread. Only enters main history if explicitly promoted via the TUI (`"This conversation was promoted from a side question"`). |
| **Tool Execution** | **Disallowed.** Pure completion call (`n.generate()`) with no agent tool loop. Explicit prompt instruction forbids tools. | **Allowed (Read-Only).** Full toolset available for inspecting files/code search; tools requiring user permissions are auto-denied; workspace mutations/subagents are forbidden. |
| **Hooks & Goals** | Unaffected (no lifecycle triggers). | Inhibited. Inherited `/goal` is cancelled; user/workspace hooks (e.g. stop hooks) are suppressed. |

### Prompt Formulations

**OpenCode:**
```text
The user is asking a quick side question about the conversation so far.
Answer directly and concisely in markdown from what you already know.
Do not call any tools and do not take any actions.

<user question>
```

**Antigravity:**
```text
Above is the conversation history from the parent conversation; what follows is a side question (/btw) branched from that conversation (including any follow-up messages in this side thread). When tools are available, you have access to your full toolset, so inspect files, search code, and read external context freely to answer concisely, but default to read-only exploration: do not edit files, mutate workspace or external state, spawn subagents/workflows, or schedule background tasks unless the user explicitly asks you to in this side question. When tools are disabled for this turn, answer directly in a single response using only the existing conversation context and do not promise to look things up.
```

---

## 2. OpenCode: Recommended Aether Implementation

Aether already talks to the shared `opencode serve` HTTP hub via `crates/driver/src/opencode/service.rs`. OpenCode exposes a first-class HTTP route specifically designed for `/btw`.

### Exact Endpoint & Payloads

- **Method & Route:** `POST /api/session/:sessionID/generate` (operation ID `session.generate`)
- **Headers:** `Content-Type: application/json`
- **Request Body:**
  ```json
  {
    "prompt": "The user is asking a quick side question about the conversation so far.\nAnswer directly and concisely in markdown from what you already know.\nDo not call any tools and do not take any actions.\n\n<question>"
  }
  ```
- **Success Response (`200 OK`):**
  ```json
  {
    "data": {
      "text": "concise answer in markdown..."
    }
  }
  ```
- **Error Responses:** `404 Not Found` (session does not exist), `500 Internal Error` (generation failure).

### Notes on Other Endpoints
- `POST /api/session/:sessionID/command`: Only executes user-defined markdown command files in `.opencode/command/`. It does not handle `/btw`.
- `POST /api/session/:sessionID/fork`: Body `{"before": "<messageID>"}` or `{}`. Creates a persistent child session in SQLite. Usable if tools are mandatory, but leaves durable clutter on the server.

### Aether Integration Path
1. Composer intercepts `/btw <question>`.
2. Driver invokes `api.call(Method::POST, &format!("/api/session/{}/generate", session_id), Some(payload))` asynchronously without locking or interrupting the active driver stream.
3. Renders the response directly into an ephemeral side-dialog or peek card with a "Copy" button (identical to OpenCode's `_l` dialog).

---

## 3. Antigravity (`agy`): Recommended Aether Implementation

Aether drives `agy` via `crates/driver/src/antigravity.rs` with `--input-format stream-json --output-format stream-json`.

### Reaching Native `/btw` is Impossible in Stream Mode
1. `agy` restricts slash commands in headless print/stream mode (`"slash command is not available in print mode"`).
2. Aether's `antigravity.rs` explicitly places `"btw"` in `TERMINAL_ONLY` (`line 43`) and intercepts it (`line 280, 301`), producing `Update::Warning("`/btw` only works in the agy terminal")`.
3. If sent across stdin as a user prompt (`{"event": "user", ...}`), `agy` treats it as a regular message and mutates the main conversation.
4. `agy` does NOT have a CLI `--fork` flag; `--conversation <id>` only resumes and mutates that specific conversation.

### Recommended Emulation Path: Independent One-Shot Subprocess
To emulate `/btw` without corrupting the active conversation:
1. Intercept `/btw <question>` in Aether composer.
2. Assemble prompt context: take the recent visible transcript turns from Aether's chat model + prepend the Antigravity side-question system preamble + append `<question>`.
3. Spawn a one-shot `agy` process in the same workspace directory (without `--conversation`):
   ```bash
   agy -p "<composed_prompt>" \
       --mode accept-edits \
       --output-format stream-json \
       --sandbox
   ```
4. Read the single turn output via NDJSON stdout, display as an ephemeral card, and allow the child process to exit cleanly.
5. Does not touch `<appDataDir>/brain/<main_conversation_id>` or `conversation_summaries.db`.

---

## 4. Risks & Trade-offs

1. **OpenCode Tool Blindness:** OpenCode's native `/generate` route uses a zero-tool completion. If the user asks a side question about a file that hasn't been mentioned in the active turn's history, the model cannot inspect the file.
2. **OpenCode Concurrency:** Calling `POST /api/session/:sessionID/generate` while an execution turn is streaming reads DB state snapshot safely, but may hit token limits or rate limits on the backend provider.
3. **Antigravity CLI Latency:** Spawning an ephemeral `agy` process carries a 1-2s process startup and model initialization cost.
4. **Antigravity Tool Permissions:** Running one-shot `agy` with `--dangerously-skip-permissions` could accidentally allow write tools if the model hallucinates an edit. Passing `--sandbox` and strict prompt instructions is required to enforce read-only semantics.
5. **Session File Contention in agy:** Running a one-shot `agy` process in the same workspace does not conflict with the primary stream-json process, provided `--conversation` is omitted (a fresh temporary conversation ID is used).
