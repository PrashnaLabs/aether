//! Changes panel: a right drawer with the active chat folder's git changes,
//! one file's diff at a time, staging, commit and push. Git runs on the
//! agent's blocking pool; the panel only paints the last result.

use crate::session_workspace::WindowId;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use aether_git::{Change, ChangeKind, GitOp, Status, StatusError};
use aether_ui::{icon_chip, theme};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::component::{Selectable, v_flex};
use gpui_kit::{
    AnyElement, Context, Entity, FocusHandle, KeyDownEvent, ListAlignment, ListState, SharedString,
    Subscription, UniformListScrollHandle, Window, div, prelude::*, px,
};

use crate::actions::ToggleSidePanel;
use crate::app::{ChatApp, Page, format_full, now_ms};

mod commit;
mod diff;
mod history;
mod list;
mod picker;
mod rows;
mod stash;
mod status;
mod sync;
mod widgets;
mod worktrees;

use diff::{Diff, Target};
use history::History;
use picker::Picker;
pub(crate) use picker::{PickMode, PickRow, first_pick, pick_rows};
use rows::{Row, Section, as_tree, files_under, row_key, rows};
pub(crate) use rows::{changed_files, step};
pub(crate) use sync::{Push, push_state, remote_ops};
pub(crate) use widgets::note;
use worktrees::{TreeJob, Trees};

/// Sidebar totals: unique changed files, added lines, deleted lines, and
/// ahead/behind, so N chats in one worktree paint from 1 git call.
/// Ahead/behind follows the drawer's push rules: hidden with no commit yet
/// or nowhere to push to.
pub(crate) fn sidebar_status_summary(status: &Status) -> (usize, u32, u32, Option<(u32, u32)>) {
    let ahead_behind = match push_state(status) {
        Push::Hidden => None,
        _ => status.ahead_behind,
    };
    let (added, removed) = crate::workspace_popover::line_totals(&status.changes);
    (changed_files(&status.changes), added, removed, ahead_behind)
}

/// Cached sidebar counts for `folder`: `None` until a good read exists.
pub(crate) fn sidebar_cached_summary(
    statuses: &HashMap<PathBuf, Loaded>,
    folder: &Path,
) -> Option<(usize, u32, u32, Option<(u32, u32)>)> {
    let status = statuses.get(folder)?.status.as_ref().ok()?;
    Some(sidebar_status_summary(status))
}

/// Deduped folders needing a sidebar git read: no good read yet and none
/// in flight. Transient `Locked`/`Failed` reads retry; `NotRepo` stays
/// cached so non-repos don't cost a git call per refresh.
pub(crate) fn sidebar_pending(
    statuses: &HashMap<PathBuf, Loaded>,
    inflight: &HashSet<PathBuf>,
    folders: &[PathBuf],
) -> Vec<PathBuf> {
    let mut missing: Vec<PathBuf> = folders
        .iter()
        .filter(|f| {
            !inflight.contains(*f)
                && match statuses.get(*f) {
                    None => true,
                    Some(loaded) => {
                        matches!(
                            loaded.status,
                            Err(StatusError::Locked) | Err(StatusError::Failed(_))
                        )
                    }
                }
        })
        .cloned()
        .collect();
    missing.sort();
    missing.dedup();
    missing
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", format_full(n as u64), if n == 1 { one } else { many })
}

pub(crate) struct Loaded {
    pub(crate) status: Result<Status, StatusError>,
    /// When it was read, in ms.
    pub(crate) at: i64,
}

struct Job {
    op: GitOp,
    /// Git finished; the job shows until the refresh after it lands.
    done: bool,
}

/// How the last commit, push or staging went, in the folder it ran in.
struct Feedback {
    folder: Option<PathBuf>,
    error: bool,
    text: SharedString,
    detail: Option<SharedString>,
    /// A fetch, pull, push or sync, which may need the terminal.
    remote: bool,
}

struct OpenDiff {
    folder: PathBuf,
    key: SharedString,
    target: Target,
    diff: Option<Result<Diff, SharedString>>,
    /// The user asked to see a diff past the size limit.
    full: bool,
    scroll: UniformListScrollHandle,
}

pub(crate) struct GitPanel {
    /// Chats whose panel is open.
    pub(crate) open: HashSet<i64>,
    /// Last read per chat folder.
    statuses: HashMap<PathBuf, Loaded>,
    /// Sidebar folders with a read in flight; rapid refreshes share it.
    sidebar_loading: HashSet<PathBuf>,
    /// Folder being read; one read at a time.
    loading: Option<PathBuf>,
    /// Asked to read again while a read ran.
    again: bool,
    /// The running read came from the refresh button, which spins for it.
    manual: bool,
    /// Bumped whenever a command changes the repo; reads begun before it are stale.
    generation: u64,
    lock_retries: u8,
    job: Option<Job>,
    feedback: Option<Feedback>,
    diff: Option<OpenDiff>,
    diff_run: u64,
    diff_focus: FocusHandle,
    rows: Vec<Row>,
    keys: Vec<SharedString>,
    list: ListState,
    focus: HashMap<SharedString, FocusHandle>,
    /// Row keyboard focus last sat on; the list's single tab stop.
    cursor: Option<SharedString>,
    tab_stop: Option<SharedString>,
    /// Move focus into the panel on the next frame, once its rows exist.
    pub(crate) refocus: bool,
    root: FocusHandle,
    message: Entity<TextareaState>,
    /// Unsent commit messages of other folders, and the folder `message` is for.
    drafts: HashMap<PathBuf, String>,
    draft_for: Option<PathBuf>,
    collapsed: HashSet<Section>,
    /// Closed tree view folders, by section and path.
    collapsed_dirs: HashSet<(Section, String)>,
    /// Last history read per chat folder; read only while History is open.
    history: HashMap<PathBuf, History>,
    /// Last worktree listing per chat folder.
    trees: HashMap<PathBuf, Trees>,
    tree_job: Option<TreeJob>,
    picker: Option<Picker>,
    /// The branch picker's field, made on first open, and its change listener.
    query: Option<(Entity<InputState>, Subscription)>,
}

impl GitPanel {
    pub(crate) fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub(crate) fn is_refreshing(&self) -> bool {
        self.manual && self.loading.is_some()
    }

    pub(crate) fn invalidate(&mut self, folder: &Path) {
        self.generation += 1;
        self.statuses.remove(folder);
    }

    pub(crate) fn new(message: Entity<TextareaState>, cx: &mut Context<ChatApp>) -> Self {
        Self {
            open: HashSet::new(),
            statuses: HashMap::new(),
            sidebar_loading: HashSet::new(),
            loading: None,
            again: false,
            manual: false,
            generation: 0,
            lock_retries: 0,
            job: None,
            feedback: None,
            diff: None,
            diff_run: 0,
            diff_focus: cx.focus_handle(),
            rows: Vec::new(),
            keys: Vec::new(),
            list: ListState::new(0, ListAlignment::Top, px(200.)),
            focus: HashMap::new(),
            cursor: None,
            tab_stop: None,
            refocus: false,
            root: cx.focus_handle(),
            message,
            drafts: HashMap::new(),
            draft_for: None,
            collapsed: HashSet::from([Section::History]),
            collapsed_dirs: HashSet::new(),
            history: HashMap::new(),
            trees: HashMap::new(),
            tree_job: None,
            picker: None,
            query: None,
        }
    }

    /// For chat delete.
    pub(crate) fn forget(&mut self, chat: i64) {
        self.open.remove(&chat);
    }

    /// Sidebar row counts for `folder`: `None` until it has a good read.
    pub(crate) fn sidebar_git_summary(&self, folder: &Path) -> Option<(usize, u32, u32, Option<(u32, u32)>)> {
        sidebar_cached_summary(&self.statuses, folder)
    }

    /// Deduped folders with no good read yet, claimed in flight so rapid
    /// refreshes share 1 git call per folder.
    pub(crate) fn sidebar_missing(&mut self, folders: &[PathBuf]) -> Vec<PathBuf> {
        let missing = sidebar_pending(&self.statuses, &self.sidebar_loading, folders);
        self.sidebar_loading.extend(missing.iter().cloned());
        missing
    }

    /// A sidebar background read landed; the panel paints it like its own.
    /// The folder leaves the in-flight set either way, so transient
    /// failures retry on the next refresh.
    pub(crate) fn insert_sidebar_status(
        &mut self,
        folder: PathBuf,
        status: Result<Status, StatusError>,
    ) {
        self.sidebar_loading.remove(&folder);
        self.statuses.insert(folder, Loaded { status, at: now_ms() });
    }

    fn busy_with(&self, change: &Change) -> bool {
        match self.job.as_ref().map(|job| &job.op) {
            Some(GitOp::Stage(paths)) => !change.staged && paths.contains(&change.path),
            Some(GitOp::Unstage(paths)) => change.staged && paths.contains(&change.path),
            Some(GitOp::StageAll | GitOp::DiscardAll(_)) => {
                !change.staged && change.kind != ChangeKind::Conflicted
            }
            Some(GitOp::UnstageAll) => change.staged,
            Some(GitOp::Discard(paths)) => !change.staged && paths.contains(&change.path),
            _ => false,
        }
    }
}

impl ChatApp {
    /// Ctrl+Shift+G / Cmd+Shift+G and the header button. From the keyboard, an
    /// open panel without focus takes it instead of closing.
    pub(crate) fn toggle_changes(
        &mut self,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != Page::Chat
            || !self.is_agent_chat(&self.chats[self.active])
            || self.git_repo().is_none()
        {
            return;
        }
        if self.chats[self.active].worktree_removed() || self.worktree_missing(self.active) {
            return;
        }
        let open = self.changes_view_shown(self.host_of(window));
        if open && (!keyboard || self.git.root.contains_focused(window, cx)) {
            return self.close_changes(window, cx);
        }
        self.open_explorer_view(crate::explorer::ExplorerView::Changes, window, cx);
    }

    pub(crate) fn close_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.git.open.remove(&self.chats[self.active].id);
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Header shortcut to the Changes tab, with changed-file status.
    pub(crate) fn side_button(&self, host: WindowId, cx: &mut Context<Self>) -> AnyElement {
        let open = self.changes_view_shown(host);
        let files = self.git_status().map_or(0, |s| changed_files(&s.changes));
        let label = match (open, files) {
            (true, _) => "Return to agent".to_string(),
            (false, 0) => "Show changes".to_string(),
            (false, n) => format!("Show changes, {} changed", count(n, "file", "files")),
        };
        let tip = match files {
            0 => "Changes".to_string(),
            n => format!("Changes · {} changed", count(n, "file", "files")),
        };
        icon_chip("side-toggle", open, cx)
            .selected(open)
            .toggled(open)
            .icon(Lucide::GitBranch)
            .when(files > 0, |b| {
                b.child(
                    div()
                        .absolute()
                        .top(px(4.))
                        .right(px(4.))
                        .size(px(6.))
                        .rounded_full()
                        .bg(theme::success_text(cx)),
                )
            })
            .accessibility_label(label)
            .tooltip_with_action(tip, &ToggleSidePanel, None)
            .on_click(cx.listener(|this, _, window, cx| this.toggle_side(window, cx)))
            .into_any_element()
    }

    /// The list's rows and their keys for the active folder.
    fn git_listing(&self) -> (Vec<Row>, Vec<SharedString>) {
        let Some(status) = self.git_status() else {
            return (Vec::new(), Vec::new());
        };
        let (commits, trees) = (self.history_commits(), self.trees());
        let history = status.commit.is_some().then_some(commits);
        let rows =
            rows(&status.changes, &status.stashes, trees.len(), history, &self.git.collapsed);
        let rows = match self.settings.git_tree_view {
            true => as_tree(&rows, &status.changes, &self.git.collapsed_dirs),
            false => rows,
        };
        let keys = rows
            .iter()
            .map(|row| row_key(row, &status.changes, &status.stashes, trees, commits))
            .collect();
        (rows, keys)
    }

    /// The changes tab's content: header, list or diff, commit area. `None`
    /// outside a repo.
    /// The selected change's, commit's or stash's diff for the section beside the
    /// sidebar, or a note to pick one.
    pub(crate) fn git_diff_pane(&self, cx: &mut Context<Self>) -> AnyElement {
        use gpui_kit::component::ActiveTheme as _;
        let folder = self.git_folder();
        if self.git.diff.as_ref().is_some_and(|d| Some(&d.folder) == folder.as_ref()) {
            return v_flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(self.git_diff_view(cx))
                .into_any_element();
        }
        note(
            Lucide::FileDiff,
            cx.theme().muted_foreground,
            "No change selected",
            "Pick a changed file to see its diff here.",
            cx,
        )
        .h_full()
        .into_any_element()
    }

    pub(crate) fn git_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.git_repo()?;
        self.sync_history(cx);
        self.sync_trees(cx);
        let mut shut = std::mem::take(&mut self.git.collapsed_dirs);
        if let Some(status) = self.git_status() {
            shut.retain(|(section, dir)| !files_under(&status.changes, *section, dir).is_empty());
        }
        self.git.collapsed_dirs = shut;
        let (rows, keys) = self.git_listing();
        self.sync_git_rows(rows, keys, window, cx);
        if self.git_folder() != self.git.draft_for {
            let view = cx.entity();
            window.defer(cx, move |window, cx| {
                view.update(cx, |this, cx| this.swap_commit_draft(window, cx))
            });
        }
        let body = self.git_body(cx);
        let panel = v_flex()
            .id("git-panel")
            .track_focus(&self.git.root)
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                // only when the panel itself holds focus, as after its last row goes
                if !this.git.root.is_focused(window) || ev.keystroke.modifiers.modified() {
                    return;
                }
                match ev.keystroke.key.as_str() {
                    "escape" => this.close_changes(window, cx),
                    "down" | "home" | "up" | "end" => {
                        let target =
                            this.git.tab_stop.as_ref().and_then(|k| this.git.focus.get(k)).cloned();
                        if let Some(handle) = target {
                            window.focus(&handle, cx);
                        }
                    }
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .children(self.git_branch_bar(cx))
            .children(self.git_commit_area(cx))
            .child(body);
        Some(panel.into_any_element())
    }
}

#[cfg(test)]
use aether_git::Stat;

#[cfg(test)]
pub(super) fn change(path: &str, kind: ChangeKind, staged: bool) -> Change {
    Change { path: path.into(), from: None, kind, staged, stat: Stat::Unknown }
}

#[cfg(test)]
#[path = "../../tests/unit/git_panel/mod.rs"]
mod tests;
