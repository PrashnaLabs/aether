---
name: Session persistence interplay with Store
type: wayfinder:grilling
status: closed
assignee: wayfinder-agent
blocked-by: [04-driver-trait, 08-rollback-fork-resume, 09-model-options]
---

## Question

`Settings` (`crates/core/src/settings.rs:52`) is global with `model: String` only; `SavedChat` (`crates/agent/src/store.rs:21`) has id/title/messages/pinned/archived only. Tickets 04/08/09 say per-session `Settings.provider` + `provider_models` map, `Store` persists provider kind + model map, resume = fresh replay. Where do provider kind + per-provider model land on disk (new `chats` columns vs `messages` JSON vs global settings file), how do old DBs migrate, and what does reload reconnect to?

## Resolution

- Per-chat `chats.provider TEXT DEFAULT 'openrouter'` + `chats.provider_model TEXT DEFAULT ''`, surfaced on `SavedChat`. Per-session provider is runtime view over active chat, not persisted global.
- Global `Settings.provider_models: BTreeMap<String, String>` holds last-used default per provider; per-chat column wins when non-empty, else map fallback, empty = omit `--model`.
- Migrate via `ALTER TABLE ADD COLUMN` with defaults (same pattern as `pinned`/`archived`); old rows load as OpenRouter. Reload = fresh replay, no CLI session-id column first cut.
