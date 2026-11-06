---
name: Driver trait and provider enum
type: wayfinder:grilling
status: closed
blocked-by: [01-claude-wire, 02-opencode-wire, 03-pi-wire]
---

## Question

What is the minimal `Driver` trait + `ProviderKind` enum living in `crates/agent/driver/` that holds all three CLIs while `Agent::send` keeps working? Decide trait methods (first cut: `prompt`, `cancel`, `close` only?), `DriverHandle` shape, error type, per-provider options struct, and how the UI picks a provider per session. Blocked on the three wire-protocol tickets.

## Resolution

- `trait Driver { prompt, cancel, close }` + `struct DriverHandle` (sender + child guard), boxed trait objects.
- Drivers emit `aether-core::Update` direct into the `Reply` channel. No `DriverEvent`.
- `Settings.provider: OpenRouter | Claude | Pi | OpenCode`, per session, default OpenRouter.
- One `DriverStartOptions { binary, cwd, model }` shared by all three drivers.
