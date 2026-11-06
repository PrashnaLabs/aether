# Research: Claude Code wire (ticket 01-claude-wire)

Source: `reference/crates/waku-core/src/driver/claude.rs` (2801 lines), `title_refresh.rs`, `waku-client/src/process.rs`.
Subagent: ses_eca1c32f7ffendQoxycUbK5mO2.

- Spawn: `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --permission-prompt-tool stdio --permission-mode <map>`, stdin/stdout/stderr piped. Mode map Ask→default, AutoAcceptEdits→acceptEdits, Auto→auto, FullAccess→bypassPermissions + --dangerously-skip-permissions.
- Protocol: JSONL both ways. Out: `user` message, `interrupt`, `set_model`, `control_response allow/deny`. In: `system/control_request/control_response/stream_event/assistant/user/result`.
- Perms: `can_use_tool` → permission Ask or auto-allow; AskUserQuestion → UserInput form.
- Cancel: `interrupt` message. `supports_steer=true`, steer = same user payload mid-turn.
- Titles: poll `claude_session::session_metadata().title` at 5s/10s + turn-end re-read → `AutoTitleUpdated`.
- Failures: transport-write error, stderr `error:` line filter, non-zero exit → `provider_exited` + `ProcessExited`.

Copy for aether: flags, 4-thread reader/writer/stderr/reaper shape, exactly-once turn settle, title poll. Skip: background-task reconcile, detached-Bash tail, computer-use extras.
