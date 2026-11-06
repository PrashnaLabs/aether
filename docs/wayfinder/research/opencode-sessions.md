# Research: OpenCode sessions, steer/queue, modes, effort, tools, cwd

Sources: Waku `reference/crates/waku-core/src/` (`driver/opencode.rs` = **D**, `opencode_api.rs` = **A**, `opencode_service.rs` = **S**, `http_wire.rs`), live `opencode v2.0.28` (`service.json` says 2.0.28, pid 6414, `http://127.0.0.1:49374`, Basic auth). OpenAPI pulled read-only with `opencode api GET /openapi.json` (276 KB, 38 `/api/session*` ops). Nothing on :4096 was listening, so the spec came from the user's own service. No sessions created, no prompts sent.

## 0. Ground truth: this is the v2 API, not the v1 one

`opencode --help` (verbatim): `serve  Start the v2 API and web server`; `run`: `--session, -s string  Session ID to continue, or to create if it does not exist`, `--continue, -c`, `--fork`, `--model, -m string  Model to use in the format provider/model#variant`, `--agent string`, `--auto  Auto-approve permissions that are not explicitly denied`, `--server string  Connect to a server URL instead of the background service`, `--standalone  Run with a private server instead of the background service`.
`opencode serve --help` (verbatim flags): `--hostname string`, `--port integer`, `--cors string  Additional allowed CORS origin (repeat for multiple origins)`, `--service`, `--stdio`. `opencode api [flags] <operation | method path...>` with `--server`, `--data/-d`, `--header/-H`, `--param key=value` is a handy read/write probe that handles auth.

Consequences for aether:
- Routes are `/api/session/...`, envelope `{ "data": ... }` (except `/api/info` and `POST .../interrupt`, bare: A:1349-1362, 1142). Waku adopts the **service** (`opencode serve --service`, registration `${XDG_STATE_HOME:-~/.local/state}/opencode/service.json`, S:110-160, spawn S:644) and talks HTTP Basic, user `opencode`, password from that file (S:57, http_wire.rs:52-66). Unauthenticated GET on the service returns 401 (checked). Aether's hub (`crates/agent/src/driver/opencode.rs:96-147`) assumes `serve --service --port 4096` with no auth; verify whether an explicit port still demands the password, else read `service.json` like Waku.
- The event names are `session.*` (`session.execution.*`, `session.text.delta`, `session.tool.*`, `session.inbox.*`, `permission.asked/replied`). The v1 `message.part.updated` / `tool` part shapes (task item 5 premise) are **not** emitted by 2.x. The SSE payload type in the spec is an opaque JSON string, so shapes below come from Waku's decoders and its fixtures (D:2645-2700, 3043-3075).
- Aether's current mapper (`opencode.rs:443-516`) matches `message.part.updated`, `tool.use`, `execution.succeeded` (no `session.` prefix) and a `permission.asked` with `tool/title/input`. Against 2.x almost none of that fires; only `session.renamed` is real. The mapper needs a rewrite, not a patch (see section 5).

## 1. Session continuity

Waku pattern (D `OpenCodeDriver::start`, 435-614):
1. **Id storage.** Id lives in the app's own task record as `ProviderResumeCursor::OpenCode { session_id, directory: Option<String> }` (waku-protocol/src/model.rs:189-192). The driver emits `DriverEvent::Connected { provider_cursor }` right after create/resume (D:579-585) and the daemon persists it (daemon.rs:588, 1251, 1417). The server stores the session; the client stores only the id (+ canonical dir).
2. **Client-minted id.** `POST /api/session` accepts `id` (pattern `^ses`). Waku mints `ses_<12 hex time><random base62>` itself (A:874-898) and **subscribes to `/api/event` before creating** (D:488-492) so the first event cannot be missed. The format is load-bearing: free-tier models reject ids OpenCode could not have minted (A:868-873). Aether's current `{"clientID": ...}` body (`opencode.rs:338`) is not in the 2.x schema; the create body is `{id?, parentID?, title?, agent?, model?, location?, metadata?, permissions?}`.
3. **Verify exists.** `GET /api/session/{id}` -> 200 `{data: SessionInfo{id, projectID, agent, model, title, location{directory}, outcome, tokens...}}`, 404 when gone (`resumed_session`, D:~770-797; `ApiError::is_not_found` A:94). Other errors abort (not "recreate").
4. **Gone -> recreate under the same id** with `create()` (D:510-520). The comment (D:511-513): cursor can outlive session because the user's own client can delete it, so resume that finds nothing starts fresh under the same id.
5. **Resumed session keeps its stored agent/model**; Waku re-applies only an explicit selection: `POST /api/session/{id}/agent {agent}` / `POST /api/session/{id}/model {model: ModelRef}` (D:524-545; A:1169-1182).
6. **Reattach after the stream was down**: `reconcile` (D:1146-1260): `GET session` (404 => emit error + `ProcessExited`, D:1160-1168), `GET /api/session/{id}/permission`, `GET .../form`, `GET .../inbox`, `GET /api/session/active` (map keyed by session id; membership = still draining, A:1247-1262), then `gap_fill` from `GET /api/session/{id}/message?order=desc&limit=4` (D:1353-1399, append-only text/reasoning suffix + tool repair). `session.deleted` event is also terminal (D:1815-1822).
7. **Same session, many prompts**: nothing special. Each user turn is another `POST /api/session/{id}/prompt`; the driver stays alive across turns and settles on `session.execution.succeeded|failed|interrupted` (D:1497-1530), NOT `session.idle` (service never emits it, D:11-17).
8. **Closing** only unsubscribes from the hub; the service and session survive (D:Drop ~720-735). Fork: `POST .../fork {before?: messageId}`.

Aether changes for this: see list. Aether today (`agent.rs:190-262`): `send_cli` constructs a new `DriverStartOptions` + `OpencodeDriver::spawn` per message, which calls `POST /api/session` every turn and ships `context_prompt(history, text)` (`agent.rs:304`) as the text. Needed: a per-chat session id (persisted with the chat), spawn/attach once, plain `{"text": user_text}` thereafter, no history replay when the session was reattached (replay only when the session was recreated after 404).

## 2. Steer + queue (busy session)

- Endpoint: `POST /api/session/{id}/prompt` body `{ "text": "...", "delivery": "steer"|"queue" }` (A:1069-1087). Other body fields from spec: `id` (message id), `files[]`, `agents[]`, `skills[]`, `metadata`, `resume: bool|null` ("Durably admit one session input and schedule agent-loop execution unless resume is false"). Response 200 `{data: Inbox.User{id, delivery, ...}}`; 409 ConflictError exists. A prompt is always an **inbox entry**, not a direct run.
- Delivery omitted: server chooses. Idle session: server picks `steer` (D:972-977 comment: "it picks `steer` for a prompt into an idle session"). Busy session: Waku always says explicitly (A:1071-1075 doc).
- Plain prompt flow (D:967-1004): `begin_turn`, `POST prompt` with no delivery, remember `PendingInput{inbox_id, message, steer:false}`. On HTTP error emit error and settle turn itself (no execution will ever start).
- **Steer flow** (D:1005-1040): if no active turn -> `SteerRejected`. Else `POST prompt {text, delivery:"steer"}`; if the returned entry's `delivery != steer` (server queued anyway) -> `PATCH /api/session/{id}/inbox/{inboxId}` body `{"delivery":"steer"}` -> 204 ("Change a pending inbox item's delivery mode. Steering wakes session execution", A:1121-1135). A steered message joins the running execution (no new `execution.started`), so settle is idempotent (D:11-17).
- **Queue**: `delivery:"queue"` = parked until the current run finishes, then delivered as the next turn. Interrupt: `POST /api/session/{id}/interrupt?resume=true|false` (spec: "When resume=true, execution resumes pending steering input... while queued prompts remain parked").
- Inbox events (data keyed `inboxID`; only the POST answer says `id`): `session.inbox.enqueued`, `session.inbox.delivered` (clear pending, D:1676-1682), `session.inbox.cancelled` (steer -> `SteerRejected` so the app falls back to its own queue, D:1684-1699), `session.inbox.delivery.changed` (ignored list, D:1879). List undelivered: `GET /api/session/{id}/inbox`; cancel: `DELETE /api/session/{id}/inbox/{inboxId}`.
- Aether: `Driver` trait has only `prompt/cancel/answer`; add `steer(text)` (and optionally `queue`) that POST with `delivery`, track `inboxID`, and map cancel to "re-queue in app".

## 3. Access modes

Waku does **not** configure OpenCode for modes. Mode is purely client-side "who answers `permission.asked`" (D:911-934; module doc 33-36):
```
Ask            -> never auto-reply, surface a card
AutoAcceptEdits-> auto-reply once iff action in {edit, write, patch}
Auto|FullAccess-> auto-reply once to everything
```
(`auto_replies`, D:928-934; applied in `request_permission` D:2297-2345, auto path posts `Respond{once}` via the worker's own channel.) Agent is NOT chosen by mode: `resolve_agent` (D:817-829) is user preset or `"build"`. Mode changes apply in place (D:`apply_options`, no restart).

- `permission.asked` data (fixture D:3043-3053): `{id:"per_...", sessionID, action:"bash"|"edit"|..., resources:[patterns], save:[patterns], metadata?, source:{type:"tool", messageID, id:callId}, message?}`. Title shown = first resource.
- `permission.replied` / `permission.rejected`: id is `data.requestID` (asked uses `data.id`; the reply route takes the `asked` id) (D:1702-1712).
- Reply: `POST /api/session/{sessionID}/permission/{requestID}/reply` body `{"decision":"once"|"always"|"reject","message"?:string}` -> 204 (A:1195-1215). Waku **never sends `always`** (rejects client-side, A:1195-1200): it writes to the global `/api/permission/saved` store shared with the user's terminal. Remember "always" locally (support.rs:294-395).
- Snapshot of pending: `GET /api/session/{id}/permission` (D:1197-1210), `GET /api/permission/request` (global).
- **Server-side alternative that Waku does not use**: spec has `permissions: Permission.Ruleset` on `POST /api/session` and `PATCH /api/session/{id}`; rule = `{action, resource, effect: "allow"|"deny"|"ask"}`. Could pre-allow edits (`{action:"edit",resource:"*",effect:"allow"}`) or everything for FullAccess so no prompts arrive at all. Untested here; client-side auto-reply is the proven path. CLI hint: `opencode run --auto` = "Auto-approve permissions that are not explicitly denied".
- Aether today (`opencode.rs:419-430`) replies to `/api/session/{id}/permissions/{n}/reply` (plural, numeric id) with `{"allow":bool}`: wrong path, wrong id type, wrong body. Needs `/permission/{requestID}/reply {"decision":"once"|"reject"}`, using the real string id, not the renumbered counter (keep a map aether-id -> opencode request id).

## 4. Effort (variants)

- Effort is the model **variant**: `Model.Ref = { id, providerID, variant? }` (A:194-201; spec: `id`, `providerID`, `variant`, no `modelID`). Waku splits stored `"provider/model"` at the first `/` and sets `variant = reasoning_effort` when non-empty (`model_ref`, D:802-811). Examples in docs/tests: `low`, `high` (D:798-799, test D:3449-3460). Allowed values are per model; discover with `GET /api/model[?location[directory]=...]` (catalogue entry lists variants; Waku keeps only context windows, S:907-960; check `variants` field before building a picker). `opencode run -m provider/model#variant` confirms the `#variant` convention on the CLI.
- **No per-prompt field.** The 2.x `POST .../prompt` body has no `model`/`variant`/`agent`. Variant is per **session**: set at create (`model` in `POST /api/session`) or `POST /api/session/{id}/model {"model": ModelRef}` (A:1169-1175). `apply_options` (D:~1100-1130) calls `switch_model` when the ref (incl. variant) changed, then updates state.
- Aether today sends `body["model"] = "<string>"` (`opencode.rs:335`): schema wants the object. Convert `opts.model` -> `{providerID,id,variant}`.

## 5. Tool activity (2.x events)

All wrapped as `{"type": "...", "data": {...}}` on `GET /api/event`. Tool keying is `(assistantMessageID, id)` where `id` is the call id (`tool_key`, D:1933-1941; `assistantMessageID` from `session.step.started`, D:1507-1517).

| event | data fields used | Waku action |
|---|---|---|
| `session.step.started` | `assistantMessageID, agent, model` | set current step |
| `session.step.ended` | `tokens` | usage |
| `session.step.failed` | `error` | error |
| `session.text.started/.delta/.ended` | `assistantMessageID, ordinal, delta` / `text` | stream `TextDelta`; `.ended` emits nothing (D:1573-1578) |
| `session.reasoning.*` | same; shares ordinal namespace with text | `ReasoningDelta` |
| `session.tool.input.started` | `id, name, assistantMessageID` | emit tool row now, title=name (D:1581-1611) |
| `session.tool.input.delta` | `id, delta` (JSON text fragment) | accumulate; re-parse; only re-emit when `input.title` changes (D:1612-1640) |
| `session.tool.called` | `id, input{...}, executed:bool, state{...provider secrets}` | row with full input; `executed:false` = denied/interrupted, never render as done (D:1641-1655) |
| `session.tool.progress` | `id, metadata` | live metadata (long bash/grep) |
| `session.tool.success` | `id, content:[{type:"text",text}|{type:"file",uri,mime,name?}], metadata` | complete row (D:1991-2014) |
| `session.tool.failed` | `id, error{type,message}, content?, metadata` | complete row, failed |
| `session.execution.started/succeeded/failed/interrupted` | `error`, `reason` (`user`, `shutdown`, ...) | turn begin/settle; `shutdown` does NOT settle (D:1497-1530) |
| `session.retry.scheduled`, `session.status{status:{type:"retry"}}` | `error`, `action.title/message` | retry row (D:2053-2075) |

- Strip provider control state before display: `state` sibling of `input` and `metadata.resultState` hold multi-KB `reasoningEncryptedContent` (D:94-100, `strip_provider_state`).
- Persisted shape (same as `GET .../message`): `Assistant.content[]` of `{type:"text"|"reasoning"|"tool"}`; tool = `{type:"tool", id, name, executed, state{status: streaming|running|completed|error, input (JSON **string** while streaming, object after), content[], metadata, error}, time{created,ran,completed}}` (A:376-402, spec `Session.Message.Assistant.Tool`).
- Row build: all rows go through `activity::tool_activity(source_id=callId, kind=classify_tool(name), title, input, output, metadata, failed, complete)` (D:2019-2048; `input_title` reads `input.title`). Aether equivalent: one `Update` tool-row keyed by callId with pending/running/done/error, upserted by id (today it emits a fenced markdown block into text, `opencode.rs:532-535`).

## 6. Per-chat cwd on one shared server

One service serves every folder because each **session carries its own location**; the process itself sits in `$HOME` (A:203-207).
- Create: `POST /api/session` body `"location": {"directory": "<abs path>"}` (A:908-925; spec `Location.PublicRef{directory: AbsolutePath}`).
- List: `GET /api/session?directory=<same string>` (spec also `project`, `subpath`, `parentID`, `search`, `order`, `limit`, `cursor`; list query builder A:1033). Catalogues take `?location[directory]=...` (`location_query`, A:1437-1443): `/api/model`, `/api/agent`, `/api/command`, `/api/mcp`.
- The path must be **canonicalized, no trailing slash**, and reuse the exact same string for create and list: server compares by exact string, a subdirectory or trailing slash yields zero sessions, and an unresolvable dir gives HTTP 500 with empty body (D:464-481).
- No `x-opencode-directory` header is used by Waku (that is the v1 convention); v2 binds the directory to the session at create. A session's directory is fixed afterwards (there is a `POST /api/session/{id}/move`, unused by Waku).
- Aether today: `cwd` is computed in `send_cli` (`agent.rs:243-247`) and passed in `DriverStartOptions.cwd`, but `OpencodeDriver::spawn` never puts it in the create body, so sessions land in the service's cwd.

## Aether changes needed (ordered)

1. **Auth + discovery**: read `service.json` (url, password), Basic `opencode:<password>` on every request and the SSE connection; prefer adopting the running service over `serve --service --port 4096`. Verify 401 behaviour on an explicit port.
2. **Create body** (`opencode.rs:335-338`): `{id: client-minted "ses_..." , location:{directory: canonicalize(cwd)}, model: {providerID,id,variant?}, agent?}`; drop `clientID`. Subscribe to the hub keyed by the minted id before POST (kills the `client_id` -> `sid` rekey dance).
3. **Per-chat persistent driver**: store `{session_id, directory}` per chat in settings/chat file; `send_cli` attaches (GET session -> 200 reuse, 404 recreate under same id and replay history once) instead of `spawn`+`context_prompt` each turn. Keep the driver (or at least the hub subscription) alive across turns; unsubscribe on chat close only.
4. **Settle on `session.execution.succeeded|failed|interrupted`** (not `session.idle`); `shutdown` interrupt does not settle. After SSE reconnect run reconcile (`GET session`, `/permission`, `/active`, `/message?order=desc&limit=4` gap-fill).
5. **Rewrite `map_opencode_event`** for 2.x names/shapes in section 5; add tool-row `Update` upserted by callId (pending -> running -> done/error), strip `state`/`resultState`.
6. **Permissions**: surface `permission.asked` using `data.id` (string), `action`, `resources[0]`; reply `POST /api/session/{id}/permission/{id}/reply {"decision":"once"|"reject"}`; never send `always`; implement Ask/AutoAcceptEdits/Auto/FullAccess as client-side auto-reply (`edit|write|patch` for AutoAcceptEdits), changeable mid-session without restart. Keep an aether-id -> opencode-id map.
7. **Steer/queue**: add `Driver::steer(text)`; `POST prompt {text, delivery:"steer"}` + `PATCH inbox/{id} {"delivery":"steer"}` if not honored; track `inboxID`; on `session.inbox.cancelled` return the message to the app queue. Normal busy-send = `delivery:"queue"`.
8. **Effort**: add effort to `DriverStartOptions`; send as `model.variant` at create, and `POST /api/session/{id}/model` on change. Populate allowed values from `GET /api/model` (check the entry's variants field live before hardcoding `low|medium|high`).
9. **Cancel**: `POST /api/session/{id}/interrupt` (bare `{interrupted}` response) then wait for `session.execution.interrupted` instead of settling immediately (`opencode.rs:399-417`); consider `?resume=true` when a queued prompt exists.
10. Optional later: `permissions` ruleset on create/PATCH for server-side mode enforcement; `session.renamed` already handled; `GET /api/session/active` for startup status of restored chats.
