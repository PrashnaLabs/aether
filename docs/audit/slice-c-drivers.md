# Slice C: provider drivers and chat send path

## Scope and method

Reviewed crates/driver and crates/app/src/app/chat plus providers.rs against SECURITY.md, README.md, docs/orchestration.md and GLOSSARY.md. Static inspection followed launch options into process creation, stream ingestion into event parsing and UI delivery, and driver Drop implementations. No GUI was launched. This is a focused audit, not proof that all provider protocols are safe.

## Findings

| ID | Severity | Finding | Status |
| --- | --- | --- | --- |
| C-PERF-1 | Medium | Stream framing has no memory limit | Documented only: requires coordinated protocol failure handling |
| C-SEC-2 | Low | Optional recordings retain credential-bearing tool content | Fixed permissions; secret-redaction policy documented |

| C-PERF-3 | Medium | Pi reparses growing tool arguments per delta | Documented only |
| C-PERF-4 | Low | Claude copies growing activity prefix | Copy fixed; scan documented |
| C-PERF-5 | Medium | Active reply rows rebuilt per update | Fixed |

## C-PERF-1

Severity: Medium. Locations: crates/driver/src/claude/spawn.rs:153, crates/driver/src/codex/spawn.rs:102, crates/driver/src/pi.rs:147, crates/driver/src/antigravity.rs:510; crates/driver/src/opencode/session.rs:683.

The four CLI readers use BufReader::lines without a byte limit. Allocation happens before JSON validation. A provider emitting a large tool result, or an unterminated record, can consume arbitrary memory. Stderr readers have the same issue; retaining only 40 lines in StderrTail does not bound each line. OpenCode's frames function scans the accumulated buffer for a delimiter and drains complete frames. Missing delimiters retain input and repeated scans revisit the existing prefix.

Evidence: spawn launches piped children, reader threads obtain each whole line, then call serde_json parsing or map_line. Antigravity read follows the same order. OpenCode frames parses only after finding the entire frame. JSON parse errors therefore do not protect the allocation. Valid events also feed unbounded update senders, so limiting record sizes alone would not bound total pending updates.

Impact: local application memory exhaustion and delayed UI response from unexpectedly large provider output. This is a performance robustness finding; no hostile remote provider exploit was demonstrated.

Proposed fix: establish maximum JSON/SSE record sizes with provider-specific allowances for images and tool results, use bounded framing, and terminate/reap the affected session with a visible error on overflow. Add fragmented-input, oversized-record and EOF tests. Bound or coalesce pending updates without dropping approval events.

Status: Documented only. A safe fix requires coordinated error propagation, process shutdown and compatibility decisions across the five protocols. Silently truncating a JSON record or stopping a reader could leave an approval/session pending and is not a safe local fix.

## C-SEC-2

Severity: Low. Location: crates/driver/src/record.rs:19-63.

AETHER_RECORD enables raw stream recording. Claude's stdout reader calls rec.line before parsing. Recorder::line scrubs workspace/home paths and emails, but does not scrub secrets embedded in tool results, prompts or JSON fields. Recorder::create previously used default file permissions. Thus a tool response containing an API key remains in the optional recording, and its file may be readable by other users depending on umask and parent-directory permissions.

Impact: additional persistent copies of sensitive transcript content when diagnostic recording is explicitly enabled. No default recording or unconditional key leak was found.

Proposed fix: make diagnostic recording's sensitivity explicit, create private files and directories, and define which fields must be removed before recordings may be shared. Generic regex redaction cannot guarantee removal of arbitrary credentials.

Status: File-permission part Fixed. New directories request Unix mode 0700 using DirBuilderExt and new files request 0600 using OpenOptionsExt. Existing files/directories are not chmodded. A Unix regression test checks creation modes. Secret-redaction policy remains Documented only; complete secret removal requires a content policy.

## Other observations and limits

Shared spawn::command removes inherited orchestration socket/token/specs and OpenCode inline configuration; individual lead drivers explicitly add the selected lead credentials. Other environment variables remain inherited for CLI authentication. Direct Command argv construction was observed rather than interpolation into shell command strings. This does not establish that provider-side argument parsing accepts every possible model identifier safely.

Claude and Pi Drop close stdin, wait briefly on a worker thread, then kill and wait on the immediate child. This reaps that child; descendant process cleanup was not demonstrated. Claude's MCP configuration uses a token-derived temporary filename and mode 0600 on creation. Existing-file/symlink attacks need local access and were not promoted to an in-scope vulnerability under the supplied threat model.

## Seen outside my slice

No confirmed finding reported outside this slice.

## Verification

Follow-up changes touch aether-driver recording creation and its Unix unit tests. See verification results below. Performance findings are static source findings without benchmarks.

## Second pass: streamed event performance

Normal text/thinking deltas are owned delta strings emitted through Update and appended with push_str in Message::apply (crates/core/src/lib.rs:630-658, inspected read-only). The driver conversions allocate JSON Values, small output vectors and owned delta strings per event. No whole-transcript clone was found on the normal per-delta path in chat/send.rs:744-809. History snapshots occur at send/steer setup rather than each streamed event. apply persists at a two-second interval and refreshes explorer/git on completed tools. It calls cx.notify for every update; GPUI may coalesce notifications, so a render storm is not established by this call alone.

### C-PERF-3

Severity: Medium. Location: crates/driver/src/pi.rs:1306-1312. Status: Documented only, requires incremental parser/state redesign.

Each toolcall_delta appends with push_str, then serde_json::from_str parses the entire accumulated argument string, including incomplete prefixes. A large JSON string arriving in small fragments repeatedly scans the growing prefix, producing quadratic aggregate parsing work. toolcall_end provides final arguments, but removing intermediate parsing would change live tool classification, which existing tests exercise. Proposed fix: incremental parsing or explicitly rate-limited intermediate classification plus unconditional final classification; preserve live classification tests and add fragmented large-argument coverage.

### C-PERF-4

Severity: Low. Location: crates/driver/src/claude/events.rs:501-507 and :156-165. Status: Whole-prefix copy Fixed; repeated scanning Documented only.

Subagent text deltas append to sub_text, scan the accumulated prefix for a newline, previously cloned the entire prefix before set_activity selected at most ACTIVITY_CHARS. The conversion now selects the same trimmed first nonblank line and copies at most ACTIVITY_CHARS. A long first line therefore has repeated prefix scans and copies despite a bounded visible activity label. Proposed fix: store bounded first-line activity with explicit completion state, retaining whitespace/newline semantics. This needs more than a few-line safe patch because leading whitespace and line completion determine when updates stop.

### C-PERF-5

Severity: Medium. Location: crates/app/src/app/chat/send.rs:790-797; crates/app/src/app/chat/mod.rs:365-435. Status: Fixed.

Every active streamed update calls sync, which calls build_rows from the touched reply and constructs a fresh Vec of TranscriptRow values before computing splices. For the current reply, growing tool/step history is revisited even when only the final text/thought block changed. This scales with reply size per event, though it does not clone all Message vectors. Proposed fix: update the affected row and append new blocks incrementally, retaining full rebuilding for structural events; verify row splice and thought/tool remeasurement behavior. No full transcript cloning or per-delta sorting was confirmed in this path.

Fix: `ChatApp::extend_tail` (crates/app/src/app/chat/mod.rs) runs before the rebuild in `sync`. For a text delta it extends the last `Block::Text` row in place and remeasures only that row; for thinking that adds no step it leaves the rows untouched and keeps the existing thought remeasure. It only applies when the chat is busy, the reply is the last message, the last row is a `Text` ending at the old text length, no step or request position lies past it, and the old unfinished line cannot open a code fence (which would move earlier splits). Anything else (first text, text after a tool or thought, new step, request, non-live reply, fold change) returns `None` and takes the full `build_rows` + `row_splice` path. `Changed` now carries the pre-update text length and step count for this check, and toggling `show_hooks` now resyncs rows because the fast path no longer rebuilds on every delta. Tests in crates/app/tests/unit/app/chat/mod.rs compare the fast result with a full `build_rows`.

Follow-up verification: `CARGO_TARGET_DIR=/Users/iris/Downloads/xzx/PERSONAL/aether/target cargo clippy --locked -p aether-driver --all-targets` passed, including a final recheck. `cargo test --locked -p aether-driver` with the same target directory passed. `cargo fmt --all -- --check` and `git diff --check` passed. No app crate source changed. No commits were made.
