# Outside-agent scan: where session/pid come from, and where they go missing

Sources: `crates/agent/src/external.rs` (`scan`, `agent_rows`, `claude_registry`,
`pi_session`, `codex_session`, `antigravity_session`, `merge`), `crates/app/src/agents_view.rs`
(`open_blocker`, `open_external`), live `ps`/`lsof`/`ls` on this machine 2026-11-06.

## Gate

`open_external` returns silently when `session.is_empty() || pid.is_none()`.
`open_blocker` shows the same for session, plus "Busy in another OpenCode client"
when session is set but pid is None. `pid` is always `Some` from `ps` for TUI rows.
Only OpenCode rows synthesised without a TUI have `pid: None`.

## How each provider fills fields

### Claude (`claude_registry`)

- Needs `~/.claude/sessions/<pid>.json` (config dir from `claude::config_dir()`),
  read with `std::fs::read_to_string`; sibling `<pid>.*.key` ignored by design.
- `claude_entry` requires `v["pid"] == pid` and non-empty `sessionId` (`.ok()?` otherwise).
  Fills `session=sessionId`, `cwd`, `name->title`, `status->state`, `tmux->origin`.
- Fallback on any miss: `session=""`, `title=""` (then last cwd segment). No other source consulted.
  Transcript later re-finds `<session>.jsonl` anywhere under `projects/`, but resume still needs sessionId.

### Pi (`pi_session`)

- `cwd` comes from `lsof -d cwd` only (`parse_lsof_cwd`); empty when lsof fails or pid already gone.
- Session dir = `--<cwd sans leading /, / and : -> ->--` under `~/.pi/agent/sessions`.
  Picks newest `*.jsonl` (not in `own_sessions`) whose mtime `>= now - (etime+1s)` i.e. since proc start.
- `session = full file path`, `state` from tail 64KB heuristic (`pi_state`), `title` stays `""`
  (filled with cwd basename by `merge`).
- Miss reasons: empty cwd; dir-name mismatch (symlink, custom `--session-dir`/`--session`,
  cwd with `:` encoded differently by pi itself, macOS /tmp vs /private/tmp); mtime older
  than proc start (file created before start, clock skew, copied/restored tree); all files in own-set.

### OpenCode (`merge` over `opencode_sessions`)

- TUI row gets its `ses_` id only from the local service API (`service_api()` + `/api/session`,
  `/api/session/active`, `/api/permission/request`, 2s timeout each).
- Match requires `session.location.directory == cwd` from lsof AND `time.updated >= proc start`
  (`fresh`). First (newest) match wins. Miss if service unreachable (all rows stay `session=""`),
  cwd empty/mismatched, or reopened session with no new activity (`updated` before start).
- Service rows with non-Idle state and no TUI are added with `pid: None` -> always blocked.
- Headless `opencode serve --service` (arg1 == `serve`) is filtered out as helper, correct.

### Codex / Antigravity (global newest, pid-independent)

- `codex_session`: last parseable line of `~/.codex/session_index.jsonl` not in own-set,
  regardless of pid/cwd. `session=id`, `title=thread_name`.
- `antigravity_session`: newest dir under `~/.gemini/antigravity-cli/brain/` not in own-set.
- Neither ties the session to the scanned pid, so `session` is almost always non-empty when
  data dirs exist, but potentially the wrong session when several CLIs run.

### Row filtering (`agent_rows`)

- Excluded even when session exists: helpers (`serve`, `app-server`, `--chrome-native-host`),
  headless (`-p`, `--print`, `--mode rpc`, `--mode=rpc`), and any pid whose ancestry
  (walked 64 steps via ppid) hits Aether's own pid.

## Live check (this machine, 2026-11-06 ~23:12-23:16)

Aether pid 26530 (`/Applications/Aether.app/.../aether`, ppid 1).

| Proc | Verdict |
|---|---|
| `pi` 28083/28214/28356, ppid 26530, cwd aether | Aether-owned (harness sessions), filtered by ancestry. Not outside. Their files exist under `--Users-iris-Downloads-xzx-PERSONAL-aether--/` (3 jsonl, 17:30-17:31Z), correctly named. |
| `claude -p ... --resume f767bc1b...` 27004, ppid 26530 | Aether-owned headless. Registry file `~/.claude/sessions/27004.json` exists and is parseable (`sessionId` matches `--resume` arg, `cwd`=aether, name `aether-34`, status idle). Would produce a session if it were outside, but excluded twice over. |
| `claude --chrome-native-host` 1358, cwd `~/.claude/chrome` | Outside, but filtered as helper. Scan produces no row at all. |
| `opencode serve --service` 26596, cwd `~` | Outside, but filtered as helper (`serve`). Scan produces no row. |
| No standalone interactive `claude`, `pi`, `opencode`, `codex`, `agy` outside Aether | Nothing left for scan to report. All other hits were the grep/bash pipeline itself. |

So at check time there was nothing resumable outside Aether on this box, by design.

## Launch modes where resume silently no-ops

1. Claude with no registry row: older versions, `claude -p` variants (filtered anyway),
   a registry file written after/at different path, stale `<pid>.json` whose inner `pid`
   no longer matches, or empty `sessionId`. Any of these -> `session=""` -> dead button.
2. Claude `--resume`/`--continue`: normally fine because registry carries the resumed id,
   BUT breaks if the resumed session was started before the registry feature or was since
   garbage-collected; registry then holds a different/empty id than the CLI arg suggests.
3. Claude in a git worktree: `cwd` from registry wins over lsof, so worktree path is kept.
   Registry is the only source; a worktree launched with `--resume <id>` whose registry
   file is keyed to the original worktree pid still resolves via transcript search. Low risk.
4. Pi with no explicit `--session`: scan guesses newest jsonl since proc start in the cwd
   dir. Multiple `pi` in same cwd all map to the same file. Pi with `--session <path>`
   or `--session-dir` outside the cwd dir is invisible to the guess. Pi resumed from a
   file saved elsewhere (different cwd dir) also invisible.
5. Pi where session file mtime predates proc start (resume an idle session with no new
   messages writes nothing) -> filtered out by `mtime >= start` -> `session=""`.
6. Codex: session basically always found, but it is the newest global entry, not the one
   this pid owns. Resume opens whatever the last thread was. If index file is missing/
   empty/all-own -> `session=""`.
7. Antigravity: same global-newest model as Codex; empty `brain/` -> `session=""`.
8. OpenCode reopened idle session: `fresh` check drops it -> `session=""` even though
   the session exists and service answered. Plus every service-only busy session -> pid None.
9. Headless/background CLI (`-p`, `--print`, `--mode rpc`, `codex app-server`,
   `opencode serve`, `claude --chrome-native-host`) never reaches the list; no button, no error.

## Most likely root causes (ranked)

1. Pi/OpenCode `session=""` from heuristic mismatch (dir-name, mtime/fresh, cwd, service timeout), the two
   open-by-construction paths besides Claude.
2. Claude missing/stale `<pid>.json` registry (version, pid mismatch, empty sessionId).
3. Codex/Antigravity global-newest guess silently attaching the wrong session.
4. OpenCode service-only rows blocked on pid; headless rows absent entirely.
