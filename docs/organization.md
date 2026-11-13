# Organization

An organization runs one project like a small company. A CEO lead takes your
goals and routes them. Persistent heads (Engineering, Product, Security, and so
on) each keep their own provider, model, role file and memory file. Heads hire
cheap workers for the actual work, within rules you set.

It is off by default, because heads keep spending tokens while they work.

## Turn it on

**Settings > Organization > Enable organizations.** With it on you get:

- on each project in the sidebar's Projects list: an **Organization** icon
  (first of the hover icons) when the project has an org, or **Set up
  organization** in its `⋯` menu when it doesn't;
- a status bar item: `N heads · busy/cap workers · K need you`. Click it for
  a popover: `Organization · <project>` with running/idle, one row per head
  (role chip, title, provider/model, status, doing now, workers n/max; click
  to open its chat), worker slots and builds at a time, `K need you` (opens
  the page), then **Open organization** and **Hire worker**. Escape, a click
  outside or a second click closes it;
- **Alt+O** and the palette command **Organization**.

Turning it off hides all of that. Nothing is deleted.

Alt+O, the palette and the status bar popover open the page for the current
project; the project row opens it for that project. While the page is open the sidebar
shows **Organizations**: every project with an org as a project row (folder,
name, branch line). Click a row to open that org's page; its chevron folds it,
remembered per project and apart from the Projects list; its **+** hires into
that org. The org on the page is highlighted. Under each row are its heads with
their role chips, each followed by its workers. Heads and workers are org agents: they
stay out of Pinned and Projects, and opening one from anywhere (sidebar, page,
palette, watch list, notifications, tabs) shows its chat with this sidebar for
its org. Opening any other chat brings back the sidebar view you had before.
The header's **+** opens the hire dialog for the current org (Add head while only the CEO exists).
**Agent Projects** at the bottom returns to the page and chat you had before
the org; Escape on the page returns to the page before it.

## Set up a project

Pick **Set up organization** from the project's `⋯` menu in the sidebar, or
open the Organization page for the project and click **Set up organization**.
The wizard has four steps:

1. **Template.** Solo builder (CEO and Engineering), Startup (adds Product and
   Security; the default), Full company (adds Design, QA, Release ops and
   Marketing), or Custom (just a CEO).
2. **Staff.** Per head: title, model, reviewer. Add or remove roles. The model
   control is the composer's picker: search, a tab per installed CLI that can
   lead (Claude Code, Codex, Pi, OpenCode, Antigravity) with its icon, and a
   Reasoning row with the efforts the chosen model supports.
3. **Rules.** Worker pool, builds at a time, workers per head, hiring policy,
   rotate threshold.
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
**Settings**. The goal box at the bottom of the main area works like the chat composer (Enter sends, Shift+Enter adds a line); its chip picks the CEO or one head as the target.

## Heads

Click a head on the page to open its editor: title,
reviewer, model and reasoning (same picker as the wizard), max workers, what
it owns, its role file (editable) and its memory file (read only). From there
you can **Rotate
session** (clear the CLI session and onboard again with the same role),
**Open chat**, **Remove head** (archives the chat, keeps its files) or
**Save**. Changing the provider hands the chat off like any provider switch.

The org has no budgets; spend today shows as a plain amount. Context bars turn
amber at **Settings > Organization > Rotate a head's session at**.

## Card review and notes

Cards carry `dept:<role>` (owning head) and `review:<role>` (reviewer) labels.
Aether wakes heads on board changes so nobody has to poll:

- When a card enters Review (agent finished, a lead moved it, or you dragged
  it), the head named by `review:<role>` gets a message to review it and leave
  approve or concrete changes as a card note.
- When a head leaves a note on a card with `update_card`, the owning head
  (`dept:<role>`) and the CEO get the note, except whoever wrote it.

Done stays yours: move reviewed cards to Done on the board.

## Workers and hiring rules

Workers are child chats of a head, in their own worktree by default. The
**+ Hire worker** dialog picks the worker's model and reasoning with the same
picker. A hire passes the rules when:

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
