# Aether as an organization: concept

Status: idea, not a plan. Captures the owner's description (2026-11-12) in a
structured form so the research files can be checked against it. Companion
files in this folder:

- `org-prior-art.md`: what papers and frameworks say (Antigravity, Gemini 3.8 Flash High).
- `org-aether-fit.md`: how the idea maps onto Aether's current code (Antigravity, Claude Sonnet 5.5 High).
- `README.md`: merged verdict and recommended path.

## The idea in one paragraph

A single lead session runs the "company". It does not build; it staffs,
routes, and keeps track of who is doing what. Below it sits a small, fixed
set of department heads. Each head is a long-lived chat with a persona, a
pinned provider and model, and its own memory, so the CTO stays the CTO
across restarts. Heads are woken when work in their area arrives. Their
output is reviewed by peers. Builders ("workers" or "interns") are cheap,
short-lived, many, sized to the machine, and always reviewed by a head or
peer before their work counts. A head that is overloaded asks for a worker;
a council of heads approves or denies the hire.

## Entities

| Entity | Lifetime | Count | Model tier | Example |
| --- | --- | --- | --- | --- |
| Lead (CEO/COO) | Persistent | 1 | Strong, good at planning | Claude Code + Sonnet 5.5 |
| Head | Persistent | Fixed, small (4 to 7) | Matched to the domain | CTO, Product, Project, Security, Tech lead, Marketing (Gemini 3.8 Flash) |
| Worker / intern | Ephemeral, per task | Scales with CPU and RAM | Cheap, fast | Builders, test writers, doc writers |
| Council | Not a session; a decision procedure | All heads, or a quorum | n/a | Approves hires, resolves cross-department conflicts |

Merged roles: where two heads would message each other on nearly every task
(for example Project manager and Product manager, or Tech lead and CTO on a
small project), keep one head that carries both hats. Fewer heads means fewer
hand-offs, which is where multi-agent systems lose information.

## Flows

1. Intake. The human gives the lead a goal. The lead writes board cards and
   assigns each to a department.
2. Delegation. The lead messages the owning head. The head either does the
   work itself or asks for workers.
3. Hiring. A head requests N workers with a reason and a budget. The council
   votes (or the lead decides with the council's notes). Approved workers are
   spawned under the head, or under the lead on the head's behalf.
4. Review. Worker output goes to its head or a peer for review before the
   card moves to Review. Head output is reviewed by a peer head (security
   reviews the CTO's design; tech lead reviews the security lead's patch).
5. Done. Stays human-only, as on today's board.

## Properties the owner asked for

- Heads are persistent and keep their persona across sessions.
- Each head has a dedicated model; model choice is part of the role.
- Head count is fixed; worker count is small per team and bounded by machine resources.
- The lead always knows who is employed and how loaded each head is.
- Over-burdened heads can request help; hires pass a council.
- A round-table style view of the org.

## Open questions for the owner

1. Is the lead allowed to do work itself, or only route? Pure routers add a hop to every task.
2. Is a head one chat forever, or a role that can be re-instantiated from a memory file when its context fills up?
3. Does the council vote for real (several model calls per hire), or is it the lead deciding with a rule (load, budget, CPU)? The second is far cheaper.
4. Do heads talk to each other directly, or only through the lead and the board?
5. Is this per project, or one org shared across all projects?
6. What is the budget unit: tokens, dollars, wall-clock, or concurrent sessions?
