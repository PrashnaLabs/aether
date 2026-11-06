# Research: Pi wire (ticket 03-pi-wire)

Source: `reference/crates/waku-core/src/driver/pi.rs` (2413). Subagent: ses_eca1c32f5ffeZLFz9p2BYjTjIg.

- Spawn: `<binary> --mode rpc --approve (pi) | --yolo (omp)`, cwd set, piped stdio. `PI_SKIP_VERSION_CHECK=1` pi-only.
- Handshake: `negotiate_protocol v2` (omp) → `get_state` → `switch_session?` → `set_model?` → `set_thinking_level?` → `get_state` → stats + `Connected` + title.
- NDJSON RPC: out `prompt/steer/abort/get_state/clone/fork|branch`; in `response/rpc_chunk/agent_start|turn_start/message_start|update|end/tool_execution_*/agent_settled|agent_end/session_info_changed|update/command_output/available_commands_update/auto_retry_end`.
- Deltas → Text/ReasoningDelta; `message_end` gap-fills + Usage. Tool error alone never fails turn.
- Cancel: fire-and-forget `abort`; prompt sends without timeout so abort fits mid-turn.
- OMP diffs: `agent_end` + `isTerminal`, `session_info_update{title}`, `branch` words, chunked frames, push commands, `--fork` copy, early ack `agentInvoked:false`.

Copy for aether: handshake order, abort path, gap-fill, stale-prompt eviction. Skip: computer-use extension, fork/clone helpers (later).
