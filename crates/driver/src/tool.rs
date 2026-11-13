use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use futures::channel::mpsc::UnboundedSender;
use serde_json::Value;

use aether_core::{
    Attachment, MutexExt, PlanItem, PlanStatus, ToolCall, ToolDetail, ToolStatus, Update,
    hunk_header,
};

use crate::spawn::log_unknown;

const OUTPUT_HALF: usize = 8_000;
pub(crate) const OUTPUT_CAP: usize = 64 * 1024;
const LINE_SLACK: usize = 1024;
const HIDDEN: &str = " bytes earlier output hidden\n";
const DIFF_LINES: usize = 400;
pub const INPUT_CHARS: usize = 4_000;
const DIFF_LINE_CHARS: usize = 1_500;
const COMMAND_CHARS: usize = 8_000;
const DESCRIPTION_CHARS: usize = 2_000;
const TITLE_CHARS: usize = 200;
/// Past this either side of an edit shows counts only; diffing it costs more than it shows.
const SYNTH_BYTES: usize = 200_000;
/// Line-diff budget (old lines x new lines); past it an edit shows as remove-all, add-all.
const LCS_CELLS: usize = 250_000;

/// Row title and typed detail for a tool call. Names match case-insensitively;
/// MCP tools stay `Other` even when their leaf name looks like a file tool.
pub(crate) fn classify(name: &str, input: &Value) -> (String, ToolDetail) {
    let text = |keys: &[&str]| {
        keys.iter().find_map(|k| input[*k].as_str().filter(|v| !v.is_empty())).map(str::to_string)
    };
    let lower = name.to_ascii_lowercase();
    let lower = if lower.starts_with("mcp__") { String::new() } else { lower };
    let titled = |fallback: &str, keys: &[&str]| {
        text(keys)
            .and_then(|t| t.lines().next().map(|l| clip(l, TITLE_CHARS)))
            .unwrap_or_else(|| fallback.to_string())
    };
    match lower.as_str() {
        "bash" | "shell" => (
            titled("Run command", &["description"]),
            ToolDetail::Command {
                command: clip(&text(&["command", "cmd"]).unwrap_or_default(), COMMAND_CHARS),
                cwd: text(&["cwd", "workdir"]),
                output: String::new(),
                exit_code: None,
            },
        ),
        "edit" | "multiedit" | "str_replace" | "write" | "patch" | "apply_patch"
        | "notebookedit" => {
            let title = match lower.as_str() {
                "write" => "Write file",
                "notebookedit" => "Edit notebook",
                "patch" | "apply_patch" => "Apply patch",
                _ => "Edit file",
            };
            (title.into(), edit(input))
        }
        "read" | "view" => (
            "Read file".into(),
            ToolDetail::Read {
                path: text(&["file_path", "filePath", "path"]).unwrap_or_default(),
                range: range(input["offset"].as_u64(), input["limit"].as_u64()),
            },
        ),
        "grep" | "glob" | "find" | "search" | "ls" | "list" => {
            let title = match lower.as_str() {
                "grep" => "Search files",
                "glob" | "find" => "Find files",
                "ls" | "list" => "List files",
                _ => "Search",
            };
            let detail = ToolDetail::Search {
                pattern: text(&["pattern", "query", "glob"]).unwrap_or_default(),
                path: text(&["path", "directory", "dir"]),
                matches: None,
                output: String::new(),
            };
            (title.into(), detail)
        }
        "webfetch" | "web_fetch" | "websearch" | "web_search" => {
            let title = if lower.contains("fetch") { "Fetch" } else { "Web search" };
            let target = text(&["url", "query"]).unwrap_or_default();
            (title.into(), ToolDetail::Web { target, output: String::new() })
        }
        "task" | "agent" | "subagent" => {
            let description =
                clip(&text(&["description", "prompt"]).unwrap_or_default(), DESCRIPTION_CHARS);
            let title = titled("Run agent", &["description"]);
            (
                title,
                ToolDetail::Task {
                    description,
                    output: String::new(),
                    agent: text(&["subagent_type", "agent", "agentType"]).unwrap_or_default(),
                    prompt: clip(&text(&["prompt"]).unwrap_or_default(), DESCRIPTION_CHARS),
                    tools: 0,
                    tokens: 0,
                    activity: String::new(),
                },
            )
        }
        "todowrite" | "todo_write" | "update_plan" | "plan" => {
            let title = if lower.contains("todo") { "Update to-dos" } else { "Update plan" };
            (title.into(), ToolDetail::Plan { items: plan(input) })
        }
        _ if is_question_tool(name) => {
            let question = parse_question_and_choices(input).map(|(q, _)| q).unwrap_or_else(|| {
                input
                    .get("question")
                    .and_then(Value::as_str)
                    .filter(|q| !q.trim().is_empty())
                    .map(str::to_string)
                    .unwrap_or_default()
            });
            let detail = ToolDetail::Other {
                name: if name.is_empty() { "question".to_string() } else { name.to_string() },
                input: clip(&question, INPUT_CHARS),
                output: String::new(),
            };
            ("Ask question".into(), detail)
        }
        _ => {
            let input = match input {
                Value::Null => String::new(),
                v => clip(&serde_json::to_string_pretty(v).unwrap_or_default(), INPUT_CHARS),
            };
            let detail = ToolDetail::Other { name: name.to_string(), input, output: String::new() };
            (humanize(name), detail)
        }
    }
}

/// The CLIs' own ask-the-user tools; names only, so MCP tools that take a `question` stay themselves.
fn is_question_tool(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "question"
            | "askuserquestion"
            | "ask_user_question"
            | "ask_question"
            | "askfollowupquestion"
            | "ask_followup_question"
    )
}

pub(crate) fn parse_question_and_choices(input: &Value) -> Option<(String, Vec<String>)> {
    if let Some(questions) = input.get("questions").and_then(Value::as_array)
        && let Some(first) = questions.first()
    {
        let question = first
            .get("question")
            .and_then(Value::as_str)
            .or_else(|| first.get("header").and_then(Value::as_str))?
            .trim()
            .to_string();
        let mut choices = Vec::new();
        if let Some(options) = first.get("options").and_then(Value::as_array) {
            for opt in options {
                if let Some(label) = opt.get("label").and_then(Value::as_str) {
                    let label = label.trim();
                    if !label.is_empty() {
                        if let Some(desc) = opt
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|d| !d.is_empty())
                        {
                            choices.push(format!("{label} — {desc}"));
                        } else {
                            choices.push(label.to_string());
                        }
                    }
                } else if let Some(s) = opt.as_str() {
                    let s = s.trim();
                    if !s.is_empty() {
                        choices.push(s.to_string());
                    }
                }
            }
        }
        return Some((question, choices));
    }
    if let Some(q) = input.get("question").and_then(Value::as_str) {
        let question = q.trim().to_string();
        let mut choices = Vec::new();
        if let Some(options) = input.get("options").and_then(Value::as_array) {
            for opt in options {
                if let Some(label) = opt.get("label").and_then(Value::as_str) {
                    let label = label.trim();
                    if !label.is_empty() {
                        if let Some(desc) = opt
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|d| !d.is_empty())
                        {
                            choices.push(format!("{label} — {desc}"));
                        } else {
                            choices.push(label.to_string());
                        }
                    }
                } else if let Some(s) = opt.as_str() {
                    let s = s.trim();
                    if !s.is_empty() {
                        choices.push(s.to_string());
                    }
                }
            }
        }
        return Some((question, choices));
    }
    None
}

/// A finished call read back from a CLI's own log: typed like a live one, with its result when known.
pub fn logged_call(id: &str, name: &str, input: &Value, result: Option<(&str, bool)>) -> ToolCall {
    let mut call = start(id, name, input);
    match result {
        Some((text, failed)) => finish(&mut call, text, failed),
        None => call.status = ToolStatus::Done,
    }
    call
}

pub(crate) fn start(id: impl Into<String>, name: &str, input: &Value) -> ToolCall {
    let (title, detail) = classify(name, input);
    ToolCall::new(id, title, detail)
}

/// Stores a result or live snapshot in the detail's output, capped.
pub(crate) fn set_output(detail: &mut ToolDetail, text: &str) {
    let text = &clean(text);
    let kept = match detail {
        ToolDetail::Command { output, .. }
        | ToolDetail::Search { output, .. }
        | ToolDetail::Web { output, .. }
        | ToolDetail::Task { output, .. }
        | ToolDetail::Other { output, .. } => !output.is_empty(),
        _ => false,
    };
    // an empty final result must not wipe output that streamed in
    if kept && text.trim().is_empty() {
        return;
    }
    match detail {
        ToolDetail::Command { output, .. }
        | ToolDetail::Web { output, .. }
        | ToolDetail::Task { output, .. }
        | ToolDetail::Other { output, .. } => *output = head_tail(text),
        ToolDetail::Search { output, matches, .. } => {
            *output = head_tail(text);
            *matches = Some(match_count(text));
        }
        _ => {}
    }
}

/// Adds to a detail's output, keeping only the last `OUTPUT_CAP` bytes.
pub(crate) fn append_output(detail: &mut ToolDetail, extra: &str) {
    if let ToolDetail::Command { output, .. }
    | ToolDetail::Search { output, .. }
    | ToolDetail::Web { output, .. }
    | ToolDetail::Task { output, .. }
    | ToolDetail::Other { output, .. } = detail
    {
        output.push_str(extra);
        if output.starts_with('\n') {
            *output = output.trim_start().to_string();
        }
        cap_output(output);
    }
}

/// Cuts `text` to its last `OUTPUT_CAP` bytes behind one `… N bytes earlier output hidden` line.
/// The count of an earlier cut is read back from that line, so recapping keeps one accurate marker.
pub(crate) fn cap_output(text: &mut String) {
    let (hidden, body) = text
        .strip_prefix("… ")
        .and_then(|rest| rest.split_once(HIDDEN))
        .and_then(|(n, rest)| Some((n.parse::<usize>().ok()?, text.len() - rest.len())))
        .unwrap_or((0, 0));
    if text.len() - body <= OUTPUT_CAP {
        return;
    }
    let mut cut = text.len() - OUTPUT_CAP;
    while !text.is_char_boundary(cut) {
        cut += 1;
    }
    let near = &text.as_bytes()[cut..text.len().min(cut + LINE_SLACK)];
    if let Some(nl) = near.iter().position(|&b| b == b'\n').filter(|nl| cut + nl + 1 < text.len()) {
        cut += nl + 1;
    }
    let hidden = hidden + cut - body;
    text.replace_range(..cut, &format!("… {hidden}{HIDDEN}"));
}

/// Settles a call from its result text: output, exit code, status and error line.
pub(crate) fn finish(call: &mut ToolCall, text: &str, failed: bool) {
    let cleaned = clean(text);
    let mut text = cleaned.as_ref();
    if text == "(Bash completed with no output)" {
        text = "";
    }
    if let ToolDetail::Command { exit_code, .. } = &mut call.detail
        && failed
    {
        let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
        if let Some(code) = first.strip_prefix("Exit code ").and_then(|c| c.trim().parse().ok()) {
            *exit_code = Some(code);
            text = rest;
        } else if let Some(code) =
            text.lines().rev().find(|l| !l.trim().is_empty()).and_then(trailing_exit)
        {
            *exit_code = Some(code);
        }
    }
    set_output(&mut call.detail, text);
    call.status = if failed { ToolStatus::Failed } else { ToolStatus::Done };
    call.error = failed.then(|| error_line(text)).flatten();
}

/// Recasts a failure the user caused: declined in an approval, or cut by a cancel.
pub(crate) fn halted(call: &mut ToolCall, denied: bool, cancelled: bool) {
    if call.status == ToolStatus::Failed {
        if denied {
            call.status = ToolStatus::Denied;
        } else if cancelled {
            call.status = ToolStatus::Stopped;
        }
    }
}

/// Swaps an edit's synthesized diff for Claude's positioned hunks, when given.
pub(crate) fn apply_hunks(detail: &mut ToolDetail, hunks: &Value) {
    let Some(hunks) = hunks.as_array().filter(|h| !h.is_empty()) else { return };
    let ToolDetail::Edit { diff, added, removed, .. } = detail else { return };
    let mut d = DiffBuilder::default();
    for h in hunks {
        let n = |k: &str| h[k].as_u64().unwrap_or(0);
        d.raw(&format!(
            "@@ -{},{} +{},{} @@",
            n("oldStart"),
            n("oldLines"),
            n("newStart"),
            n("newLines")
        ));
        for line in h["lines"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            d.raw(line);
        }
    }
    (*diff, *added, *removed) = d.finish();
}

/// Swaps an edit's diff for ready-made patch text (OpenCode metadata, patch tools).
pub(crate) fn apply_patch_text(detail: &mut ToolDetail, text: &str) {
    let ToolDetail::Edit { path, diff, added, removed } = detail else { return };
    if !text.lines().any(|l| l.starts_with("@@")) {
        return;
    }
    if path.is_empty() {
        *path = patch_path(text).unwrap_or_default();
    }
    let mut d = DiffBuilder::default();
    text.lines().for_each(|l| d.raw(l));
    (*diff, *added, *removed) = d.finish();
}

/// Text parts of a tool result: a string, or blocks carrying `text`.
pub(crate) fn result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => {
            blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n")
        }
        _ => String::new(),
    }
}

/// Writes a tool result's image blocks, Claude's `source` shape or the flat
/// `data` + `mimeType` one (Pi, Codex MCP), into `dir`.
pub(crate) fn save_images(content: &Value, dir: Option<&Path>) -> Vec<Attachment> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let Some(dir) = dir else { return Vec::new() };
    let images = content.as_array().into_iter().flatten().filter(|b| b["type"] == "image");
    let mut saved = Vec::new();
    for block in images {
        let image = if block["source"].is_object() { &block["source"] } else { block };
        let mime = image["media_type"].as_str().or(image["mimeType"].as_str()).unwrap_or_default();
        let ext = match mime {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            _ => {
                log_unknown("tool", &format!("image {mime}"));
                continue;
            }
        };
        let Some(bytes) = image["data"].as_str().and_then(|d| STANDARD.decode(d).ok()) else {
            log_unknown("tool", "image without base64 data");
            continue;
        };
        let name = format!("tool-{}-{}.{ext}", now_ms(), SEQ.fetch_add(1, Ordering::Relaxed));
        let path = dir.join(&name);
        match std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&path, &bytes)) {
            Ok(()) => {
                let size = bytes.len() as u64;
                saved.push(Attachment { name, path, mime: mime.to_string(), size });
            }
            Err(e) => eprintln!("aether: couldn't save a tool image to {}: {e}", path.display()),
        }
    }
    saved
}

/// Shows a subagent's latest child call on its Task row; `new` counts the child.
pub(crate) fn child_activity(task: &mut ToolCall, child: &ToolCall, new: bool) {
    if let ToolDetail::Task { activity, tools, .. } = &mut task.detail {
        *activity = format!("{} {}", child.title, child.detail.summary());
        *tools += u32::from(new);
    }
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// Caps output at ~16k chars, keeping head and tail (errors sit at the end).
pub fn head_tail(text: &str) -> String {
    let chars = text.chars().count();
    if chars <= 2 * OUTPUT_HALF {
        return text.to_string();
    }
    let at = |n: usize| text.char_indices().nth(n).map_or(text.len(), |(i, _)| i);
    let mut head = at(OUTPUT_HALF);
    if let Some(nl) = text[..head].rfind('\n') {
        head = nl + 1;
    }
    let mut tail = at(chars - OUTPUT_HALF);
    if let Some(nl) = text[tail..].find('\n').filter(|nl| tail + nl + 1 < text.len()) {
        tail += nl + 1;
    }
    let omitted = text[head..tail].lines().count();
    let unit = if omitted == 1 { "line" } else { "lines" };
    let sep = if text[..head].ends_with('\n') { "" } else { "\n" };
    format!("{}{sep}… {omitted} {unit} omitted …\n{}", &text[..head], &text[tail..])
}

fn edit(input: &Value) -> ToolDetail {
    let text =
        |keys: &[&str], v: &Value| keys.iter().find_map(|k| v[*k].as_str()).map(str::to_string);
    let mut path =
        text(&["file_path", "filePath", "path", "notebook_path"], input).unwrap_or_default();
    let patch =
        text(&["patchText", "patch", "diff"], input).or_else(|| input.as_str().map(str::to_string));
    let mut d = DiffBuilder::default();
    if let Some(patch) = patch {
        if path.is_empty() {
            path = patch_path(&patch).unwrap_or_default();
        }
        patch.lines().for_each(|l| d.raw(l));
    } else if let Some(content) = input["content"].as_str().or(input["new_source"].as_str()) {
        if content.is_empty() {
            d.note("Empty file");
        } else if content.len() > SYNTH_BYTES {
            d.too_large(content.lines().count(), 0);
        } else {
            d.raw(&format!("@@ -0,0 +1,{} @@", content.lines().count()));
            content.lines().for_each(|l| d.line('+', l));
        }
    } else {
        let one = [input.clone()];
        let edits = input["edits"].as_array().map_or(&one[..], Vec::as_slice);
        for (i, e) in edits.iter().enumerate() {
            if i > 0 {
                d.raw("@@");
            }
            let old =
                text(&["old_string", "oldString", "oldText", "old_str"], e).unwrap_or_default();
            let new =
                text(&["new_string", "newString", "newText", "new_str"], e).unwrap_or_default();
            if old.len() > SYNTH_BYTES || new.len() > SYNTH_BYTES {
                d.too_large(new.lines().count(), old.lines().count());
            } else {
                d.replace(&old, &new);
            }
        }
    }
    let (diff, added, removed) = d.finish();
    ToolDetail::Edit { path, diff, added, removed }
}

#[derive(Default)]
struct DiffBuilder {
    out: String,
    lines: usize,
    added: u32,
    removed: u32,
    /// Lines the current positioned hunk still owes (old, new).
    left: (u32, u32),
    skipped: bool,
}

impl DiffBuilder {
    fn line(&mut self, sign: char, text: &str) {
        match sign {
            '+' => self.added += 1,
            '-' => self.removed += 1,
            _ => {}
        }
        self.push(sign.encode_utf8(&mut [0; 4]), text);
    }

    /// A patch line as is. `---`/`+++` are file headers only outside a hunk.
    fn raw(&mut self, line: &str) {
        if self.left == (0, 0) {
            if let Some((_, old, _, new)) = hunk_header(line) {
                self.left = (old, new);
            } else if line.starts_with('+') && !line.starts_with("+++ ") {
                self.added += 1;
            } else if line.starts_with('-') && !line.starts_with("--- ") {
                self.removed += 1;
            }
        } else {
            let (old, new) = &mut self.left;
            match line.as_bytes().first() {
                Some(b'+') => (self.added, *new) = (self.added + 1, new.saturating_sub(1)),
                Some(b'-') => (self.removed, *old) = (self.removed + 1, old.saturating_sub(1)),
                Some(b'\\') => {}
                _ => (*old, *new) = (old.saturating_sub(1), new.saturating_sub(1)),
            }
        }
        self.push("", line);
    }

    fn push(&mut self, sign: &str, text: &str) {
        if self.lines < DIFF_LINES {
            self.out.push_str(sign);
            match text.char_indices().nth(DIFF_LINE_CHARS) {
                Some((at, _)) => {
                    self.out.push_str(&text[..at]);
                    self.out.push('…');
                }
                None => self.out.push_str(text),
            }
            self.out.push('\n');
        }
        self.lines += 1;
    }

    fn note(&mut self, text: &str) {
        self.out.push_str(&format!("… {text}\n"));
    }

    fn too_large(&mut self, added: usize, removed: usize) {
        self.added += added as u32;
        self.removed += removed as u32;
        self.skipped = true;
    }

    /// Line diff of one string replacement: shared lines as context.
    fn replace(&mut self, old: &str, new: &str) {
        let (a, b): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
        let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
        let suf =
            a[pre..].iter().rev().zip(b[pre..].iter().rev()).take_while(|(x, y)| x == y).count();
        let (ma, mb) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);
        a[..pre].iter().for_each(|l| self.line(' ', l));
        if ma.len() * mb.len() > LCS_CELLS {
            ma.iter().for_each(|l| self.line('-', l));
            mb.iter().for_each(|l| self.line('+', l));
        } else {
            // lcs[i][j]: longest common run of ma[i..] and mb[j..]
            let w = mb.len() + 1;
            let mut lcs = vec![0u32; (ma.len() + 1) * w];
            for i in (0..ma.len()).rev() {
                for j in (0..mb.len()).rev() {
                    lcs[i * w + j] = if ma[i] == mb[j] {
                        lcs[(i + 1) * w + j + 1] + 1
                    } else {
                        lcs[(i + 1) * w + j].max(lcs[i * w + j + 1])
                    };
                }
            }
            let (mut i, mut j) = (0, 0);
            while i < ma.len() || j < mb.len() {
                if i < ma.len() && j < mb.len() && ma[i] == mb[j] {
                    self.line(' ', ma[i]);
                    (i, j) = (i + 1, j + 1);
                } else if j == mb.len()
                    || i < ma.len() && lcs[(i + 1) * w + j] >= lcs[i * w + j + 1]
                {
                    self.line('-', ma[i]);
                    i += 1;
                } else {
                    self.line('+', mb[j]);
                    j += 1;
                }
            }
        }
        a[a.len() - suf..].iter().for_each(|l| self.line(' ', l));
    }

    fn finish(mut self) -> (String, u32, u32) {
        if self.skipped {
            self.out = "… Diff too large to show\n".into();
        } else if self.lines > DIFF_LINES {
            let more = self.lines - DIFF_LINES;
            self.note(&format!("{more} more lines not shown"));
        }
        (self.out, self.added, self.removed)
    }
}

fn patch_path(patch: &str) -> Option<String> {
    patch.lines().find_map(|l| {
        let path = ["*** Update File: ", "*** Add File: ", "*** Delete File: ", "+++ b/", "+++ "]
            .iter()
            .find_map(|p| l.strip_prefix(p))?;
        Some(path.trim().to_string()).filter(|p| p != "/dev/null")
    })
}

fn range(offset: Option<u64>, limit: Option<u64>) -> Option<String> {
    match (offset, limit) {
        (Some(o), Some(l)) if l > 0 => Some(format!("lines {o}–{}", o + l - 1)),
        (Some(o), _) => Some(format!("from line {o}")),
        (None, Some(l)) => Some(format!("first {l} lines")),
        (None, None) => None,
    }
}

fn plan(input: &Value) -> Vec<PlanItem> {
    let items = input["todos"].as_array().or_else(|| input["plan"].as_array());
    items
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let text =
                ["content", "step", "text", "description"].iter().find_map(|k| t[*k].as_str())?;
            let status = match t["status"].as_str().unwrap_or_default() {
                "completed" | "done" => PlanStatus::Done,
                "in_progress" | "active" => PlanStatus::Active,
                "cancelled" | "canceled" => PlanStatus::Cancelled,
                _ => PlanStatus::Pending,
            };
            Some(PlanItem { text: text.to_string(), status })
        })
        .collect()
}

/// `mcp__server__create_thread` and `ToolSearch` read as "Create thread", "Tool search".
fn humanize(name: &str) -> String {
    let leaf = name.rsplit("__").next().unwrap_or(name);
    let mut words = String::new();
    let mut prev_lower = false;
    for c in leaf.chars() {
        if c == '_' || c == '-' || c == ' ' {
            words.push(' ');
        } else if c.is_uppercase() && prev_lower {
            words.push(' ');
            words.extend(c.to_lowercase());
        } else {
            words.extend(c.to_lowercase());
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
    }
    let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => name.to_string(),
    }
}

/// Text as a terminal would show it: ANSI CSI codes dropped, and of each
/// line only what follows the last `\r` overwrite.
pub(crate) fn clean(text: &str) -> Cow<'_, str> {
    if !text.contains(['\x1b', '\r']) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let line = line.strip_suffix('\r').unwrap_or(line);
        let mut chars = line.rsplit('\r').next().unwrap_or(line).chars();
        while let Some(c) = chars.next() {
            if c != '\x1b' {
                out.push(c);
            } else if chars.clone().next() == Some('[') {
                chars.find(|c| ('\x40'..='\x7e').contains(c) && *c != '[');
            }
        }
    }
    Cow::Owned(out)
}

pub fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_string(),
    }
}

fn match_count(output: &str) -> u32 {
    let mut lines = output.lines().filter(|l| !l.trim().is_empty());
    let Some(first) = lines.next() else { return 0 };
    if first.starts_with("No ") && first.contains("found") {
        return 0;
    }
    if let Some(n) =
        first.strip_prefix("Found ").and_then(|r| r.split_whitespace().next()?.parse().ok())
    {
        return n;
    }
    1 + lines.count() as u32
}

/// "Command exited with code 1" / "exit code: 1" on an output's last line.
fn trailing_exit(line: &str) -> Option<i32> {
    let lower = line.to_ascii_lowercase();
    let at = lower
        .rfind("exited with code")
        .map(|i| i + 16)
        .or_else(|| lower.rfind("exit code").map(|i| i + 9))?;
    lower[at..]
        .trim_start_matches([':', ' '])
        .split(|c: char| !c.is_ascii_digit() && c != '-')
        .next()?
        .parse()
        .ok()
}

fn error_line(text: &str) -> Option<String> {
    let line = text
        .lines()
        .map(|l| l.replace("<tool_use_error>", "").replace("</tool_use_error>", ""))
        .find(|l| !l.trim().is_empty())?;
    Some(clip(line.trim(), 200))
}

/// Rate-limits running snapshots to ~10/s per tool. A held snapshot goes out
/// before any later update, or from a timer if the stream goes quiet.
#[derive(Clone)]
pub(crate) struct Throttle {
    tx: UnboundedSender<Update>,
    held: Arc<Mutex<Held>>,
}

#[derive(Default)]
struct Held {
    sent: HashMap<String, Instant>,
    pending: Vec<ToolCall>,
    timer: bool,
}

const SNAPSHOT_EVERY: Duration = Duration::from_millis(100);

impl Throttle {
    pub(crate) fn new(tx: UnboundedSender<Update>) -> Self {
        Self { tx, held: Arc::default() }
    }

    pub(crate) fn send(&self, update: Update) {
        let mut h = self.held.guard();
        let update = match update {
            Update::Tool(call) if call.status == ToolStatus::Running => {
                let wait = h
                    .sent
                    .get(&call.id)
                    .map(|at| SNAPSHOT_EVERY.saturating_sub(at.elapsed()))
                    .filter(|w| !w.is_zero());
                let Some(wait) = wait else {
                    flush(&mut h, &self.tx, Some(&call.id));
                    record(&mut h, &call);
                    let _ = self.tx.unbounded_send(Update::Tool(call));
                    return;
                };
                h.pending.retain(|p| p.id != call.id);
                h.pending.push(call);
                if !std::mem::replace(&mut h.timer, true) {
                    let (held, tx) = (self.held.clone(), self.tx.clone());
                    std::thread::spawn(move || {
                        std::thread::sleep(wait);
                        let mut h = held.guard();
                        h.timer = false;
                        flush(&mut h, &tx, None);
                    });
                }
                return;
            }
            update => update,
        };
        let id = match &update {
            Update::Tool(call) => Some(call.id.clone()),
            _ => None,
        };
        flush(&mut h, &self.tx, id.as_deref());
        if let Update::Tool(call) = &update {
            record(&mut h, call);
        }
        let _ = self.tx.unbounded_send(update);
    }
}

/// Sends held snapshots, dropping the one `superseded` by a newer update.
fn flush(h: &mut Held, tx: &UnboundedSender<Update>, superseded: Option<&str>) {
    for call in std::mem::take(&mut h.pending) {
        if Some(call.id.as_str()) != superseded {
            record(h, &call);
            let _ = tx.unbounded_send(Update::Tool(call));
        }
    }
}

fn record(h: &mut Held, call: &ToolCall) {
    if call.status == ToolStatus::Running {
        h.sent.insert(call.id.clone(), Instant::now());
    } else {
        h.sent.remove(&call.id);
    }
}

#[cfg(test)]
#[path = "../tests/unit/tool.rs"]
mod tests;
