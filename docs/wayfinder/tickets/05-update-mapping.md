---
name: Update mapping and cancel semantics
type: wayfinder:grilling
status: closed
blocked-by: [01-claude-wire, 02-opencode-wire, 03-pi-wire]
---

## Question

How does every CLI stream event map onto `aether-core::Update` (`Delta/Final/Thinking/Error`, see `crates/core/src/lib.rs:23`), which new `Update` variants are forced (title? permission ask via `Update::Ask`?), and what does cancel guarantee (process kill vs graceful stop, what the transcript shows)? Decide the mapping table + `Message::apply` changes. Blocked on the three wire-protocol tickets.

## Resolution

- Add one variant: `Update::Title(String)`. No-op in `Message.text`, UI-only. Claude fills it via title poll, opencode via `session.renamed`, pi via `session_info_changed`.
- Permissions: tool allow/deny → `Ask::Approval`, question forms → `Ask::Question`. No new `Ask` variants.
- Stream table: text chunks → `Delta`, terminal full text → `Final` (gap-fill + reconcile), reasoning → `Thinking`, usage/stats → `Count` + `Usage`. Tool-call narration as `Delta` code fence first cut. Tool failure alone never `Error`; only turn failure is `Error`. No `Message::apply` change except `Title` no-op.
- Cancel: graceful wire-stop first (`interrupt` / `abort` / `POST interrupt`), SIGKILL after 3s. Transcript keeps partial text + `Final("")` settle, no `Error` on user cancel. `finish()` expires pending asks as today.
