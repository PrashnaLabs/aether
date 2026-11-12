# Aether as an organization: merged findings

Date: 2026-11-12. Research only, no code changed.

| File | What it is | Written by |
| --- | --- | --- |
| [org-concept.md](org-concept.md) | The idea, structured: entities, flows, open questions | Lead session (Claude Opus 5.5) |
| [org-prior-art.md](org-prior-art.md) | Papers, frameworks, failure studies, with citations | Antigravity, Gemini 3.8 Flash High |
| [org-aether-fit.md](org-aether-fit.md) | Mapping onto Aether's code with file:line, gaps, 10-step path | Claude Code, Sonnet (high effort) |

The Antigravity Sonnet 5.5 High run hit its quota ("Individual quota
reached", resets in about 3h), so the codebase research was rerun through
the Claude Code provider on Sonnet at high effort.

## Verdict

Worth building, with two parts of the original idea changed.

1. A head is a role, not a chat. The evidence against long-lived sessions is
   consistent (persona drift, context rot, tool drift; see prior-art sections
   3 and 7). What persists is the head's role file, its memory file and its
   card history. The chat that runs it can be replaced whenever its context
   fills up. To the user it still looks like "the CTO". Aether's handoff
   seeding already carries history into a fresh session.
2. The council is a rule, not a vote. A hire decision depends on queue depth,
   free CPU and RAM, budget left and the head's quota, and code can check all
   of these for free. An LLM vote costs one turn per head per hire, adds
   latency, and debate studies show conformity and sycophancy (prior-art 2).
   Keep a real vote, if at all, for rare contested decisions, and make the
   human the tiebreaker.

The rest of the idea holds up. A strong lead with cheap workers matches the
best-supported pattern in the literature, the orchestrator-worker split with
model tiering. Peer review helps when it is grounded in tests or a compiler,
not when one LLM simply reads another's work. Merging roles that
interdepend is the right instinct, because handoffs are where multi-agent
systems lose information.

Aether is closer than expected. The Lead chip is only a UI gate: it requires
`parent.is_none()` (`crates/app/src/app/orchestrate.rs:22-24`), and
`set_lead` has no parent check (`crates/app/src/app/chat/manage.rs:492`).
That means a top-level chat can act as a head, be a lead, spawn its own
workers, and message the CEO and other heads today. What's missing is
persona injection for CLI providers, pinned models, a role field, a global
worker cap (today `max_children` applies per lead, so six heads at the
default of 8 allow 48 workers), sender tags and a loop guard on
`message_agent`, and budgets.

## Where the evidence pushes back on the idea

| Idea element | Evidence | Adjustment |
| --- | --- | --- |
| Persona makes the CTO better at CTO work | Persona prompts do not reliably improve accuracy and can hurt it (arXiv 2311.10054, 2408.08631) | Use persona for scope, permissions and output format. Choose the model by capability, not by title |
| Heads persist forever as one session | Identity drift grows over long conversations (arXiv 2412.00804); memory can induce tool drift (arXiv 2605.24941) | Keep the role file permanent and rotate the session. Head memory is a file it re-reads |
| Round-table where heads discuss | Most failures in MAST are inter-agent misalignment and spec issues (arXiv 2503.13657); debate drifts toward conformity (arXiv 2509.23055, 2606.00820) | Make the round table a view, not a chat room. Heads communicate point to point through cards |
| Council approves hires | Same debate evidence; the decision has deterministic inputs | Use a rule-based gate first and escalate to the human |
| Peer review of worker output | Reflection helps when grounded in execution feedback (Reflexion 2303.11366) | Gate on `cargo test`/`clippy`, then the reviewing head reads the diff |
| More agents means more output | At equal thinking-token budgets, single agents match or beat multi-agent setups on multi-hop reasoning (arXiv 2604.02460) | Add a head only where it buys parallel work or a distinct model strength |

## Recommended path

Taken from `org-aether-fit.md` section 3 and reordered by the verdict above.

0. **Zero code trial.** Make the heads human-created, pinned, top-level lead chats with the persona in the first message, a roster in the CEO's first message, board labels `dept:<role>` and `review:<role>`, and the hire rule written into the prompts. Run one real feature through it. This tells you whether the shape works before any code exists.
1. **Make the org legible.** `list_chats` reports model, cost, queue length and last-reply age, so the CEO can see who is overloaded.
2. **Sender tag and loop guard** on `message_agent`.
3. **Role, persona and pinned model on the chat.** The persona is prepended to every fresh CLI session, which is what makes session rotation safe.
4. **Roster file plus `ensure_head(role)`.** The org then survives deleted chats and fresh checkouts.
5. **Global worker pool.** Size it as the minimum of the per-head quota and `available_parallelism()`/memory headroom. Over-cap spawns join the existing queue.
6. **Review routing.** When a card enters Review, message the head named in `review:<role>`.
7. **Per-role budget**, built from the existing `cost_micros`.
8. **Round-table view.** Read-only, reusing the crew strip, `card_status` and watch-peek patterns.
9. **Council.** Rule-based first, with an LLM vote only if the rule proves too blunt.

The view comes later than in the fit doc because it is only worth it once
role labels and cost data exist to show.

## Suggested starting roster

Five or fewer heads, with interdependent roles merged:

| Head | Merges | Tier |
| --- | --- | --- |
| CEO (lead) | Operations, project management | Strongest planner |
| Product | PM, marketing | Mid tier, e.g. Gemini 3.8 Flash |
| Engineering | CTO, tech lead | Strong coder |
| Security | Security lead, release gate | Strong reviewer, different vendor from Engineering |
| Workers | n/a | Cheapest model that passes the tests |

A reviewer from a different vendor than the author is the one heterogeneity
claim worth testing first. The X-MAS paper (arXiv 2505.16997) reports gains
from heterogeneous teams. The size of that gain was not checked here.

## Answers to the open questions in org-concept.md (recommended defaults)

1. Does the lead do work itself? No, it only routes. Keep it cheap by having it read cards rather than transcripts.
2. One chat forever, or rotate? Rotate (see verdict 1).
3. Real council vote? No, use a rule (see verdict 2).
4. Do heads talk directly? Yes, but point to point and tagged, and anything that changes scope goes on a card.
5. One org per project, or shared? Per project. The board's write scope is already the project (`orchestrate.rs:371-386`).
6. Budget unit? Dollars per day per head, built from `cost_micros`. Concurrency is capped separately by the worker pool.

## Verification notes

- I checked every arXiv id in `org-prior-art.md` against the arXiv API and the repo links against the GitHub API. Three were wrong and are fixed in place:
  - Kong et al. was cited as 2410.15858, an unrelated paper. The correct id is 2408.08631.
  - The Google ADK repo link was `google/agent-development-kit`, which returns 404. The correct repo is `google/adk-python`.
  - Two citations pointed to arXiv 2605.14820, which is a physics paper. Both are now marked UNVERIFIED.
- Claims I did not check beyond the paper title: the X-MAS "up to 47%" figure, the "4 to 6 children" span-of-control figure attributed to Google ADK, and the author names on 2604.02460.
- One correction to `org-aether-fit.md`: non-lead chats always get the board tools, but `read_history` only when the chat was seeded with history (`crates/agent/src/agent.rs:273-286`), not unconditionally.
- Nothing was run. Real per-head cost and drift rates are unknown until the increment 0 trial.
