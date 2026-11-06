---
name: Model picking and per-provider options surface
type: wayfinder:grilling
status: closed
assignee: wayfinder-agent
blocked-by: [04-driver-trait]
---

## Question

How do model picking and per-provider options work once `Settings.provider` exists: does `Settings.model` stay one shared string or split per provider, how are CLI model lists surfaced (`claude --model` values, `pi --model`, opencode models), where do binary overrides + cwd + model live in settings/UI, and what ships first cut vs second slice?

## Resolution

- `Settings.model` stays OpenRouter-only. New `provider_models: BTreeMap<String, String>` holds one opaque Provider-model string per CLI; empty = omit `--model`, use CLI default. `DriverStartOptions.model` reads from that map.
- First cut model entry = free text per provider, passed as `--model <value>` only when non-empty. No list probing.
- Global `Settings`: `claude_binary`, `pi_binary`, `opencode_binary` (empty = PATH search) + one workspace-root picker feeding `DriverStartOptions.cwd`. No per-chat persistence first cut.
- `Store` persists provider kind + provider-model map only. `Message.model` set to opaque string; `Count/Usage` only if CLI reports it.
- Second slice: list probing, per-chat overrides, CLI session-id persistence, cost display.
