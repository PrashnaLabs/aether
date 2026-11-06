use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};

use aether_git::{Change, ChangeKind, Commit, Flow, GitOp, Listed, Stash, Stat};
use gpui_kit::SharedString;

use super::count;
use super::worktrees::tree_key;
use crate::app::format_full;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Section {
    Merge,
    Staged,
    Changes,
    Stashes,
    Worktrees,
    History,
}

/// A row of the changes list: a section caption with its item count, a
/// file by its index in `Status::changes`, a stash by its index in
/// `Status::stashes`, a worktree by its index in the repo's listing, a
/// history commit, or what stands in for unread history. The tree view
/// has folders, and files with their depth in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Row {
    Header(Section, usize),
    File(usize),
    Dir(Dir),
    TreeFile(usize, usize),
    Stash(usize),
    Worktree(usize),
    Commit(usize),
    HistoryNote,
}

/// Conflicts, staged files, then work-tree changes, each under its caption,
/// then stashes, the repo's `worktrees` when there's more than the main one
/// and, once there's a commit (`history` is `Some`), the history; collapsed
/// sections omit their items.
pub(super) fn rows(
    changes: &[Change],
    stashes: &[Stash],
    worktrees: usize,
    history: Option<&[Commit]>,
    collapsed: &HashSet<Section>,
) -> Vec<Row> {
    let mut rows = Vec::new();
    for section in [Section::Merge, Section::Staged, Section::Changes] {
        let files: Vec<Row> = changes
            .iter()
            .enumerate()
            .filter(|(_, c)| section_of(c) == section)
            .map(|(ix, _)| Row::File(ix))
            .collect();
        if !files.is_empty() {
            rows.push(Row::Header(section, files.len()));
            if !collapsed.contains(&section) {
                rows.extend(files);
            }
        }
    }
    if !stashes.is_empty() {
        rows.push(Row::Header(Section::Stashes, stashes.len()));
        if !collapsed.contains(&Section::Stashes) {
            rows.extend((0..stashes.len()).map(Row::Stash));
        }
    }
    if worktrees > 1 {
        rows.push(Row::Header(Section::Worktrees, worktrees));
        if !collapsed.contains(&Section::Worktrees) {
            rows.extend((0..worktrees).map(Row::Worktree));
        }
    }
    if let Some(commits) = history {
        rows.push(Row::Header(Section::History, commits.len()));
        if !collapsed.contains(&Section::History) {
            match commits.len() {
                0 => rows.push(Row::HistoryNote),
                n => rows.extend((0..n).map(Row::Commit)),
            }
        }
    }
    rows
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Dir {
    pub(super) section: Section,
    /// From the repo's top: `src/app/chat`.
    pub(super) path: String,
    /// Past its parent's row, lone subfolders joined on: `app/chat`.
    pub(super) name: String,
    pub(super) depth: usize,
    pub(super) collapsed: bool,
}

#[derive(Default)]
struct Node<'a> {
    dirs: BTreeMap<&'a str, Node<'a>>,
    files: Vec<(&'a str, usize)>,
}

/// [`rows`] with each section's files as a folder tree, VS Code style:
/// folders before files, each by name, a chain of lone subfolders on one
/// row, and what's inside `collapsed` folders left out.
pub(super) fn as_tree(
    rows: &[Row],
    changes: &[Change],
    collapsed: &HashSet<(Section, String)>,
) -> Vec<Row> {
    rows.chunk_by(|a, b| matches!((a, b), (Row::File(_), Row::File(_))))
        .flat_map(|run| {
            let [Row::File(first), ..] = run else {
                return run.to_vec();
            };
            let mut root = Node::default();
            for row in run {
                let Row::File(ix) = *row else { continue };
                let (name, dir) = split_path(&changes[ix].path);
                let node = dir
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .fold(&mut root, |node, part| node.dirs.entry(part).or_default());
                node.files.push((name, ix));
            }
            let mut out = Vec::new();
            push_tree(&root, "", 0, section_of(&changes[*first]), collapsed, &mut out);
            out
        })
        .collect()
}

fn push_tree(
    node: &Node,
    parent: &str,
    depth: usize,
    section: Section,
    collapsed: &HashSet<(Section, String)>,
    out: &mut Vec<Row>,
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
        let shut = collapsed.contains(&(section, path.clone()));
        out.push(Row::Dir(Dir { section, path: path.clone(), name, depth, collapsed: shut }));
        if !shut {
            push_tree(child, &path, depth + 1, section, collapsed, out);
        }
    }
    let mut files = node.files.clone();
    files.sort_by(|a, b| by_name(a.0, b.0));
    out.extend(files.into_iter().map(|(_, ix)| Row::TreeFile(ix, depth)));
}

fn by_name(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
}

/// `section`'s files under folder `dir`, where the tree puts them.
pub(super) fn files_under<'a>(
    changes: &'a [Change],
    section: Section,
    dir: &str,
) -> Vec<&'a Change> {
    let inside = |c: &Change| {
        let rest = split_path(&c.path).1.strip_prefix(dir);
        rest.is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    };
    changes.iter().filter(|c| section_of(c) == section && inside(c)).collect()
}

/// The change a folder takes its color from: conflicts, then deletes, edits,
/// renames and adds.
#[allow(dead_code)]
pub(super) fn loudest(files: &[&Change]) -> Option<ChangeKind> {
    files.iter().map(|c| c.kind).max_by_key(|kind| match kind {
        ChangeKind::Conflicted => 4,
        ChangeKind::Deleted => 3,
        ChangeKind::Modified | ChangeKind::TypeChanged => 2,
        ChangeKind::Renamed | ChangeKind::Copied => 1,
        ChangeKind::Added | ChangeKind::Untracked => 0,
    })
}

/// A folder's Stage, or Unstage in Staged Changes, by exact paths; a
/// staged rename takes its old path along, as its own row's does.
pub(super) fn stage_op(section: Section, files: &[&Change]) -> GitOp {
    match section {
        Section::Staged => GitOp::Unstage(
            files.iter().flat_map(|c| c.from.iter().cloned().chain([c.path.clone()])).collect(),
        ),
        _ => GitOp::Stage(files.iter().map(|c| c.path.clone()).collect()),
    }
}

/// A folder row in words: "src/app folder, 3 files. Press Enter to collapse".
pub(super) fn dir_label(dir: &Dir, files: usize) -> String {
    let verb = if dir.collapsed { "expand" } else { "collapse" };
    format!("{} folder, {}. Press Enter to {verb}", dir.path, count(files, "file", "files"))
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Fold {
    Toggle,
    Focus(usize),
}

/// What Left or Right does on tree row `ix`: close an open folder or open
/// a closed one, else move to the parent row or into an open folder.
pub(super) fn fold(rows: &[Row], ix: usize, key: &str) -> Option<Fold> {
    let (depth, open) = match rows.get(ix)? {
        Row::Dir(dir) => (dir.depth, Some(!dir.collapsed)),
        Row::TreeFile(_, depth) => (*depth, None),
        _ => return None,
    };
    match (key, open) {
        ("right", Some(false)) | ("left", Some(true)) => Some(Fold::Toggle),
        ("right", Some(true)) => (ix + 1 < rows.len()).then_some(Fold::Focus(ix + 1)),
        ("left", _) => rows[..ix]
            .iter()
            .rposition(|row| match row {
                Row::Dir(dir) => dir.depth < depth,
                Row::Header(..) => true,
                _ => false,
            })
            .map(Fold::Focus),
        _ => None,
    }
}

pub(super) fn section_of(change: &Change) -> Section {
    match (change.staged, change.kind) {
        (true, _) => Section::Staged,
        (false, ChangeKind::Conflicted) => Section::Merge,
        _ => Section::Changes,
    }
}

pub(super) fn row_key(
    row: &Row,
    changes: &[Change],
    stashes: &[Stash],
    trees: &[Listed],
    commits: &[Commit],
) -> SharedString {
    match *row {
        Row::Header(Section::Merge, _) => "merge".into(),
        Row::Header(Section::Staged, _) => "staged".into(),
        Row::Header(Section::Changes, _) => "changes".into(),
        Row::Header(Section::Stashes, _) => "stashes".into(),
        Row::Header(Section::Worktrees, _) => "worktrees".into(),
        Row::Header(Section::History, _) => "history".into(),
        Row::File(ix) | Row::TreeFile(ix, _) => file_key(&changes[ix]),
        Row::Dir(ref dir) => {
            let section = row_key(&Row::Header(dir.section, 0), changes, stashes, trees, commits);
            format!("{section}:{}", dir.path).into()
        }
        Row::Stash(ix) => stash_key(&stashes[ix]),
        Row::Worktree(ix) => tree_key(&trees[ix]),
        Row::Commit(ix) => commit_key(&commits[ix]),
        Row::HistoryNote => "history-note".into(),
    }
}

pub(super) fn stash_key(stash: &Stash) -> SharedString {
    format!("stash-{}", stash.sha).into()
}

pub(super) fn commit_key(commit: &Commit) -> SharedString {
    format!("commit-{}", commit.sha).into()
}

pub(super) fn section_title(section: Section) -> &'static str {
    match section {
        Section::Merge => "Merge Changes",
        Section::Staged => "Staged Changes",
        Section::Changes => "Changes",
        Section::Stashes => "Stashes",
        Section::Worktrees => "Worktrees",
        Section::History => "History",
    }
}

/// A section caption in words: "Stashes, 2 stashes. Press Enter to collapse".
/// Unread history has no count yet.
pub(super) fn header_label(section: Section, n: usize, collapsed: bool) -> String {
    let what = match section {
        Section::History if n == 0 => String::new(),
        Section::History => format!(", {}", count(n, "commit", "commits")),
        Section::Stashes => format!(", {}", count(n, "stash", "stashes")),
        Section::Worktrees => format!(", {}", count(n, "worktree", "worktrees")),
        _ => format!(", {}", count(n, "file", "files")),
    };
    let verb = if collapsed { "expand" } else { "collapse" };
    format!("{}{what}. Press Enter to {verb}", section_title(section))
}

/// A stash row in words; `age` is how long ago it was made.
pub(super) fn stash_label(stash: &Stash, age: &str) -> String {
    let on = if stash.branch.is_empty() { String::new() } else { format!(", on {}", stash.branch) };
    format!(
        "{}{on}, {age}. Press Enter to open it, Space to apply it, Delete to drop it",
        stash.message
    )
}

/// A history row in words, with where the commit is relative to the upstream.
pub(super) fn commit_label(commit: &Commit, age: &str) -> String {
    let flow = match commit.flow {
        Some(Flow::Outgoing) => ", outgoing",
        Some(Flow::Incoming) => ", incoming",
        None => "",
    };
    format!("{}, {}, {age}{flow}", commit.subject, commit.author)
}

pub(super) fn file_key(change: &Change) -> SharedString {
    format!("{}:{}", if change.staged { "s" } else { "c" }, change.path).into()
}

pub(super) fn key_path(key: &str) -> Option<&str> {
    key.split_once(':').map(|(_, path)| path)
}

/// Files with any change, staged or not, each counted once.
pub(crate) fn changed_files(changes: &[Change]) -> usize {
    changes.iter().map(|c| c.path.as_str()).collect::<HashSet<_>>().len()
}

/// File name and the folder it's in.
pub(super) fn split_path(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((dir, name)) if !name.is_empty() => (name, dir),
        _ => (path, ""),
    }
}

/// Where an open diff's file is after a refresh: the same row, else the same
/// file in the other section (it was just staged or unstaged).
pub(super) fn follow(changes: &[Change], key: &str, path: &str) -> Option<usize> {
    changes
        .iter()
        .position(|c| file_key(c) == key)
        .or_else(|| changes.iter().position(|c| c.path == path))
}

/// The row an arrow, Home or End key moves to.
pub(crate) fn step(from: usize, len: usize, key: &str) -> Option<usize> {
    match key {
        "up" => from.checked_sub(1),
        "down" => (from + 1 < len).then_some(from + 1),
        "home" => (len > 0).then_some(0),
        "end" => len.checked_sub(1),
        _ => None,
    }
}

/// Who a change is for in words: "Modified, staged", "Renamed from a.rs".
pub(super) fn describe(change: &Change) -> String {
    let mut text = change.kind.label().to_string();
    if let Some(from) = &change.from {
        text.push_str(&format!(" from {from}"));
    }
    text.push_str(match (change.kind, change.staged) {
        (ChangeKind::Untracked | ChangeKind::Conflicted, _) => "",
        (_, true) => ", staged",
        (_, false) => ", not staged",
    });
    text
}

pub(super) fn stat_words(stat: Stat) -> Option<String> {
    match stat {
        Stat::Lines(added, removed) => Some(format!("{added} added, {removed} removed")),
        Stat::Binary => Some("binary".into()),
        Stat::Unknown => None,
    }
}

/// Discarding it deletes the file: untracked, or added with `git add -N`.
pub(super) fn discard_deletes(change: &Change) -> bool {
    change.kind == ChangeKind::Untracked || (change.kind == ChangeKind::Added && !change.staged)
}

/// [`discard_prompt`] for the files under folder `dir`.
pub(super) fn discard_dir_prompt(dir: &str, changes: &[Change]) -> (String, String, &'static str) {
    let (title, text, ok) = discard_prompt(changes);
    match changes {
        [_] => (title, text, ok),
        _ => {
            let n = format_full(changes.len() as u64);
            (format!("Discard all {n} changes in {dir}?"), text, ok)
        }
    }
}

/// Title, text and button of the confirmation before discarding `changes`.
pub(super) fn discard_prompt(changes: &[Change]) -> (String, String, &'static str) {
    let undone = "This can't be undone.";
    let untracked = changes.iter().filter(|c| discard_deletes(c)).count();
    let tracked = changes.len() - untracked;
    match changes {
        [one] if one.kind == ChangeKind::Untracked => {
            (format!("Delete untracked file {}?", one.path), undone.into(), "Delete file")
        }
        [one] if untracked == 1 => {
            (format!("Delete file {}?", one.path), undone.into(), "Delete file")
        }
        [one] => (format!("Discard changes in {}?", one.path), undone.into(), "Discard changes"),
        _ => {
            let lost = format!("Changes to {} will be lost", count(tracked, "file", "files"));
            let deleted = count(untracked, "untracked file", "untracked files");
            let lead = match (tracked, untracked) {
                (_, 0) => format!("{lost}."),
                (0, _) => format!("{deleted} will be deleted."),
                _ => format!("{lost} and {deleted} deleted."),
            };
            let title = format!("Discard all {} changes?", format_full(changes.len() as u64));
            (title, format!("{lead} {undone}"), "Discard all")
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/git_panel/rows.rs"]
mod tests;
