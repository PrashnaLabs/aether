# Orchestration adapters: per-harness flags (phase 0 spike, 2026-11-04)

How each CLI gets Aether's lead tools, measured against the installed versions with a dummy
MCP server (or a dummy Pi extension). Spec: `docs/superpowers/specs/2026-11-04-agent-orchestration-design.md`.

## Decision changes

- **Codex:** every launch also passes `-c mcp_servers.aether.default_tools_approval_mode="approve"`. Without it, ReadOnly (policy `never`) refuses the tool call outright. The `-c` args must be passed again on every respawn or resume.
- **Claude:** the flags hold. ReadOnly (`plan`) still asks before an `mcp__aether__*` call even with `--allowedTools`; that ask goes to the user. In the spike harness, `--permission-prompts host` alone produced no `can_use_tool` requests at all (it needed `--permission-prompt-tool stdio`). Check whether Aether's GUI still gets approval cards on 2.1.318.
- **OpenCode:** a dedicated `opencode serve --port 0` plus `OPENCODE_CONFIG_CONTENT` works, with two additions. The MCP entry needs `"codemode": false`, or the tools hide behind one `execute` tool. Poll `GET /api/mcp` until the server reports `connected` before the first prompt. The driver also needs its own API constructor for a url and password (no `service.json`). MCP tools never trigger permission asks.
- **Antigravity:** `--settings` rejects MCP servers. The only path is a one-time global `agy mcp add aether <exe> mcp`, which needs user confirmation in the UI. The registered server inherits env from the agy process, so it stays inert without a token. The model sees a generic `call_mcp_tool{ServerName,ToolName,Arguments}`, which tool-row mapping needs to handle. Pre-approve per session with `--settings '{"permissions":{"allow":["mcp(aether/*)"]}}'`.
- **Pi:** `-e <file>` holds, and the token passes through the process env. In Read-only mode, add the Aether tool names to `--tools`, or the tools load but stay inactive. Extension tools never ask for approval.

## Claude Code 2.1.318

Scripts and logs: `/tmp/aether-orch-spike/claude/` (`echo_mcp.py`, `mcp.json`, `run.py`, `calls.log`). Model: haiku.

### Working args

Aether's `launch_args` output plus:

```
--mcp-config /path/to/mcp.json --allowedTools mcp__aether
```

```json
{"mcpServers":{"spike":{"command":"python3","args":["/tmp/aether-orch-spike/claude/echo_mcp.py"],"env":{"SPIKE_TOKEN":"tok-123"}}}}
```

- `--mcp-config` works in `-p --input-format stream-json` mode. `system/init` lists `{"name":"spike","status":"connected","source":"dynamic"}` and tool `mcp__spike__echo`.
- `env` from the config reaches the server (server logged `SPIKE_TOKEN=tok-123`, and the tool result echoed it).
- Tool name the model sees: `mcp__<server>__<tool>` = `mcp__spike__echo`. It is deferred: the model first calls `ToolSearch select:mcp__spike__echo`, then the tool.
- Server-level `--allowedTools mcp__spike` pre-approves every tool of that server. Per-tool `mcp__spike__echo` also works. No per-tool names needed.
- Server handshake seen: a `server/discover` probe (id `server-discover-probe-1`, answered with `{}` by my dummy; a server should return a JSON-RPC error or ignore), then `initialize` (protocolVersion 2025-11-25), `notifications/initialized`, `tools/list`, `tools/call`.

### Approval table (did a `can_use_tool` control_request arrive for the MCP tool?)

Measured with `--permission-prompt-tool stdio` added (see caveat 1; without it no asks ever arrive).

| mode | no allowedTools | `--allowedTools mcp__spike` |
|---|---|---|
| manual (`manual`) | ASK | no ask, runs |
| AutoEdit (`acceptEdits`) | ASK | no ask, runs |
| Auto (`auto`) | no ask, runs (classifier allowed) | no ask, runs |
| ReadOnly (`plan`) | ASK | ASK (allow rule does NOT bypass plan mode) |
| Full (`bypassPermissions` + `--dangerously-skip-permissions`) | no ask, runs | no ask, runs |

Without the extra flag (Aether's exact args), the "ASK" cells become silent denials: tool_use is blocked, `result.permission_denials` lists it, and a `system/permission_denied` event is emitted.

### Resume

`--session-id <uuid>` first run, then `--resume <uuid>` with the same `--mcp-config` and `--allowedTools`: server reconnects (new process, `START` logged again), tool is callable, same approval behavior (allowed: no ask; not allowed: ask). `--mcp-config` must be passed again on every launch; it is not persisted in the session.

### Caveats

1. IMPORTANT, outside this task's scope but it blocks approval testing: with Aether's exact args (`--permission-prompts host`, no `--permission-prompt-tool`), I got no `can_use_tool` request even for builtin Bash `touch` in manual mode. Claude auto-denied instead. Adding `--permission-prompt-tool stdio` made asks arrive. Reproduced with env stripped of all `CLAUDE*` vars and with `CLAUDE_CODE_SESSION_ATTENDED=1`, and after waiting for the initialize response, so not caused by my nested-session env. Worth checking whether the Aether GUI really gets ask cards on 2.1.318, or whether my harness differs in some way I missed.
2. Startup latency: `system/init` at about 4 to 6 s (spikes to 10 to 15 s seen), mostly the user's own MCP/claude.ai connectors; the spike server itself connects fast.
3. No MCP trust prompt appeared for `--mcp-config` servers in -p mode. No stderr noise (stderr empty in all runs).
4. `--mcp-config` is additive: user and claude.ai connector servers still load (`--strict-mcp-config` would suppress them; Aether does not use it today, so a lead chat also sees those).
5. `plan` mode asks for the MCP tool even when allowed, so lead tools in ReadOnly produce an approval card (or auto-allow in the host answer if wanted).

### Decision changes vs `--mcp-config <0600 temp file>` + `--allowedTools mcp__aether`

None to the flags: both hold as assumed, including the server-level allow and resume. Notes only: the allowlist does not cover `plan` mode, and the host must handle `can_use_tool` for `mcp__aether__*` in ReadOnly (and in manual/acceptEdits if the allowlist is omitted).

## Codex 0.166.0

Spike code: `/tmp/aether-orch-spike/codex/` (`echo_mcp.py`, `drive.py <policy> <sandbox> <sandboxType> <reviewer> [resumeId] [-- extra codex args]`, `calls.log`). Model used: `gpt-5.6-luna`. Runs were via ChatGPT auth.

### Working args (copy-pasteable)

```
codex app-server \
  -c 'mcp_servers.aether.command="/path/to/bin"' \
  -c 'mcp_servers.aether.args=["mcp","--sock","/tmp/x.sock"]' \
  -c 'mcp_servers.aether.env={SPIKE_TOKEN="t"}' \
  -c 'mcp_servers.aether.default_tools_approval_mode="approve"'
```

- `-c` overrides on `codex app-server` are honored. Server spawned by the app-server (once per process-level MCP manager; a second instance appears when `mcpServerStatus/list` is called). `env` reaches the server (`SPIKE_TOKEN=t` in every logged request, none null). Codex also inherits its own env.
- MCP handshake: protocolVersion `2025-06-18`, clientInfo `codex-mcp-client`, then `notifications/initialized`, `tools/list`, plus `resources/list` and `resources/templates/list` (answer with `{}`/empty or an error; both tolerated).
- Config is process-wide, not per-thread: `thread/start` params have no mcp field. One codex process per Aether session (as `codex.rs:177` does) makes it per-session anyway. Startup shows `mcpServer/startupStatus/updated` notifications (`starting` then `ready`) for `spike`, already handled at `codex.rs:1188`.

### Tool name

App-server item: `{"type":"mcpToolCall","server":"spike","tool":"echo","arguments":{"text":"hi"}}`. Model-facing name not read from the rollout (Codex convention is `mcp__<server>__<tool>`); the model called it fine from the prompt "echo tool (MCP server spike)". Server name `aether` gives server `aether`, tool names as the server lists them.

### Approval table (default config, no pre-approve)

Request is `mcpServer/elicitation/request`, `mode:"form"`, `_meta.codex_approval_kind:"mcp_tool_call"`, `_meta.persist:["session","always"]`, `message:'Allow the spike MCP server to run tool "echo"?'`. Reply `{"action":"accept","content":null}`. Already handled at `codex.rs:1865` (ApprovalKind::Mcp).

| Aether mode | policy / sandbox / reviewer | Result |
|---|---|---|
| Ask | untrusted / read-only / user | elicitation approval, then runs |
| ReadOnly | never / read-only / user | NO prompt, call FAILS: "MCP tool call requires approval, but approval policy is never" |
| AutoEdit | untrusted / workspace-write / user | elicitation approval, then runs |
| Auto | on-request / workspace-write / auto_review | no prompt, auto reviewer allowed it, ran |
| Full | never / danger-full-access / user | no prompt, ran |

### Pre-approve key

`mcp_servers.<name>.default_tools_approval_mode` = `"auto" | "prompt" | "writes" | "approve"` (enum `AppToolApproval` in the generated schema). Per tool: `mcp_servers.<name>.tools.<tool>.approval_mode`.

Verified with `"approve"`:
- Ask row (untrusted): no elicitation, call ran.
- ReadOnly row (never): call ran (fixes the failure above).
- Auto row: ran.
- Per-tool `-c 'mcp_servers.spike.tools.echo.approval_mode="approve"'` on ReadOnly: ran.

So the Codex lead chat needs `default_tools_approval_mode="approve"` for the aether server in every mode; otherwise ReadOnly hard-fails. Caveat: `approve` also skips the prompt in Ask/AutoEdit, which matches "orchestration tools are pre-trusted".

### Resume

`thread/resume` (with `excludeTurns:true`, same params as Aether) in a NEW app-server process: MCP server re-spawned and `ready`, tool call worked when the same `-c` args were passed. Without the `-c approve` arg on the resumed process, ReadOnly row failed with the approval-policy-never error, so config is NOT stored in the thread; Aether must pass the same `-c` args on every spawn (start and resume). Note: two processes resuming the same thread at once made one resume fail; irrelevant for Aether.

### Timeouts and caveats

- `tool_timeout_sec` (per server) exists; unset, a 75 s tool call completed, so default is > 75 s (Codex default is 120 s per config key docs in binary strings; not tested past 75 s). `startup_timeout_sec` default 10 s: server must answer `initialize` and `tools/list` quickly.
- `tools/call` request `_meta` carries `threadId`, `sessionId`, `itemId`, `callId`, `x-codex-turn-metadata` (session/thread/turn ids, model, sandbox mode). Server can use `_meta.threadId` to tell which Aether session called, no extra env needed.
- A server error in `tools/call` (my buggy `{}` result) surfaces as item `status:"failed"`, "Unexpected response type"; the turn continues.
- Codex runs a skills read first in this user's setup (superpowers plugin); in AutoEdit/Ask rows that produced extra `item/commandExecution/requestApproval` for harmless `sed`/`cat`, unrelated to MCP.
- `~/.codex/config.toml` is read too: user's own `mcp_servers` merge with the `-c` ones; `codex_apps` server also starts.
- Server-side `resources/list` is called; return `{"resources":[]}` or method-not-found.

### Decision changes vs assumption "`-c mcp_servers.aether.*` overrides on app-server"

- Assumption holds (command, args, env all work via `-c`).
- ADD `-c 'mcp_servers.aether.default_tools_approval_mode="approve"'`; without it ReadOnly mode (policy `never`) fails every aether tool call, and Ask/AutoEdit prompt each call.
- Args must be repeated on resume spawns; not persisted in thread.
- Tool-call origin can be identified from `_meta.threadId` in `tools/call`.

## Pi

Version: `pi --version` = 1.3.0 (managed install, `~/.pi/agent/install/releases/1.3.0`). Docs: `.../node_modules/@earendil-works/pi-coding-agent/docs/extensions.md`, `rpc-extension-ui.md`. Verified end to end in rpc mode with a throwaway extension (`/tmp/aether-orch-spike/pi/ext.ts`) on model `openrouter/nvidia/nemotron-3-ultra-550b-a55b:free` (most free/cheap models were 429/400/401/402 during the spike; unrelated to the extension).

### Working launch

```
pi --mode rpc -e /abs/path/aether-ext.ts [--tools read,grep,find,ls,aether_echo --exclude-tools 'mcp__*'] [--session X] [--approve]
```

- Tool appears and is called in rpc mode. Events: `tool_execution_start {toolName:"aether_echo",args:{text:"hi"}}` then `tool_execution_end {result:{content:[{type:"text",text:...}]},isError:false,durationMs:5}`. Same events as built-in tools, so the existing driver handling should work unchanged.
- Env passthrough works: extension runs inside the pi process, `process.env.SPIKE_TOKEN` set by the parent (Aether) was visible both at load and in `execute`. Token in process env is fine.
- Extension is TypeScript, loaded by `jiti`: no compile step. `import type { ExtensionAPI } from "@earendil-works/pi-coding-agent"` and `import { Type } from "typebox"` resolve even from a file in /tmp with no node_modules. Plain `.js` should also load (docs: "direct TypeScript or JavaScript files"; not tested).
- Startup: extension load adds no noticeable cost (first ui event ~0.8s total, same as without).

### Read-only allowlist

Yes, the name must be added. With `--tools read,grep,find,ls` the extension LOADED (log shows `load`) but the tool was not active: model only had read/grep/find/ls, never saw `aether_echo`. With `--tools read,grep,find,ls,aether_echo` the call worked. So in `launch_args` read-only branch: `"read,grep,find,ls,aether_echo"` (or a joined list of whatever Aether tool names it registers). `--exclude-tools mcp__*` does not affect it. Unlisted/disabled tool calls return `Tool bash not found` as an `isError` result (no crash).

### Approval

None. No `extension_ui_request` confirm/select was emitted for the extension tool in either mode; only unrelated `setStatus`/`notify` from the user's installed ponytail package. Pi does not gate tool calls by default (security.md: "does not ask for approval before every tool call"). An approval only appears if some extension hooks `tool_call` and calls `ctx.ui.confirm` (arrives as `extension_ui_request method:"confirm"`, which pi.rs `extension_ui` already maps to a Form/Ask). `--approve` is project-trust (project-local resources), not tool approval, and is not needed for explicit `-e` paths.

### Socket and timeouts

- `node:net` `createConnection(unixPath)` from `execute` works; round trip through a python listener returned `{"ok":true,"echo":{"text":"hi","token":"tok-123"}}` in 5 ms.
- No per-tool timeout found in extensions.md; `ctx.signal`/execute's `signal` param is the abort path (user stop). Not stress-tested with a long call. For long-running lead tools, honor the abort signal and close the socket.

### Skeleton

```ts
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import net from "node:net";
export default function (pi: ExtensionAPI) {
  pi.registerTool({
    name: "aether_echo", label: "Aether Echo", description: "...",
    parameters: Type.Object({ text: Type.String() }),
    async execute(_id, params, signal) {
      const reply = await new Promise<string>((res, rej) => {
        const c = net.createConnection(process.env.AETHER_SOCK!);
        let b = "";
        c.on("connect", () => c.write(JSON.stringify({ token: process.env.AETHER_TOKEN, ...params }) + "\n"));
        c.on("data", d => (b += d)); c.on("end", () => res(b.trim())); c.on("error", rej);
      });
      return { content: [{ type: "text", text: reply }], details: {} };
    },
  });
}
```

Docs say do not open sockets in the factory; open inside `execute` (as above) or `session_start`.

### Caveats

- The extension file must exist on disk: Aether must write/embed it (e.g. `include_str!` then write to its data dir) and pass the absolute path.
- User-installed packages/extensions (superpowers, ponytail, pi-opencode-free in ~/.pi settings) also load and emit `setStatus`/`notify` ui requests in rpc mode. Not caused by us; `--no-extensions` still honors explicit `-e` if Aether wants a clean lead session (not tested).
- Extensions run in-process with full user permissions; the tool is only as sandboxed as pi is. Read-only mode still relies on the allowlist.
- Only the factory runs when pi loads extensions without a session; keep it side-effect free.
- Tool names: lowercase `[a-z0-9_]`; keep `aether_` prefix to avoid colliding with `mcp__*` exclusion.

### Decision changes vs assumption "`-e <aether extension>`, token in process env"

None. Confirmed as is. Only addition: append each Aether tool name to the `--tools` allowlist in Read-only mode (without it the tool is silently inactive). No approval handling needed.

## OpenCode 2.2.5

Status: hypothesis confirmed, with one tool-name caveat (set `codemode:false`). Throwaway files: `/tmp/aether-orch-spike/opencode/` (`echo_mcp.py`, `calls.log`, `ev-*.log`, `openapi.json`).

### Launch (copy-pasteable, verified)

```sh
OPENCODE_CONFIG_CONTENT='{"mcp":{"aether":{"type":"local","command":["/path/to/aether-mcp","--lead","<id>"],"environment":{"SPIKE_TOKEN":"t-lead"},"codemode":false}}}' \
  opencode serve --port 0 --hostname 127.0.0.1
```

- `type:"local"`, `command` is an array (argv), `environment` is a string map. Schema `Mcp.LocalConfigEncoded` also has `cwd`, `disabled`, `codemode`, `timeout{startup,catalog,execution}`.
- `environment` reached the server (`SPIKE_TOKEN=t-lead` in calls.log and in the tool result). MCP child cwd = session/serve cwd. Parent env passthrough not tested; put everything needed in `environment`.
- `--port 0` works. Stdout prints `server listening on http://127.0.0.1:<port>` and `server password <pw>` (when `OPENCODE_SERVER_PASSWORD` is unset). Same Basic auth as the shared service: user `opencode`, that password. Aether can either parse stdout or pre-set `OPENCODE_SERVER_PASSWORD` and read the port line. No `service.json` is written for the dedicated server (no `--service`), so it cannot be adopted by `read_service()`; Aether must hold the port/password itself.
- Same HTTP API as the shared service: `POST /api/session` body from `create_body`, `POST /api/session/{id}/prompt`, SSE `GET /api/event`. Nothing in `session.rs`/`events.rs` needs to change besides a per-driver `Api` pointing at this server.
- Startup: ready (listening + `/api/info` answers) in about 2 s; MCP child connects asynchronously after that (`GET /api/mcp` shows `connected`).
- Kill the child on lead close; it is a normal process (no reaping concern like the shared service).

### Tool name the model sees

- Default (`codemode` on): MCP tools are NOT direct tools. The model sees one `execute` tool and writes `return await tools.spike.echo({text:"hello"})`. Aether's event stream shows `session.tool.called` with name `execute` and `input.code`; result metadata has `toolCalls:[{tool:"spike.echo"}]`. Ugly for Aether's tool cards.
- With `"codemode": false` on the mcp entry: direct tool named `<server>_<tool>` = `spike_echo`, input `{text}` as normal. Use this.
- RACE: a session prompted before the MCP server reached `connected` did not get the tool (model only saw `opencode.*` catalog and gave up). Poll `GET /api/mcp` until the server is `connected` before the first prompt.
- The MCP `tools/call` carries `_meta["ai.opencode/sessionID"]`, so the MCP server can tell which OpenCode session called it (useful for lead identification, no extra plumbing).

### Permissions / approvals

- Default build agent + Aether's `rules()` (ask/deny only `edit`, `shell`): MCP tools run with NO `permission.asked` in every mode tried: ask-style rules, read-only deny edit/shell rules, and `[]`. So even Aether `ReadOnly` does not block the lead's tools. That is probably what we want for orchestration tools, but note that nothing gates them.
- Permission action for an MCP tool = the tool name (`spike_echo`), `resources:["*"]`, `save:["*"]`, `metadata.input` = tool args, `source:{type:"tool",...}`. Verified by a session rule `{"action":"*","resource":"*","effect":"ask"}` which produced `permission.asked` with `action:"spike_echo"`.
- Session rule `{"action":"spike_echo","resource":"*","effect":"deny"}` blocked the call (server saw no `tools/call`). Rule `{"action":"mcp",...ask}` had no effect. Wildcard `spike_*` deny was inconclusive (a second server existed in that run), do not rely on it.
- Aether `auto_approves` (events.rs:51): `Full` approves everything, `AutoEdit` only edit/write/patch, `Ask/Auto/ReadOnly` approve nothing. If we ever add an ask rule for lead tools, `AutoEdit`/`Ask` users would see a card for `spike_echo`-style actions; `Full` auto-approves. Phase 2b decision: leave ungated (default) or add `{action:"aether_*"...}` after confirming wildcard syntax.

### Shared service unaffected

- Dedicated server is a separate process/port with its own in-memory MCP state. `GET /api/mcp` on the shared service (pid 1133, `--service`) stayed `data:[]` before and after; its pid/uptime untouched. `OPENCODE_CONFIG_CONTENT` is per-process env, so no global config is touched. Both servers share `~/.local/share/opencode/opencode.db` (sessions created on the dedicated server are stored in the same DB; harmless, but a session id lives in both views).

### Alternatives checked

- Per-prompt tool toggle: NOT in 2.x. Prompt body schema is only `id,text,files,agents,skills,metadata,delivery,resume`. Session create body is `id,parentID,title,agent,model,location,metadata,permissions`. No `tools` map.
- Runtime add: `PUT /api/experimental/mcp/{server}?location[directory]=<dir>` body `{"config":{...Mcp config...}}` works on a live server and connects immediately (verified). But MCP state is scoped to the `location.directory`, not the session: every session in that directory sees it. Query is deepObject form (`location%5Bdirectory%5D=`; JSON string errors). Using it on the shared service would leak the tools to other chats in the same project dir; only usable together with per-session deny rules on everyone else. Not recommended.
- Plugin with sessionID: not tried (would need a plugin file in global/project config, which is out of scope and not per-chat anyway).
- `opencode serve --stdio` exists (flag listed) but untested; no need.

### Caveats

- A second `opencode` process per lead costs memory (the shared one is ~600 MB RSS; fresh one smaller) and a separate event stream; `Api::probe`/`service_api()` assume the shared `service.json`, so the lead driver needs its own Api constructor taking url+password.
- Free-tier model used for testing was `opencode/deepseek-v4.1-flash-free`; `google/*` key in auth.json is invalid and `opencode/gpt-5.4-nano` returned 402. Global CLAUDE.md-style instructions leak into OpenCode runs on this machine (model replied "WARDEN - ").
- Resume: MCP config comes from the launching env, so resuming a lead session means relaunching the dedicated server with the same `OPENCODE_CONFIG_CONTENT` (not tested after restart).

### Decision changes vs assumption "dedicated `opencode serve` per lead with OPENCODE_CONFIG_CONTENT"

1. Assumption holds. Add `"codemode": false` to the mcp entry or the tool is hidden behind `execute`.
2. Tool name is `<server>_<tool>`; pick the server key so names read well (e.g. server `aether` -> `aether_spawn_agent`).
3. Wait for `/api/mcp` connected before first prompt.
4. Use `--port 0` and parse the stdout port/password lines; no `service.json` for it.
5. Tools are ungated by Aether's current rules in all access modes; gating is opt-in via a session rule on the exact tool name.

## Antigravity

Tested with `agy` 1.5.0, model `gemini-3.8-flash-low`, launch shape copied from `antigravity.rs::launch_args` (no `-p`; `--input-format stream-json --output-format stream-json`; user line `{"event":"user","message":{"content":"..."}}`).

### 1. `--settings` does NOT accept MCP servers

`--settings` validates keys strictly: `mcpServers`, `mcp`, `mcp_servers`, `mcpConfig`, `plugins`, ... all fail with `Error: --settings: unknown setting "<key>"` (exit 1). MCP servers live only in `mcp_config.json`:

- Global: `~/.gemini/config/mcp_config.json` (`{"mcpServers":{name:{command,args,env}}}` or `{url}`), managed by `agy mcp add|remove|list|enable|disable`.
- Plugin: `plugins/<name>/mcp_config.json` (active only while plugin enabled; servers namespaced `<plugin>_<server>`).
- agy docs (embedded) mention project-level `<root>/.agents/` for skills/rules/plugins. Not tested; would write into the user's repo, so rejected.

No session-only MCP form found. `--settings` IS useful for permissions (see 3).

### 2. Fallback: global registration works, env IS inherited

```
agy mcp add spike python3 /tmp/aether-orch-spike/agy/echo_mcp.py   # no --env
SPIKE_TOKEN=inherited-ok agy --input-format stream-json --output-format stream-json --model gemini-3.8-flash-low --mode accept-edits ...
```
Server log: `START SPIKE_TOKEN='inherited-ok'`. Same registration, agy launched without the var: `START SPIKE_TOKEN=None`. So a per-session secret exported only in the agy child env reaches the stdio server.

Inert outside Aether: agy still spawns the server in EVERY session (init + tools/list at startup, also from the user's own terminal), so the server must exit/serve zero tools itself when `SPIKE_TOKEN`/socket env is missing. Registration with a fixed command can be a small `aether mcp-shim`-style binary that does exactly that. Reuse of `agy mcp add --env KEY=val` is not needed (and would bake the token into the file).

### 3. Tool name and approvals

- Model sees ONE generic tool, `call_mcp_tool{ServerName, ToolName, Arguments}` (stream event `tool_name:"call_mcp_tool"`, parameters `{"ServerName":"spike","ToolName":"echo","Arguments":{"text":"hi"}}`). MCP tools are loaded on demand: agy writes `~/.gemini/antigravity-cli/mcp/<server>/<tool>.json` and the model `read_file`s it first. Aether's tool mapping (`antigravity.rs` ~989-1050) needs a `call_mcp_tool` case, else it renders as unknown.
- Permission target: `mcp(<server>/<tool>)`; allow-rule patterns per agy docs: `serverName/toolName`, `serverName/*`, `*`.
- Denial text: `permission check failed for mcp "spike/echo": user denied permission ...`; result carries `denied_actions:[{"action":"mcp","display_name":"CallMcpTool"}]` (feeds Aether's existing "Rerun with full access?" flow, display name is `CallMcpTool`, `pascal("call_mcp_tool")` matches it).

| launch | MCP call |
|---|---|
| `--mode accept-edits` | denied (headless can't prompt), server never receives tools/call |
| `--mode auto` | this account: `warning: --mode auto is not available for this account; continuing in the default mode.` then denied like default |
| `--mode plan` | model only writes a plan, no call |
| `--dangerously-skip-permissions` | call succeeds |
| `--mode accept-edits --settings '{"permissions":{"allow":["mcp(spike/*)"]}}'` | call succeeds, session-only |

`--settings` permission allow-rule does not persist: `~/.gemini/antigravity-cli/settings.json` sha1 unchanged after the run.

### Copy-pasteable launch (lead chat)

Once registered once (`agy mcp add aether <aether-shim> ...`):
```
AETHER_TOKEN=<per-session> agy --input-format stream-json --output-format stream-json \
  --mode accept-edits --settings '{"permissions":{"allow":["mcp(aether/*)"]}}' [existing args]
```
Append the `--settings` arg in `launch_args` for every non-`full` mode; `full` already skips permissions.

### 4. Caveats

- Global side effect: registration edits the user's `~/.gemini/config/mcp_config.json` (and `agy mcp remove` leaves `{"mcpServers": {}}`, 23 bytes, vs original 0-byte file). Needs explicit consent UI / one-time setup in Aether; uninstall path = `agy mcp remove aether`.
- Server is spawned in every agy session of the user; shim must be cheap and silent without the env var.
- Server receives `server/discover` (2026 MCP draft) before `initialize`; the dummy answered `{}` and agy fell back fine. Real server should return a JSON-RPC error or ignore.
- Clean state confirmed: `agy mcp list` before and after both "No MCP servers configured."; I also removed the `~/.gemini/antigravity-cli/mcp/spike/` cache agy created. Test registration lasted a few minutes.
- `--mode auto` unavailability is account dependent, pre-existing Aether behavior.
- Not tested: `--conversation` resume, subagents calling MCP, multiple concurrent agy processes sharing the registration (expected fine, env is per-process).

### Decision changes vs assumption

- Assumption "`--settings` if it accepts MCP servers, else `agy mcp add` + env passthrough": branch 2 holds. `--settings` rejects MCP keys; `agy mcp add` + process-env passthrough works.
- Add: `--settings` permissions allow-rule `mcp(<server>/*)` so lead-chat tools do not trip approval in accept-edits (plan mode will not call tools at all).
- Add: Aether must map `call_mcp_tool` events, and the server must self-disable without the Aether env var.

