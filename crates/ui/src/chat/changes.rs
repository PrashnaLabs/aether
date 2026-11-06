use crate::theme::Corners;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use aether_core::{Step, ToolDetail, ToolStatus};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable, StyledExt, button::ButtonVariants, h_flex, tooltip::Tooltip, v_flex,
};
use gpui_kit::{
    AnyElement, App, Div, Global, Point, Role, ScrollHandle, SharedString, Window, div, prelude::*,
    rems,
};

use super::diff_view::{DiffRow, DiffView, parse};
use super::links::{open_file, parse_file_link};
use super::sidebar::row_style;
use super::tool_row::{
    OnOpen, Reflow, abs_path, cached_rows, count, refresh, relative_path, tabular,
};
use crate::{file_icon, file_icon_color, theme};

/// Opens a reply's edits in the side panel, at one file when given.
pub type OnOpenDiff = Rc<dyn Fn(Option<String>, &mut Window, &mut App)>;
type OnSelect = Rc<dyn Fn(usize, &mut Window, &mut App)>;

const ROW_HEIGHT: f32 = 1.625;

/// A file one reply edited: its path from the chat folder, its line totals
/// and each edit's diff, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileEdits {
    pub path: String,
    pub added: u32,
    pub removed: u32,
    pub diffs: Vec<String>,
}

/// The files a reply's edits touched, by first edit. Only finished edits
/// count: denied, failed, stopped and unfinished ones are left out.
pub fn reply_edits<'a>(
    steps: impl IntoIterator<Item = &'a Step>,
    cwd: Option<&str>,
) -> Vec<FileEdits> {
    let mut files: Vec<FileEdits> = Vec::new();
    for (path, diff, added, removed) in steps.into_iter().filter_map(applied_edit) {
        let path = relative_path(path, cwd);
        let ix = files.iter().position(|f| f.path == path).unwrap_or_else(|| {
            files.push(FileEdits { path, ..Default::default() });
            files.len() - 1
        });
        let file = &mut files[ix];
        file.added += added;
        file.removed += removed;
        file.diffs.push(diff.to_string());
    }
    files
}

pub fn edit_totals<'a>(steps: impl IntoIterator<Item = &'a Step>) -> (u32, u32) {
    steps.into_iter().filter_map(applied_edit).fold((0, 0), |(a, r), (_, _, x, y)| (a + x, r + y))
}

pub(crate) fn applied_edit(step: &Step) -> Option<(&str, &str, u32, u32)> {
    let Step::Tool(call) = step else { return None };
    let ToolDetail::Edit { path, diff, added, removed } = &call.detail else { return None };
    let applied = matches!(call.status, ToolStatus::Done | ToolStatus::Unknown);
    (applied && !path.is_empty()).then_some((path, diff, *added, *removed))
}

fn totals(files: &[FileEdits]) -> (u32, u32) {
    files.iter().fold((0, 0), |(a, r), f| (a + f.added, r + f.removed))
}

fn changes_title(files: &[FileEdits]) -> String {
    count(files.len(), "changed file", "changed files")
}

/// The card in words: "2 changed files, 41 added, 3 removed".
fn changes_label(files: &[FileEdits]) -> String {
    let (added, removed) = totals(files);
    format!("{}, {added} added, {removed} removed", changes_title(files))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum EditRow {
    /// `path` from the chat folder; `name` is past its parent's row, a chain
    /// of lone subfolders joined on.
    Dir { path: String, name: String, depth: usize, added: u32, removed: u32, collapsed: bool },
    /// A file by its index in the edits.
    File { ix: usize, depth: usize },
}

#[derive(Default)]
struct Node<'a> {
    dirs: BTreeMap<&'a str, Node<'a>>,
    files: Vec<(&'a str, usize)>,
}

impl Node<'_> {
    fn totals(&self, files: &[FileEdits]) -> (u32, u32) {
        let own = self.files.iter().map(|&(_, ix)| (files[ix].added, files[ix].removed));
        own.chain(self.dirs.values().map(|dir| dir.totals(files)))
            .fold((0, 0), |(a, r), (x, y)| (a + x, r + y))
    }
}

/// The files as a folder tree, like the git panel's: folders before files,
/// each by name, a chain of lone subfolders on one row, and nothing listed
/// inside `collapsed` folders.
fn edit_tree(files: &[FileEdits], collapsed: &dyn Fn(&str) -> bool) -> Vec<EditRow> {
    let mut root = Node::default();
    for (ix, file) in files.iter().enumerate() {
        let (dir, name) = file.path.rsplit_once('/').unwrap_or(("", &file.path));
        let node = dir
            .split('/')
            .filter(|part| !part.is_empty())
            .fold(&mut root, |node, part| node.dirs.entry(part).or_default());
        node.files.push((name, ix));
    }
    let mut rows = Vec::new();
    push_tree(&root, "", 0, files, collapsed, &mut rows);
    rows
}

fn push_tree(
    node: &Node,
    parent: &str,
    depth: usize,
    files: &[FileEdits],
    collapsed: &dyn Fn(&str) -> bool,
    rows: &mut Vec<EditRow>,
) {
    let mut dirs: Vec<_> = node.dirs.iter().collect();
    dirs.sort_by(|a, b| by_name(a.0, b.0));
    for (name, mut child) in dirs {
        let mut name = name.to_string();
        while let (Some((next, only)), 1, true) =
            (child.dirs.first_key_value(), child.dirs.len(), child.files.is_empty())
        {
            name = format!("{name}/{next}");
            child = only;
        }
        let path = if parent.is_empty() { name.clone() } else { format!("{parent}/{name}") };
        let (added, removed) = child.totals(files);
        let shut = collapsed(&path);
        rows.push(EditRow::Dir {
            path: path.clone(),
            name,
            depth,
            added,
            removed,
            collapsed: shut,
        });
        if !shut {
            push_tree(child, &path, depth + 1, files, collapsed, rows);
        }
    }
    let mut leaves = node.files.clone();
    leaves.sort_by(|a, b| by_name(a.0, b.0));
    rows.extend(leaves.into_iter().map(|(_, ix)| EditRow::File { ix, depth }));
}

fn by_name(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
}

/// Edit indexes in tree order, which the Diff tab stacks them in.
fn file_order(files: &[FileEdits]) -> Vec<usize> {
    edit_tree(files, &|_| false)
        .into_iter()
        .filter_map(|row| match row {
            EditRow::File { ix, .. } => Some(ix),
            EditRow::Dir { .. } => None,
        })
        .collect()
}

/// Each edit's rows in order, with a break before an edit that opens with no hunk.
fn file_rows(diffs: &[String]) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    for diff in diffs {
        let next = parse(diff);
        if !rows.is_empty() && !matches!(next.first(), Some(DiffRow::Hunk(_))) {
            rows.push(DiffRow::Hunk(SharedString::default()));
        }
        rows.extend(next);
    }
    rows
}

/// Scrolls to stacked diff `at`, the file list being child 0, or to the top.
fn scroll_to(scroll: &ScrollHandle, at: Option<usize>) {
    match at {
        Some(at) => scroll.scroll_to_top_of_item(at + 1),
        None => scroll.set_offset(Point::default()),
    }
}

/// Scrolls a [`ReplyDiff`] on `scroll` to `path`'s diff, or to the top.
pub fn reveal_file(scroll: &ScrollHandle, files: &[FileEdits], path: Option<&str>) {
    let order = file_order(files);
    scroll_to(scroll, path.and_then(|p| order.iter().position(|&ix| files[ix].path == p)));
}

/// Folded change cards and tree folders, by card id and `{id}:{folder}`.
#[derive(Default)]
struct Folds(HashSet<SharedString>);

impl Global for Folds {}

fn toggle_fold(cx: &mut App, key: SharedString) {
    let folds = &mut cx.default_global::<Folds>().0;
    if !folds.remove(&key) {
        folds.insert(key);
    }
}

/// `+N −M` in add and remove colors; a side with nothing is left out.
pub(crate) fn stats(added: u32, removed: u32, cx: &App) -> Div {
    h_flex()
        .flex_none()
        .gap_1()
        .map(tabular)
        .when(added > 0, |r| {
            r.child(div().text_color(theme::success_text(cx)).child(format!("+{added}")))
        })
        .when(removed > 0, |r| {
            r.child(div().text_color(theme::danger_text(cx)).child(format!("−{removed}")))
        })
}

/// A row's left padding in rems; a file's leaves a chevron's room.
fn indent(depth: usize, file: bool) -> f32 {
    0.25 + depth as f32 * 0.875 + if file { 1.125 } else { 0. }
}

/// The edits' tree rows: folders fold, files go to `select`, and to `open` from their button.
fn tree(
    id: &SharedString,
    files: &[FileEdits],
    select: OnSelect,
    open: Option<OnSelect>,
    reflow: Option<Reflow>,
    cx: &mut App,
) -> Div {
    let key = |path: &str| SharedString::from(format!("{id}:{path}"));
    let rows = {
        let folds = &cx.default_global::<Folds>().0;
        edit_tree(files, &|path| folds.contains(&key(path)))
    };
    let cx: &App = cx;
    let muted = cx.theme().muted_foreground;
    let mono = cx.theme().mono_font_family.clone();
    v_flex().children(rows.into_iter().map(|row| match row {
        EditRow::Dir { path, name, depth, added, removed, collapsed } => {
            let (chevron, folder) = match collapsed {
                true => (Lucide::ChevronRight, Lucide::Folder),
                false => (Lucide::ChevronDown, Lucide::FolderOpen),
            };
            let (fold, reflow) = (key(&path), reflow.clone());
            row_style(
                h_flex()
                    .id(SharedString::from(format!("{id}-dir-{path}")))
                    .role(Role::Button)
                    .aria_label(SharedString::from(format!(
                        "{path} folder, {added} added, {removed} removed"
                    )))
                    .aria_expanded(!collapsed)
                    .tab_index(0),
                false,
                cx,
            )
            .h(rems(ROW_HEIGHT))
            .pl(rems(indent(depth, false)))
            .pr_1()
            .gap_1p5()
            .items_center()
            .child(Icon::new(chevron).size(rems(0.75)).flex_none().text_color(muted))
            .child(Icon::new(folder).size(rems(0.875)).flex_none().text_color(muted))
            .child(div().flex_1().min_w_0().truncate().text_sm().text_color(muted).child(name))
            .child(stats(added, removed, cx).text_xs())
            .on_click(move |_, window, cx| {
                toggle_fold(cx, fold.clone());
                refresh(&reflow, window, cx);
            })
            .into_any_element()
        }
        EditRow::File { ix, depth } => {
            let file = &files[ix];
            let path = Path::new(&file.path);
            let name = file.path.rsplit('/').next().unwrap_or(&file.path).to_string();
            let tip = SharedString::from(file.path.clone());
            let (select, open) = (select.clone(), open.clone());
            row_style(
                h_flex()
                    .id(SharedString::from(format!("{id}-file-{ix}")))
                    .role(Role::Button)
                    .aria_label(SharedString::from(format!(
                        "{}, {} added, {} removed",
                        file.path, file.added, file.removed
                    )))
                    .aria_description("Opens its diff")
                    .tab_index(0),
                false,
                cx,
            )
            .h(rems(ROW_HEIGHT))
            .pl(rems(indent(depth, true)))
            .pr_1()
            .gap_1p5()
            .items_center()
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .child(
                Icon::new(file_icon(path))
                    .size(rems(0.875))
                    .flex_none()
                    .text_color(file_icon_color(path, cx)),
            )
            .child(
                div().flex_1().min_w_0().truncate().text_sm().font_family(mono.clone()).child(name),
            )
            .child(stats(file.added, file.removed, cx).text_xs())
            .when_some(open, |row, open| {
                row.child(
                    crate::components::button(SharedString::from(format!("{id}-open-{ix}")))
                        .ghost()
                        .xsmall()
                        .icon(Lucide::FileOutput)
                        .accessibility_label(SharedString::from(format!(
                            "Open {} in Explorer",
                            file.path
                        )))
                        .tooltip("Open in Explorer")
                        .on_click(move |_, window, cx| open(ix, window, cx)),
                )
            })
            .on_click(move |_, window, cx| select(ix, window, cx))
            .into_any_element()
        }
    }))
}

/// The files a finished reply edited: a count and totals over their tree,
/// which folds away, and a way to open them all in the side panel.
#[derive(IntoElement)]
pub struct ChangesCard {
    id: SharedString,
    files: Vec<FileEdits>,
    on_open: OnOpenDiff,
    on_reflow: Option<Reflow>,
    cwd: Option<SharedString>,
}

impl ChangesCard {
    pub fn new(
        id: SharedString,
        files: Vec<FileEdits>,
        on_open: OnOpenDiff,
        on_reflow: Option<Reflow>,
    ) -> Self {
        Self { id, files, on_open, on_reflow, cwd: None }
    }

    /// Folder the paths are from, for opening a file in the Explorer.
    pub fn cwd(mut self, cwd: Option<SharedString>) -> Self {
        self.cwd = cwd;
        self
    }
}

impl RenderOnce for ChangesCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let open = !cx.default_global::<Folds>().0.contains(&self.id);
        let (added, removed) = totals(&self.files);
        let theme = cx.theme();
        let (fg, border, bg) = (theme.foreground, theme.border, theme.secondary.opacity(0.5));
        let (fold_icon, fold_label) = match open {
            true => (Lucide::ChevronsDownUp, "Collapse changed files"),
            false => (Lucide::ChevronsUpDown, "Expand changed files"),
        };
        let fold = {
            let (key, reflow) = (self.id.clone(), self.on_reflow.clone());
            crate::components::button(SharedString::from(format!("{}-fold", self.id)))
                .ghost()
                .xsmall()
                .icon(fold_icon)
                .accessibility_label(fold_label)
                .tooltip(fold_label)
                .on_click(move |_, window, cx| {
                    toggle_fold(cx, key.clone());
                    refresh(&reflow, window, cx);
                })
        };
        let open_all = {
            let on_open = self.on_open.clone();
            crate::components::button(SharedString::from(format!("{}-open", self.id)))
                .ghost()
                .xsmall()
                .icon(Lucide::FileDiff)
                .label("Open diff")
                .on_click(move |_, window, cx| on_open(None, window, cx))
        };
        let paths: Rc<Vec<String>> = Rc::new(self.files.iter().map(|f| f.path.clone()).collect());
        let select: OnSelect = {
            let (on_open, paths) = (self.on_open.clone(), paths.clone());
            Rc::new(move |ix, window, cx| on_open(paths.get(ix).cloned(), window, cx))
        };
        let open_in_explorer: OnSelect = {
            let cwd = self.cwd.as_deref().map(PathBuf::from);
            Rc::new(move |ix, window, cx| {
                if let Some(link) = paths.get(ix).and_then(|p| parse_file_link(p, cwd.as_deref())) {
                    open_file(link, window, cx);
                }
            })
        };
        let header = h_flex()
            .gap_2()
            .items_center()
            .text_sm()
            .child(div().font_medium().text_color(fg).child(changes_title(&self.files)))
            .child(stats(added, removed, cx))
            .child(div().flex_1())
            .child(fold)
            .child(open_all);
        v_flex()
            .id(self.id.clone())
            .role(Role::Group)
            .aria_label(SharedString::from(changes_label(&self.files)))
            .w_full()
            .p_3()
            .gap_2()
            .corner(rems(0.5))
            .border_1()
            .border_color(border)
            .bg(bg)
            .child(header)
            .when(open, |card| {
                let reflow = self.on_reflow.clone();
                card.child(tree(&self.id, &self.files, select, Some(open_in_explorer), reflow, cx))
            })
    }
}

/// A reply's edits in the side panel: their tree, then each file's diff
/// stacked in tree order; picking a file scrolls to its diff.
#[derive(IntoElement)]
pub struct ReplyDiff {
    id: SharedString,
    files: Vec<FileEdits>,
    scroll: ScrollHandle,
    cwd: Option<SharedString>,
    on_open_file: Option<Rc<dyn Fn(&str, &mut Window, &mut App)>>,
}

impl ReplyDiff {
    pub fn new(id: impl Into<SharedString>, files: Vec<FileEdits>, scroll: &ScrollHandle) -> Self {
        Self { id: id.into(), files, scroll: scroll.clone(), cwd: None, on_open_file: None }
    }

    /// Folder the paths are from, so gaps between hunks can expand from disk.
    pub fn cwd(mut self, cwd: Option<SharedString>) -> Self {
        self.cwd = cwd;
        self
    }

    /// Shows an Open file button on each diff, called with the file's path.
    pub fn on_open_file(mut self, handler: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_open_file = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ReplyDiff {
    // ponytail: draws every row of every diff; virtualize if huge replies lag
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let order = file_order(&self.files);
        let select: OnSelect = {
            let (scroll, order) = (self.scroll.clone(), order.clone());
            Rc::new(move |ix, window, _| {
                scroll_to(&scroll, order.iter().position(|&i| i == ix));
                window.refresh();
            })
        };
        let (added, removed) = totals(&self.files);
        let (fg, border) = (cx.theme().foreground, cx.theme().border);
        let list = v_flex()
            .p_2()
            .gap_1()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .px_1()
                    .gap_2()
                    .items_center()
                    .text_sm()
                    .child(div().font_medium().text_color(fg).child(changes_title(&self.files)))
                    .child(stats(added, removed, cx)),
            )
            .child(tree(&self.id, &self.files, select, None, None, cx));
        let diffs: Vec<AnyElement> = order
            .iter()
            .map(|&ix| {
                let file = &self.files[ix];
                let key = SharedString::from(format!("{}-{}", self.id, file.path));
                let raw = file.diffs.join("\n");
                let rows = cached_rows(cx, &key, &raw, || file_rows(&file.diffs));
                let on_open = self.on_open_file.clone().map(|open| {
                    let path = file.path.clone();
                    Rc::new(move |window: &mut Window, cx: &mut App| open(&path, window, cx))
                        as OnOpen
                });
                div()
                    .px_2()
                    .pt_2()
                    .child(
                        DiffView::new(key, file.path.clone().into(), rows, raw.into())
                            .file(abs_path(&file.path, self.cwd.as_deref()))
                            .stats(file.added, file.removed)
                            .on_open(on_open)
                            .uncapped(),
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                v_flex()
                    .id(self.id.clone())
                    .size_full()
                    .pb_2()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(list)
                    .children(diffs),
            )
            .vertical_scrollbar(&self.scroll)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/changes.rs"]
mod tests;
