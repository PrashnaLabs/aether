---
name: Steer / mid-turn interrupt policy per CLI
type: wayfinder:grilling
status: closed
assignee: wayfinder-agent
blocked-by: [04-driver-trait, 05-update-mapping]
---

## Question

What is the first-cut steer (mid-turn follow-up) policy for the three CLI drivers: does `Driver` get `steer()` now or cancel+reprompt only, how does steered text land in the transcript (same turn vs new turn), what does steer-while-idle do, and any per-CLI quirks (claude user-payload, pi `steer` type, opencode inbox+promote)?

## Resolution

- First cut: no `steer()` on the trait. Cancel + reprompt only (`prompt/cancel/close` stays). Steer is second slice after one vertical works.
- When steer lands: same-turn policy. Steered text continues current `Message` as `Delta`; `Final` reconcile gap-fills as today. No new `Message`.
- Steer-while-idle = plain `prompt`. No queue, no error path.
- Opencode two-step (inbox delivery + PATCH promotion) hidden behind `steer()`; claude user-payload and pi `steer` type likewise impl-detail. Trait stays uniform.
