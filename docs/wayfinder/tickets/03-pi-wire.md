---
name: Pi wire protocol
type: wayfinder:research
status: closed
blocks: []
blocked-by: []
---

## Question

How does `reference/crates/waku-core/src/driver/pi.rs` (2413 lines, covers Pi + Oh My Pi) spawn the pi CLI, stream NDJSON, and support prompt / cancel / `session_info_changed`? Record binary + args, NDJSON message shapes, event translation, cancel path, and how Oh My Pi differs. Output decides what aether's pi driver must copy vs skip.
