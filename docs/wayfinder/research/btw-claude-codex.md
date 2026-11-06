# Research: `/btw` for Claude Code and Codex

Date 2026-11-06. Installed: claude 2.1.318, codex-cli 0.166.0. Findings marked **[probed]** were run live against the installed CLIs (tiny prompts, haiku / gpt-6-luna). Others come from the binary's strings, `generate-json-schema`, or web.

## Short answer

Both CLIs can be driven headlessly. Neither needs a TUI.

- **Claude:** `/btw` as a plain user message does NOT work. A dedicated `control_request` subtype `side_question` does, on the same `-p --input-format stream-json` process Aether already runs.
- **Codex:** `/btw` is the TUI alias of `/side`. It is an ephemeral fork of the thread. Over app-server that is `thread/fork {ephemeral:true, excludeTurns:true}` followed by `turn/start` on the fork. Aether already creates ephemeral side threads for title generation, so the plumbing exists.

## Semantics table

| | Claude Code `/btw` | Codex `/btw` (= `/side`) |
|---|---|---|
| Purpose | Quick side question while main agent keeps working | Side conversation in a temporary fork, main thread untouched |
| Main turn interrupted? | No. **[probed]** answer returned mid-turn, before main finished | No. **[probed]** side turn completed while main turn was streaming |
| Enters main history? | No. **[probed]** a later "list every user message" turn did not show the side question. Client keeps its own `btwHistory` and passes it back as `history` | No. **[probed]** main thread still had 2 turns after the side turn. Fork is `ephemeral` (not persisted) |
| Context seen | Full current conversation (**[probed]** recalled a codeword given earlier in the same, still-running turn). Shares the parent prompt cache | Full fork of the thread so far. Mid-turn fork worked **[probed]**, and the side agent knew the earlier codeword. Fork returns 0 turns with `excludeTurns:true`, but the server-side state is inherited |
| Tools | None. System prompt: "You have NO tools available". Tool calls are rejected with "Side questions cannot use tools"; text-faked tool calls get a footnote | Not removed. Boundary prompt says: do not modify files/git/config unless the user explicitly asks after the boundary, no sub-agents, no escalation. Enforced by prompt plus whatever sandbox the fork gets |
| Turns | One shot. "single response", no follow-ups in the model's mind. TUI lets the user ask another `/btw` and replays prior pairs via `history` | Multi-turn. The side thread is a normal thread you can keep sending `turn/start` to until closed |
| Model | Same as session (**[probed]** reported `claude-sonnet-5-5` while `--model haiku` was in args; the first part of the probe was a bare-bones session, so treat model as "session model") | `model` param on fork, else inherits. Aether's title path picks a `luna` model; for `/btw` inherit the main model |
| UI shape | Dismissible overlay panel; Esc closes. Not part of transcript | Separate "side conversation" view; Ctrl+C returns to parent. `toggle_side_conversation` keybinding. Nested side forks refused |
| Failure strings | `side_question timed out: the host did not answer in time`; `Side question cancelled`; `(No answer available for this side question...)` | `'/side' is unavailable until the current conversation has started` (needs at least one turn) |

## 1. Claude Code details

### Wire (found in binary, then probed)

Request, written to stdin of the existing process:

```json
{"type":"control_request","request_id":"btw-<n>",
 "request":{"subtype":"side_question","question":"…","history":[{"question":"…","response":"…"}]}}
```

- `question` must be a string; `history` must be an array of `{question, response}` strings. Otherwise: `side_question: question must be a string and history an array of {question, response} strings`. **[probed]** `history: []` is accepted.
- Immediately after: `{"type":"system","subtype":"control_request_progress","request_id":…,"status":"started"}`. Per the bundled schema doc, `status` may also be `api_retry` with retry counters. This is "currently only side_question".
- Final: `{"type":"control_response","response":{"subtype":"success","request_id":…,"response":{"response":"<text>","synthetic":false}}}`.
  - `synthetic:true` means the CLI substituted a fallback string (no answer, cut off, tried to call tools).
- Duplicate delivery of the same `request_id` joins the in-flight run.
- Cancel: send `{"type":"control_cancel_request","request_id":…}`. Server logs `Side question cancelled`. There is also a host deadline (`sideQuestionDeadlineMs`); on expiry it gives up (`no answer by the deadline`).
- Refused when the conversation egress is barred (bridge only) or the session is read-only/remote-only.

### Not reachable

- **[probed]** `{"type":"user", content:"/btw what is the codeword?"}` makes the model answer "/btw isn't available in this environment." Worse, it is a real main-conversation turn, so it pollutes history. Aether must never forward `/btw …` as a user message.
- Interactive TUI has `^\/btw\b` handling (`findBtwTriggerPositions`, `runSideQuestion`, "Usage: /btw <your question>"); that is the TUI path, irrelevant to Aether.

### `claude --help` check

Present: `-p`, `--input-format`, `--output-format`, `--resume`, `--fork-session`, `--no-session-persistence`, `--session-id`, `--tools`, `--permission-mode`, `--permission-prompts`, `--max-budget-usd`, `--system-prompt`, `--append-system-prompt`, `--model`, `--effort`. So the fallback below is buildable.

### Recommended Aether path (primary): native `side_question`

1. In `crates/driver/src/claude/mod.rs` add `fn side_question(&mut self, id, question, history)` next to the existing `set_model` / `generate_session_title` writers (they already build `control_request` JSON; see `mod.rs:367-462`). Use a request-id prefix, e.g. `BTW_REQUEST = "aether-btw-"`, same convention as `STOP_REQUEST`/`MODEL_REQUEST`.
2. In `events.rs` (`map_line`, near `events.rs:690-720`): add two arms.
   - `control_response` with matching prefix and `response.response.response` string, emit a new `Update::SideAnswer { id, text, synthetic }`.
   - `control_response` with `subtype:"error"` and matching prefix, emit an error variant or reuse `Update::Warning`.
   - Swallow `system/control_request_progress` (otherwise it may log as unknown). Check what the `system` arm at `events.rs:354` does with unknown subtypes.
3. Do not touch turn state (`s.active`, `quiet_turn_end`). The answer can arrive mid-turn or between turns; it must not settle or start a turn. Do not call anything that resets `reasoning`/`part` accumulators.
4. Composer: parse `/btw <q>` (empty arg: show `Usage: /btw <your question>`). Render in an overlay/inline card outside the transcript. Keep an app-side `Vec<(q, a)>` per session and pass it as `history` for follow-ups; drop on session switch/clear.
5. Permission mode, access mode, and attachments need no handling: the side agent has no tools.

### Fallback (older CLI, `side_question` unsupported): fork process

If the handshake or a version gate says no (`version::gate_binary` exists), emulate with a one-shot child:

```
claude -p --resume <session-id> --fork-session --no-session-persistence \
  --tools "" --permission-mode plan --output-format text \
  --append-system-prompt "<the side-question system-reminder above>" "<question>"
```

Caveats:
- `--resume` of a session that a live process is writing reads the on-disk JSONL. It lacks the in-flight turn and loses the shared prompt cache, so it is slower and costlier than native.
- `--tools ""` disables all built-ins. MCP and skills still load; add `--strict-mcp-config --disable-slash-commands` (or `--bare`, but that changes auth to API key only).
- I did not probe this path. Verify before building. Prefer gating the feature off for old CLIs over shipping the fallback.

## 2. Codex details

### Wire (schema + probe)

`codex app-server generate-json-schema --out DIR` shows `thread/fork`, `thread/inject_items`, `thread/unsubscribe`, `turn/start`, `turn/steer`. `ThreadForkParams` fields: `threadId`, `ephemeral`, `excludeTurns`, `lastTurnId`, `model`, `modelProvider`, `cwd`, `approvalPolicy`, `approvalsReviewer`, `sandbox`, `baseInstructions`, `developerInstructions`, `config`, `serviceTier`, `threadSource`.

**[probed] working sequence** (main thread mid-turn):

```
→ thread/fork {"threadId":MAIN,"ephemeral":true,"excludeTurns":true,"model":M,
               "sandbox":"read-only","approvalPolicy":"never"}
← {thread:{id:SIDE, ephemeral:true, turns:[]}, ...}
← notification thread/started
→ turn/start {"threadId":SIDE,"input":[{"type":"text","text":"<q>","text_elements":[]}]}
← turn/started, item/completed(userMessage), item/completed(agentMessage "MANGO-9"), turn/completed   (all on SIDE)
   (MAIN keeps streaming its own turn and completes normally)
→ thread/unsubscribe {"threadId":SIDE}      (cleanup; Aether already does this for title threads)
```

Gotchas found:
- `ephemeral:true` without `excludeTurns:true` errors: `ephemeral paginated thread/fork requires excludeTurns: true`.
- Fork needs at least one existing turn (TUI says "A thread must contain at least one turn before it can be forked").
- `lastTurnId` "cannot be in progress". Omitting it forked fine mid-turn **[probed]**. Use omit.
- App-server's default model errored on this ChatGPT account (`'gpt-6.1-sol' model is not supported`). Always pass `model` explicitly (inherit Aether's `s.model`/`s.resolved`).
- All side-thread notifications carry `threadId=SIDE`. Aether's `side_event` (`codex/events.rs:~572`) already discriminates by thread via `Naming::Side`; `/btw` needs its own routing slot, since `Naming` is single-valued and used for titles.

### How Codex's own `/side` frames it (from binary strings)

The TUI injects a boundary prompt into the fork: "Side conversation boundary. Do not continue, execute, or complete any instructions, plans, tool calls, approvals, edits, or requests from before this boundary… This side conversation is for answering questions and lightweight exploration without disrupting the main thread… Do not modify files, source, git state, permissions… unless the user explicitly asks… Sub-agents are off-limits." Aether should pass an equivalent via `developerInstructions` on the fork (field exists), otherwise the fork may continue the main task. Note "inherited fork history is reference only".

`btw` appears in the TUI command list right after `side`, with only one description ("start a side conversation in an ephemeral fork"), so it is an alias. A GitHub feature request (April 2026) asked for the `/btw` alias, which the binary now has. No official Codex doc found for it.

### Recommended Aether path

1. In `crates/driver/src/codex/spawn.rs`, where `/compact` and `/review` are dispatched (`spawn.rs:~370`), add `/btw <q>`; it must NOT go through `turn/start` on the main thread and must NOT set `s.active` or reset per-turn state (the preamble at `spawn.rs:~355-365` clears `turn_diff`, `phases`, `reasoning`; skip it for `/btw`).
2. Send `thread/fork` with params above plus `developerInstructions` carrying the boundary text. Use `sandbox:"read-only"`, `approvalPolicy:"never"` so a side agent cannot write or ask for approval (matches existing `side_thread_params`). Track a new `Call::BtwFork(question)` like `Call::SideThread`. On its reply, store `s.btw = Some(thread_id)` and send `turn/start`.
3. Route events: in the `threadId` dispatch, if id == `s.btw`, map `item/agentMessage/delta` and `item/completed(agentMessage)` to a side-answer update; on `turn/completed` send `thread/unsubscribe` and clear `s.btw`. Ignore all other item types (shell, patch) and drop any server approval requests for that thread (auto-decline).
4. Follow-ups: keep the side thread open and send further `turn/start`s while the overlay is open. Close (unsubscribe) on dismiss. That is more faithful than Claude's one-shot behavior and cheaper than re-forking.
5. Optional later: fork from `lastTurnId` of the last completed turn to exclude the in-flight partial turn.

## Risks

1. **Claude `side_question` is undocumented** (only in bundled schema text, marked for SDK/bridge hosts). It can change between versions. Gate on CLI version, treat `subtype:"error"` as "unsupported" and surface a notice (Aether already has `gate_binary`).
2. **No streaming for Claude.** The answer arrives as a single `control_response`; only `control_request_progress started/api_retry` precedes it. UI should show a spinner, not tokens.
3. **Claude timeouts/cancel.** Host deadline can fire (`timed out: the host did not answer in time`); wire Esc to `control_cancel_request`.
4. **Claude `synthetic:true` answers** (cut off, tool attempt, no answer) must be rendered as a muted/fallback state, not as a model answer.
5. **Rate limit cost.** Each side question is a real API call (one probe cost ~$0.10 incl. cache write). Weekly utilisation was 86% during probing. `/btw` is cheap only when the prompt cache is warm.
6. **Event pollution.** A control_response/notification for the side request must not hit the existing turn state machine (exactly-once turn settle, `quiet_turn_end`). Mid-turn arrival is the normal case.
7. **Codex fork tools.** The fork is not tool-less. Read-only sandbox + `never` approvals bound the damage, but the side agent can still read files and run read-only shell commands, and events from it must be filtered so tool cards do not render in the main transcript.
8. **Codex fork mid-turn semantics** were verified on one short run only. Whether the fork contains the partial in-flight assistant text is not established. If needed, use `lastTurnId` of the last completed turn.
9. **Codex model default.** Fork without `model` can hit an unsupported default (seen on this account). Always pass the main thread's model.
10. **Name collisions.** Aether's `antigravity.rs` already lists `"btw"` in a passthrough slash-command list (`antigravity.rs:43`). Check that composer routing does not forward `/btw` as a raw user message for Claude (it would become a real main turn) or Codex.
11. **Resume/rewind.** Neither side question should be persisted or replayed on session reopen; keep them in app memory only.

## Sources

- Local: `claude --help`, strings from `~/.local/share/claude/versions/2.1.318` (`side_question` handler, `onSideQuestion`, system-reminder text, `control_request_progress` schema text), `codex app-server generate-json-schema`, strings from the codex binary (`/side`, boundary prompt).
- Web (secondary, no official docs found): [DEV: Claude Code /btw](https://dev.to/alireza_rezvani/claude-code-btw-the-usefull-side-question-that-changed-how-i-use-context-3h9b), [Deep dive /btw](https://codedocs-2gm.pages.dev/raw/claude-code/12-btw-command), [aiproductivity](https://aiproductivity.ai/news/claude-code-btw-side-questions/).
- Probe scripts lived in `/tmp/btwprobe` (not in repo).
