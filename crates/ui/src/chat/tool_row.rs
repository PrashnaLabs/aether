use crate::theme::Corners;
use std::{
    cell::Cell,
    collections::HashSet,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use aether_core::{PlanStatus, Step, ToolCall, ToolDetail, ToolStatus, hunk_header};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::SelectableText;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, StyledExt,
    button::ButtonVariants,
    clipboard::Clipboard,
    h_flex,
    highlighter::HighlightTheme,
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    scroll::{ScrollableMask, Scrollbar},
    spinner::Spinner,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, Axis, ClipboardItem, Div, FontFeatures, Global, HighlightStyle, Hsla, Pixels,
    Role, ScrollHandle, SharedString, Stateful, StyledText, Window, div, prelude::*, px, relative,
    rems,
};

use super::activity::{ActivityFacts, ActivityGroup, ActivityKind, activity_group_open};
use super::cache::RenderCache;
use super::changes::stats;
use super::diff_view::{DiffRow, DiffView, parse};
use super::highlight::highlight_with;
use super::links::{FileLink, open_file, parse_file_link};
use super::live::{clock, short_ms, since};
use crate::theme;

pub type Reflow = Rc<dyn Fn(&mut App)>;
type Styles = Vec<(Range<usize>, HighlightStyle)>;
pub type OnOpen = Rc<dyn Fn(&mut Window, &mut App)>;

/// Command output lines shown until "Show all".
pub const OUTPUT_LINES: usize = 12;

/// Rems from a row's edge to its title: the 15 px icon and the gap after it.
const INDENT: f32 = 0.9375 + 0.5;

/// Row expansion and parsed diffs, keyed by row key. Kept outside the
/// virtualized transcript so rows scrolled away come back as they were.
struct ToolRows {
    open: RenderCache<bool>,
    /// Rows that opened by themselves, kept apart from `open` so they don't
    /// count as opened by the user.
    auto: RenderCache<()>,
    /// Commands showing all their output, by `{row key}-all`.
    all: HashSet<SharedString>,
    diffs: RenderCache<(String, Rc<Vec<DiffRow>>)>,
    /// Open command rows' highlighted command, by row key.
    commands: RenderCache<(String, Arc<HighlightTheme>, Styles)>,
    /// Rows the user never toggled start open.
    verbose: bool,
}

impl Default for ToolRows {
    fn default() -> Self {
        Self {
            open: RenderCache::new(4096),
            auto: RenderCache::new(4096),
            all: HashSet::new(),
            diffs: RenderCache::new(256),
            commands: RenderCache::new(256),
            verbose: false,
        }
    }
}

impl Global for ToolRows {}

impl ToolRows {
    fn opened(&self, session: &str) -> HashSet<String> {
        let prefix = format!("{session}-");
        self.open
            .iter()
            .filter(|(_, open)| **open)
            .filter_map(|(key, _)| key.strip_prefix(prefix.as_str()).map(str::to_string))
            .collect()
    }

    fn show_all(&self, key: &SharedString) -> bool {
        self.all.contains(key)
    }

    fn toggle_all(&mut self, key: SharedString) {
        if !self.all.remove(&key) {
            self.all.insert(key);
        }
    }

    /// A command's highlights, parsed again only when its text or the theme changes.
    fn command_styles(
        &mut self,
        key: &SharedString,
        command: &str,
        theme: &Arc<HighlightTheme>,
        parse: impl FnOnce() -> Styles,
    ) -> Styles {
        if let Some((seen, at, styles)) = self.commands.get(key)
            && seen == command
            && Arc::ptr_eq(at, theme)
        {
            return styles.clone();
        }
        let styles = parse();
        self.commands.insert(key.clone(), (command.to_string(), theme.clone(), styles.clone()));
        styles
    }
}

pub fn tool_key(session: &str, tool_id: &str) -> SharedString {
    format!("{session}-{tool_id}").into()
}

pub fn open_tool(cx: &mut App, key: SharedString) {
    set_row_open(cx, key, true);
}

/// Ids of the tools whose rows the user opened in `session`.
pub fn opened_tools(cx: &mut App, session: &str) -> HashSet<String> {
    cx.default_global::<ToolRows>().opened(session)
}

pub fn set_verbose_rows(cx: &mut App, verbose: bool) {
    cx.default_global::<ToolRows>().verbose = verbose;
}

pub(crate) fn verbose_rows(cx: &App) -> bool {
    cx.try_global::<ToolRows>().is_some_and(|rows| rows.verbose)
}

/// Whether the user opened or closed the row at `key`; `None` until they do,
/// or `Some(true)` in verbose mode.
pub(crate) fn row_open(cx: &mut App, key: &SharedString) -> Option<bool> {
    let rows = cx.default_global::<ToolRows>();
    let verbose = rows.verbose;
    rows.open.get(key).copied().or(verbose.then_some(true))
}

pub(crate) fn peek_row_open(cx: &App, key: &SharedString) -> Option<bool> {
    cx.try_global::<ToolRows>()
        .and_then(|rows| rows.open.peek(key).copied().or(rows.verbose.then_some(true)))
}

pub(crate) fn set_row_open(cx: &mut App, key: SharedString, open: bool) {
    cx.default_global::<ToolRows>().open.insert(key, open);
}

/// One tool call in the work log: a single-line summary that expands, when
/// there is more to show, into its command output, diff or result.
#[derive(IntoElement)]
pub struct ToolRow {
    key: SharedString,
    call: ToolCall,
    cwd: Option<SharedString>,
    default_open: bool,
    nested: bool,
    on_reflow: Option<Reflow>,
    on_diff: Option<OnOpen>,
    on_child: Option<OnOpen>,
}

impl ToolRow {
    pub fn new(key: SharedString, call: ToolCall) -> Self {
        Self {
            key,
            call,
            cwd: None,
            default_open: false,
            nested: false,
            on_reflow: None,
            on_diff: None,
            on_child: None,
        }
    }

    /// "Open diff" in the row's menu, for the reply's diff in the side panel.
    pub fn on_diff(mut self, handler: Option<OnOpen>) -> Self {
        self.on_diff = handler;
        self
    }

    /// "Open agent" in the row's menu, for the chat a spawn call started.
    pub fn on_child(mut self, handler: Option<OnOpen>) -> Self {
        self.on_child = handler;
        self
    }

    /// Folder the paths are shown relative to.
    pub fn cwd(mut self, cwd: Option<SharedString>) -> Self {
        self.cwd = cwd;
        self
    }

    /// Open until the user toggles it, e.g. a running call.
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn on_reflow(mut self, reflow: Option<Reflow>) -> Self {
        self.on_reflow = reflow;
        self
    }

    /// A row a subagent ran, under its card.
    pub fn nested(mut self, nested: bool) -> Self {
        self.nested = nested;
        self
    }
}

/// Whether a call has anything to show beyond its row.
pub(crate) fn has_body(call: &ToolCall) -> bool {
    match &call.detail {
        ToolDetail::Command { command, .. } => !command.is_empty(),
        ToolDetail::Edit { diff, .. } => !diff.is_empty(),
        ToolDetail::Search { output, .. }
        | ToolDetail::Web { output, .. }
        | ToolDetail::Task { output, .. } => !output.trim().is_empty(),
        ToolDetail::Plan { items } => !items.is_empty(),
        ToolDetail::Other { input, output, .. } => !input.is_empty() || !output.trim().is_empty(),
        ToolDetail::Read { .. } | ToolDetail::Plain(_) => false,
    }
}

fn is_live(status: ToolStatus) -> bool {
    matches!(status, ToolStatus::Running | ToolStatus::Waiting | ToolStatus::Background)
}

/// The last `lines` lines of `output`, and how many lines come before them.
pub fn tail(output: &str, lines: usize) -> (&str, usize) {
    let hidden = output.lines().count().saturating_sub(lines);
    let start = match hidden {
        0 => 0,
        n => output.match_indices('\n').nth(n - 1).map_or(0, |(at, _)| at + 1),
    };
    (&output[start..], hidden)
}

/// The status a row spells out: a non-zero exit code, or any state but
/// running and done. A stop or a denial outranks the exit code it caused.
pub fn status_word(call: &ToolCall) -> Option<String> {
    let word = match (call.status, &call.detail) {
        (ToolStatus::Waiting, _) => "Needs approval",
        (ToolStatus::Background, _) => "Background",
        (ToolStatus::Stopped, _) => "Stopped",
        (ToolStatus::Denied, _) => "Denied",
        (_, ToolDetail::Command { exit_code: Some(code), .. }) if *code != 0 => {
            return Some(format!("Exit {code}"));
        }
        (ToolStatus::Failed, _) => "Failed",
        _ => return None,
    };
    Some(word.into())
}

/// The colour a row's status takes.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Tone {
    Plain,
    Danger,
    Warning,
    Info,
}

/// Only failed, waiting and background rows take colour.
pub fn tone(status: ToolStatus) -> Tone {
    match status {
        ToolStatus::Failed => Tone::Danger,
        ToolStatus::Waiting => Tone::Warning,
        ToolStatus::Background => Tone::Info,
        _ => Tone::Plain,
    }
}

/// A live row's clock, or a finished row's time; sub-second times show only
/// for failures.
pub fn timer_text(call: &ToolCall, now_ms: i64) -> Option<String> {
    match call.status {
        status if is_live(status) => since(call.started_ms, now_ms),
        ToolStatus::Failed => call.ms.map(short_ms),
        _ => call.ms.filter(|ms| *ms >= 1000).map(clock),
    }
}

/// Opens by itself: a running row with output to follow. A waiting row stays shut; its
/// approval card below shows what waits.
fn auto_open(call: &ToolCall) -> bool {
    match (call.status, &call.detail) {
        (ToolStatus::Running, ToolDetail::Edit { diff, .. }) => !diff.is_empty(),
        (
            ToolStatus::Running,
            ToolDetail::Command { output, .. }
            | ToolDetail::Search { output, .. }
            | ToolDetail::Web { output, .. }
            | ToolDetail::Task { output, .. }
            | ToolDetail::Other { output, .. },
        ) => !output.trim().is_empty(),
        _ => false,
    }
}

/// "Read N files", and the files' names.
pub fn read_group_label(calls: &[ToolCall]) -> (String, String) {
    let names: Vec<&str> = calls
        .iter()
        .filter_map(|call| match &call.detail {
            ToolDetail::Read { path, .. } => Path::new(path).file_name()?.to_str(),
            _ => None,
        })
        .collect();
    (format!("Read {} files", calls.len()), names.join(", "))
}

/// The diff creates its file: its first hunk starts from `-0,0`.
pub fn is_new_file(diff: &str) -> bool {
    diff.lines().find(|l| l.starts_with("@@")).is_some_and(|h| h.starts_with("@@ -0,0"))
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// `path` made absolute against `cwd`; `None` when it is relative and there is no `cwd`.
pub(crate) fn abs_path(path: &str, cwd: Option<&str>) -> Option<PathBuf> {
    let path = Path::new(path);
    match path.is_absolute() {
        true => Some(path.to_path_buf()),
        false => cwd.map(|cwd| Path::new(cwd).join(path)),
    }
}

pub(crate) fn refresh(reflow: &Option<Reflow>, window: &mut Window, cx: &mut App) {
    match reflow {
        Some(reflow) => reflow(cx),
        None => window.refresh(),
    }
}

impl RenderOnce for ToolRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (danger, muted) = (theme::danger_text(cx), cx.theme().muted_foreground);
        let mono = cx.theme().mono_font_family.clone();
        let expandable = has_body(&self.call);
        let key = self.key.clone();
        let mut call = self.call;
        let open = expandable && {
            let rows = cx.default_global::<ToolRows>();
            match rows.open.get(&key).copied() {
                Some(open) => open,
                None => match rows.auto.get(&key) {
                    Some(()) => true,
                    None if auto_open(&call) => {
                        rows.auto.insert(key.clone(), ());
                        true
                    }
                    None => self.default_open || rows.verbose,
                },
            }
        };
        let title = if !call.title.is_empty() {
            call.title.clone()
        } else {
            match &call.detail {
                ToolDetail::Other { name, output, .. } if is_question_tool_detail(name, output) => {
                    "Ask question".to_string()
                }
                ToolDetail::Other { name, .. } if !name.is_empty() => humanize_tool_name(name),
                ToolDetail::Plain(t) if !t.is_empty() => t.clone(),
                _ => "Tool".to_string(),
            }
        };
        let (subject, path_like) = subject(&call.detail, &title, self.cwd.as_deref());
        let state = status_word(&call)
            .unwrap_or_else(|| if is_live(call.status) { "running" } else { "done" }.into());
        let meta = meta(&call, timer_text(&call, now_ms()), cx);
        let failed = call.status == ToolStatus::Failed;
        let error = match call.status {
            ToolStatus::Failed | ToolStatus::Denied => call.error.as_deref(),
            _ => None,
        }
        .and_then(|e| e.lines().map(str::trim).find(|l| !l.is_empty()))
        .map(|e| {
            div()
                .pl(rems(INDENT))
                .text_xs()
                .truncate()
                .when(failed, |d| d.font_family(mono))
                .text_color(if failed { danger } else { muted })
                .child(e.to_string())
        });
        // full path or command, only when the row may not show all of it
        let tip: SharedString = match &call.detail {
            ToolDetail::Edit { path, .. } | ToolDetail::Read { path, .. } => path.clone().into(),
            detail => detail.summary().into(),
        };
        let tip = (tip != subject || tip.len() > 40).then_some(tip).filter(|t| !t.is_empty());
        let line = row_line(
            kind_icon(&call.detail),
            tone(call.status),
            title.clone().into(),
            (subject.clone(), path_like),
            meta,
            expandable.then_some(open),
            cx,
        );
        // a closed row's body never reads the detail, so the menu takes it instead of a copy
        let detail = match open {
            true => call.detail.clone(),
            false => std::mem::replace(&mut call.detail, ToolDetail::Plain(String::new())),
        };
        let file = file_at(&detail);
        let (copies, menu_diff) = (copies(detail), self.on_diff.clone());
        let menu_child = self.on_child.clone();
        let menu_cwd = self.cwd.as_deref().map(PathBuf::from);
        let row = row_box(self.nested)
            .id(key.clone())
            .aria_label(SharedString::from(format!("{title} {subject}, {state}")))
            .child(line)
            .children(error)
            .when_some(tip, |row, tip| {
                row.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            })
            .when(expandable, |row| toggle(row, key.clone(), open, self.on_reflow.clone(), cx));
        let row = match copies.is_empty() && menu_diff.is_none() && menu_child.is_none() {
            true => row.into_any_element(),
            false => row
                .context_menu(move |menu, _, _| {
                    let file = file.as_ref().and_then(|at| link_to(at, menu_cwd.as_deref()));
                    tool_menu(menu, &copies, menu_diff.clone(), menu_child.clone(), file)
                })
                .into_any_element(),
        };
        v_flex().w_full().child(row).when(open, |col| {
            // same box model as the row, so the body lines up under the title
            col.child(
                div()
                    .px_2()
                    .pt_1()
                    .pb_2()
                    .border_1()
                    .border_color(gpui_kit::transparent_black())
                    .child(div().pl(rems(INDENT)).child(body(
                        &self.key,
                        call,
                        self.cwd.as_deref(),
                        self.nested,
                        self.on_reflow.clone(),
                        window,
                        cx,
                    ))),
            )
        })
    }
}

/// A row's box: 28 px tall, 24 when nested, with the transparent border the
/// focus ring draws on.
fn row_box(nested: bool) -> Div {
    v_flex()
        .w_full()
        .min_h(rems(if nested { 1.5 } else { 1.75 }))
        .justify_center()
        .px_2()
        .corner(rems(0.375))
        .border_1()
        .border_color(gpui_kit::transparent_black())
}

/// Kind icon, title and target, then right-aligned meta and, for rows that
/// expand, a chevron.
fn row_line(
    icon: Icon,
    tone: Tone,
    title: SharedString,
    (target, path_like): (SharedString, bool),
    meta: Vec<AnyElement>,
    open: Option<bool>,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let (fg, muted, mono) =
        (theme.foreground, theme.muted_foreground, theme.mono_font_family.clone());
    let tint = tone_colors(tone, cx).map_or(muted, |(text, _)| text);
    h_flex()
        .w_full()
        .gap_2()
        .child(icon.size(rems(0.9375)).flex_none().text_color(tint))
        .child(
            div()
                .min_w_0()
                .when(!target.is_empty(), |d| d.flex_shrink_0().max_w(relative(0.5)))
                .truncate()
                .text_size(rems(0.8125))
                .font_medium()
                .text_color(fg)
                .child(title),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .font_family(mono)
                .text_size(rems(0.78125))
                .text_color(muted)
                .overflow_hidden()
                .whitespace_nowrap()
                .map(|d| if path_like { d.text_ellipsis_middle() } else { d.text_ellipsis() })
                .child(target),
        )
        .child(h_flex().flex_none().gap_2().text_xs().text_color(muted).map(tabular).children(meta))
        .child(div().flex_none().size(rems(0.75)).children(open.map(|open| {
            Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight })
                .xsmall()
                .text_color(muted)
        })))
}

/// Text and wash colours of a tone; `None` for plain.
fn tone_colors(tone: Tone, cx: &App) -> Option<(Hsla, Hsla)> {
    match tone {
        Tone::Plain => None,
        Tone::Danger => Some((theme::danger_text(cx), cx.theme().danger.opacity(0.12))),
        Tone::Warning => Some((theme::mango_text(cx), theme::mango().opacity(0.12))),
        Tone::Info => Some((theme::info_text(cx), cx.theme().info.opacity(0.12))),
    }
}

fn pill(icon: Option<AnyElement>, word: String, tone: Tone, cx: &App) -> AnyElement {
    let plain = (cx.theme().muted_foreground, cx.theme().border);
    let (text, wash) = tone_colors(tone, cx).unwrap_or(plain);
    h_flex()
        .gap_1()
        .px_1()
        .corner(rems(0.375))
        .bg(wash)
        .text_color(text)
        .children(icon)
        .child(word)
        .into_any_element()
}

/// Makes `row` a button that opens and closes it.
fn toggle(
    row: Stateful<Div>,
    key: SharedString,
    open: bool,
    reflow: Option<Reflow>,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (hover, pressed, ring) = (theme.secondary, theme.secondary_active, theme.ring);
    row.role(Role::Button)
        .aria_expanded(open)
        .tab_index(0)
        .cursor_pointer()
        .hover(|s| s.bg(hover))
        .active(|s| s.bg(pressed))
        .focus_visible(|s| s.border_color(ring))
        .on_click(move |_, window, cx| {
            set_row_open(cx, key.clone(), !open);
            refresh(&reflow, window, cx);
        })
}

/// Consecutive finished reads.
#[derive(IntoElement)]
pub struct ReadGroup {
    key: SharedString,
    calls: Vec<ToolCall>,
    cwd: Option<SharedString>,
    on_reflow: Option<Reflow>,
}

impl ReadGroup {
    pub fn new(key: impl Into<SharedString>, calls: Vec<ToolCall>) -> Self {
        Self { key: key.into(), calls, cwd: None, on_reflow: None }
    }

    pub fn cwd(mut self, cwd: Option<SharedString>) -> Self {
        self.cwd = cwd;
        self
    }

    pub fn on_reflow(mut self, reflow: Option<Reflow>) -> Self {
        self.on_reflow = reflow;
        self
    }
}

impl RenderOnce for ReadGroup {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (_, names) = read_group_label(&self.calls);
        let source: Vec<_> = self.calls.iter().cloned().map(Step::Tool).collect();
        let mut facts =
            ActivityFacts::new(ActivityKind::Reads, &source.iter().collect::<Vec<_>>(), false);
        facts.preview = names;
        let open = activity_group_open(cx, &self.key);
        let children = self
            .calls
            .into_iter()
            .filter(|_| open)
            .map(|call| {
                ToolRow::new(format!("{}-{}", self.key, call.id).into(), call)
                    .cwd(self.cwd.clone())
                    .nested(true)
                    .on_reflow(self.on_reflow.clone())
                    .into_any_element()
            })
            .collect();
        ActivityGroup::new(self.key, facts, children).on_reflow(self.on_reflow)
    }
}

/// A web search the model ran, and how it went.
#[derive(IntoElement)]
pub struct SearchRow {
    query: String,
    results: Option<Result<usize, String>>,
}

impl SearchRow {
    pub fn new(query: String, results: Option<Result<usize, String>>) -> Self {
        Self { query, results }
    }
}

impl RenderOnce for SearchRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let (verb, outcome, tone) = match self.results {
            None => (
                "Searching",
                Spinner::new()
                    .icon(IconName::LoaderCircle)
                    .xsmall()
                    .color(muted)
                    .into_any_element(),
                Tone::Plain,
            ),
            Some(Ok(n)) => (
                "Searched",
                div().child(count(n, "result", "results")).into_any_element(),
                Tone::Plain,
            ),
            Some(Err(error)) => {
                ("Search failed", div().child(error).into_any_element(), Tone::Danger)
            }
        };
        let target = (SharedString::from(self.query), false);
        row_box(false).child(row_line(
            Icon::new(Lucide::Search),
            tone,
            verb.into(),
            target,
            vec![outcome],
            None,
            cx,
        ))
    }
}

/// A skill the model loaded.
#[derive(IntoElement)]
pub struct SkillRow {
    name: String,
}

impl SkillRow {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

impl RenderOnce for SkillRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let target = (SharedString::from(self.name), false);
        row_box(false).child(row_line(
            Icon::new(Lucide::BookOpen),
            Tone::Plain,
            "Used skill".into(),
            target,
            Vec::new(),
            None,
            cx,
        ))
    }
}

/// What a row's menu can copy, as label and text; empty texts are left out.
fn copies(detail: ToolDetail) -> Vec<(&'static str, String)> {
    let all = match detail {
        ToolDetail::Command { command, output, .. } => {
            vec![("Copy command", command), ("Copy output", output)]
        }
        ToolDetail::Edit { path, .. } | ToolDetail::Read { path, .. } => vec![("Copy path", path)],
        ToolDetail::Search { path, output, .. } => {
            vec![("Copy path", path.unwrap_or_default()), ("Copy results", output)]
        }
        ToolDetail::Web { output, .. } | ToolDetail::Task { output, .. } => {
            vec![("Copy result", output)]
        }
        ToolDetail::Other { input, output, .. } => {
            vec![("Copy input", input), ("Copy output", output)]
        }
        ToolDetail::Plan { .. } | ToolDetail::Plain(_) => Vec::new(),
    };
    all.into_iter().filter(|(_, text)| !text.trim().is_empty()).collect()
}

/// The path a Read or Edit row names, and its 1-indexed first line when known.
fn file_at(detail: &ToolDetail) -> Option<(String, Option<usize>)> {
    let (path, line) = match detail {
        // the driver's `range()` text: `lines 10–49`, `from line 10` or `first 40 lines`
        ToolDetail::Read { path, range } => {
            let start = range
                .as_deref()
                .and_then(|r| r.strip_prefix("lines ").or_else(|| r.strip_prefix("from line ")));
            (path, start.and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()))
        }
        ToolDetail::Edit { path, diff, .. } => {
            (path, diff.lines().find_map(hunk_header).map(|(_, _, new, _)| new as usize))
        }
        _ => return None,
    };
    (!path.is_empty()).then(|| (path.clone(), line.filter(|&n| n > 0)))
}

/// A file from [`file_at`] as a link to open, when it is there.
fn link_to((path, line): &(String, Option<usize>), cwd: Option<&Path>) -> Option<FileLink> {
    let link = parse_file_link(path, cwd)?;
    Some(FileLink { line: line.or(link.line), ..link })
}

fn tool_menu(
    mut menu: PopupMenu,
    copies: &[(&'static str, String)],
    on_diff: Option<OnOpen>,
    on_child: Option<OnOpen>,
    file: Option<FileLink>,
) -> PopupMenu {
    let opens = on_diff.is_some() || on_child.is_some() || file.is_some();
    if let Some(on_child) = on_child {
        menu = menu.item(
            PopupMenuItem::new("Open agent")
                .icon(Lucide::Bot)
                .on_click(move |_, window, cx| on_child(window, cx)),
        );
    }
    if let Some(on_diff) = on_diff {
        menu = menu.item(
            PopupMenuItem::new("Open diff")
                .icon(Lucide::FileDiff)
                .on_click(move |_, window, cx| on_diff(window, cx)),
        );
    }
    if let Some(file) = file {
        menu = menu.item(
            PopupMenuItem::new("Open in Explorer")
                .icon(Lucide::FileOutput)
                .on_click(move |_, window, cx| open_file(file.clone(), window, cx)),
        );
    }
    if opens && !copies.is_empty() {
        menu = menu.separator();
    }
    for (label, text) in copies {
        let text = text.clone();
        menu =
            menu.item(PopupMenuItem::new(*label).icon(IconName::Copy).on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
            }));
    }
    menu
}

pub(crate) fn is_question_tool_detail(name: &str, output: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("question")
        || output.contains("User has answered your question")
        || output.contains("User has answered your questions")
}

pub(crate) fn extract_question_from_output(output: &str) -> Option<String> {
    let lower = output.to_ascii_lowercase();
    let marker = "user has answered your question";
    let at = lower.find(marker)?;
    let after = &output[at + marker.len()..];
    let rest = after.trim_start_matches(['s', ':', ' ']);
    if let Some(inner) = rest.strip_prefix('"')
        && let Some(end) = inner.find('"')
    {
        let q = inner[..end].trim();
        if !q.is_empty() {
            return Some(q.to_string());
        }
    }
    None
}

pub(crate) fn humanize_tool_name(name: &str) -> String {
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

fn kind_icon(detail: &ToolDetail) -> Icon {
    Icon::new(match detail {
        ToolDetail::Command { .. } => Lucide::SquareTerminal,
        ToolDetail::Edit { diff, .. } if is_new_file(diff) => Lucide::FilePlus,
        ToolDetail::Edit { .. } => Lucide::FilePen,
        ToolDetail::Read { .. } => Lucide::FileText,
        ToolDetail::Search { .. } => Lucide::Search,
        ToolDetail::Web { .. } => Lucide::Globe,
        ToolDetail::Task { .. } => Lucide::Bot,
        ToolDetail::Plan { .. } => Lucide::ListChecks,
        ToolDetail::Other { name, output, .. } if is_question_tool_detail(name, output) => {
            Lucide::CircleQuestionMark
        }
        ToolDetail::Other { name, .. } if name.starts_with("mcp__") => Lucide::Plug,
        ToolDetail::Other { .. } | ToolDetail::Plain(_) => Lucide::Wrench,
    })
}

/// The muted text after the title, and whether it is a path (middle-truncated).
fn subject(detail: &ToolDetail, title: &str, cwd: Option<&str>) -> (SharedString, bool) {
    let (text, path) = match detail {
        ToolDetail::Edit { path, .. } | ToolDetail::Read { path, .. } => {
            (relative_path(path, cwd), true)
        }
        ToolDetail::Search { pattern, path: Some(path), .. } if !pattern.is_empty() => {
            (format!("{pattern} in {}", relative_path(path, cwd)), false)
        }
        ToolDetail::Search { pattern, path: Some(path), .. } if pattern.is_empty() => {
            (relative_path(path, cwd), true)
        }
        ToolDetail::Other { name, input, output } => {
            if is_question_tool_detail(name, output)
                || title.to_ascii_lowercase().contains("question")
            {
                if !input.is_empty() && !input.starts_with('{') {
                    (input.lines().next().unwrap_or_default().trim().to_string(), false)
                } else if let Some(q) = extract_question_from_output(output) {
                    (q, false)
                } else {
                    (String::new(), false)
                }
            } else if !input.is_empty() && !input.starts_with('{') && !input.starts_with('[') {
                (input.lines().next().unwrap_or_default().trim().to_string(), false)
            } else {
                (String::new(), false)
            }
        }
        detail => (detail.summary(), false),
    };
    let text = if text == title { String::new() } else { text };
    (text.into(), path)
}

pub fn relative_path(path: &str, cwd: Option<&str>) -> String {
    let Some(cwd) = cwd.map(|c| c.trim_end_matches('/')).filter(|c| !c.is_empty()) else {
        return path.to_string();
    };
    // macOS reports /tmp paths through their /private target
    let path = path
        .strip_prefix("/private")
        .filter(|p| !cwd.starts_with("/private") && p.starts_with(cwd))
        .unwrap_or(path);
    match path.strip_prefix(cwd).and_then(|p| p.strip_prefix('/')) {
        Some(rel) if !rel.is_empty() => rel.to_string(),
        _ => path.to_string(),
    }
}

/// The icon in a row's status pill; Background shows a dot instead.
fn pill_icon(status: ToolStatus) -> Option<Lucide> {
    Some(match status {
        ToolStatus::Waiting => Lucide::Shield,
        ToolStatus::Denied => Lucide::ShieldX,
        ToolStatus::Stopped => Lucide::Square,
        ToolStatus::Failed => Lucide::CircleX,
        _ => return None,
    })
}

/// Live rows: a spinner or status pill, then the clock. Finished rows: line
/// totals, matches and the like, the status pill, then the time.
fn meta(call: &ToolCall, timer: Option<String>, cx: &App) -> Vec<AnyElement> {
    let (muted, info) = (cx.theme().muted_foreground, theme::info_text(cx));
    let text = |s: String| div().child(s).into_any_element();
    let mut out = Vec::new();
    match (call.status, &call.detail) {
        (ToolStatus::Running, _) => out.push(
            Spinner::new().icon(IconName::LoaderCircle).xsmall().color(muted).into_any_element(),
        ),
        (ToolStatus::Waiting | ToolStatus::Background, _) => {}
        (_, ToolDetail::Edit { added, removed, .. }) => {
            out.push(stats(*added, *removed, cx).into_any_element())
        }
        (_, ToolDetail::Search { matches: Some(n), .. }) => {
            out.push(text(count(*n as usize, "match", "matches")))
        }
        (_, ToolDetail::Read { range: Some(range), .. }) => out.push(text(range.clone())),
        (_, ToolDetail::Plan { items }) if !items.is_empty() => {
            let done = items.iter().filter(|i| i.status == PlanStatus::Done).count();
            out.push(text(format!("{done} of {} done", items.len())));
        }
        _ => {}
    }
    let icon = match call.status {
        ToolStatus::Background => {
            Some(div().flex_none().size(rems(0.375)).rounded_full().bg(info).into_any_element())
        }
        status => pill_icon(status).map(|icon| Icon::new(icon).xsmall().into_any_element()),
    };
    out.extend(status_word(call).map(|word| pill(icon, word, tone(call.status), cx)));
    out.extend(timer.map(text));
    out
}

fn body(
    key: &SharedString,
    call: ToolCall,
    cwd: Option<&str>,
    nested: bool,
    reflow: Option<Reflow>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    if matches!(call.detail, ToolDetail::Command { .. }) {
        return term(key, call, nested, reflow, window, cx);
    }
    let id = |suffix: &str| SharedString::from(format!("{key}-{suffix}"));
    let at = file_at(&call.detail);
    match call.detail {
        ToolDetail::Edit { path, diff, added, removed } => {
            let rows = cached_rows(cx, key, &diff, || parse(&diff));
            let link_cwd = cwd.map(PathBuf::from);
            let on_open = at.map(|at| {
                Rc::new(move |window: &mut Window, cx: &mut App| {
                    if let Some(link) = link_to(&at, link_cwd.as_deref()) {
                        open_file(link, window, cx);
                    }
                }) as OnOpen
            });
            DiffView::new(id("diff"), relative_path(&path, cwd).into(), rows, diff.into())
                .file(abs_path(&path, cwd))
                .stats(added, removed)
                .on_open(on_open)
                .bare(nested)
                .into_any_element()
        }
        ToolDetail::Search { output, .. } => output_box(
            id("out"),
            "Results",
            output.into(),
            "No results",
            "Copy results",
            nested,
            window,
            cx,
        ),
        ToolDetail::Web { output, .. } | ToolDetail::Task { output, .. } => output_box(
            id("out"),
            "Result",
            output.into(),
            "No result",
            "Copy result",
            nested,
            window,
            cx,
        ),
        ToolDetail::Other { input, output, .. } => v_flex()
            .gap_2()
            .when(!input.is_empty(), |c| {
                c.child(output_box(
                    id("in"),
                    "Input",
                    input.into(),
                    "",
                    "Copy input",
                    nested,
                    window,
                    cx,
                ))
            })
            .when(!output.trim().is_empty(), |c| {
                c.child(output_box(
                    id("out"),
                    "Output",
                    output.into(),
                    "",
                    "Copy output",
                    nested,
                    window,
                    cx,
                ))
            })
            .into_any_element(),
        ToolDetail::Plan { items } => {
            let success = theme::success_text(cx);
            let theme = cx.theme();
            let (fg, muted) = (theme.foreground, theme.muted_foreground);
            v_flex()
                .gap_1()
                .children(items.into_iter().enumerate().map(|(i, item)| {
                    let (icon, color, label) = match item.status {
                        PlanStatus::Done => (Icon::new(IconName::CircleCheck), success, "done"),
                        PlanStatus::Active => (Icon::new(Lucide::CircleDot), fg, "in progress"),
                        PlanStatus::Pending | PlanStatus::Unknown => {
                            (Icon::new(Lucide::Circle), muted, "to do")
                        }
                        PlanStatus::Cancelled => {
                            (Icon::new(Lucide::CircleSlash), muted, "cancelled")
                        }
                    };
                    let text = if item.status == PlanStatus::Active { fg } else { muted };
                    h_flex()
                        .id(id(&format!("plan-{i}")))
                        .gap_2()
                        .items_start()
                        .aria_label(SharedString::from(format!("{}, {label}", item.text)))
                        .child(
                            div().flex_none().pt(rems(0.2)).child(icon.xsmall().text_color(color)),
                        )
                        .child(div().min_w_0().text_color(text).child(item.text))
                }))
                .into_any_element()
        }
        ToolDetail::Command { .. } | ToolDetail::Read { .. } | ToolDetail::Plain(_) => {
            div().into_any_element()
        }
    }
}

/// A command's terminal: `$ command` and its folder, the output's last lines
/// or all of them, then the exit code, time, line count and Copy.
fn term(
    key: &SharedString,
    call: ToolCall,
    nested: bool,
    reflow: Option<Reflow>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let (status, ms) = (call.status, call.ms);
    let ToolDetail::Command { command, cwd, output, exit_code } = call.detail else {
        return div().into_any_element();
    };
    let id = |suffix: &str| SharedString::from(format!("{key}-{suffix}"));
    let all_key = id("all");
    let all = cx.default_global::<ToolRows>().show_all(&all_key);
    let danger = theme::danger_text(cx);
    let command = command.trim_end().to_string();
    let highlight = cx.theme().highlight_theme.clone();
    let styles = cx
        .default_global::<ToolRows>()
        .command_styles(key, &command, &highlight, || highlight_with(&command, "bash", &highlight));
    let command = StyledText::new(command).with_highlights(styles);
    let theme = cx.theme();
    let (fg, muted, border, bg) =
        (theme.foreground, theme.muted_foreground, theme.border, theme.secondary.opacity(0.5));
    let mono = theme.mono_font_family.clone();
    let output = SharedString::from(output);
    let lines = output.lines().count();
    let (last, hidden) = tail(&output, OUTPUT_LINES);
    let text = if all || hidden == 0 { output.clone() } else { last.to_string().into() };

    let shown = h_flex()
        .items_start()
        .gap_2()
        .child(div().flex_none().text_color(muted).child("$"))
        .child(div().flex_1().min_w_0().text_color(fg).child(command));
    let header = h_flex()
        .items_start()
        .gap_2()
        .px_2()
        .py_1()
        .font_family(mono.clone())
        .child(
            div()
                .flex_none()
                .h(rems(1.25))
                .flex()
                .items_center()
                .child(Icon::new(IconName::SquareTerminal).xsmall().text_color(muted)),
        )
        .child(div().flex_1().min_w_0().child(scroll_box(
            id("head"),
            Some(px(96.)),
            None,
            true,
            shown.into_any_element(),
            window,
            cx,
        )))
        .children(cwd.map(|cwd| {
            div()
                .flex_none()
                .max_w(relative(0.4))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_middle()
                .text_color(muted)
                .child(cwd)
        }));

    let content = if output.trim().is_empty() {
        let empty = match status {
            ToolStatus::Waiting => "Not run yet",
            ToolStatus::Running | ToolStatus::Background => "Waiting for output…",
            _ => "No output",
        };
        div().px_2().py_1().text_color(muted).child(empty).into_any_element()
    } else {
        let len = text.len();
        let shown = div()
            .px_2()
            .py_1()
            .font_family(mono)
            .text_color(fg)
            .whitespace_nowrap()
            .child(SelectableText::new(id("text"), text));
        scroll_box(
            id("out"),
            all.then_some(px(420.)),
            Some(len),
            false,
            shown.into_any_element(),
            window,
            cx,
        )
    };

    let exit = exit_code.filter(|code| *code != 0);
    let took = ms.filter(|_| !is_live(status)).map(short_ms);
    let show_all = (hidden > 0).then(|| {
        let label = match all {
            true => format!("Show last {OUTPUT_LINES} lines"),
            false => format!("Show all {lines} lines"),
        };
        crate::components::button(id("show-all")).ghost().xsmall().label(label).on_click(
            move |_, window, cx| {
                cx.default_global::<ToolRows>().toggle_all(all_key.clone());
                refresh(&reflow, window, cx);
            },
        )
    });
    let footer = (exit.is_some() || took.is_some() || lines > 0).then(|| {
        h_flex()
            .items_center()
            .gap_2()
            .pl_2()
            .pr_1()
            .min_h(rems(1.75))
            .border_t_1()
            .border_color(border)
            .text_color(muted)
            .map(tabular)
            .children(exit.map(|code| div().text_color(danger).child(format!("exit {code}"))))
            .children(took.map(|took| div().child(took)))
            .child(div().flex_1())
            .when(lines > 0 && (all || hidden == 0), |f| {
                f.child(div().child(count(lines, "line", "lines")))
            })
            .children(show_all)
            .when(lines > 0, |f| {
                f.child(
                    Clipboard::new(id("copy"))
                        .value(output.clone())
                        .tooltip("Copy output")
                        .accessibility_label("Copy output")
                        .xsmall(),
                )
            })
    });
    v_flex()
        .w_full()
        .corner(px(9.))
        .when(!nested, |d| d.border_1().border_color(border))
        .bg(bg)
        .overflow_hidden()
        .text_xs()
        .line_height(rems(1.25))
        .child(header)
        .child(div().border_t_1().border_color(border).child(content))
        .children(footer)
        .into_any_element()
}

/// A labelled box of mono, selectable text with a copy button, capped in
/// height with its own scroll.
#[allow(clippy::too_many_arguments)]
fn output_box(
    id: SharedString,
    label: &'static str,
    text: SharedString,
    empty: &'static str,
    copy_tip: &'static str,
    nested: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme();
    let (fg, muted, border, bg) =
        (theme.foreground, theme.muted_foreground, theme.border, theme.secondary.opacity(0.5));
    let mono = theme.mono_font_family.clone();
    let has_text = !text.trim().is_empty();
    let copy = has_text.then(|| {
        Clipboard::new(SharedString::from(format!("{id}-copy")))
            .value(text.clone())
            .tooltip(copy_tip)
            .accessibility_label(copy_tip)
            .xsmall()
    });
    let bar = h_flex()
        .items_center()
        .gap_2()
        .pl_2()
        .pr_1()
        .min_h(rems(1.75))
        .child(div().flex_1().min_w_0().text_color(muted).child(label))
        .children(copy);
    let content: Option<AnyElement> = if has_text {
        let body = div()
            .px_2()
            .py_1()
            .font_family(mono)
            .text_color(fg)
            .whitespace_nowrap()
            .child(SelectableText::new(SharedString::from(format!("{id}-text")), text));
        Some(scroll_box(id, Some(px(240.)), None, false, body.into_any_element(), window, cx))
    } else {
        (!empty.is_empty())
            .then(|| div().px_2().py_1().text_color(muted).child(empty).into_any_element())
    };
    v_flex()
        .w_full()
        .corner(px(9.))
        .when(!nested, |d| d.border_1().border_color(border))
        .bg(bg)
        .overflow_hidden()
        .text_xs()
        .line_height(rems(1.25))
        .child(bar)
        .children(content.map(|c| div().border_t_1().border_color(border).child(c)))
        .into_any_element()
}

/// A box capped at `max_h`, if any, with its own scroll inside the transcript: both
/// ways for unwrapped text, vertical only when it `wrap`s. With `follow`, it
/// opens at the end, like a terminal, and sticks there while `len` grows
/// unless the user scrolled up.
pub(crate) fn scroll_box(
    id: SharedString,
    max_h: Option<Pixels>,
    follow: Option<usize>,
    wrap: bool,
    content: AnyElement,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let state = window.use_keyed_state(SharedString::from(format!("{id}-scroll")), cx, |_, _| {
        (ScrollHandle::new(), Cell::new(None::<usize>))
    });
    let (handle, seen) = {
        let state = state.read(cx);
        (state.0.clone(), state.1.get())
    };
    if let Some(len) = follow {
        let at_end = -handle.offset().y >= handle.max_offset().y - px(4.);
        if seen != Some(len) && (seen.is_none() || at_end) {
            handle.scroll_to_bottom();
        }
        state.read(cx).1.set(Some(len));
    }
    let child_id = |suffix: &str| SharedString::from(format!("{id}-{suffix}"));
    let area = div()
        .id(id.clone())
        .flex()
        .w_full()
        .when_some(max_h, |area, max_h| area.max_h(max_h))
        .track_scroll(&handle);
    let area = if wrap {
        area.overflow_y_scroll().child(div().w_full().child(content))
    } else {
        area.overflow_scroll().child(div().flex_none().min_w_full().child(content))
    };
    // Masks own the wheel inside the box; without them the transcript list
    // scrolls on the same event. They must follow the area: they cover the
    // sibling laid out before them.
    div()
        .relative()
        .w_full()
        .min_w_0()
        .child(area)
        .child(ScrollableMask::new(Axis::Vertical, &handle).id(child_id("mask-y")))
        .when(!wrap, |d| {
            d.child(ScrollableMask::new(Axis::Horizontal, &handle).id(child_id("mask-x")))
        })
        .child(div().absolute().inset_0().child(Scrollbar::new(&handle).id(child_id("bar"))))
        .into_any_element()
}

/// Rows parsed from `raw`, kept per `key` until `raw` changes.
pub(crate) fn cached_rows(
    cx: &mut App,
    key: &SharedString,
    raw: &str,
    build: impl FnOnce() -> Vec<DiffRow>,
) -> Rc<Vec<DiffRow>> {
    let cache = &mut cx.default_global::<ToolRows>().diffs;
    if let Some((seen, rows)) = cache.get(key)
        && seen == raw
    {
        return rows.clone();
    }
    let rows = Rc::new(build());
    cache.insert(key.clone(), (raw.to_string(), rows.clone()));
    rows
}

pub fn tabular<E: Styled>(e: E) -> E {
    e.font_features(FontFeatures(Arc::new(vec![("tnum".into(), 1)])))
}

/// A count after an arrow icon, as in commits ahead or behind.
pub fn arrow_count(icon: Lucide, n: u32) -> Div {
    h_flex()
        .gap_0p5()
        .items_center()
        .map(tabular)
        .child(Icon::new(icon).size(rems(0.75)).flex_none())
        .child(n.to_string())
}

pub(crate) fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

#[cfg(test)]
#[path = "../../tests/unit/chat/tool_row.rs"]
mod tests;
