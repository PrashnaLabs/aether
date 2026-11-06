use crate::theme::Corners;
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable, StyledExt, h_flex, input::InputState, spinner::Spinner,
    tooltip::Tooltip,
};
use gpui_kit::{
    Action, AnyElement, App, ClickEvent, ElementId, Entity, Hsla, SharedString, Window, div,
    prelude::*, px, rems,
};

use super::live;
use crate::components::ClickHandler;
use crate::theme;
use aether_core::{Provider, SidebarStyle};

mod chat_row;
mod chrome;
mod menu;
mod project_row;
mod render;
mod rows;
mod state;

use chrome::tabular;

pub use chat_row::{middle_truncate, project_color, project_mark, project_mark_sized};
pub use chrome::{count_badge, row_hover, row_style};
pub use rows::outside_aria;
pub use state::splice_range;

/// What a chat is doing, shown next to its title.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatStatus {
    Idle,
    /// Turn sent, nothing back yet.
    Starting,
    /// Turn running since `since_ms`.
    Working {
        since_ms: i64,
    },
    /// Paused until the user answers.
    Waiting,
    /// The provider is retrying or rate limited.
    Retrying,
    /// Running but silent since `since_ms`.
    Quiet {
        since_ms: i64,
    },
    /// Turn over; this many background tasks still run.
    Background(usize),
    /// Finished while the user was in another chat.
    Done,
    Failed,
}

impl ChatStatus {
    pub fn is_waiting(self) -> bool {
        matches!(self, Self::Waiting)
    }

    pub fn is_working(self) -> bool {
        matches!(self, Self::Starting | Self::Working { .. })
    }

    pub fn is_busy(self) -> bool {
        matches!(self, Self::Starting | Self::Working { .. } | Self::Waiting | Self::Retrying)
    }

    /// Words beside the mark; the clock is how long the state has held.
    pub fn text(&self, now_ms: i64) -> String {
        let clock = |since: i64| live::since(Some(since), now_ms).unwrap_or_default();
        match *self {
            Self::Idle => String::new(),
            Self::Starting => "Starting".into(),
            Self::Working { since_ms } => format!("Working · {}", clock(since_ms)),
            Self::Waiting => "Needs you".into(),
            Self::Retrying => "Retrying".into(),
            Self::Quiet { since_ms } => format!("No output {}", clock(since_ms)),
            Self::Background(n) => format!("{n} in background"),
            Self::Done => "Done".into(),
            Self::Failed => "Failed".into(),
        }
    }

    /// Stable words for screen readers; the clock would change every second.
    fn label(self) -> &'static str {
        match self {
            Self::Idle => "",
            Self::Starting => "starting",
            Self::Working { .. } => "working",
            Self::Waiting => "needs you",
            Self::Retrying => "retrying",
            Self::Quiet { .. } => "no output",
            Self::Background(_) => "in background",
            Self::Done => "new reply",
            Self::Failed => "failed",
        }
    }

    /// Mark beside the words; each state has its own shape, so color is never the only cue.
    /// Quiet is still running, so it keeps the spinner, dimmed, instead of an alert.
    fn mark(self, cx: &App) -> AnyElement {
        let icon = |name: Lucide, color: Hsla| {
            Icon::new(name).size(rems(0.875)).text_color(color).into_any_element()
        };
        match self {
            Self::Idle => div().into_any_element(),
            Self::Starting | Self::Working { .. } => {
                Spinner::new().xsmall().color(theme::mango_text(cx)).into_any_element()
            }
            Self::Waiting => icon(Lucide::CircleDot, theme::mango_text(cx)),
            Self::Failed => icon(Lucide::CircleX, theme::danger_text(cx)),
            Self::Done => icon(Lucide::CircleCheck, theme::info_text(cx)),
            Self::Background(_) => icon(Lucide::Layers, theme::info_text(cx)),
            Self::Quiet { .. } => {
                Spinner::new().xsmall().color(cx.theme().muted_foreground).into_any_element()
            }
            Self::Retrying => icon(Lucide::RefreshCw, theme::info_text(cx)),
        }
    }

    /// The mark, with the words as a tooltip only when they aren't beside it.
    pub fn badge(self, id: impl Into<ElementId>, now_ms: Option<i64>, cx: &App) -> AnyElement {
        let tip = now_ms.map(|now| SharedString::from(self.text(now)));
        let mango = theme::mango_text(cx);
        let pill = match self {
            Self::Working { since_ms } => {
                let timer = now_ms.and_then(|now| live::since(Some(since_ms), now));
                h_flex()
                    .gap_1()
                    .items_center()
                    .px_1p5()
                    .py_0p5()
                    .corner(px(4.))
                    .text_color(mango)
                    .font_features(tabular())
                    .child(Spinner::new().xsmall().color(mango))
                    .when_some(timer, |el, t| el.child(div().text_xs().child(t)))
                    .into_any_element()
            }
            Self::Waiting => h_flex()
                .gap_1()
                .items_center()
                .px_1p5()
                .py_0p5()
                .corner(px(4.))
                .text_color(mango)
                .child(Icon::new(Lucide::CircleDot).size(rems(0.75)))
                .child(div().text_xs().font_semibold().child("Needs you"))
                .into_any_element(),
            Self::Done => {
                let info = theme::info_text(cx);
                h_flex()
                    .gap_1()
                    .items_center()
                    .px_1()
                    .py_0p5()
                    .corner(px(4.))
                    .text_color(info)
                    .child(Icon::new(Lucide::CircleCheck).size(rems(0.75)))
                    .into_any_element()
            }
            Self::Failed => {
                let danger = theme::danger_text(cx);
                h_flex()
                    .gap_1()
                    .items_center()
                    .px_1()
                    .py_0p5()
                    .corner(px(4.))
                    .text_color(danger)
                    .child(Icon::new(Lucide::CircleX).size(rems(0.75)))
                    .into_any_element()
            }
            _ => self.mark(cx),
        };
        div()
            .id(id)
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .when_some(tip, |d, tip| {
                d.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            })
            .child(pill)
            .into_any_element()
    }
}

/// What an agent is doing in a chat, shown as words in the Agents section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    /// Running; holds the queued message count.
    Working(usize),
    Waiting(&'static str),
    Queued(usize),
    Failed,
    Done,
    Open,
}

impl AgentState {
    pub fn label(&self) -> String {
        match self {
            Self::Working(0) => "Working".into(),
            Self::Working(n) => format!("Working · {n} queued"),
            Self::Waiting(label) => (*label).into(),
            Self::Queued(n) => format!("Queued {n}"),
            Self::Failed => "Failed".into(),
            Self::Done => "Done".into(),
            Self::Open => "Idle · session open".into(),
        }
    }

    pub fn busy(&self) -> bool {
        matches!(self, Self::Working(_) | Self::Waiting(_))
    }

    fn color(&self, cx: &App) -> Hsla {
        match self {
            Self::Waiting(_) => theme::mango_text(cx),
            Self::Failed => theme::danger_text(cx),
            Self::Working(_) => cx.theme().foreground,
            _ => cx.theme().muted_foreground,
        }
    }
}

/// What an agent running outside aether is doing, from the last scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutsideState {
    Working,
    Waiting,
    Idle,
}

impl OutsideState {
    fn color(self, cx: &App) -> Hsla {
        match self {
            Self::Working => cx.theme().foreground,
            Self::Waiting => theme::mango_text(cx),
            Self::Idle => cx.theme().muted_foreground,
        }
    }
}

/// One row of the Outside aether section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutsideLine {
    /// Pid, else session; keeps focus on the same agent across rescans.
    pub id: SharedString,
    pub title: SharedString,
    pub provider: Provider,
    /// Folder · origin · elapsed.
    pub detail: SharedString,
    pub state: OutsideState,
    /// Short state words on the row, such as "Needs approval".
    pub label: SharedString,
    /// Hover text: full state, estimate flag, path.
    pub tip: SharedString,
}

/// Where an agent chat runs, on its last line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowPlace {
    /// Git checkout; `name` is the short commit when detached.
    Branch { name: SharedString, worktree: bool, detached: bool },
    /// Folder name, where the row sits outside its project group.
    Folder(SharedString),
    /// Inside its project group, in a folder that isn't a git repository.
    NoGit,
}

impl RowPlace {
    fn icon(&self) -> Lucide {
        match self {
            Self::Branch { worktree: true, .. } => Lucide::GitFork,
            Self::Branch { detached: true, .. } => Lucide::GitCommitHorizontal,
            Self::Branch { .. } => Lucide::GitBranch,
            Self::Folder(_) | Self::NoGit => Lucide::Folder,
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::Branch { name, worktree: true, detached: true } => {
                format!("Worktree, detached at {name}")
            }
            Self::Branch { name, worktree: true, .. } => format!("Worktree on {name}"),
            Self::Branch { name, detached: true, .. } => format!("Detached at {name}"),
            Self::Branch { name, .. } => format!("Branch {name}"),
            Self::Folder(name) => format!("In {name}"),
            Self::NoGit => "Not a git repository".into(),
        }
    }
}

/// Project, branch and provider lines of an agent chat row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatLine {
    pub provider: Provider,
    /// Project folder name; `None` shows "No project".
    pub project: Option<SharedString>,
    /// Model display name, for the hover card.
    pub model: SharedString,
    pub place: Option<RowPlace>,
    /// Relative time, or elapsed time while a turn runs; empty for a draft.
    pub time: SharedString,
    /// Full folder path: Reveal, Copy path, and the tooltip outside its group.
    pub path: SharedString,
    /// Its worktree was removed: the row dims, says so, and can't be revealed.
    pub removed: bool,
    /// Unique changed files in its folder, from the shared git cache.
    pub changed: usize,
    /// Added and deleted lines across staged and unstaged changes.
    pub added: u32,
    pub deleted: u32,
    /// Commits ahead of and behind its upstream; hidden where push hides.
    pub ahead_behind: Option<(u32, u32)>,
    /// Top-level rows still running in the background.
    pub background: usize,
}

impl ChatLine {
    /// The git chip opens that chat's Changes drawer. Removed rows and rows
    /// with nothing to show keep the chip display-only.
    pub fn status_clickable(&self) -> bool {
        if self.removed {
            return false;
        }
        let (ahead, behind) = self.ahead_behind.unwrap_or((0, 0));
        self.changed > 0 || ahead > 0 || behind > 0
    }
}

/// Header of one project group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectHeader {
    pub name: SharedString,
    pub path: SharedString,
    pub count: usize,
    pub collapsed: bool,
    /// Highest-priority status of its chats, shown while collapsed.
    pub status: ChatStatus,
    /// The folder no longer exists.
    pub missing: bool,
}

type RowHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
/// Builds the controls a section header carries, e.g. the Minimal filter menu.
pub type ToolsFactory = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;
type CollapseHandler = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

/// Long enough that the title likely truncates at the default sidebar width.
const LONG_TITLE: usize = 24;
/// Branch names past this keep both ends: `feature/…login-fix`.
const LONG_BRANCH: usize = 22;
/// Left inset, in rems, that puts the nested rows' guide under the folder icon.
const NEST: f32 = 0.9375;
pub const SIDEBAR_WIDTH: f32 = 255.;
/// Narrowest and widest the sidebar can be dragged.
pub const SIDEBAR_MIN_WIDTH: f32 = 200.;
pub const SIDEBAR_MAX_WIDTH: f32 = 480.;

type WatchHandler = Rc<dyn Fn(i64, &mut Window, &mut App)>;
type HandoffHandler = Rc<dyn Fn(i64, Provider, &mut Window, &mut App)>;
type PickHandler = Rc<dyn Fn(i64, &mut Window, &mut App)>;

/// A row's bulk-select state: picked, any row picked, toggle.
#[derive(Clone)]
struct Pick {
    picked: bool,
    picking: bool,
    on_toggle: PickHandler,
}
type ResizeHandler = Rc<dyn Fn(f32, &mut Window, &mut App)>;
type ResizeEndHandler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone)]
struct ChatItem {
    ix: usize,
    id: i64,
    title: SharedString,
    active: bool,
    status: ChatStatus,
    pinned: bool,
    watched: bool,
    child: bool,
    lead: bool,
    on_watch: Option<WatchHandler>,
    handoff: Option<(Vec<Provider>, HandoffHandler)>,
    line: Option<ChatLine>,
    on_click: RowHandler,
    on_rename: RowHandler,
    on_duplicate: RowHandler,
    on_pin: RowHandler,
    on_archive: RowHandler,
    on_delete: RowHandler,
    on_remove_worktree: Option<RowHandler>,
    on_status: Option<RowHandler>,
    /// Lives in the Archive view; its menu offers Unarchive.
    archived: bool,
    pick: Option<Pick>,
    now: i64,
}

struct AgentItem {
    project: Option<SharedString>,
    ix: usize,
    title: SharedString,
    provider: Provider,
    detail: SharedString,
    state: AgentState,
    on_click: RowHandler,
    on_stop: Option<RowHandler>,
}

struct OutsideItem {
    line: OutsideLine,
    on_click: RowHandler,
    on_stop: RowHandler,
}

struct ProjectItem {
    header: ProjectHeader,
    on_collapse: CollapseHandler,
    on_new: RowHandler,
    on_files: RowHandler,
    on_hide: RowHandler,
    on_page: RowHandler,
    chats: Vec<ChatItem>,
    worktrees: Vec<WorktreeItem>,
}

/// One worktree's chats inside a project group.
struct WorktreeItem {
    header: ProjectHeader,
    on_collapse: CollapseHandler,
    on_new: RowHandler,
    chats: Vec<ChatItem>,
}

struct SectionButtonItem {
    icon: Icon,
    label: SharedString,
    count: Option<usize>,
    on_click: RowHandler,
}

/// App sidebar: brand header with a new-chat button; live agents, agents
/// outside aether, pinned chats, agent chats grouped by folder and plain
/// chats.
#[derive(IntoElement)]
pub struct ChatSidebar {
    title: SharedString,
    mark: Option<SharedString>,
    pinned: Vec<ChatItem>,
    pinned_collapse: Option<(bool, CollapseHandler)>,
    projects: Vec<ProjectItem>,
    projects_collapse: Option<(bool, CollapseHandler)>,
    chats: Vec<ChatItem>,
    hidden: Option<(SharedString, RowHandler)>,
    agents: Vec<AgentItem>,
    agents_collapse: Option<(bool, CollapseHandler)>,
    outside: Vec<OutsideItem>,
    count: Option<usize>,
    empty: Option<SharedString>,
    on_new_chat: Option<ClickHandler>,
    on_back: Option<ClickHandler>,
    on_add_project: Option<RowHandler>,
    new_chat_action: Option<Box<dyn Action>>,
    on_collapse: Option<ClickHandler>,
    collapse_action: Option<Box<dyn Action>>,
    custom_sections: Vec<(SharedString, Vec<ChatItem>)>,
    style: SidebarStyle,
    archived: bool,
    top: Vec<SectionButtonItem>,
    section_tools: Option<ToolsFactory>,
    rename_ix: Option<usize>,
    rename_input: Option<Entity<InputState>>,
    width: f32,
    on_resize: Option<ResizeHandler>,
    on_resize_end: Option<ResizeEndHandler>,
    now: i64,
    watched: Vec<i64>,
    children: Vec<i64>,
    collapsed_crews: std::collections::HashSet<i64>,
    leads: Vec<i64>,
    on_watch: Option<WatchHandler>,
    handoff: Option<(Vec<Provider>, Vec<i64>, HandoffHandler)>,
    picks: Option<(Vec<i64>, PickHandler)>,
    bulk_bar: Option<AnyElement>,
}

impl ChatSidebar {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            mark: None,
            pinned: Vec::new(),
            pinned_collapse: None,
            projects: Vec::new(),
            projects_collapse: None,
            chats: Vec::new(),
            hidden: None,
            agents: Vec::new(),
            agents_collapse: None,
            outside: Vec::new(),
            count: None,
            empty: None,
            on_new_chat: None,
            on_back: None,
            on_add_project: None,
            new_chat_action: None,
            on_collapse: None,
            collapse_action: None,
            custom_sections: Vec::new(),
            style: SidebarStyle::Default,
            archived: false,
            top: Vec::new(),
            section_tools: None,
            rename_ix: None,
            rename_input: None,
            width: SIDEBAR_WIDTH,
            on_resize: None,
            on_resize_end: None,
            now: 0,
            watched: Vec::new(),
            children: Vec::new(),
            collapsed_crews: std::collections::HashSet::new(),
            leads: Vec::new(),
            on_watch: None,
            handoff: None,
            picks: None,
            bulk_bar: None,
        }
    }

    /// Adds Watch / Unwatch to chat menus; `watched` holds the watched chat ids.
    pub fn watching(
        mut self,
        watched: Vec<i64>,
        on_watch: impl Fn(i64, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.watched = watched;
        self.on_watch = Some(Rc::new(on_watch));
        self
    }

    /// Adds "Hand off to" to the menus of chats in `ids`, listing `targets` but the row's own agent.
    pub fn handoff(
        mut self,
        targets: Vec<Provider>,
        ids: Vec<i64>,
        on_pick: impl Fn(i64, Provider, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.handoff = Some((targets, ids, Rc::new(on_pick)));
        self
    }

    /// Chat ids drawn one level deeper, under their lead.
    pub fn child_agents(mut self, ids: Vec<i64>) -> Self {
        self.children = ids;
        self
    }

    /// Chat ids that orchestrate others; their rows carry a Lead marker.
    pub fn lead_agents(mut self, ids: Vec<i64>) -> Self {
        self.leads = ids;
        self
    }

    /// Unix ms the status clocks count to; the app passes one value for every row.
    pub fn now(mut self, now_ms: i64) -> Self {
        self.now = now_ms;
        self
    }

    /// Sets the sidebar width and the handlers for dragging its right edge:
    /// `on_resize` gets the desired width while dragging, `on_resize_end` the release.
    pub fn resizable(
        mut self,
        width: f32,
        on_resize: impl Fn(f32, &mut Window, &mut App) + 'static,
        on_resize_end: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.width = width;
        self.on_resize = Some(Rc::new(on_resize));
        self.on_resize_end = Some(Rc::new(on_resize_end));
        self
    }

    /// Adds an Agents row. The open chat isn't marked here: its own row is.
    pub fn agent(
        mut self,
        ix: usize,
        title: impl Into<SharedString>,
        provider: Provider,
        detail: SharedString,
        project: Option<SharedString>,
        state: AgentState,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_stop: Option<impl Fn(&ClickEvent, &mut Window, &mut App) + 'static>,
    ) -> Self {
        self.agents.push(AgentItem {
            project,
            ix,
            title: title.into(),
            provider,
            detail,
            state,
            on_click: Rc::new(on_click),
            on_stop: on_stop.map(|f| Rc::new(f) as RowHandler),
        });
        self
    }

    /// Adds an Outside aether row, for an agent running outside the app.
    pub fn outside_agent(
        mut self,
        line: OutsideLine,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_stop: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.outside.push(OutsideItem {
            line,
            on_click: Rc::new(on_click),
            on_stop: Rc::new(on_stop),
        });
        self
    }

    /// Count chip beside the header title, hidden at zero.
    pub fn count(mut self, count: usize) -> Self {
        self.count = (count > 0).then_some(count);
        self
    }

    /// Text shown when the body has no rows.
    pub fn empty(mut self, text: impl Into<SharedString>) -> Self {
        self.empty = Some(text.into());
        self
    }

    /// Adds a project group; chats join it by passing its index, in call order.
    /// `on_collapse` gets the wanted folded state.
    pub fn project(
        mut self,
        header: ProjectHeader,
        on_collapse: impl Fn(&bool, &mut Window, &mut App) + 'static,
        on_new: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_files: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_hide: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_page: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.projects.push(ProjectItem {
            header,
            on_collapse: Rc::new(on_collapse),
            on_new: Rc::new(on_new),
            on_files: Rc::new(on_files),
            on_hide: Rc::new(on_hide),
            on_page: Rc::new(on_page),
            chats: Vec::new(),
            worktrees: Vec::new(),
        });
        self
    }

    /// Adds a worktree container under project `project`; chats join it by
    /// passing its index, in call order. `on_collapse` gets the wanted
    /// folded state.
    pub fn worktree(
        mut self,
        project: usize,
        header: ProjectHeader,
        on_collapse: impl Fn(&bool, &mut Window, &mut App) + 'static,
        on_new: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        if project < self.projects.len() {
            self.projects[project].worktrees.push(WorktreeItem {
                header,
                on_collapse: Rc::new(on_collapse),
                on_new: Rc::new(on_new),
                chats: Vec::new(),
            });
        }
        self
    }

    /// Last Projects row, offering to bring removed groups back.
    pub fn hidden_projects(
        mut self,
        label: impl Into<SharedString>,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.hidden = Some((label.into(), Rc::new(on_click)));
        self
    }

    /// Adds a chat row; `id` keeps its scroll anchor and focus across reorders.
    /// Pinned chats go to Pinned; others to project `project` (an index from
    /// [`Self::project`]), its worktree `worktree` (an index from
    /// [`Self::worktree`]), or the plain Chats list. `line` makes it a
    /// two-line agent row. `on_remove_worktree` adds "Remove worktree…" to
    /// its menu.
    pub fn chat(
        mut self,
        ix: usize,
        id: i64,
        title: impl Into<SharedString>,
        active: bool,
        status: ChatStatus,
        pinned: bool,
        project: Option<usize>,
        worktree: Option<usize>,
        line: Option<ChatLine>,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_rename: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_duplicate: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_pin: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_archive: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_delete: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_remove_worktree: Option<ClickHandler>,
        on_status: Option<ClickHandler>,
    ) -> Self {
        let item = ChatItem {
            ix,
            id,
            title: title.into(),
            active,
            status,
            pinned,
            watched: self.watched.contains(&id),
            child: self.children.contains(&id),
            lead: self.leads.contains(&id),
            on_watch: self.on_watch.clone(),
            handoff: self.handoff.as_ref().and_then(|(targets, ids, pick)| {
                ids.contains(&id).then(|| (targets.clone(), pick.clone()))
            }),
            line,
            on_click: Rc::new(on_click),
            on_rename: Rc::new(on_rename),
            on_duplicate: Rc::new(on_duplicate),
            on_pin: Rc::new(on_pin),
            on_archive: Rc::new(on_archive),
            on_delete: Rc::new(on_delete),
            on_remove_worktree: on_remove_worktree.map(Rc::from),
            on_status: on_status.map(Rc::from),
            archived: false,
            pick: self.pick_for(id),
            now: 0,
        };
        match (project, worktree) {
            _ if pinned => self.pinned.push(item),
            (Some(k), Some(w))
                if k < self.projects.len() && w < self.projects[k].worktrees.len() =>
            {
                self.projects[k].worktrees[w].chats.push(item)
            }
            (Some(k), _) if k < self.projects.len() => self.projects[k].chats.push(item),
            _ => self.chats.push(item),
        }
        self
    }

    /// Shows the rename editor in place of chat `ix`'s title.
    pub fn renaming(mut self, ix: usize, input: &Entity<InputState>) -> Self {
        self.rename_ix = Some(ix);
        self.rename_input = Some(input.clone());
        self
    }

    pub fn on_new_chat(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_new_chat = Some(Box::new(handler));
        self
    }

    /// Footer button that leaves a sub view back to the project list.
    pub fn on_back(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_back = Some(Box::new(handler));
        self
    }

    /// Makes the entire Projects section collapsible without changing its groups.
    pub fn projects_collapsible(
        mut self,
        collapsed: bool,
        handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.projects_collapse = Some((collapsed, Rc::new(handler)));
        self
    }

    /// Makes the Pinned section collapsible.
    pub fn pinned_collapsible(
        mut self,
        collapsed: bool,
        handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.pinned_collapse = Some((collapsed, Rc::new(handler)));
        self
    }

    /// Makes the Agents section collapsible.
    pub fn agents_collapsible(
        mut self,
        collapsed: bool,
        handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.agents_collapse = Some((collapsed, Rc::new(handler)));
        self
    }

    /// Adds a "+" to the Projects heading.
    pub fn on_add_project(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_add_project = Some(Rc::new(handler));
        self
    }

    pub fn on_collapse(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_collapse = Some(Box::new(handler));
        self
    }

    /// Action whose shortcut the new-chat tooltip shows.
    pub fn new_chat_action(mut self, action: impl Action) -> Self {
        self.new_chat_action = Some(Box::new(action));
        self
    }

    /// Action whose shortcut the collapse tooltip shows.
    pub fn collapse_action(mut self, action: impl Action) -> Self {
        self.collapse_action = Some(Box::new(action));
        self
    }

    /// Row layout for chats in custom sections: Minimal or Focus.
    /// Rows added after this offer Unarchive instead of Archive.
    pub fn archived(mut self) -> Self {
        self.archived = true;
        self
    }

    /// Rows added after this show a checkbox on hover; `picked` are checked.
    pub fn pickable(
        mut self,
        picked: Vec<i64>,
        on_toggle: impl Fn(i64, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.picks = Some((picked, Rc::new(on_toggle)));
        self
    }

    /// Bulk actions pinned under the list, e.g. for picked rows.
    pub fn bulk_bar(mut self, bar: impl IntoElement) -> Self {
        self.bulk_bar = Some(bar.into_any_element());
        self
    }

    fn pick_for(&self, id: i64) -> Option<Pick> {
        let (picked, on_toggle) = self.picks.as_ref()?;
        Some(Pick {
            picked: picked.contains(&id),
            picking: !picked.is_empty(),
            on_toggle: on_toggle.clone(),
        })
    }

    pub fn style(mut self, style: SidebarStyle) -> Self {
        self.style = style;
        self
    }

    /// A full-width action row above everything else, e.g. New chat or Search.
    pub fn top_action(
        mut self,
        icon: impl Into<Icon>,
        label: impl Into<SharedString>,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.top.push(SectionButtonItem {
            icon: icon.into(),
            label: label.into(),
            count: None,
            on_click: Rc::new(on_click),
        });
        self
    }

    /// Controls on the first section's header.
    pub fn section_tools(
        mut self,
        tools: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.section_tools = Some(Rc::new(tools));
        self
    }

    /// Adds a chat row into a custom titled section (e.g. "Today", "Yesterday").
    pub fn chat_in_section(
        mut self,
        section: impl Into<SharedString>,
        ix: usize,
        id: i64,
        title: impl Into<SharedString>,
        active: bool,
        status: ChatStatus,
        pinned: bool,
        line: Option<ChatLine>,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_rename: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_duplicate: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_pin: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_archive: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_delete: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_remove_worktree: Option<ClickHandler>,
        on_status: Option<ClickHandler>,
    ) -> Self {
        let item = ChatItem {
            ix,
            id,
            title: title.into(),
            active,
            status,
            pinned,
            watched: self.watched.contains(&id),
            child: self.children.contains(&id),
            lead: self.leads.contains(&id),
            on_watch: self.on_watch.clone(),
            handoff: self.handoff.as_ref().and_then(|(targets, ids, pick)| {
                ids.contains(&id).then(|| (targets.clone(), pick.clone()))
            }),
            line,
            on_click: Rc::new(on_click),
            on_rename: Rc::new(on_rename),
            on_duplicate: Rc::new(on_duplicate),
            on_pin: Rc::new(on_pin),
            on_archive: Rc::new(on_archive),
            on_delete: Rc::new(on_delete),
            on_remove_worktree: on_remove_worktree.map(Rc::from),
            on_status: on_status.map(Rc::from),
            archived: self.archived,
            pick: self.pick_for(id),
            now: 0,
        };
        let section = section.into();
        if let Some((_, list)) = self.custom_sections.iter_mut().find(|(s, _)| s == &section) {
            list.push(item);
        } else {
            self.custom_sections.push((section, vec![item]));
        }
        self
    }
}

impl ChatSidebar {
    pub fn mark(mut self, glyph: impl Into<SharedString>) -> Self {
        self.mark = Some(glyph.into());
        self
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/chat/sidebar/mod.rs"]
mod tests;
