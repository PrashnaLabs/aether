# Glossary

- **Provider**: an external coding CLI aether can drive (`claude`, `codex`, `pi`, `opencode`, `antigravity`), plus the OpenRouter API path.
- **Driver**: in-process code in `crates/driver/` that spawns one provider CLI and translates its stream into `Update`.
- **Wrapper**: the driver layer as a whole; spawning + translating, no daemon.
- **Update**: `aether-core::Update`, the single stream the UI reads.
- **Binary override**: per-provider custom path to a CLI binary, `~/` expanded, else PATH search.
- **Session cwd**: the workspace root a driver launches its CLI in, one per session.
- **Fork**: new chat from old messages; first cut = fresh CLI session re-seeded with transcript text.
- **Rollback**: edit-and-resend / retry truncating the current chat, then prompt anew; transcript is the cursor.
- **Resume**: reconnect after app restart; first cut = fresh session + replay, no persisted CLI id.
- **OpenRouter model**: `Settings.model`, e.g. `openrouter/auto`; only used by the OpenRouter path.
- **Provider model**: opaque per-CLI string passed as `--model` when non-empty, else CLI default; one stored per provider.
_Avoid_: workspace root, working dir
- **Explorer**: the per-folder file browser and editor a chat switches to in place of its transcript.
- **File link**: a file path in an agent reply, as a markdown link or inline code, that names an existing file; opens in the Explorer at its line.
- **Unit test file**: tests for `crates/<crate>/src/<path>.rs` live in `crates/<crate>/tests/unit/<path>.rs`, never inline. The source file ends with `#[cfg(test)] #[path = "<relative path>"] mod tests;` (path relative to the source file's folder), and the test file starts with `use super::*;` for private access. One per source file; split test files when the source splits. `tests/*.rs` stays for real integration tests; never add `tests/unit/main.rs`, cargo would build it as one.
- **Project**: a folder the user works in, kept once added or chatted in; its chats and worktrees group under it.
_Avoid_: workspace, repo
- **Agent chat**: a chat run by a Provider inside a Project.
- **Direct chat**: a chat with an OpenRouter model and no Project.
- **Draft**: a chat's unsent composer text plus its Attachments.
- **Attachment**: a file or image added to a message; the chat keeps its own copy.
