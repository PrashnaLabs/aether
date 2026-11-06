---
name: OpenCode wire protocol
type: wayfinder:research
status: closed
blocks: []
blocked-by: []
---

## Question

How does `reference/crates/waku-core/src/driver/opencode.rs` (4062 lines) spawn the OpenCode CLI, stream SSE, and support prompt / cancel / session rename? Record server-spawn vs attach flow, endpoint + event names (`session.renamed`, etc.), translation into driver events, cancel/close path, and failure modes. Output decides what aether's opencode driver must copy vs skip.
