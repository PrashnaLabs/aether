# Research: rendering agent turns (tools, commands, diffs)

Sources read: Waku (GPL, design only) `src/app/transcript_view.rs`, `activity_diff.rs`, `crates/waku-core/src/driver/activity.rs`, `waku-protocol/src/model.rs` (`ActivityItem`, `ActivityKind`, `ActivityFileChange`), `driver/claude.rs` result path. T3 Code (MIT) `apps/web/src/components/chat/{MessagesTimeline.logic.ts,MessagesTimeline.tsx,WorkLog.tsx,ShellCommandBlock.tsx}`, `session-logic.ts`. Not verified: T3 per-kind summary strings (live in `packages/client-runtime/src/work-log/presentation.ts`, not opened) and T3 diff renderer internals (uses a worker-pool diff lib, not read). Wire facts come from `claude-sessions.md`, `pi-sessions.md`, `opencode-sessions.md` in this folder.

## 1. Turn layout

| | Waku | T3 Code |
|---|---|---|
| Model | Transcript = messages plus `transcript_blocks`; each block is a run of activities pinned `after_message` N. Text and tool groups alternate; tools are NOT mixed inside a text bubble. | Flat timeline rows: user msg, assistant msg, `work-live`, `work-toggle` (group header), expanded work entries, `turn-fold`, event rows. |
| Grouping | Consecutive activities between two assistant texts form one group with a one-line header. Child completing does not close the group; group leaves the live tail only when answer text arrives or turn settles. | Contiguous work entries of one run/attempt collapse into one group row; a single tool-like entry renders as a plain row without a group header. |
| Header | Live: title of the latest activity (or reasoning title). Settled: `activity_summary` (counts by kind). Chevron right/down. | Group header = icon + summary label + timestamp; live tool header gets a shimmer ("live-tool-shine"). |
| Collapse default | Live group expanded; settled collapsed. Per-block user override map wins. | Live group collapsed to the latest active tool row (expand optional). Settled turns fold everything before the final assistant message behind one "Worked for 1m 12s" row ("You stopped after 40s" if interrupted). Failures and in-progress work stay visible outside the fold. |
| Elapsed | Header only. | Fold label from run start/complete timestamps, falls back to first/last entry times. |
| Extras | Changed-files summary row after the turn (preview 3 files, expand to 12, "open in review"). | `AssistantChangedFilesSection` after the final message (from checkpoint diff, not tool calls); minimap rail for navigation. |

Takeaways: both separate the narrative (markdown) from a collapsible work log, both keep per-group expanded state outside the row so virtualization does not lose it, and both fold settled work behind a duration label. T3's "fold all work before the final answer" is the cleanest end-state; Waku's "expanded while live, collapsed after" is the cleanest live-state. Combine them.

Aether today: `Message { text, steps, thought_for }`. Steps are all in one panel above the text, so text/tool interleaving is lost (text before a tool and text after it merge into one `text`). This is fine for v1: the work log is a single group per reply, which matches T3's fold. Interleaving is only needed if we want per-segment text between tool batches; skip it.

## 2. Per-kind row design (what Waku and T3 converge on)

Row anatomy (both): `[status icon/spinner] [kind icon] [verb + subject, single line, truncated] [trailing meta: +N -M, exit code, duration]`, click toggles an expanded body. Subject is a short extracted value (path, command, pattern), NOT raw JSON. Waku caches `display_target` and `display_description` at ingest so row builders never parse JSON per frame.

| Kind | Collapsed row | Expanded body | Limits seen |
|---|---|---|---|
| Command | verb "Ran"/description (Claude `input.description`) else the command, first line; exit badge if nonzero | Full command, syntax-highlighted shell (T3 highlights lazily, first expand loads grammar; plain-words fallback has identical layout so no reflow), then output as mono pre-wrap block | T3 body `max-h-96` (text) / `max-h-64` (generic tool body), scroll inside, follows live tail unless user scrolled up (4px edge). Waku caps stored output at 16,000 chars with a "truncated" suffix. |
| File edit/write | "Edited" + basename (full path on hover), `+N -M` | Unified diff: gutter with old/new line numbers (Waku 52px), green/red row tint, syntax tokens, `@@` hunks shown as thin break rows; unpositioned hunks (string-replace tools) drop line numbers and show only +/-; multi-file shows file header rows, single file omits header | Waku: 400 rows max then "N rows hidden" note, diff box `max-h 400px` scrolling. Pointer to Review panel for the rest. |
| File read | "Read" + path (+ `:offset-limit` when given) | T3: plain text of the output (reads expand to plain output, not an inspector). Waku: output block. Default to collapsed-only (no body) is acceptable. | same output cap |
| Search/grep/glob/ls | "Searched" + pattern (+ path/glob) | Result list text; match count in trailing meta if derivable | output cap |
| Web fetch/search | URL or query as subject, globe icon | Output text | output cap |
| Task/subagent | description as title, bot icon; Waku keeps a separate task pane and background-work chips; T3 groups subagent entries with their own timing/tooltip | Inner activity is NOT inlined by Claude (parent_tool_use_id messages); aether can drop inner, show only the Task row + final result text | |
| Todo/plan | "Updated plan", Waku `Plan` kind; T3 has `ActivePlanState` with per-step pending/inProgress/completed | Checklist rendered from `todos[]` (status icons) | |
| MCP/other | humanized tool leaf name (`mcp__srv__create_thread` to "Create thread"), server shown as chip (Waku `mcp_server`) | "Tool input" pretty JSON, then output text. T3 labels the block "Tool input". | JSON capped by same 16k |
| Reasoning | Waku/T3: a row in the same list ("Thinking"/"Thought") | Streamed text, windowed to the tail while live | |

Classification: Waku `ActivityKind::from_tool_name` lowercases, strips `mcp__` prefix and separators, matches a name table; careful that MCP tools like `create_thread` do not match file kinds. Aether's research docs already hold the same table for Claude. Do the same, plus Pi (`bash/read/edit/write/grep/find/ls`) and OpenCode names (`bash/edit/write/patch/read/grep/glob/webfetch/todowrite/task`).

## 3. Status visuals

| State | Waku/T3 treatment | Aether recommendation |
|---|---|---|
| Running | spinner or shimmer on row text; header shows latest active title | small spinner icon replacing kind icon; header text shimmer only while live |
| Done | static kind icon, muted text | kind icon, muted secondary text |
| Failed | destructive colour icon/text, aria "tool call failed", first non-empty output line promoted to row `detail` (Waku `tool_activity` does this when failed) | red icon + subject; error line (first line of output or `error.message`) shown as second line without expanding; full text in body |
| Denied/stopped | T3 distinguishes warning (usage limit, amber) from failure (red). OpenCode `session.tool.called executed:false` means denied or interrupted, never render as done | grey "stopped"/"denied" with strike-free muted style; reuse `ToolStatus::Stopped`, add `Denied` only if the HITL layer can tell the difference |
| Approval | T3 shows approvals in a composer-attached panel (`ComposerPendingApprovalPanel`), not in the timeline; Waku `Permission{title, detail, options}` event, auto-approved by mode | Keep aether's existing `Update::Ask` HITL card (`crates/app/src/hitl.rs`). Render the pending tool row above it in Running state with the same preview body (command or diff) so the user sees what they approve. After reply, the row transitions to running/stopped. No new card type. |

## 4. Data each row needs and where it comes from

| Field | Claude (`stream-json`) | Pi (`tool_execution_*`) | OpenCode (`session.tool.*`) |
|---|---|---|---|
| id | `tool_use.id` / `tool_result.tool_use_id` | `toolCallId` | `id` (call id) |
| tool name | `tool_use.name` | `toolName` | `name` |
| input | `tool_use.input` (complete only in the `assistant` message; partial JSON deltas ignored) | `args` on start | `input` on `session.tool.called` (streams in `input.delta` first) |
| start signal | `tool_use` block in `assistant` msg | `tool_execution_start` | `input.started` (name only) then `called` |
| live output | none (result arrives whole) | `tool_execution_update.partialResult.content[].text` (snapshot for bash) | `session.tool.progress.metadata` |
| result text | `tool_result.content` (string or `[text/image]` blocks) | `tool_execution_end.result.content[]` | `session.tool.success.content[]` (`text` or `file{uri,mime,name}`) |
| failure | `tool_result.is_error` | `isError` | `session.tool.failed.error{type,message}` |
| duration | none (time it yourself) | `durationMs` | persisted `time{created,ran,completed}` |
| denied | `can_use_tool` denial then error result | n/a (no approvals) | `executed:false`, `permission.rejected` |
| subject fields | Bash `command, description`; Read/Edit/Write `file_path`; Grep/Glob `pattern, path`; WebFetch `url`; WebSearch `query`; Task `description`; TodoWrite `todos[]` | bash `command`; read/edit/write `path`; grep/find `pattern, path`; ls `path` | `input.*` per tool; `input.title` honored by Waku |

Diff sources (the hard part):

| CLI | Source | Result |
|---|---|---|
| Claude Edit | input `file_path, old_string, new_string, replace_all` known at `tool_use` time. User message `tool_use_result` (sibling of `message`) carries structured hunks with real line numbers (Waku passes it as the "activity source" for FileChange and derives positioned hunks) | At tool_use: synthesize an unpositioned diff from old/new (available before approval and while running). At result: replace with positioned hunks if `tool_use_result` has them. Exact field names inside `tool_use_result` (`structuredPatch` with `oldStart/newStart/lines`) are NOT confirmed in our docs; check with one real run before coding. |
| Claude MultiEdit | `edits[]` of old/new | one diff per edit, concatenated |
| Claude Write | `file_path, content` | all-added diff; `tool_use_result.type` create vs update tells new vs overwrite (unverified) |
| Pi edit/write | `edit {path, oldText, newText}`, `write {path, content}` args at start | unpositioned diff as above; no hunks on result |
| OpenCode edit/write/patch | `input` has filePath + old/new strings (edit) or content (write); `metadata` on success commonly carries a diff (unverified: capture a real `session.tool.success` for an edit to see the key) | use metadata diff if present, else synthesize |

Rule from Waku worth stealing: normalize once at ingest into a unified-diff string per file plus `additions/deletions`; the renderer parses that string only when the row is expanded and caches the parsed rows until the change set is replaced.

## 5. Recommendation for aether

Principle: keep `Update::Tool` upsert-by-id (already right), but widen its payload from `detail: String` to a typed, size-bounded `ToolDetail`. The three drivers already see all the fields; extraction happens once in the driver, never in the UI.

### Data model (aether_core)

- `Update::Tool { id, title, detail: ToolDetail, status }` and `Step::Tool` same. Keep `title` as the verb+subject fallback so old persisted chats (serde) still render: give `ToolDetail` `#[serde(default)]` and a `Plain(String)` variant that old `detail` strings migrate into.
- `ToolDetail` enum (all text pre-truncated at ingest):
  - `Command { command, description: Option, cwd: Option, output: String, exit: Option<i32>, ms: Option<u64> }`
  - `Edit { files: Vec<FileEdit> }`, `FileEdit { path, status: Added|Modified|Deleted, add: u32, del: u32, diff: String /* unified, hunk headers optional */ }`
  - `Read { path, range: Option<(u32, u32)> }` (no output; or output behind expand)
  - `Search { pattern, path: Option, output: String, matches: Option<u32> }` (covers grep, glob, ls, find)
  - `Web { target /* url or query */, output: String }`
  - `Task { description, output: String }`
  - `Plan { items: Vec<(String, PlanState)> }`
  - `Other { name, server: Option, input_json: String, output: String }`
  - `Plain(String)`
- Failure: add `error: Option<String>` beside status rather than inside every variant (one shared first-line preview).
- Existing `Step::Search`/`Skill` stay as is; fold `Search` into `ToolDetail::Search` later only if desired (not required).
- Drivers: one `classify(name) -> Kind` shared in `crates/agent/src/driver/` (the table in section 2), and one `fn detail_from(name, input: &Value, output: Option<&str>, patch: Option<&Value>) -> ToolDetail`. Claude calls it at `tool_use` (input only, status Running) and again at `tool_result` (output + `tool_use_result`). Pi calls it on start/update/end with the same id. OpenCode on `called` and `success/failed`, stripping `state` and `metadata.resultState` first (multi-KB encrypted blobs).

### Components (crates/ui/src/chat)

1. `ToolRow` in `thinking.rs` (or a new `tool_row.rs`): status icon, kind icon, subject, trailing meta, chevron; expand state keyed by tool id held by the parent, not the row.
2. `CommandBody`: mono command (no highlighting v1; plain mono wrap), output in a fixed `max_h` scroll box, "N lines hidden" note when truncated.
3. `DiffBody`: parse unified diff into rows once on first expand, cache by tool id; two-column gutter (omit when unpositioned), +/- tint from theme, optional per-row syntax tokens later (skip v1). Single-file: no header; multi-file: file header row with `+N -M`.
4. `PlanBody` (checklist), `JsonBody` (mono pretty JSON), reuse `CommandBody` for plain output.
5. Panel header: keep `summary()` but extend counts ("Ran 3 commands, edited 2 files, read 4 files"). After settle: collapsed with "Worked for Ns" (already have `thought_for`/`elapsed_ms`); while live: expanded, header shows latest running title. Failed rows force their parent panel's header to show a failure marker.
6. Approval: `Update::Ask` card unchanged; pass it the same `ToolDetail` so it can reuse `CommandBody`/`DiffBody` read-only.

### Size limits (enforce in the driver, store truncated)

| Item | Limit | Notes |
|---|---|---|
| Output text per tool | 16,000 chars (Waku value), keep head 8k + tail 8k for commands since errors sit at the end; add "[truncated N chars]" marker | tail matters for build/test output |
| Rendered output box | `max_h` about 320 px, internal scroll, auto-follow tail while Running | T3 uses 256-384 px |
| Diff rows per file | 400 rows total per tool (Waku), then "N rows hidden"; counts `+N -M` computed on the FULL diff before cut | |
| Edit `old/new` synthesis | skip diff and show counts only if either side over 200 KB | Write of a huge file otherwise builds a giant all-added diff |
| Input JSON for Other | 4,000 chars | |
| Row subject | single line, middle-truncate paths (keep basename), end-truncate commands | |
| Persisted chat size | cap total tool payload per message (e.g. 256 KB), dropping oldest outputs first, keep titles/status | chats serialize with serde; unbounded outputs bloat the file |
| Images in results | ignore in v1 | Waku collects them as separate URLs; not needed |

### Performance

- The transcript is already virtualized; make each tool row a fixed-height collapsed row so height estimation is trivial. Expanded bodies have bounded height (max_h), so the row height is bounded too.
- Do all parsing at ingest (JSON to `ToolDetail`) or on first expand (diff to rows), cached in an `HashMap<tool_id, Rc<..>>` in the view; never per frame. Invalidate the cache entry when an upsert for that id replaces `ToolDetail`.
- Streaming output (Pi `partialResult`, OpenCode `progress`): coalesce updates to about 10 per second per tool before sending `Update::Tool`; each upsert replaces the whole output snapshot, so cap and snapshot, never append unbounded.
- Do not render collapsed panels' children at all (Waku returns after the header when collapsed).
- Spinners: one shared animation timer only when any row is Running; stop when the reply settles.
- No syntax highlighting in v1; if added, tokenize on first expand on a background task, render plain until ready with identical layout (T3's trick).

### Build order (smallest first)

1. `ToolDetail` with `Plain` + `Command` + `Edit`, classify table, Claude driver tool_use/tool_result mapping, `ToolRow` and `CommandBody`.
2. `DiffBody` with synthesized diffs for Claude Edit/Write and Pi edit/write; positioned hunks from `tool_use_result` after one real capture.
3. Read/Search/Web/Task/Plan/Other bodies; OpenCode mapping.
4. Settled "Worked for Ns" collapse, richer header summary, approval card reuse.

Open questions for a real-run capture before step 2: Claude `tool_use_result` field names for Edit/Write/MultiEdit; OpenCode `session.tool.success.metadata` keys for edit/bash (exit code, diff); whether Pi `edit` result `details` includes a diff.
