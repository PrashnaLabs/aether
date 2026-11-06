# Research: OpenCode wire (ticket 02-opencode-wire)

Source: `reference/crates/waku-core/src/driver/opencode.rs` (4062), `opencode_service.rs`, `opencode_api.rs`.
Subagent: ses_eca1c32f6ffeo0m89Rl9b0jEH0.

- No per-session CLI spawn. Shared `opencode serve --service`, idempotent, 20s budget, v1 refused. Driver: subscribe-before-create, `POST /api/session` with client-minted id.
- Stream: `GET /api/event` SSE hub, heartbeat 15s, `tui.*` dropped, per-session routing by `sessionID`.
- Events: `execution.started/succeeded/failed/interrupted` settle turns; `step/text/reasoning/tool.*` stream; `permission.asked/replied`, `form.*`, `session.renamed` → title (placeholder filtered).
- Prompt: `POST prompt`, steer = inbox delivery + PATCH promotion. Perms reply `once|reject` only (`always` rejected client-side).
- Cancel: `POST interrupt`; service-down → deferred stop + 60s resume grace + gap-fill reconcile.
- Close: unsubscribe only, never kills service. `ProcessExited` only on foreign delete / missing session.

Copy for aether: shared-service adopt, execution-outcome settle, inbox tracking, gap-fill reconcile. Skip: computer-use, v1 migration, catalog cache, fork/rollback, compaction rows.
