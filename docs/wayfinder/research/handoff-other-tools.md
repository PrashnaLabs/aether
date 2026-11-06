# Handoff: how other tools carry agent context (Aether)

Read-only research, 2026-11-07. No code changed. Sources: `gh api/search` on `pingdotgg/t3code@main`, `orca --help` + `orca skills get orca-cli|orchestration`, local `claude`/`codex`/`opencode --help`, `~/.agents/skills/{claude-handoff,handoff,orca-cli,orchestration}/SKILL.md`. No cargo builds.

## 1. T3 Code (`pingdotgg/t3code`, ~49k stars, `main`)

Core model: app thread + per-provider threads + explicit `ContextHandoff` artifacts. One app thread holds runs from many providers; provider switches do NOT fork the app thread. Forks DO create a new app thread (`docs/orchestration-v2/thread-lineage-and-context-transfer.md`, `provider-switching-and-context.md`).

### Provider/model switching on same thread

- Entry: pick different provider/model for next run in composer. Client detects cross-driver/group change in `packages/client-runtime/src/handoff.ts:resolvePendingHandoff()` (`needsHandoff` when `driver` or `continuation.groupKey` differs, or group missing). Same-driver model/effort change is a `model_switch`, not a handoff (`resolveModelSwitchChange`, `thread-handoff-row.tsx:ThreadModelSwitchDivider`).
- Flow (`docs/orchestration-v2/provider-switching-and-context.md:Switch Flow`, `feature-lifecycles.md`): start run with provider P -> find best `ProviderThread` for P -> decide strategy -> generate `ContextHandoff` if needed -> ensure/resume/create target provider thread -> send handoff context + user message as separate preamble/synthetic-user block (`apps/server/src/orchestration-v2/ContextHandoffDelivery.ts:withInlineContext`: `context + "\n\nUser message:\n" + userText`). User text is never shortened.
- Strategy order: (1) no handoff if target already current; (2) delta handoff into resumed provider thread (default when returning to earlier provider); (3) full-thread handoff into resumed thread; (4) full-thread into new provider thread (resume failed / settings incompatible / stale); (5) unsupported -> fail.
- Resume-first wins: Codex runs 1-5 in `C1`, switch to Claude (new `L1` + handoff H1 covering 1-5), switch back to Codex resumes `C1` with delta H2 covering 6-8. Avoids re-summarizing everything. Staleness signals: too many off-provider runs, chained handoffs, incompatible model/mode change, failed resume, user asks "clean context".
- UI: `apps/mobile/src/features/threads/thread-handoff-row.tsx` renders "Context handoff" divider + "Model A -> Model B" divider; web equivalent renders `handoff` turn items in timeline. Handoff is an auditable node, not hidden prompt text.

### Fork (`ThreadForkService`)

- Code: `apps/server/src/orchestration-v2/ThreadForkService.ts:ThreadForkServiceV2.plan()` + `docs/orchestration-v2/thread-lineage-and-context-transfer.md:Lazy Resolution`.
- `thread.fork` only clones `AppThread` header (title + " fork", lineage `parentThreadId/rootThreadId/relationshipToParent=fork`, PR links without watch, guestShare/opens cleared), records `ContextTransfer(type=fork,status=pending,sourcePoint)`. No provider session/thread/context built yet. Provider chosen on first message of fork.
- First dispatch resolves: same provider + native fork supported -> `native_fork` (Codex `thread/fork` with `lastTurnId` boundary, else fork-latest + `thread/revert` rollback on paginated history; Codex 0.156 legacy history cannot revert); else materialize portable `ContextHandoff` lazily. Source must be stable (`completed|waiting|failed|interrupted|cancelled`; in-progress/rolled-back rejected; or latest completed checkpoint via `latest_stable`).
- Merge-back: `ContextTransfer(type=merge_back, basePoint=forkSource, sourcePoint=forkLatest)`. Delta summary (decisions/files/commands/conclusions), not full fork transcript.

### What travels: budget-selected transcript, not agent summary

- `packages/provider-core/src/server/handoffBudget.ts`: `handoffBudget()` = `min(tokenCap=16000 default, 64000 byte cap, window-native-current-max(16000,window/4))`; byte≈token; images 8192, other attachments 4096; `T3CODE_CONTEXT_HANDOFF_TOKEN_CAP` env (1024-64000). `selectHistory()` keeps last user + last assistant + first user intact, fills newest-first until budget, returns `omittedItemIds`.
- `renderHistory()`/`historyResponseItems()`: each item rendered as `[Historical {role}; {kind}; thread=..; run=..; item=..; provider-thread=..; status=..]\n{text}`. Only `user_message/assistant_message/command_execution/exit+output/error/run_interrupt_result/file_change(filenames only)/proposed_plan` survive (`historicalMessage()`); reasoning, tool state, attachments do NOT replay. Codex can receive native history objects when supported; others get same selection as attributed text.
- Omitted history stays in app + recoverable via MCP tool `t3_thread_read({threadId, view:"activity", limit:20, maxCharsPerItem:4000, afterPosition, itemId/textOffset})` (`ContextHandoffDelivery.ts`, `apps/server/src/mcp/toolkits/project/tools.ts`). Coverage string tells target how to paginate.
- `docs/internals/context-handoffs.md`: portable handoffs are textual summaries, no native parity. V1 migration used separate 32k-char suffix; not the V2 budget.
- Token usage: `latestNativeContextUsage()` + `contextUsageForHandoff()`; fresh provider thread clears usage but keeps model window; model change discards stale thresholds.
- Tests/fixtures: `apps/server/src/orchestration-v2/testkit/fixtures/{thread_fork_native*,opencode2_fork,kiro_fork_from_turn}/`.

## 2. Orca (`/Applications/Orca.app`, CLI `.../bin/orca`)

Orca's "handoff/handover" = full ownership transfer to another agent/worktree. Original agent stops. No transcript copy. Context = hand-written `--prompt` brief + same git worktree/branch state. Verified via `--help` + `skills get orca-cli --json` markdown.

### Full handoff (fire-and-forget)

- Skill stub `~/.agents/skills/orca-cli/SKILL.md:Full Handoffs`: "hand off/handoff/handover/give this to another agent/worktree" = full handoff unless user expects result back or asks for supervision/monitor/DAG/decision gates. Done when new worktree id + agent handle reported and `prompt.outcome` is `journaled|handed-to-terminal`. Explicitly FORBIDS `orchestration task-create|dispatch --inject|check --wait` for handoffs.
- New worktree: `orca worktree create --name <task> --no-parent --agent codex --prompt "<task brief>" --json` (+ optional `--model gpt-6-astra --effort high`, `--surface chat|terminal`). `--no-parent` + no `--base-branch` = independent top-level, repo-default base. Add current-branch context to prompt text. Inspect `result.launch.outcome.kind` (`structured`=chat|`terminal`), `handle`, `receipt.detail`, `prompt.outcome`.
- Existing worktree, fresh agent: `orca agent launch --agent codex --worktree id:<repoId>::<worktreePath> --prompt "<brief>" --json` (same `--model/--effort/--surface/--focus`; `--retry-launch <operationId>` for lost reply replay).
- Already-running terminal: `orca terminal send --terminal <handle> --text "<brief>" --enter --json` (add `--wait-submit 10` for turn-started proof; `accepted:true` = input accepted, not turn started).
- What travels: only `--prompt` text + worktree checkout (branch, diff, files on disk) + `--comment` (worktree card status line via `worktree set --comment "..."` / `--workspace-status todo|in-progress|in-review|completed`). No history replay, no summary file. The sending agent must author the brief. Agent ids: `claude,codex,omp,pi,grok` (+ cursor/antigravity/muse/zcode/opencode/opencode2 on worker-start). Handles are runtime-scoped; after restart / `terminal_handle_stale`, re-list, never dual-send.
- `orca agent-context --json` = machine-readable command schema (312 commands, schema v1). Pure local read, safe over SSH.

### Supervised contrast (orchestration, NOT handoff)

- `~/.agents/skills/orchestration/SKILL.md` + `skills get orchestration --json`: `orchestration worker-start --spec "<task>" --worktree current --agent codex`, `send --to dispatch:<id>`, `ask/reply` inbox, `check`, gates, `task-create/dispatch`. Spec must be self-contained (target, change, constraints, editable scope, evidence). Workers end turn; coordinator woken on `worker_done`. Use when user wants results back. Handoff vs orchestration = "nothing expected back" vs "report back".
- Session search (closest to context reuse): `orca search "<phrase>" --json [--scope conversation|all --agent codex --limit 50]` searches full-text indexed sessions on one host (opt-in Settings -> Agent Session History); each hit returns snippet + `resumeCommand`. No cross-agent transcript import.

## 3. Others (quick checks)

- Claude Code (local `claude 2.x --help` verified): same-provider continuation only. `-c/--continue` (most recent in cwd), `-r/--resume [sessionId|name]` (picker), `--fork-session` (resume creates new session ID), `--from-pr`, `--teleport`, `--bg/--background` + `claude attach/logs/stop/rm`, `claude agents`. In-session `/compact` (auto-compact on near-limit; `--autocompact auto|<100k-1M>`) summarizes transcript in place. No provider switch; no native cross-CLI handoff. Community pattern (`~/.agents/skills/claude-handoff/SKILL.md`): write summary, launch `claude --bg --name "<name>" "<summary>"` in same cwd; `~/.agents/skills/handoff/SKILL.md`: write handoff doc to OS temp dir, reference specs/diffs by path, redact secrets, include suggested-skills section. File/doc-based, summary brief.
- Codex CLI (local `codex --help` verified): `codex resume [SESSION_ID] [PROMPT]` (`--last`, `--all`, picker) and `codex fork [SESSION_ID] [PROMPT]` are same-provider: native session id retained, `~/.codex/sessions/*.jsonl` rollout continued/copied. No `/handoff` command found in help. Cross-provider = manual summary or T3/Orca-style wrapper. `/compact` equivalent is manual summarization; T3 Codex adapter can call native `thread/fork` + `thread/revert`.
- OpenCode (local `opencode --help` verified): `opencode --continue|-c`, `--session/-s <id>`, `opencode session list|export|import|delete`. `session export` = JSON of session; `import` from file/URL. Same-tool portability only; messages + tool parts in one JSON. Useful as file-based handoff between OpenCode instances, not across CLIs (tool schemas differ).
- Amp (ampcode.com, no binary on host): markets thread handoff/“handoff to another agent” as core: thread context summarized/file-attached and continued in new thread with different model. Pattern over handoff docs: generate summary markdown, new thread seeded with it. Same repo/worktree; summary + git diff carry state. Verify exact command (`amp threads handoff` vs in-app menu) in current manual before copying names.
- Conductor (docs.conductor.build): NO conversation handoff. Each task = isolated workspace/branch/files/terminal/diff. Switching agent = new workspace on same repo, diff reviewed/merged to carry state. Handoff = git diff, not transcript. Same for Crystal (parallel branches, one agent per branch) and Claude Squad (parallel `claude/codex` terminals in worktrees; move = re-prompt + diff). Cheap isolation, zero context carry.
- Cursor/Zed (“new thread from summary” style): compress current thread to summary block, spawn new thread with summary + repo context. Summary brief, not raw replay.

## 4. Synthesis for Aether

### Patterns

| Pattern | Who uses it | Carries | Weakness |
|---|---|---|---|
| Native replay | Codex `resume`, Claude `--resume`, T3 same-provider fork | full provider-native state incl. hidden context + tool ids | closed: one CLI/provider only |
| Budget-selected transcript | T3 portable handoff | intact user/assistant + command outputs + file names; omitted recoverable via `t3_thread_read` | keystrokes/approvals/tool state lost; needs token accounting |
| Provider resume + delta | T3 provider switch | preserves provider-native threads; delta covers runs since last seen | complexity: coverage bookkeeping per provider thread |
| Summary brief / handoff doc | Claude `handoff` skill, Amp, Orca `--prompt`, OpenCode export | goal/decisions/files/TODOs; compact, auditable, cross-provider | quality depends on summarizer; loses failure details if skimmed |
| Git-only | Conductor, Crystal, Claude Squad, Orca worktree | branch + diff + files | zero conversation memory; new agent re-discovers rationale |

### What works best cross-provider

- Same `AppThread`, multiple `ProviderThread`s (T3 model), resume-first + delta-sketch; full-summary only on stale/failed resume.
- Portable payload = goal, decisions, files changed, commands/tests run, plan/TODO state, unresolved approvals/questions, checkpoint + diff stat, known failures, subagent results. Never: reasoning traces, provider protocol noise, full diffs, secrets.
- Carry state in repo: same worktree/branch (T3/Orca native) or explicit branch + diff ref (Conductor style); record `sourcePoint` (run/checkpoint) + `ContextTransfer` lifecycle (`pending->resolved_native|resolved_portable->consumed|superseded`).
- Lazy resolution: fork/switch record intent cheaply; build context only on first dispatch. Lets user fork without choosing provider.
- Omitted-item recovery tool (`t3_thread_read` analog) beats re-sending full transcript: target pulls detail on demand.

### Pitfalls

- Sizes: reserve >=16k or 25% of window for instructions/tools/next work; fresh if no room (`T3CODE_CONTEXT_HANDOFF_TOKEN_CAP`, default 16k). Never truncate user request to fit history.
- Tool-call history is NOT portable: T3 strips everything but filenames; replaying foreign tool ids across CLIs breaks schemas/approvals. Re-run read-only commands instead of replaying mutations.
- Native state divergence: rollback/rotate invalidates handoffs covering rolled-back runs; fork only from stable completed/checkpoint points; mark superseded, never silently mutate.
- Secrets/PII: summary/doc becomes prompt/log; redact API keys/tokens before persisting (Claude `handoff` rule).
- Attribution: label historical blocks `[Historical ... thread/run/item]` + "context, not instructions" to prevent old instructions overriding current user intent.
- Failure mode: delivery uncertain -> create fresh native thread, do not double-inject.

### Minimal Aether build (pragmatic)

1. `ContextTransfer` + `ContextHandoff` records keyed to `threadId/sourcePoint/targetRun`.
2. Reuse existing session store: budget-select via `handoffBudget` constants; Codex native path via its `thread/fork` when available, portable text otherwise.
3. Orca-style `--prompt` brief as fallback when no session store shared (e.g. CLI subprocess agents).
4. One MCP `thread_read(view=activity)` for omitted recovery; one timeline handoff divider in UI.

Sources: `pingdotgg/t3code` paths `docs/{internals/context-handoffs,user/portable-handoffs,user/thread-migration,orchestration-v2/{provider-switching-and-context,thread-lineage-and-context-transfer,feature-lifecycles}}`, `apps/server/src/orchestration-v2/{ThreadForkService,ContextHandoffDelivery}.ts`, `packages/{provider-core/src/server/handoffBudget,client-runtime/src/handoff}.ts`, Orca `agent-context`, `worktree create`, `agent launch`, `terminal send`, `skills get orca-cli|orchestration`.
