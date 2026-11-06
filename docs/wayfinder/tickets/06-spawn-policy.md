---
name: Spawn policy and binary discovery
type: wayfinder:grilling
status: closed
assignee: wayfinder-agent
blocked-by: [04-driver-trait]
---

## Question

How do the drivers find and launch binaries (`claude`, `pi`, `opencode`): PATH lookup vs setting, cwd = workspace root, env passthrough (API keys?), concurrent-session policy, log capture, and missing-binary error copy? Check `reference/crates/waku-client/src/process.rs` + `support.rs` (566 + 233 lines) for what to steal. Decide one shared `spawn` helper vs per-driver code.

## Resolution

- Lookup: expanded PATH search (login-shell PATH + `~/.local/bin`, `~/.cargo/bin`, brew dirs) + per-provider override string (`~/` expanded). Lives in shared helper.
- cwd: per-session workspace root via `DriverStartOptions.cwd`, applied with `current_dir`. Drivers never guess.
- Env: inherit full parent env + repaired child `PATH`. No custom env UI first cut; per-provider extras only when a CLI proves need.
- Concurrency: one child per session for `claude`/`pi`; `opencode` reuses one shared `serve --service` hub.
- Logs: pipe stderr, forward only visible error lines (`error:`, `fatal`, `no such file`), suppress transport-closed noise.
- Missing binary: probe at session start, `"<name> is not installed"` + install hint as `Update::Error`; spawn failure gets `failed to start \`<cmd>\`` context.
- Shape: one `crates/agent/driver/spawn.rs` (`find_binary`, `command`, `spawn`, `visible_stderr`). Drivers only add args.
