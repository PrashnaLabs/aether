---
name: Rollback / fork / resume cursors per CLI
type: wayfinder:grilling
status: closed
assignee: wayfinder-agent
blocked-by: [04-driver-trait, 05-update-mapping]
---

## Question

How do retry / edit-and-resend / fork (`Agent::fork_session`, see `crates/agent/src/agent.rs:71`) and app-restart resume work for the three CLI drivers: does each CLI get a native cursor (pi `fork|branch|clone`, opencode session fork, claude `--resume` / re-seed), or does aether re-seed history as text, and what does `Store` (`crates/agent/src/store.rs:21`) persist per chat so a reload can reconnect?

## Resolution

- Fork = fresh CLI session, re-seed visible transcript as user/assistant text. No native `fork|branch` / opencode fork / `--resume` first cut.
- Rollback (edit-and-resend / retry) = truncate `Vec<Message>` at index, `close` + fresh `prompt`. Transcript is the cursor, no checkpoint ids.
- Resume after restart = always fresh, replay messages as context. No `Store` schema change; persist provider kind only (ticket 04). CLI session-id column is second slice with model picking.
- `fork_session(id, history: &[(Role, String)])` signature unchanged; drivers join pairs into one context prompt. Widen only when native fork lands.
