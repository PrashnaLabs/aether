---
name: Ship order for CLI providers
type: wayfinder:grilling
status: closed
assignee: wayfinder-agent
blocked-by: []
---

## Question

What ships first: one CLI vertical slice or all three at once?

## Resolution

- All three CLI drivers land together against the shared trait + spawn + Update mapping, then one integration pass for Store + settings.
- No scaffold-only slice, no single-provider proving slice. Wire quirks per CLI handled in parallel; one stuck CLI does not re-scope the slice.
- Map is now complete: no open decisions left before implementation can start.
