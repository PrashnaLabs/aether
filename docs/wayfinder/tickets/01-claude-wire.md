---
name: Claude Code wire protocol
type: wayfinder:research
status: closed
blocks: []
blocked-by: []
---

## Question

How does `reference/crates/waku-core/src/driver/claude.rs` (2801 lines) spawn the Claude Code CLI, stream its output, and support prompt / cancel / titles? Record exact binary + args, stdin/stdout protocol (JSONL transcript on disk?), event translation into driver events, cancel path, title poll (`title_refresh.rs`, `claude_session`), and failure modes. Output decides what aether's claude driver must copy vs skip.
