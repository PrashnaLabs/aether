use crate::theme::Corners;
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use aether_core::hunk_header;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::scroll::{ScrollableElement, ScrollbarAxis};
use gpui_kit::component::{ActiveTheme, Sizable, StyledExt, clipboard::Clipboard, h_flex, v_flex};
use gpui_kit::component::{Icon, highlighter::HighlightTheme};
use gpui_kit::{
    AnyElement, App, ElementId, Global, HighlightStyle, Hsla, ListHorizontalSizingBehavior,
    SharedString, StyledText, UniformListScrollHandle, Window, combine_highlights, div, prelude::*,
    px, rems, uniform_list,
};

use super::cache::RenderCache;
use super::changes::stats;
use super::highlight::{highlight_with, lang_for_path};
use super::tool_row::{OnOpen, scroll_box, tabular};
use crate::components::Segmented;
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Context,
    Added,
    Removed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffRow {
    /// File header of a multi-file patch.
    File(SharedString),
    /// Hunk start: its `@@` header, or empty when the position is unknown.
    Hunk(SharedString),
    Line {
        change: Change,
        old: Option<u32>,
        new: Option<u32>,
        text: SharedString,
    },
    /// Truncation note written by the driver.
    Note(SharedString),
}

/// Rows of a driver-built diff. Positioned hunks number both sides; lines
/// outside one are content only when the diff has no positions at all.
pub fn parse(diff: &str) -> Vec<DiffRow> {
    let patch = diff.lines().any(|l| {
        hunk_header(l).is_some() || l.starts_with("diff --git ") || l.starts_with("Index: ")
    });
    let mut rows = Vec::new();
    let mut left = (0u32, 0u32);
    let mut at: Option<(u32, u32)> = None;
    let mut removed_path = String::new();
    for l in diff.lines() {
        if l.starts_with('…') {
            rows.push(DiffRow::Note(l.trim_start_matches('…').trim().to_string().into()));
            left = (0, 0);
            continue;
        }
        if left == (0, 0) {
            if l.starts_with("@@") {
                at = hunk_header(l).map(|(old, old_n, new, new_n)| {
                    left = (old_n, new_n);
                    (old, new)
                });
                let header = if l.trim() == "@@" { "" } else { l.trim() };
                rows.push(DiffRow::Hunk(header.to_string().into()));
                continue;
            }
            if let Some(rest) = l.strip_prefix("*** ") {
                let path = ["Update File: ", "Add File: ", "Delete File: "]
                    .iter()
                    .find_map(|p| rest.strip_prefix(p));
                rows.extend(path.map(|p| DiffRow::File(p.trim().to_string().into())));
                continue;
            }
            if patch {
                if let Some(path) = l.strip_prefix("--- ") {
                    removed_path = strip_side(path, "a/");
                } else if let Some(path) = l.strip_prefix("+++ ") {
                    let path = strip_side(path, "b/");
                    let path = if path == "/dev/null" { removed_path.clone() } else { path };
                    rows.push(DiffRow::File(path.into()));
                }
                continue;
            }
        }
        let (change, text) = match l.as_bytes().first() {
            Some(b'+') => (Change::Added, &l[1..]),
            Some(b'-') => (Change::Removed, &l[1..]),
            Some(b' ') => (Change::Context, &l[1..]),
            None => (Change::Context, ""),
            _ => continue,
        };
        let (mut old, mut new) = (None, None);
        if let Some((o, n)) = at.as_mut() {
            if change != Change::Added {
                old = Some(*o);
                *o += 1;
                left.0 = left.0.saturating_sub(1);
            }
            if change != Change::Removed {
                new = Some(*n);
                *n += 1;
                left.1 = left.1.saturating_sub(1);
            }
        }
        rows.push(DiffRow::Line { change, old, new, text: text.to_string().into() });
    }
    if rows.iter().filter(|r| matches!(r, DiffRow::File(_))).count() < 2 {
        rows.retain(|r| !matches!(r, DiffRow::File(_)));
    }
    rows
}

/// Byte ranges of the changed middle of two lines, after the common prefix
/// and suffix. `None` when too little is shared to read as an edit.
pub fn word_marks(old: &str, new: &str) -> Option<(Range<usize>, Range<usize>)> {
    let prefix: usize =
        old.chars().zip(new.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum();
    let (old_rest, new_rest) = (&old[prefix..], &new[prefix..]);
    let suffix: usize = old_rest
        .chars()
        .rev()
        .zip(new_rest.chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let shared =
        old[..prefix].chars().count() + old_rest[old_rest.len() - suffix..].chars().count();
    (shared >= 4).then(|| (prefix..old.len() - suffix, prefix..new.len() - suffix))
}

/// Word marks per row: the i-th removed line of a run pairs with the i-th
/// added line that follows it.
pub fn line_marks(rows: &[DiffRow]) -> Vec<Option<Range<usize>>> {
    let mut marks = vec![None; rows.len()];
    let change = |ix: usize| match rows.get(ix) {
        Some(DiffRow::Line { change, .. }) => Some(*change),
        _ => None,
    };
    let mut ix = 0;
    while ix < rows.len() {
        let removed = ix..run_end(ix, Change::Removed, &change);
        let added = removed.end..run_end(removed.end, Change::Added, &change);
        for (r, a) in removed.clone().zip(added.clone()) {
            if let (DiffRow::Line { text: old, .. }, DiffRow::Line { text: new, .. }) =
                (&rows[r], &rows[a])
                && let Some((old_range, new_range)) = word_marks(old, new)
            {
                marks[r] = Some(old_range);
                marks[a] = Some(new_range);
            }
        }
        ix = added.end.max(ix + 1);
    }
    marks
}

fn run_end(start: usize, kind: Change, change: &impl Fn(usize) -> Option<Change>) -> usize {
    (start..).take_while(|&ix| change(ix) == Some(kind)).last().map_or(start, |ix| ix + 1)
}

/// ("Line 44", "function Stats") from "@@ -40,6 +44,7 @@ function Stats".
pub fn hunk_title(header: &str) -> Option<(String, Option<String>)> {
    let (_, _, new, _) = hunk_header(header)?;
    let context = header.splitn(3, "@@").nth(2).map(str::trim).filter(|c| !c.is_empty());
    Some((format!("Line {new}"), context.map(str::to_string)))
}

/// Unchanged lines hidden between two positioned hunks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gap {
    pub before_row: usize,
    pub old_start: u32,
    pub new_start: u32,
    pub lines: u32,
}

pub fn gaps(rows: &[DiffRow]) -> Vec<Gap> {
    let mut out = Vec::new();
    let mut next: Option<(u32, u32)> = None;
    for (ix, row) in rows.iter().enumerate() {
        match row {
            DiffRow::Hunk(header) => {
                let Some((old, _, new, _)) = hunk_header(header) else {
                    next = None;
                    continue;
                };
                if let Some((next_old, next_new)) = next {
                    let lines = new.saturating_sub(next_new);
                    if lines > 0 {
                        out.push(Gap {
                            before_row: ix,
                            old_start: next_old,
                            new_start: next_new,
                            lines,
                        });
                    }
                }
                next = Some((old, new));
            }
            DiffRow::Line { change, .. } => {
                if let Some((old, new)) = next.as_mut() {
                    *old += (*change != Change::Added) as u32;
                    *new += (*change != Change::Removed) as u32;
                }
            }
            DiffRow::File(_) | DiffRow::Note(_) => next = None,
        }
    }
    out
}

/// A row of the side-by-side layout, as indexes into the diff rows.
#[derive(Debug, PartialEq, Eq)]
pub enum Split {
    Full(usize),
    Pair(Option<usize>, Option<usize>),
}

pub fn split_rows(rows: &[DiffRow]) -> Vec<Split> {
    let change = |ix: usize| match rows.get(ix) {
        Some(DiffRow::Line { change, .. }) => Some(*change),
        _ => None,
    };
    let mut out = Vec::new();
    let mut ix = 0;
    while ix < rows.len() {
        match change(ix) {
            None => {
                out.push(Split::Full(ix));
                ix += 1;
            }
            Some(Change::Context) => {
                out.push(Split::Pair(Some(ix), Some(ix)));
                ix += 1;
            }
            Some(_) => {
                let removed = ix..run_end(ix, Change::Removed, &change);
                let added = removed.end..run_end(removed.end, Change::Added, &change);
                for n in 0..removed.len().max(added.len()) {
                    out.push(Split::Pair(removed.clone().nth(n), added.clone().nth(n)));
                }
                ix = added.end;
            }
        }
    }
    out
}

/// First file a patch names, read from its headers since `parse` drops the
/// header of a single-file patch.
pub fn patch_path(diff: &str) -> Option<String> {
    diff.lines().take_while(|l| !l.starts_with("@@")).find_map(|l| {
        let path = match l.strip_prefix("+++ ") {
            Some(path) => strip_side(path, "b/"),
            None => ["*** Update File: ", "*** Add File: ", "*** Delete File: "]
                .iter()
                .find_map(|p| l.strip_prefix(p))?
                .trim()
                .to_string(),
        };
        (path != "/dev/null").then_some(path)
    })
}

fn strip_side(path: &str, prefix: &str) -> String {
    let path = path.split('\t').next().unwrap_or(path).trim();
    path.strip_prefix(prefix).unwrap_or(path).to_string()
}

type Styles = Rc<Vec<(Range<usize>, HighlightStyle)>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unavailable {
    NoFile,
    Short,
}

type Opened = HashMap<usize, Result<Rc<Vec<DiffRow>>, Unavailable>>;

/// Syntax styles per line text, valid for one highlight theme.
struct Highlights {
    theme: Arc<HighlightTheme>,
    lines: HashMap<SharedString, Styles>,
}

/// Split choice, expanded gaps and highlights per diff key. Kept outside the
/// virtualized transcript so rows scrolled away come back as they were.
struct DiffState {
    split: RenderCache<bool>,
    /// Per original row index of a gap: its lines, or why there are none.
    gaps: RenderCache<Opened>,
    highlights: RenderCache<Highlights>,
}

impl Default for DiffState {
    fn default() -> Self {
        Self {
            split: RenderCache::new(256),
            gaps: RenderCache::new(256),
            highlights: RenderCache::new(64),
        }
    }
}

impl Global for DiffState {}

/// A gap still shown as a row before `rows[at]` of the spliced rows.
struct Pending {
    orig: usize,
    gap: Gap,
    unavailable: Option<Unavailable>,
}

/// Expanded gaps become context rows; the others are keyed by the spliced
/// index of the hunk row they sit before.
fn splice(rows: &[DiffRow], opened: &Opened) -> (Vec<DiffRow>, HashMap<usize, Pending>) {
    let mut out = Vec::with_capacity(rows.len());
    let mut pending = HashMap::new();
    let gaps = gaps(rows);
    for (ix, row) in rows.iter().enumerate() {
        if let Some(gap) = gaps.iter().find(|g| g.before_row == ix) {
            match opened.get(&ix) {
                Some(Ok(lines)) => out.extend(lines.iter().cloned()),
                other => {
                    let unavailable = other.and_then(|r| r.as_ref().err().copied());
                    pending.insert(out.len(), Pending { orig: ix, gap: *gap, unavailable });
                }
            }
        }
        out.push(row.clone());
    }
    (out, pending)
}

/// Syntax styles for each row's text, parsed once per line and highlight theme.
fn styles_for(
    key: &SharedString,
    rows: &[DiffRow],
    lang: &str,
    cx: &mut App,
) -> Vec<Option<Styles>> {
    let theme = cx.theme().highlight_theme.clone();
    let state = cx.default_global::<DiffState>();
    if !state.highlights.get(key).is_some_and(|h| Arc::ptr_eq(&h.theme, &theme)) {
        state
            .highlights
            .insert(key.clone(), Highlights { theme: theme.clone(), lines: HashMap::new() });
    }
    let Some(cache) = state.highlights.get(key) else { return vec![None; rows.len()] };
    rows.iter()
        .map(|row| {
            let DiffRow::Line { text, .. } = row else { return None };
            let styles = cache
                .lines
                .entry(text.clone())
                .or_insert_with(|| Rc::new(highlight_with(text, lang, &theme)));
            Some(styles.clone())
        })
        .collect()
}

fn expand_gap(key: SharedString, at: usize, gap: Gap, file: Option<PathBuf>, cx: &mut App) {
    let read = cx.background_executor().spawn(async move {
        let text = std::fs::read_to_string(file.ok_or(Unavailable::NoFile)?)
            .map_err(|_| Unavailable::NoFile)?;
        let lines: Vec<DiffRow> = text
            .lines()
            .skip(gap.new_start.saturating_sub(1) as usize)
            .take(gap.lines as usize)
            .zip(0..)
            .map(|(line, n)| DiffRow::Line {
                change: Change::Context,
                old: Some(gap.old_start + n),
                new: Some(gap.new_start + n),
                text: line.to_string().into(),
            })
            .collect();
        if lines.is_empty() { Err(Unavailable::Short) } else { Ok(lines) }
    });
    cx.spawn(async move |cx| {
        let lines = read.await.map(Rc::new);
        cx.update(|cx| {
            let state = cx.default_global::<DiffState>();
            let mut open = state.gaps.get(&key).cloned().unwrap_or_default();
            open.insert(at, lines);
            state.gaps.insert(key, open);
            cx.refresh_windows();
        });
    })
    .detach();
}

/// A parsed diff in a bounded, scrollable box: a header with the path and
/// totals, old/new line numbers when the hunk is positioned, word marks,
/// syntax colour, unified or side-by-side, and gaps that expand from disk.
#[derive(IntoElement)]
pub(crate) struct DiffView {
    id: SharedString,
    path: SharedString,
    file: Option<PathBuf>,
    rows: Rc<Vec<DiffRow>>,
    raw: SharedString,
    stats: Option<(u32, u32)>,
    on_open: Option<OnOpen>,
    capped: bool,
    bare: bool,
}

impl DiffView {
    pub(crate) fn new(
        id: SharedString,
        path: SharedString,
        rows: Rc<Vec<DiffRow>>,
        raw: SharedString,
    ) -> Self {
        Self {
            id,
            path,
            file: None,
            rows,
            raw,
            stats: None,
            on_open: None,
            capped: true,
            bare: false,
        }
    }

    /// Absolute path the gaps between hunks are read from.
    pub(crate) fn file(mut self, file: Option<PathBuf>) -> Self {
        self.file = file;
        self
    }

    /// Line totals beside the path.
    pub(crate) fn stats(mut self, added: u32, removed: u32) -> Self {
        self.stats = Some((added, removed));
        self
    }

    /// An Open file button beside Copy.
    pub(crate) fn on_open(mut self, handler: Option<OnOpen>) -> Self {
        self.on_open = handler;
        self
    }

    /// Grows with its rows instead of scrolling past 320 px.
    pub(crate) fn uncapped(mut self) -> Self {
        self.capped = false;
        self
    }

    /// No outer border, for a diff inside a card.
    pub(crate) fn bare(mut self, bare: bool) -> Self {
        self.bare = bare;
        self
    }
}

/// Colors and line-number column widths shared by every row of one diff.
#[derive(Clone, Copy)]
struct Look {
    fg: Hsla,
    muted: Hsla,
    added_fg: Hsla,
    removed_fg: Hsla,
    added_bg: Hsla,
    removed_bg: Hsla,
    added_mark: Hsla,
    removed_mark: Hsla,
    hunk_bg: Hsla,
    file_bg: Hsla,
    /// Digits per side; a side without numbers has no column.
    old_digits: Option<usize>,
    new_digits: Option<usize>,
}

impl Look {
    fn new(rows: &[DiffRow], cx: &App) -> Self {
        let digits = |side: fn(&DiffRow) -> Option<u32>| {
            rows.iter().filter_map(side).max().map(|n| n.to_string().len())
        };
        let theme = cx.theme();
        Self {
            fg: theme.foreground,
            muted: theme.muted_foreground,
            added_fg: theme::success_text(cx),
            removed_fg: theme::danger_text(cx),
            added_bg: theme.success.opacity(0.10),
            removed_bg: theme.danger.opacity(0.10),
            added_mark: theme.success.opacity(0.32),
            removed_mark: theme.danger.opacity(0.32),
            hunk_bg: theme.info.opacity(0.10),
            file_bg: theme.secondary,
            old_digits: digits(|r| match r {
                DiffRow::Line { old, .. } => *old,
                _ => None,
            }),
            new_digits: digits(|r| match r {
                DiffRow::Line { new, .. } => *new,
                _ => None,
            }),
        }
    }

    fn number(&self, digits: Option<usize>, n: Option<u32>) -> Option<gpui_kit::Div> {
        // ~0.6em per mono digit at text_xs, plus padding
        digits.map(|d| {
            div()
                .flex_none()
                .w(rems(0.45 * d as f32 + 0.75))
                .pr_2()
                .text_right()
                .text_color(self.muted)
                .children(n.map(|n| n.to_string()))
        })
    }

    fn indent(&self) -> gpui_kit::Rems {
        let col = |d: Option<usize>| d.map_or(0., |d| 0.45 * d as f32 + 0.75);
        rems(col(self.old_digits) + col(self.new_digits) + 1.)
    }

    fn code(
        &self,
        text: &SharedString,
        change: Change,
        mark: Option<Range<usize>>,
        base: Option<&Styles>,
    ) -> StyledText {
        let mut styles = base.map(|base| base.to_vec()).unwrap_or_default();
        if let Some(mark) = mark {
            let background_color =
                Some(if change == Change::Added { self.added_mark } else { self.removed_mark });
            let mark = (mark, HighlightStyle { background_color, ..Default::default() });
            styles = combine_highlights(styles, [mark]).collect();
        }
        StyledText::new(text.clone()).with_highlights(styles)
    }

    /// One side of a line: its numbers, `+`/`−` gutter and code.
    fn line(
        &self,
        row: &DiffRow,
        sides: (bool, bool),
        mark: Option<Range<usize>>,
        base: Option<&Styles>,
    ) -> AnyElement {
        let DiffRow::Line { change, old, new, text } = row else {
            return div().into_any_element();
        };
        let (bg, sign, sign_color) = match change {
            Change::Added => (Some(self.added_bg), "+", self.added_fg),
            Change::Removed => (Some(self.removed_bg), "−", self.removed_fg),
            Change::Context => (None, " ", self.muted),
        };
        h_flex()
            .when_some(bg, |r, bg| r.bg(bg))
            .children(sides.0.then(|| self.number(self.old_digits, *old)).flatten())
            .children(sides.1.then(|| self.number(self.new_digits, *new)).flatten())
            .child(div().flex_none().w(rems(1.)).text_center().text_color(sign_color).child(sign))
            .child(div().pr_3().text_color(self.fg).child(self.code(text, *change, mark, base)))
            .into_any_element()
    }

    fn row(
        &self,
        row: &DiffRow,
        mark: Option<Range<usize>>,
        base: Option<&Styles>,
        cx: &App,
    ) -> AnyElement {
        let muted = self.muted;
        match row {
            DiffRow::Line { .. } => self.line(row, (true, true), mark, base),
            DiffRow::Hunk(header) if header.is_empty() => {
                div().h(px(6.)).bg(self.hunk_bg).into_any_element()
            }
            DiffRow::Hunk(header) => {
                let (title, context) =
                    hunk_title(header).unwrap_or_else(|| (header.to_string(), None));
                h_flex()
                    .px_2()
                    .gap_1()
                    .bg(self.hunk_bg)
                    .text_color(muted)
                    .child(Icon::new(Lucide::ChevronRight).xsmall())
                    .child(
                        div()
                            .font_family(cx.theme().font_family.clone())
                            .text_color(theme::info_text(cx))
                            .child(title),
                    )
                    .children(context.map(|c| format!("· {c}")))
                    .into_any_element()
            }
            DiffRow::File(path) => div()
                .px_2()
                .py_0p5()
                .bg(self.file_bg)
                .text_color(self.fg)
                .font_medium()
                .child(path.clone())
                .into_any_element(),
            DiffRow::Note(note) => {
                div().px_2().py_0p5().text_color(muted).child(note.clone()).into_any_element()
            }
        }
    }

    fn pair(
        &self,
        rows: &[DiffRow],
        marks: &[Option<Range<usize>>],
        styles: &[Option<Styles>],
        (left, right): (Option<usize>, Option<usize>),
        cx: &App,
    ) -> AnyElement {
        let side = |ix: Option<usize>, sides: (bool, bool)| {
            let cell = div().flex_1().min_w_0().min_h(rems(1.25)).overflow_hidden();
            match ix {
                Some(ix) => {
                    cell.child(self.line(&rows[ix], sides, marks[ix].clone(), styles[ix].as_ref()))
                }
                None => cell.bg(cx.theme().muted.opacity(0.5)),
            }
        };
        h_flex()
            .items_stretch()
            .child(side(left, (true, false)))
            .child(side(right, (false, true)).border_l_1().border_color(cx.theme().border))
            .into_any_element()
    }
}

impl RenderOnce for DiffView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let key = self.id.clone();
        let state = cx.default_global::<DiffState>();
        let split = state.split.get(&key).copied().unwrap_or(false);
        let opened = state.gaps.get(&key).cloned().unwrap_or_default();

        let (rows, pending) = splice(&self.rows, &opened);
        let marks = line_marks(&rows);
        let lang = lang_for_path(&self.path);
        let styles = styles_for(&key, &rows, lang, cx);
        let look = Look::new(&rows, cx);
        let gap_row = |at: usize| {
            let Pending { orig, gap, unavailable } = pending.get(&at)?;
            let (orig, gap) = (*orig, *gap);
            let count = if gap.lines == 1 {
                "1 unchanged line".to_string()
            } else {
                format!("{} unchanged lines", gap.lines)
            };
            let row = h_flex().pl(look.indent()).min_h(rems(1.5)).text_color(look.muted);
            let note = match unavailable {
                Some(Unavailable::NoFile) => Some("File not found"),
                Some(Unavailable::Short) => Some("Those lines are not in the file"),
                None => None,
            };
            Some(match (note, &self.file) {
                (Some(note), _) => row.child(note).into_any_element(),
                (None, None) => row.child(count).into_any_element(),
                (None, Some(file)) => {
                    let (key, file) = (key.clone(), file.clone());
                    row.child(
                        crate::components::button(SharedString::from(format!("{key}-gap-{orig}")))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::UnfoldVertical)
                            .label(count)
                            .on_click(move |_, _, cx| {
                                expand_gap(key.clone(), orig, gap, Some(file.clone()), cx)
                            }),
                    )
                    .into_any_element()
                }
            })
        };
        let mut items: Vec<AnyElement> = Vec::with_capacity(rows.len());
        if split {
            for s in split_rows(&rows) {
                let first = match s {
                    Split::Full(ix) => ix,
                    Split::Pair(l, r) => l.or(r).unwrap_or(0),
                };
                items.extend(gap_row(first));
                items.push(match s {
                    Split::Full(ix) => look.row(&rows[ix], None, None, cx),
                    Split::Pair(l, r) => look.pair(&rows, &marks, &styles, (l, r), cx),
                });
            }
        } else {
            for (ix, row) in rows.iter().enumerate() {
                items.extend(gap_row(ix));
                items.push(look.row(row, marks[ix].clone(), styles[ix].as_ref(), cx));
            }
        }

        let theme = cx.theme();
        let (fg, muted, border) = (theme.foreground, theme.muted_foreground, theme.border);
        let mono = theme.mono_font_family.clone();
        let new_file =
            matches!(self.rows.first(), Some(DiffRow::Hunk(h)) if h.starts_with("@@ -0,0"));
        let stats = self.stats.map(|(added, removed)| stats(added, removed, cx));
        let open = self.on_open.map(|open| {
            crate::components::button(SharedString::from(format!("{}-open", self.id)))
                .ghost()
                .xsmall()
                .icon(Lucide::ExternalLink)
                .label("Open")
                .accessibility_label(SharedString::from(format!("Open {}", self.path)))
                .tooltip("Open file")
                .on_click(move |_, window, cx| open(window, cx))
        });
        let name_at = self.path.rfind('/').map_or(0, |ix| ix + 1);
        let path = StyledText::new(self.path.clone()).with_highlights([
            (0..name_at, HighlightStyle { color: Some(muted), ..Default::default() }),
            (name_at..self.path.len(), HighlightStyle { color: Some(fg), ..Default::default() }),
        ]);
        let pill = theme::success_text(cx);
        let tint = cx.theme().success.opacity(0.12);
        let split_key = key.clone();
        let body = v_flex()
            .py_1()
            .font_family(mono.clone())
            .text_xs()
            .line_height(rems(1.25))
            .whitespace_nowrap()
            .map(tabular)
            .children(items);
        v_flex()
            .w_full()
            .corner(px(9.))
            .when(!self.bare, |d| d.border_1().border_color(border))
            .overflow_hidden()
            .child(
                h_flex()
                    .pl_2()
                    .pr_1()
                    .gap_2()
                    .min_h(rems(1.75))
                    .border_b_1()
                    .border_color(border)
                    .text_xs()
                    .text_color(muted)
                    .child(Icon::new(Lucide::File).xsmall())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .font_family(mono)
                            .text_ellipsis_middle()
                            .child(path),
                    )
                    .when(new_file, |h| {
                        h.child(
                            div().px_1().corner(rems(0.375)).bg(tint).text_color(pill).child("new"),
                        )
                    })
                    .children(stats)
                    .child(
                        Segmented::new(SharedString::from(format!("{}-mode", self.id)))
                            .compact()
                            .icon_option(Lucide::Rows2, "Unified diff")
                            .icon_option(Lucide::Columns2, "Split diff")
                            .selected(split as usize)
                            .on_change(move |ix, window, cx| {
                                cx.default_global::<DiffState>()
                                    .split
                                    .insert(split_key.clone(), *ix == 1);
                                window.refresh();
                            }),
                    )
                    .child(
                        Clipboard::new(SharedString::from(format!("{}-copy", self.id)))
                            .value(self.raw)
                            .tooltip("Copy diff")
                            .accessibility_label("Copy diff")
                            .xsmall(),
                    )
                    .children(open),
            )
            .child(scroll_box(
                self.id,
                self.capped.then_some(px(320.)),
                None,
                false,
                body.into_any_element(),
                window,
                cx,
            ))
    }
}

/// A whole diff that fills its parent, drawing only the rows in view, with
/// both scrollbars; rows look the same as in [`DiffView`].
#[derive(IntoElement)]
pub struct DiffLines {
    id: ElementId,
    rows: Rc<Vec<DiffRow>>,
    scroll: UniformListScrollHandle,
}

impl DiffLines {
    pub fn new(
        id: impl Into<ElementId>,
        rows: Rc<Vec<DiffRow>>,
        scroll: &UniformListScrollHandle,
    ) -> Self {
        Self { id: id.into(), rows, scroll: scroll.clone() }
    }
}

impl RenderOnce for DiffLines {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let look = Look::new(&self.rows, cx);
        let marks = line_marks(&self.rows);
        // the list sizes its width from one row, so measure the longest
        let widest = self
            .rows
            .iter()
            .enumerate()
            .max_by_key(|(_, row)| match row {
                DiffRow::Line { text, .. }
                | DiffRow::Hunk(text)
                | DiffRow::File(text)
                | DiffRow::Note(text) => text.len(),
            })
            .map(|(ix, _)| ix);
        let rows = self.rows.clone();
        div()
            .relative()
            .size_full()
            .font_family(cx.theme().mono_font_family.clone())
            .text_xs()
            .line_height(rems(1.25))
            .whitespace_nowrap()
            .map(tabular)
            .child(
                uniform_list(self.id, rows.len(), move |range, _, cx| {
                    range
                        .map(|ix| look.row(&rows[ix], marks[ix].clone(), None, cx))
                        .collect::<Vec<_>>()
                })
                .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
                .with_width_from_item(widest)
                .track_scroll(&self.scroll)
                .size_full(),
            )
            .scrollbar(&self.scroll, ScrollbarAxis::Both)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/diff_view.rs"]
mod tests;
