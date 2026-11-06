# Research: observing external coding-agent sessions (agent manager)

Read-only probes on this Mac, 2026-10-14. Nothing killed, signalled or prompted. Message content and secrets redacted; `~/.claude/sessions/*.key` and `auth.json` files deliberately not read.
Installed: `claude` 2.1.299, `pi` 1.2.0, `opencode` 2.0.28, **`codex` 0.162.x** (`~/.local/bin/codex`). Not installed: amp, cursor-agent, gemini, aider.
Reference app (Waku) has no external-session tracking: `rg "session/active|lsof|pidfile"` in `reference/crates` only hits `Mutex::lock`. Nothing to reuse.

## 0. Processes (shared)

`ps -axo pid,ppid,etime,state,command` (634 procs, 0.03s). Observed rows, paths shortened:

```
6414     1 01-01:05:10 Rs  ~/.opencode/bin/opencode serve --service          # shared background service, ppid 1
10537 7707 01-01:03:35 S+  opencode                                           # OpenCode TUI, ppid = login zsh
90078 7728    05:50:07 S+  ~/.local/bin/claude                                # Claude TUI (argv has no flags/ids)
53814 3347 01-00:27:46 S+  codex                                              # Codex TUI
15932 11021 ...            ~/.codex/packages/app-server-daemon/.../codex app-server --listen unix://   # Codex daemon, not a session
99042 99016    01:30:40 S  ~/.local/bin/claude --chrome-native-host           # helper, NOT a session
18778 7720       01:46 S+  target/debug/aether
```

- TUI processes have the bare program name as argv (`claude`, `codex`, `opencode`), parent = login shell (`-zsh`), state `S+` (foreground in a tty). Pi runs via a `#!/bin/sh` launcher in `~/.pi/agent/bin/pi` that execs node; no Pi was running, so its argv is unverified. Match on `ps` command basename and fall back to exe path.
- Helpers to exclude: `claude --chrome-native-host`, `opencode serve --service`, `codex app-server*`, `codex-code-mode-host`, `sesh`, `tmux`.
- Print/RPC children (aether-spawned) have flags (`-p --input-format stream-json`, `--mode rpc`, `opencode serve`) and `ppid == std::process::id()`. Skip rows with ppid == own pid, or whose tree contains own pid.
- cwd: `lsof -a -p 90078,10537,53814 -d cwd -Fpn` gives `p<pid>` / `n<cwd>` lines. 0.02s for 4 pids; `lsof -a -d cwd -Fpn` for ALL procs 0.15s. Always pass explicit pids.
- Env of other processes (`ps eww`) was not needed; avoid (can leak secrets).
- A TUI inside tmux: Claude registry records `"tmux":"aether:@11.%13"` (session:window.pane), usable to jump to the pane.

## 1. Claude Code

**(a) Discover.** Best source is a self-registry: `~/.claude/sessions/<pid>.json` (one per live process, 564 bytes, mode 644; sibling `<pid>.<hash>.key` is a credential, do not read). Real keys (values elided where private):

```
{"pid":90078,"sessionId":"093cf769-...","cwd":"/Users/iris/Downloads/xzx/RUST/aether",
 "startedAt":1791973044526,"procStart":"Wed Oct 14 10:17:23 2026","version":"2.1.299",
 "kind":"interactive","entrypoint":"cli","pidDomain":"darwin","tmux":"aether:@11.%13",
 "messagingSocketPath":"/tmp/cc-socks/90078.sock","name":"<derived title>","nameSource":"derived",
 "status":"waiting","waitingFor":"input needed","updatedAt":1791994053815,"statusUpdatedAt":1791994053815,
 "peerProtocol":1,"peerFeatures":["notify_idle",...]}
```

Only live sessions had a file (stale pids' `/tmp/cc-socks/*.sock` remain, so ignore sockets). Validate each entry with `kill(pid,0)` (or `ps -p`) and compare `procStart` to guard pid reuse. `ps` + `claude` basename remains the fallback for older versions lacking the registry (not confirmed which version added it).
`~/.claude/ide/` is empty (IDE lock files only appear with the IDE extension; not seen). `~/.claude.json` lastSessionId not needed. `entrypoint` values seen in binary: `cli`, `print` (aether-style `-p` children use `print`; aether-spawned print sessions did not leave a registry file in my probe, unverified).

**(b) Map.** `sessionId` + `cwd` come straight from the registry. Transcript = `~/.claude/projects/<cwd with / and . replaced by ->/<sessionId>.jsonl` (e.g. `-Users-iris-Downloads-xzx-RUST-aether/093cf769-....jsonl`, 4.9 MB). Cwd can change mid-session so prefer scanning project dirs for `<sessionId>.jsonl` only when the encoded path is missing. Resume (`--resume`/`--fork-session`) makes a new id; the registry follows the live id.

**(c) State.** Registry `status` is authoritative and cheap: strings in the binary show `status` in `idle | busy | waiting`, with `waitingFor` free text (`"input needed"` seen; a tool prompt likely reports `"tool permission"`, not observed). Update age = `now - statusUpdatedAt` (ms). Mapping: busy = working, waiting = needs user, idle = open/idle. Process gone = finished.
Transcript tail (fallback/enrichment), real entry types: `user`, `assistant`, `attachment`, `system`, `queue-operation`, `file-history-snapshot`, plus bookkeeping `last-prompt`, `ai-title`, `mode`, `permission-mode`, `atis-latch` (written after every turn, so skip them when finding the last conversational entry). Last real entries here: `assistant[tool_use]` then `user[tool_result]`. Assistant message `stop_reason:"tool_use"` with no following `tool_result` = tool running or awaiting approval; `end_turn` = idle. Timestamps are not on every entry; use file mtime. Caveat: file mtime (21:52) was later than the last timestamped message (16:07), so mtime is a weak "activity" signal; prefer registry.
Title: last `ai-title` entry (`aiTitle`) or registry `name`.

**(d) Act.** Claude TUI: Esc interrupts the turn; Ctrl-C also cancels the turn. A real SIGINT to the pid is not verified. A registry `messagingSocketPath` (unix socket, `peerProtocol:1`) exists for peer messaging; protocol undocumented, do not use. Safest: SIGINT, then offer SIGTERM only on explicit user confirm. Never SIGKILL a TUI (leaves tty raw, loses unsaved state).

**(e) Cost.** `read_dir ~/.claude/sessions` + read N small json files: ~0 ms. Transcript tail (`tail -c 64K`): <1 ms. Never parse the whole file (MBs).

## 2. Pi

**(a) Discover.** No pid, lock or socket files found in `~/.pi/agent` (entries: `agents AGENTS.md bin extensions install sessions settings.json trust.json ...`). Docs (`rpc.md`, `sessions.md`, `session-format.md`) mention none either. Only route: `ps` (command basename `pi`, or `node ... pi-coding-agent`; `--mode rpc` / `-p` = headless child) then `lsof -d cwd`. No live Pi to confirm argv; treat as unverified.

**(b) Map.** `~/.pi/agent/sessions/--<cwd, / replaced by ->--/<ts>_<uuid>.jsonl`, e.g. `--Users-iris-Downloads-xzx-RUST-aether--/2026-10-14T15-42-10-185Z_01a13b14-....jsonl`. Header line 1: `{"type":"session","version":3,"id":"01a13b14-...","timestamp":"...","cwd":"/Users/..."}`. Process to file: take the process cwd, encode, list that dir, pick the newest by mtime whose mtime >= process start. Ambiguous when two Pi TUIs share a cwd (both write; pick by recent mtime, or do not disambiguate). Pi does not hold the file open (same as Codex; lsof showed no jsonl for any agent), so lsof on files does not help.

**(c) State.** Entries: `session`, `message` (role `user|assistant|toolResult`, assistant has `stopReason` e.g. `error`/`toolUse`/`stop`), `context_edit`. Rules: last message `assistant` with `stopReason:"toolUse"` or `role:"toolResult"` last = working; assistant `stop`/`error` and file quiet >N s = idle; last `user` = just prompted, working. Pi has no built-in approval gate (approval only via extensions' `extension_ui_request`), so there is no "waiting for approval" state from transcript alone. RPC procs expose `get_state` (`isStreaming`, `isCompacting`, `pendingMessageCount`) but only to the owning stdin, so an outsider cannot use it.

**(d) Act.** TUI keybindings (`keybindings.md`): `app.interrupt` = Esc; `app.clear` = Ctrl-C (clears editor, second press exits). So SIGINT behaviour is likely exit/clear, not interrupt; do NOT rely on it. Safe option for outsiders: none for TUI except SIGTERM (ends the session; transcript persists). For aether-owned RPC children use `{"type":"abort"}`.

**(e) Cost.** `ps` + one `lsof` + `read_dir` of one session dir + tail 64 KB. Session files reach 76 KB-MBs; tail only.

## 3. OpenCode

**(a) Discover.** One shared background service (`opencode serve --service`, pid 6414, registered in `~/.local/state/opencode/service.json`, Basic auth). TUI processes (`opencode`, pid 10537) are clients: `lsof -i` shows one TCP socket to the service (`127.0.0.1:57948->127.0.0.1:49374`) and an open `~/.local/share/opencode/log/opencode.log`. `~/.local/state/opencode/locks/` is empty; `session.json` = `{"pinned":[]}`. So processes via `ps`; sessions via API (all clients' sessions live on the service).
`opencode api GET <path>` handles auth (0.06s per call). `GET /api/session/active` returned `{"data":{}}` (idle now; shape when active unverified, do not start a turn to see). Aether-spawned opencode children are `serve`/run processes owned by aether; its service client sessions appear in the same list.

**(b) Map.** `GET /api/session?limit=50` returns `{"data":[SessionInfo]}` with keys `id (ses_...), projectID, parentID, agent, model{id,providerID,variant}, title, cost, tokens{input,output,reasoning,cache}, outcome, time{created,updated,idle}, location{directory}, permissions[]`. `GET /api/session/{id}` gives one. Process to session: TUI cwd (lsof) == `location.directory` and `time.updated` newest; multiple TUIs in one dir are ambiguous.

**(c) State.** `outcome` seen: `succeeded`, `failed`; `time.idle` ms epoch = when the last run ended. Working = id present in `/api/session/active`. Waiting for approval = non-empty `GET /api/permission/request` (global; returned `{"location":{"directory":"/Users/iris"},"data":[]}`) or `GET /api/session/{id}/permission`. Idle-but-open = TUI process exists with session cwd, not active, no permissions. Finished = `outcome` terminal and no TUI. Existing note: SSE `GET /api/event` (`session.execution.*`, `permission.asked`) gives push updates, but polling is simpler for a manager.

**(d) Act.** `POST /api/session/{id}/interrupt?resume=false` (bare response, no envelope; from Waku A:1142). Clean, no signal. Do not kill the shared service (pid 6414): it hosts every client.

**(e) Cost.** 0.06s per `opencode api` spawn (process start dominates). Rust side: `curl`/raw TCP to `service.json` URL with Basic auth is cheaper but reuses existing aether opencode driver code for URL/auth; prefer that over spawning the CLI.

## 4. Codex (bonus, installed)

- Processes: TUI = bare `codex` (53814, 42634, ppid zsh). Daemon `codex app-server --listen unix://` (pid 15932) and `codex-code-mode-host` are NOT sessions. `~/.codex/ipc/ipc.sock`, `app-server-control/app-server-control.sock` exist (undocumented). TUIs hold `~/.codex/tmp/arg0/codex-arg0XXXX/.lock` open; the dir name is random, no session id.
- Map: `~/.codex/state_5.sqlite` table `threads(id, rollout_path, cwd, updated_at_ms, title, approval_mode, archived, source, ...)`; `sqlite3 -readonly` works (WAL). Rollouts at `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl` (first line `session_meta` with `cwd`, `id`, `cli_version`). Pick newest thread whose cwd matches the process cwd. aether has no sqlite crate; shelling out to `sqlite3` is allowed (`std::process`).
- State from tail: `event_msg` payload types seen `token_count`, `task_complete` (idle after this); `response_item`, `token_usage_record`. A trailing `task_complete` = idle; earlier events with no `task_complete` = working; approval-pending event name unverified.
- Act: Esc/Ctrl-C semantics unverified; same caution as Pi. Codex ships a hook (`~/.codex/hooks.json`, `herdr-agent-state.sh` posting to a unix socket) used by another tool; a push model like that needs user opt-in, skip.
- Others (amp, cursor-agent, gemini, aider): not installed; same recipe (ps basename + lsof cwd + their session dir tail).

## 5. Stop semantics summary

| CLI | Safe stop | Risk |
|---|---|---|
| Claude TUI | SIGINT (Ctrl-C equiv, interrupt; unverified), confirm before SIGTERM | exiting loses nothing on disk, transcript persisted; kills the user's terminal pane process |
| Pi TUI | none safe; SIGTERM ends it (Ctrl-C = clear/exit) | Esc only reachable via tty, not signal |
| OpenCode | `POST .../interrupt` | none; never kill service 6414 |
| Codex TUI | unverified; SIGTERM last resort | same |
| aether-owned child | existing driver interrupt/abort | n/a |

Rule: only auto-act on OpenCode (API) and aether's own children; for foreign TUIs require an explicit confirm button, SIGINT first, SIGTERM second, never SIGKILL. Verify pid + `procStart`/etime just before signalling (pid reuse). Never signal pid 1-ancestors, shells, or `tmux`.

## 6. Polling cost (measured, `/usr/bin/time -p`)

| Op | Real |
|---|---|
| `ps -axo pid,ppid,etime,state,command` | 0.03s |
| `lsof -a -p <4 pids> -d cwd -Fpn` | 0.02s |
| `lsof -a -d cwd -Fpn` (all procs) | 0.15s (avoid) |
| read `~/.claude/sessions/*.json` | ~0 |
| `tail -c 64K` transcript | ~0 |
| `opencode api GET /api/session/active` | 0.06s |

## Recommended design inputs

Discovery (one background thread, one `ps` per tick):
1. `ps -axo pid,ppid,etime,state,command`; keep rows whose command basename is `claude|pi|opencode|codex` (or node running pi-coding-agent); drop helper subcommands (`--chrome-native-host`, `serve`, `app-server*`, `codex-code-mode-host`) and any row whose ppid or ancestor is aether's pid (aether-owned; show from aether's own state instead).
2. One `lsof -a -p <pid,pid,...> -d cwd -Fpn` for the kept pids.
3. Claude: join with `~/.claude/sessions/<pid>.json` (sessionId, status, waitingFor, name, tmux). Pi: newest file in encoded-cwd dir. OpenCode: `/api/session?limit=50` + `/api/session/active` + `/api/permission/request`, matched by directory. Codex: `sqlite3 -readonly state_5.sqlite` by cwd.

State rules:
- Claude: registry `status` (busy/waiting/idle), `waitingFor` text; ignore if pid dead.
- OpenCode: active set = working; permission list non-empty = waiting; else idle; `outcome`/`time.idle` for last result.
- Pi/Codex: tail-based. Working if last conversational entry is not a terminal one (`stopReason` stop/error, `task_complete`) or mtime within ~5s; otherwise idle. No approval state for Pi.
- Process gone = finished, keep last-known row ~1 poll as "ended".
- Always mark confidence (registry/API = exact, tail = heuristic).

Actions: OpenCode interrupt via API; aether children via existing drivers; foreign TUIs: confirm-gated SIGINT then SIGTERM, re-verify pid/start time first. Offer "focus tmux pane" for Claude via `tmux` field instead of killing.

Poll interval: 2s while the manager view is open (ps+lsof+API ~0.15s total, well under 10% of one core), 10s when closed or only badge needed. Skip all work when no view/badge subscribes. Tail reads capped at 64 KB; cache by (path, mtime, size). Never block the UI thread; send a snapshot struct through the existing app channel.

Open gaps (could not verify safely): Pi live argv, Claude waiting reason for tool permission, SIGINT effect on any TUI, `/api/session/active` payload while busy, Codex approval event names, whether Claude `-p` children write a registry file.
