# Wayfinder map: CLI wrappers for aether

> Status: shipped. Drivers now live in `crates/driver/` (Claude, Codex, Pi,
> OpenCode, Antigravity), not `crates/agent/driver/` as referenced below;
> this map and its tickets are the historical spec.

## Destination

Build-ready spec for wrapping `claude`, `pi`, and `opencode` CLIs as alternate providers inside `aether/crates/agent`, coexisting with the current OpenRouter path. Done = no open decisions left before implementation can start.

## Notes

- Domain: Rust subprocess drivers translating CLI streams into `aether-core::Update`.
- Reference: `reference/crates/waku-core/src/driver/{claude.rs,opencode.rs,pi.rs,support.rs,mod.rs}`, `reference/crates/waku-client/src/{driver.rs,process.rs}`.
- Current code: `crates/agent/src/agent.rs` (`Agent::send`), `crates/core/src/lib.rs` (`Update`, `Message::apply`).
- Constraints: in-process spawn in `crates/agent/driver/`, no daemon. Reuse `Update`, add variants only when forced. First cut = prompt + stream + cancel.
- Skills per session: `grilling`, `domain-modeling`. Research tickets use `research`.

## Decisions so far

- [Claude Code wire protocol](tickets/01-claude-wire.md): JSONL pipes with `-p stream-json`, interrupt-cancel, title poll — see `research/claude-wire.md`.
- [OpenCode wire protocol](tickets/02-opencode-wire.md): shared `serve --service` + SSE hub, execution-outcome settle — see `research/opencode-wire.md`.
- [Pi wire protocol](tickets/03-pi-wire.md): `--mode rpc` NDJSON, abort-cancel, omp deltas — see `research/pi-wire.md`.
- [Driver trait and provider enum](tickets/04-driver-trait.md): trait + handle in `crates/agent/driver/`, drivers push `Update` direct, per-session `Settings.provider`, one shared start-options struct.
- [Update mapping and cancel semantics](tickets/05-update-mapping.md): new `Update::Title` only, tool prompts into existing `Ask`, tool narration as `Delta`, cancel = graceful-stop + 3s kill, partial kept, no `Error`.
- [Spawn policy and binary discovery](tickets/06-spawn-policy.md): expanded PATH + per-provider override, per-session cwd, inherit env + repaired PATH, one child per session (opencode shared hub), triaged stderr, probe-then-spawn, one shared `spawn.rs`.
- [Steer / mid-turn interrupt policy](tickets/07-steer-policy.md): first cut cancel+reprompt only, no `steer()`; second slice same-turn deltas, steer-while-idle = prompt, opencode inbox+promote hidden.
- [Rollback / fork / resume cursors](tickets/08-rollback-fork-resume.md): fresh-session re-seed text, truncate-and-reprompt rollback, fresh replay on restart, `fork_session` unchanged.
- [Model picking and per-provider options](tickets/09-model-options.md): `Settings.model` OpenRouter-only + `provider_models` map, free-text `--model` first cut, global binary overrides + cwd, Store persists provider kind + model map.
- [Session persistence interplay with Store](tickets/10-store-persistence.md): per-chat `provider` + `provider_model` columns, global map holds defaults, ALTER TABLE migrate, fresh replay reload.
- [Ship order for CLI providers](tickets/11-ship-order.md): all three drivers together, then one Store + settings integration pass.

## Not yet specified

- None. Map complete.

## Out of scope

- Daemon + RPC split (Waku-style `waku-daemon`). Stays in-process.
- Replacing the OpenRouter/`adk-rust` path. It stays.
- Transcript/UI work in `crates/app`, `crates/ui`. Spec stops at `Update` stream.
- Computer-use tooling. Later effort.
