# Organization view design

Date: 2026-11-12. Status: approved for implementation.
Research: `docs/idea/README.md`. Prototype (visual source of truth): `docs/prototypes/org-view.html`
(open with `?variant=A|B|C&state=live|empty|off`, `&open=wizard&step=1`, `&open=settings`).

## Goal

Run a project like a small company from a dedicated page. A lead (CEO) routes work; a few
persistent heads (Engineering, Product, Security, ...) each have a pinned provider and model, a
role file and a memory file; heads hire cheap workers that run as their child chats. Everything
is created and controlled from the UI: nobody hand-creates head chats.

## Decisions

- Off by default. `Settings > Organization > Enable organizations`. Off hides the page entry
  points, the sidebar section and the status bar item; nothing is deleted.
- One org per project (project = the chat project folder the board already uses).
- Three layouts of one page, user-selectable like Chat layout: Roster console (default), Org chart,
  Round table. Setting `org_layout`; also a palette command and a `CycleOrgLayout` action.
- Org files live in the config dir: `<config>/org/<project-slug>/org.json`, `roles/<role>.md`,
  `memory/<role>.md`. Not in the repo.
- Heads are ordinary top-level chats that Aether creates: `lead = true`, `pinned = true`, title
  `[<ROLE>] <Title>`, provider and model from the roster. The roster maps role to chat id. No new
  chat DB column.
- Persona: the first message of a head's session is the onboarding prompt (role file + rules +
  CEO chat id). "Rotate session" clears the chat's CLI session and sends the onboarding prompt
  again, so the role survives; the title is restored after the clear.
- Workers are child chats spawned with the head as lead, through the existing spawn path, with
  `worktree: true` by default.
- Hiring is decided by rules, never by a vote. Rules: the request has a card; the head is under
  its worker cap; the org pool has a free slot. Policy setting: `rules` (CEO decides, i.e. apply
  rules and approve or deny automatically), `auto` (approve when rules pass, else ask the user),
  `ask` (always ask the user). Over-cap approval by the user applies to that one request only.
- Done stays human-only (existing board rule).

## Data model (`crates/app/src/org/model.rs`, pure and unit tested)

```rust
pub struct Org { pub project: String, pub heads: Vec<Head>, pub requests: Vec<Request>, pub goal: String }
pub struct Head {
    pub role: String,          // "CEO", "ENG", "PRODUCT", "SEC", "DESIGN", "QA", "OPS", "MKT", or custom
    pub title: String,         // "Engineering"
    pub owns: String,
    pub provider: Provider, pub model: String, pub effort: String,
    pub reviewer: Option<String>,   // role, None = the user
    pub budget_usd: f64,            // per day, display + warning only in v1
    pub max_workers: usize,
    pub chat: Option<i64>,          // head chat id; None until created
}
pub struct Request { pub id: u64, pub head: String, pub title: String, pub why: String,
    pub card: Option<i64>, pub provider: Provider, pub model: String, pub task: String,
    pub checks: Vec<(String, bool)>, pub created_ms: i64 }
pub struct Template { pub id: &'static str, pub name: &'static str, pub desc: &'static str, pub roles: &'static [&'static str] }
```

Built-in role defaults (title, owns, provider/model, reviewer, budget) match the prototype's
`ROLE_DEF`; templates match `TEMPLATES` (Solo builder, Startup (default), Full company, Custom).
Serde JSON for `org.json`. Helpers: `role_file_text(head) -> String` (default persona markdown),
`onboarding_prompt(head, ceo_chat, paths) -> String`, `hire_checks(org, head, card, running_by_head, pool_busy, pool_cap) -> Vec<(String,bool)>`,
`pool_cap(settings, cores, mem_gb) -> usize` (auto: `min(cores/2, mem_gb/5).max(1)`; 8 cores 16 GB = 3).

## Settings (`crates/core/src/settings.rs`)

`org_enabled: bool` (false), `org_layout: OrgLayout` (`Roster` default, `Chart`, `Table`; serde
lowercase; `ALL`, `label()`, `hint()` like `ChatLayout`), `org_pool: Option<usize>` (None = auto),
`org_builds: usize` (1), `org_workers_per_head: usize` (2), `org_hire: OrgHire` (`Rules` default,
`Auto`, `Ask`), `org_budget_usd: f64` (25.0), `org_rotate_at: u8` (75), `org_template: String`
("startup"). All `#[serde(default)]`.

Settings UI: new "Organization" section in `settings_view.rs` (and `settings_search.rs` keys):
enable toggle, layout dropdown, default template, worker pool (Auto / number), builds at a time,
workers per head, hiring policy, daily budget, rotate-at percent. Layout dropdown also appears
under Appearance next to Chat layout.

## Page and entry points

- `Page::Org`, rendered by `render_org(window, cx)` in `crates/app/src/org/view/mod.rs`, which
  dispatches on `settings.org_layout`. Disabled feature: page shows the "turned off" empty state.
  No org in the current project: the "set up" empty state.
- Entry points (only when enabled): sidebar nav item with a badge = number of items needing the
  user; status bar item "N heads · busy/cap workers · $spent · K need you"; command palette
  "Organization" and "Organization layout: ..."; shortcut Alt+O; sidebar section listing heads
  with status dots (clicking opens the head editor).

## Shared UI (`crates/app/src/org/view/`)

- Page header: title, project, running pill, buttons `+ Hire worker`, `+ Add head`, `Pause org`,
  `Settings`. Goal row: text input, target select (CEO or a head), Send.
- Needs-you items (derived each render plus stored requests): pending hire requests with check
  pills and Approve / Queue / Deny; heads or workers waiting on a question or approval (existing
  wait state) with "Open chat"; org cards in Review awaiting Done with "Open card"; heads at or over
  budget.
- Head editor drawer: title, reviewer, provider, model (pinned; changing it uses the existing
  provider switch / handoff), budget, max workers, owns, role file (editable, saved to
  `roles/<role>.md`), memory file (read-only view), buttons Remove head, Rotate session, Open chat,
  Save.
- Setup wizard: Template, Staff (rows: role, title, provider, model, reviewer, budget, remove;
  add role), Rules (policy fields), Review (what will be created). Create builds the org.
- Hire dialog: head, card (existing or new), model (cheap tier list), task, own worktree,
  check pills, Hire / Queue hire.

## Layouts

- Roster console (default, variant C): table rows per head (status, who, doing now, cards, team
  n/max, spend/budget bar, context bar, menu) with indented worker rows; right panel tabs
  Needs you / Messages / Rules.
- Org chart (variant A): CEO node, connector, heads row with worker chips under each, `+ Add
  head`; right inspector for the selected head or worker with details, actions, needs-you, recent
  messages.
- Round table (variant B): heads seated on a circle around a center showing the goal, board
  column counts and pool use; left column needs-you; right column message log.

"Messages" = recent `message_agent` traffic between org chats, read from user messages starting
with the `[from ` tag in head chats (newest first).

## Backend operations (`crates/app/src/org/mod.rs`, `impl ChatApp`)

`org_for_project`, `org_create(spec)`, `org_add_head`, `org_update_head`, `org_remove_head`
(archives the chat, keeps files), `org_rotate(role)`, `org_send_goal(text, target)`,
`org_hire(head, spec)` (spawn child via the spawn path with the head chat as lead),
`org_pause` (stop running turns of heads and workers), `org_decide(request, approve|queue|deny)`,
`org_needs_you() -> Vec<NeedsYou>`, `org_spend(chat) -> f64` (reuse `chat_spend`).
Persistence: load all `org.json` files at startup, save on every change.

Lead tool `request_worker` (orchestrator tool spec + dispatch): args `title`, `task`, `card?`,
`provider?`, `model?`, `why?`. Caller must be a head. Applies policy: rules pass and policy is
`rules` or `auto` => spawn and reply `{"status":"hired","id":..}`; rules fail and policy `rules`
=> `{"status":"denied","reasons":[...]}`; otherwise store a Request and reply
`{"status":"pending","request":id}`; the user's decision is delivered to the head as a turn
(existing `tell_lead` pattern).

Org pool: when the spawning lead is an org head, `spawn_agent` also enforces
`busy org workers < pool_cap`, queuing like the existing per-lead cap.

## Out of scope for v1

Budget enforcement (display and warn only), automatic session rotation (manual button only),
drag to merge roles, multiple orgs per project, cross-project org.
