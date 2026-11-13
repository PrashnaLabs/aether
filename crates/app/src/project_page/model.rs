use std::path::{Path, PathBuf};

use aether_core::{Ask, Message, Provider};
use aether_git::{Commit, Flow, Listed, Remote, Status, owner_repo};
use aether_ui::{ChatStatus, live_label};

use crate::projects::busy_label;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Filter {
    #[default]
    All,
    Running,
    NeedsYou,
    Done,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum GroupBy {
    #[default]
    None,
    Worktree,
    Status,
    Provider,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SortBy {
    #[default]
    Activity,
    Newest,
    Name,
    Status,
}

impl Filter {
    pub(crate) const ALL: [Self; 4] = [Self::All, Self::Running, Self::NeedsYou, Self::Done];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Running => "Running",
            Self::NeedsYou => "Needs you",
            Self::Done => "Done",
        }
    }
}

impl GroupBy {
    pub(crate) const ALL: [Self; 4] = [Self::None, Self::Worktree, Self::Status, Self::Provider];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Worktree => "Worktree",
            Self::Status => "Status",
            Self::Provider => "Provider",
        }
    }
}

impl SortBy {
    pub(crate) const ALL: [Self; 4] = [Self::Activity, Self::Newest, Self::Name, Self::Status];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Activity => "Recent activity",
            Self::Newest => "Newest",
            Self::Name => "Name",
            Self::Status => "Status",
        }
    }
}

#[derive(Default)]
pub(crate) struct ProjectPageState {
    pub filter: Filter,
    pub group: GroupBy,
    pub sort: SortBy,
    pub data: Option<ProjectData>,
    pub attachments: bool,
}

pub(crate) struct ProjectData {
    pub folder: PathBuf,
    pub status: Option<Status>,
    pub repo: bool,
    pub worktrees: Vec<Listed>,
    pub remotes: Vec<Remote>,
    pub commits: Vec<Commit>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fact {
    Path,
    Branch,
    Changes,
    Remote,
    Warn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CardStatus {
    Working,
    Waiting(&'static str),
    Failed,
    Done,
    Idle,
}

#[derive(Clone, Debug)]
pub(crate) struct Card {
    pub ix: usize,
    pub id: i64,
    pub title: String,
    pub provider: Provider,
    pub model: String,
    /// Thinking level; empty means the CLI default.
    pub effort: String,
    pub status: CardStatus,
    pub since_ms: i64,
    pub place: String,
    pub added: u32,
    pub removed: u32,
    /// Commits its branch is ahead of upstream.
    pub ahead: u32,
    pub activity_ms: i64,
    pub text: String,
    pub cwd: PathBuf,
    /// Runs in its own worktree; `place` is that worktree's branch.
    pub worktree: bool,
    /// Its worktree was removed or can't be found.
    pub gone: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Activity {
    Chat { ix: usize, title: String, verb: &'static str, at_ms: i64 },
    Commit { short: String, subject: String, at_ms: i64 },
}

impl Activity {
    pub(crate) fn at_ms(&self) -> i64 {
        match self {
            Self::Chat { at_ms, .. } | Self::Commit { at_ms, .. } => *at_ms,
        }
    }
}

/// `wait` is the kind of reply a waiting chat needs: Approve, Answer or Review.
pub(crate) fn card_status(status: ChatStatus, last_failed: bool, wait: &'static str) -> CardStatus {
    match status {
        ChatStatus::Starting
        | ChatStatus::Working { .. }
        | ChatStatus::Retrying
        | ChatStatus::Quiet { .. }
        | ChatStatus::Background(_) => CardStatus::Working,
        ChatStatus::Waiting => CardStatus::Waiting(wait),
        ChatStatus::Failed => CardStatus::Failed,
        ChatStatus::Done => CardStatus::Done,
        ChatStatus::Idle if last_failed => CardStatus::Failed,
        ChatStatus::Idle => CardStatus::Idle,
    }
}

pub(crate) fn status_rank(s: CardStatus) -> u8 {
    match s {
        CardStatus::Waiting(_) => 0,
        CardStatus::Failed => 1,
        CardStatus::Working => 2,
        CardStatus::Done => 3,
        CardStatus::Idle => 4,
    }
}

pub(crate) fn in_filter(f: Filter, s: CardStatus) -> bool {
    match f {
        Filter::All => true,
        Filter::Running => s == CardStatus::Working,
        Filter::NeedsYou => matches!(s, CardStatus::Waiting(_) | CardStatus::Failed),
        Filter::Done => matches!(s, CardStatus::Done | CardStatus::Idle),
    }
}

pub(crate) fn group_key(by: GroupBy, c: &Card) -> String {
    match by {
        GroupBy::None => String::new(),
        GroupBy::Worktree => c.place.clone(),
        GroupBy::Provider => c.provider.label().to_string(),
        GroupBy::Status => match c.status {
            CardStatus::Waiting(_) => "Needs you",
            CardStatus::Failed => "Failed",
            CardStatus::Working => "Running",
            CardStatus::Done | CardStatus::Idle => "Done",
        }
        .to_string(),
    }
}

pub(crate) fn arrange(
    cards: Vec<Card>,
    f: Filter,
    g: GroupBy,
    s: SortBy,
) -> Vec<(String, Vec<Card>)> {
    let mut cards: Vec<Card> = cards.into_iter().filter(|c| in_filter(f, c.status)).collect();
    match s {
        SortBy::Activity => cards.sort_by_key(|c| std::cmp::Reverse(c.activity_ms)),
        SortBy::Newest => cards.sort_by_key(|c| std::cmp::Reverse(c.id)),
        SortBy::Name => cards.sort_by_key(|c| c.title.to_lowercase()),
        SortBy::Status => {
            cards.sort_by_key(|c| (status_rank(c.status), std::cmp::Reverse(c.activity_ms)))
        }
    }
    if g == GroupBy::None {
        return vec![(String::new(), cards)];
    }
    let mut groups: Vec<(String, Vec<Card>)> = Vec::new();
    for c in cards {
        let key = group_key(g, &c);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(c),
            None => groups.push((key, vec![c])),
        }
    }
    if g == GroupBy::Status {
        groups.sort_by_key(|(_, v)| status_rank(v[0].status).min(3));
    }
    groups
}

pub(crate) fn last_text(msg: Option<&Message>, status: CardStatus) -> String {
    match status {
        CardStatus::Working => {
            msg.and_then(|m| live_label(&m.steps)).unwrap_or_else(|| "Working…".into())
        }
        CardStatus::Waiting(_) => msg
            .and_then(|m| m.pending_request())
            .map(|r| match &r.ask {
                Ask::Approval(a) => a.title.clone(),
                Ask::Question { question, .. } => question.clone(),
                Ask::Questions(q) => q.first().map(|q| q.question.clone()).unwrap_or_default(),
                Ask::Form(f) => f.title.clone(),
                Ask::Plan(_) => "Review plan".into(),
                Ask::Unknown => String::new(),
            })
            .unwrap_or_default(),
        CardStatus::Failed => msg
            .and_then(|m| {
                m.error.as_deref().or_else(|| m.text.rsplit_once("**Error:** ").map(|(_, e)| e))
            })
            .map(|e| e.lines().next().unwrap_or_default().trim().to_string())
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| "Failed".into()),
        CardStatus::Done | CardStatus::Idle => msg
            .and_then(|m| m.text.lines().map(str::trim).find(|l| !l.is_empty()))
            .map(|l| l.chars().take(140).collect())
            .unwrap_or_default(),
    }
}

pub(crate) fn card_aria(c: &Card, age: &str) -> String {
    let status = match c.status {
        CardStatus::Working => "Working",
        CardStatus::Waiting(l) => needs(l),
        CardStatus::Failed => "Failed",
        CardStatus::Done => "Done",
        CardStatus::Idle => "Idle",
    };
    [c.title.as_str(), status, c.place.as_str(), c.model.as_str(), age]
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn recent(chats: Vec<Activity>, commits: Vec<Activity>, limit: usize) -> Vec<Activity> {
    let mut commits = commits;
    commits.sort_by_key(|a| std::cmp::Reverse(a.at_ms()));
    commits.truncate(5);
    let mut all: Vec<Activity> = chats.into_iter().chain(commits).collect();
    all.sort_by_key(|a| std::cmp::Reverse(a.at_ms()));
    all.truncate(limit);
    all
}

pub(crate) fn verb(s: CardStatus) -> &'static str {
    match s {
        CardStatus::Working => "is working",
        CardStatus::Waiting("Answer") => "needs an answer",
        CardStatus::Waiting("Review") => "needs review",
        CardStatus::Waiting(_) => "needs approval",
        CardStatus::Failed => "failed",
        CardStatus::Done | CardStatus::Idle => "finished",
    }
}

fn needs(waiting: &str) -> &'static str {
    match waiting {
        "Answer" => "Needs an answer",
        "Review" => "Needs review",
        _ => "Needs approval",
    }
}

pub(crate) fn badge(s: CardStatus, since_ms: i64, now_ms: i64) -> Option<String> {
    match s {
        CardStatus::Working => Some(busy_label("Working", since_ms, now_ms)),
        CardStatus::Waiting(label) => Some(needs(label).into()),
        CardStatus::Failed => Some("Failed".into()),
        CardStatus::Done => Some("Done".into()),
        CardStatus::Idle => None,
    }
}

pub(crate) fn facts(
    path: String,
    missing: bool,
    data: Option<&ProjectData>,
) -> Vec<(Fact, String)> {
    let mut out = vec![(Fact::Path, path)];
    if missing {
        out.push((Fact::Warn, "Folder not found".into()));
        return out;
    }
    let Some(data) = data else { return out };
    if !data.repo {
        out.push((Fact::Warn, "not a git repository".into()));
        return out;
    }
    let branch = match &data.status {
        Some(s) => s.branch.clone().unwrap_or_else(|| "detached".into()),
        None => "\u{2014}".into(),
    };
    out.push((
        Fact::Branch,
        match data.worktrees.len() {
            0 => branch,
            1 => format!("{branch} · 1 worktree"),
            n => format!("{branch} · {n} worktrees"),
        },
    ));
    if data.status.is_some() {
        out.push((Fact::Changes, "uncommitted".into()));
    }
    let remote = data.remotes.iter().find(|r| r.name == "origin").or(data.remotes.first());
    out.push((
        Fact::Remote,
        remote.map_or_else(
            || "not published".into(),
            |r| owner_repo(&r.url).unwrap_or_else(|| r.name.clone()),
        ),
    ));
    out
}

pub(crate) fn tree_badges(cards: &[Card], trees: &[Listed], path: &Path) -> [Option<String>; 3] {
    let owner = |cwd: &Path| {
        trees
            .iter()
            .map(|t| t.path.as_path())
            .filter(|t| cwd.starts_with(t))
            .max_by_key(|t| t.components().count())
    };
    let here: Vec<CardStatus> =
        cards.iter().filter(|c| owner(&c.cwd) == Some(path)).map(|c| c.status).collect();
    let running = here.iter().filter(|s| **s == CardStatus::Working).count();
    let waiting = here.iter().filter(|s| matches!(s, CardStatus::Waiting(_))).count();
    let agents = match here.len() {
        1 => "1 agent".to_string(),
        n => format!("{n} agents"),
    };
    [
        (!here.is_empty()).then_some(agents),
        (running > 0).then(|| format!("{running} running")),
        (waiting > 0).then(|| format!("{waiting} waiting")),
    ]
}

pub(crate) fn tree_label(
    branch: &str,
    short: &str,
    missing: bool,
    badges: &[Option<String>; 3],
) -> String {
    let mut parts = vec![branch, short];
    if missing {
        parts.push("missing");
    }
    parts.extend(badges.iter().flatten().map(String::as_str));
    parts.join(", ")
}

pub(crate) fn new_here_aria(branch: &str) -> String {
    format!("New agent in {branch}")
}

pub(crate) fn activity_label(a: &Activity, time: &str) -> String {
    format!("{}, {time}", activity_text(a))
}

pub(crate) fn activity_text(a: &Activity) -> String {
    match a {
        Activity::Chat { title, verb, .. } => format!("{title} {verb}"),
        Activity::Commit { short, subject, .. } => format!("Committed {short} {subject}"),
    }
}

/// HEAD's commits as activity; upstream commits HEAD lacks aren't on its log.
pub(crate) fn commit_activity(commits: &[Commit]) -> Vec<Activity> {
    commits
        .iter()
        .filter(|c| c.flow != Some(Flow::Incoming))
        .map(|c| Activity::Commit {
            short: c.short.clone(),
            subject: c.subject.clone(),
            at_ms: c.time * 1000,
        })
        .collect()
}

pub(crate) fn filter_aria(f: Filter, n: usize) -> String {
    format!("{}, {n} agent{}", f.label(), if n == 1 { "" } else { "s" })
}

#[cfg(test)]
#[path = "../../tests/unit/project_page/model.rs"]
mod tests;
