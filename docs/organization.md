# Organization

An organization runs one project like a small company. A CEO lead takes your
goals and routes them. Persistent heads (Engineering, Product, Security, and so
on) each keep their own provider, model, role file and memory file. Heads hire
cheap workers for the actual work, within rules you set.

It is off by default, because heads keep spending tokens while they work.

## Turn it on

**Settings > Organization > Enable organizations.** With it on you get:

- an **Organization** row at the top of the sidebar, with a count of things
  waiting on you;
- an `Organization · <project>` sidebar section listing each head with its
  status and role (click a head to edit it);
- a status bar item: `N heads · busy/cap workers · $spent today · K need you`
  (click it to open the page);
- **Alt+O** and the palette command **Organization**.

Turning it off hides all of that. Nothing is deleted.

## Set up a project

Open a chat in the project, then the Organization page, and click **Set up
organization**. The wizard has four steps:

1. **Template.** Solo builder (CEO and Engineering), Startup (adds Product and
   Security; the default), Full company (adds Design, QA, Release ops and
   Marketing), or Custom (just a CEO).
2. **Staff.** Per head: title, provider, model, reviewer and daily budget. Add
   or remove roles.
3. **Rules.** Worker pool, builds at a time, workers per head, hiring policy,
   budget, rotate threshold.
4. **Review.** What gets created.

**Create** makes one pinned lead chat per head, titled `[ROLE] Title`, and
sends each one its onboarding prompt (role file, rules, who the CEO is). If you
typed a goal, it goes to the CEO. One organization per project.

The default template for the wizard is under **Settings > Organization >
Default template**.

## Layouts

One page, three layouts. Pick under **Settings > Appearance > Organization
layout** (also in Settings > Organization), from the palette (**Organization
layout: ...**), or cycle with the `CycleOrgLayout` action.

- **Roster console** (default): a row per head with status, current work,
  cards, team size, spend and context, workers indented below. Needs you,
  Messages and Rules tabs on the right.
- **Org chart**: the CEO above the heads, worker chips under each head, an
  inspector for the selected head or worker.
- **Round table**: heads seated around the goal, board counts and pool use in
  the middle, needs you on the left, the message log on the right.

The header has **+ Hire worker**, **+ Add head**, **Pause org** and
**Settings**. The goal row sends a goal to the CEO, or straight to one head.

## Heads

Click a head (on the page or in the sidebar) to open its editor: title,
reviewer, provider and model, daily budget, max workers, what it owns, its role
file (editable) and its memory file (read only). From there you can **Rotate
session** (clear the CLI session and onboard again with the same role),
**Open chat**, **Remove head** (archives the chat, keeps its files) or
**Save**. Changing the provider hands the chat off like any provider switch.

Budgets warn; they don't stop anything yet. Context bars turn amber at
**Settings > Organization > Rotate a head's session at**.

## Workers and hiring rules

Workers are child chats of a head, in their own worktree by default. A hire
passes the rules when:

- the request has a board card;
- the head is under its worker cap (**Workers per head** sets the default);
- the org-wide **Worker pool** has a free slot. Auto sizes it from the machine:
  half the cores or one per 5 GB of memory, whichever is less.

**Hiring policy** decides what happens:

- **CEO decides by rules**: approve when the rules pass, deny when they don't.
- **Auto-approve within rules**: approve when they pass, otherwise ask you.
- **Always ask me**: every request waits in Needs you.

Approving a request over a cap applies to that one request only. Moving a card
to Done is still yours alone.

## The `request_worker` tool

Heads hire through the `request_worker` lead tool:

| Argument   | Required | Meaning                         |
|------------|----------|---------------------------------|
| `title`    | yes      | Short name for the job          |
| `task`     | yes      | What the worker should do       |
| `card`     | no       | Board card id                   |
| `provider` | no       | Worker provider                 |
| `model`    | no       | Worker model                    |
| `why`      | no       | Reason, shown to you if asked   |

Only heads can call it. Replies:

- `{"status":"hired","id":...}`: the worker is running;
- `{"status":"denied","reasons":[...]}`: the rules failed under the rules policy;
- `{"status":"pending","request":...}`: waiting on you. Your Approve, Queue or
  Deny comes back to the head as a new turn.

## Where files live

Org files stay out of the repo, in Aether's config folder (usually
`~/.config/aether`):

```
<config>/org/<project-slug>/
  org.json          roster, pending requests, current goal
  roles/<role>.md   each head's role file
  memory/<role>.md  each head's memory file
```

Head and worker chats are ordinary chats in the chat list.
