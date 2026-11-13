use std::collections::HashMap;
use std::fmt::Display;
use std::path::{Path, PathBuf};

use aether_core::Settings;
use aether_ui::{ChatStatus, ChoiceOption};
use chrono::{Datelike, TimeZone};
use gpui_kit::assets::IconName;

/// Agent chats that share a folder; one sidebar group.
#[derive(Debug, PartialEq)]
pub(crate) struct Project {
    pub folder: PathBuf,
    /// Chat indexes: newest first from `group_projects`, by title once `project_groups` sorts.
    pub chats: Vec<usize>,
}

/// Groups `(chat index, folder, last activity ms)` by folder. The group
/// holding the newest chat comes first.
pub(crate) fn group_projects(mut chats: Vec<(usize, PathBuf, i64)>) -> Vec<Project> {
    chats.sort_by_key(|c| std::cmp::Reverse((c.2, c.0)));
    let mut groups: Vec<Project> = Vec::new();
    let mut slot: HashMap<PathBuf, usize> = HashMap::new();
    for (ix, folder, _) in chats {
        match slot.get(&folder) {
            Some(&g) => groups[g].chats.push(ix),
            None => {
                slot.insert(folder.clone(), groups.len());
                groups.push(Project { folder, chats: vec![ix] });
            }
        }
    }
    groups
}

/// Prefixes the parent folder to names two groups share: `client/app`, `server/app`.
pub(crate) fn disambiguate(names: &mut [String], folders: &[PathBuf]) {
    let shared: Vec<bool> =
        names.iter().map(|name| names.iter().filter(|other| *other == name).count() > 1).collect();
    for ((name, folder), shared) in names.iter_mut().zip(folders).zip(shared) {
        if let Some(parent) = folder.parent().and_then(Path::file_name).filter(|_| shared) {
            *name = format!("{}/{name}", parent.to_string_lossy());
        }
    }
}

/// What a folded group shows: needs you, quiet, retrying, working, failed, background, new reply.
pub(crate) fn rollup(statuses: impl IntoIterator<Item = ChatStatus>) -> ChatStatus {
    statuses
        .into_iter()
        .max_by_key(|status| match status {
            ChatStatus::Waiting => 8,
            ChatStatus::Quiet { .. } => 7,
            ChatStatus::Retrying => 6,
            ChatStatus::Working { .. } => 5,
            ChatStatus::Starting => 4,
            ChatStatus::Failed => 3,
            ChatStatus::Background(_) => 2,
            ChatStatus::Done => 1,
            ChatStatus::Idle => 0,
        })
        .unwrap_or(ChatStatus::Idle)
}

/// Age of `at_ms` as seen at `now_ms`: now, 5m, 3h, Yesterday, Oct 12,
/// with the year added once it differs.
pub(crate) fn relative_time<Tz: TimeZone>(at_ms: i64, now_ms: i64, tz: &Tz) -> String
where
    Tz::Offset: Display,
{
    let age = now_ms.saturating_sub(at_ms);
    if age < 86_400_000 {
        return span(age).unwrap_or_else(|| "now".into());
    }
    let day = |ms| tz.timestamp_millis_opt(ms).single().map(|t| t.date_naive());
    let (Some(at), Some(today)) = (day(at_ms), day(now_ms)) else {
        return String::new();
    };
    if today.pred_opt() == Some(at) {
        "Yesterday".into()
    } else if at.year() == today.year() {
        at.format("%b %-d").to_string()
    } else {
        at.format("%b %-d, %Y").to_string()
    }
}

/// Whole minutes, hours or days in `ms`; `None` under a minute.
fn span(ms: i64) -> Option<String> {
    let minutes = ms / 60_000;
    match minutes {
        ..1 => None,
        1..60 => Some(format!("{minutes}m")),
        60..1440 => Some(format!("{}h", minutes / 60)),
        _ => Some(format!("{}d", minutes / 1440)),
    }
}

/// `"Working · 2m"` for a turn running since `since_ms`; just the verb under a minute.
pub(crate) fn busy_label(verb: &str, since_ms: i64, now_ms: i64) -> String {
    match span(now_ms.saturating_sub(since_ms)) {
        Some(span) => format!("{verb} · {span}"),
        None => verb.into(),
    }
}

pub(crate) const MAX_PINNED_PROJECTS: usize = 4;
pub(crate) const MAX_PINNED_CHATS: usize = 3;

pub(crate) fn listed(list: &[String], folder: &Path) -> bool {
    list.iter().any(|key| Path::new(key) == folder)
}

/// Puts `folder` in a saved path list, or takes it out.
pub(crate) fn set_listed(list: &mut Vec<String>, folder: &Path, on: bool) {
    list.retain(|key| Path::new(key) != folder);
    if on {
        list.push(folder.display().to_string());
    }
}

/// Pins `folder` or unpins it, first dropping pins of folders not in `shown`.
/// False, changing nothing, when pinning past the cap.
pub(crate) fn toggle_pin(pins: &mut Vec<String>, shown: &[PathBuf], folder: &Path) -> bool {
    let mut next = pins.clone();
    next.retain(|p| shown.iter().any(|f| f == Path::new(p)));
    let on = !listed(&next, folder);
    if on && next.len() >= MAX_PINNED_PROJECTS {
        return false;
    }
    set_listed(&mut next, folder, on);
    *pins = next;
    true
}

/// One row of the project switcher.
#[derive(Debug, PartialEq)]
pub(crate) struct ProjectEntry {
    pub folder: PathBuf,
    pub last_activity: Option<i64>,
}

fn lower_name(folder: &Path) -> String {
    folder.file_name().unwrap_or_default().to_string_lossy().to_lowercase()
}

/// Saved folders not in `taken`, by lowercase folder name.
fn saved_only(saved: &[String], taken: &[&Path]) -> Vec<PathBuf> {
    let mut rest: Vec<PathBuf> = Vec::new();
    for f in saved.iter().map(PathBuf::from) {
        if !taken.contains(&f.as_path()) && !rest.contains(&f) {
            rest.push(f);
        }
    }
    rest.sort_by_key(|f| (lower_name(f), f.clone()));
    rest
}

/// Folders with chats (newest first), then saved folders without, minus hidden ones.
pub(crate) fn project_list(
    mut active: Vec<(PathBuf, i64)>,
    saved: &[String],
    hidden: &[String],
) -> Vec<ProjectEntry> {
    active.sort_by_key(|a| std::cmp::Reverse(a.1));
    let taken: Vec<&Path> = active.iter().map(|a| a.0.as_path()).collect();
    let rest = saved_only(saved, &taken);
    let rest = rest.into_iter().map(|folder| ProjectEntry { folder, last_activity: None });
    active
        .iter()
        .map(|(folder, at)| ProjectEntry { folder: folder.clone(), last_activity: Some(*at) })
        .chain(rest)
        .filter(|p| !listed(hidden, &p.folder))
        .collect()
}

/// Saves `folder` as a Project and un-hides it.
pub(crate) fn add_project(settings: &mut Settings, folder: &Path) {
    set_listed(&mut settings.projects, folder, true);
    set_listed(&mut settings.hidden_projects, folder, false);
}

/// `folder` with `home` shown as `~`.
pub(crate) fn short_path(folder: &Path, home: &Path) -> String {
    match folder.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => folder.display().to_string(),
    }
}

/// Switcher rows (mark, name, short path, check on `current`), then "Add project…".
pub(crate) fn project_switcher_options(
    list: &[ProjectEntry],
    names: &HashMap<PathBuf, String>,
    current: &Path,
    home: &Path,
) -> Vec<ChoiceOption> {
    let rows = list.iter().map(|p| {
        let name = names.get(&p.folder).cloned().unwrap_or_else(|| {
            p.folder.file_name().map_or_else(
                || p.folder.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        });
        ChoiceOption {
            mark: Some(name.clone().into()),
            title: name.into(),
            description: Some(short_path(&p.folder, home).into()),
            checked: p.folder == current,
            ..Default::default()
        }
    });
    rows.chain([ChoiceOption {
        title: "Add project…".into(),
        icon: Some(IconName::Plus),
        ..Default::default()
    }])
    .collect()
}

/// Appends saved folders with no group as empty Projects.
pub(crate) fn with_saved(mut groups: Vec<Project>, saved: &[String]) -> Vec<Project> {
    let taken: Vec<&Path> = groups.iter().map(|g| g.folder.as_path()).collect();
    let extra = saved_only(saved, &taken);
    groups.extend(extra.into_iter().map(|folder| Project { folder, chats: vec![] }));
    groups
}

/// Chronological sections for non-agent chats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ChatSection {
    Today,
    Yesterday,
    Previous7Days,
    Previous30Days,
    Older,
}

impl ChatSection {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Yesterday => "Yesterday",
            Self::Previous7Days => "Previous 7 Days",
            Self::Previous30Days => "Previous 30 Days",
            Self::Older => "Older",
        }
    }
}

/// Categorizes a timestamp relative to `now_ms`.
pub(crate) fn categorize_time<Tz: TimeZone>(at_ms: i64, now_ms: i64, tz: &Tz) -> ChatSection {
    let day = |ms| tz.timestamp_millis_opt(ms).single().map(|t| t.date_naive());
    let (Some(at), Some(today)) = (day(at_ms), day(now_ms)) else {
        return ChatSection::Older;
    };
    if at >= today {
        ChatSection::Today
    } else if today.pred_opt() == Some(at) {
        ChatSection::Yesterday
    } else {
        let diff = (today - at).num_days();
        if diff <= 7 {
            ChatSection::Previous7Days
        } else if diff <= 30 {
            ChatSection::Previous30Days
        } else {
            ChatSection::Older
        }
    }
}

/// One chat as the Minimal and Focus sidebars sort it.
pub(crate) struct Listed<'a> {
    pub ix: usize,
    pub activity: i64,
    pub title: &'a str,
    pub project: &'a str,
}

/// Minimal sidebar sections, in display order, each with its chat indexes.
pub(crate) fn minimal_sections<Tz: TimeZone>(
    chats: &[Listed],
    grouping: aether_core::SidebarGrouping,
    ordering: aether_core::SidebarOrdering,
    now_ms: i64,
    tz: &Tz,
) -> Vec<(String, Vec<usize>)> {
    use aether_core::{SidebarGrouping, SidebarOrdering};
    let mut sorted: Vec<&Listed> = chats.iter().collect();
    match ordering {
        SidebarOrdering::Newest => sorted.sort_by_key(|c| std::cmp::Reverse((c.activity, c.ix))),
        SidebarOrdering::Oldest => sorted.sort_by_key(|c| (c.activity, c.ix)),
        SidebarOrdering::Name => sorted.sort_by_cached_key(|c| (c.title.to_lowercase(), c.ix)),
    }
    let mut sections: Vec<(String, Vec<usize>)> = Vec::new();
    let mut push = |label: String, ix: usize| match sections.iter_mut().find(|(l, _)| *l == label) {
        Some((_, list)) => list.push(ix),
        None => sections.push((label, vec![ix])),
    };
    match grouping {
        SidebarGrouping::Updated => {
            let mut by_time: Vec<(ChatSection, usize)> =
                sorted.iter().map(|c| (categorize_time(c.activity, now_ms, tz), c.ix)).collect();
            // stable: rows keep their order inside each section
            by_time.sort_by_key(|(section, _)| *section);
            if ordering == SidebarOrdering::Oldest {
                by_time.sort_by_key(|(section, _)| std::cmp::Reverse(*section));
            }
            for (section, ix) in by_time {
                push(section.label().to_string(), ix);
            }
        }
        SidebarGrouping::Project => {
            let mut by_project: Vec<&&Listed> = sorted.iter().collect();
            by_project.sort_by_cached_key(|c| c.project.to_lowercase());
            for c in by_project {
                let label = if c.project.is_empty() { "No project" } else { c.project };
                push(label.to_string(), c.ix);
            }
        }
    }
    sections
}

/// Focus sidebar buckets: what needs you, what runs, and the rest; newest first in each.
/// A crew sits with its most urgent member, members right after their top lead.
pub(crate) fn focus_sections(
    chats: &[(usize, ChatStatus, i64, Option<usize>)],
) -> Vec<(&'static str, Vec<usize>)> {
    let mut sorted = chats.to_vec();
    sorted.sort_by_key(|(ix, _, activity, _)| std::cmp::Reverse((*activity, *ix)));
    let bucket = |status: &ChatStatus| match status {
        ChatStatus::Waiting | ChatStatus::Failed | ChatStatus::Done => 0,
        ChatStatus::Idle => 2,
        _ => 1,
    };
    let parent: HashMap<usize, usize> =
        chats.iter().filter_map(|(ix, _, _, p)| p.map(|p| (*ix, p))).collect();
    let root = |mut ix: usize| {
        for _ in 0..chats.len() {
            let Some(&p) = parent.get(&ix) else { break };
            ix = p;
        }
        ix
    };
    let mut families: Vec<(usize, usize, Vec<usize>)> = Vec::new();
    let mut slot: HashMap<usize, usize> = HashMap::new();
    for (ix, status, ..) in &sorted {
        let top = root(*ix);
        let at = *slot.entry(top).or_insert_with(|| {
            families.push((top, 2, Vec::new()));
            families.len() - 1
        });
        let (_, rank, members) = &mut families[at];
        let own = if *ix != top && *status == ChatStatus::Done { 2 } else { bucket(status) };
        *rank = (*rank).min(own);
        members.push(*ix);
    }
    for (top, _, members) in &mut families {
        members.sort_by_key(|ix| ix != top);
    }
    ["Needs you", "Working", "Recent"]
        .into_iter()
        .enumerate()
        .map(|(k, label)| {
            let ixs = families
                .iter()
                .filter(|(_, rank, _)| *rank == k)
                .flat_map(|(_, _, members)| members.iter().copied())
                .collect();
            (label, ixs)
        })
        .filter(|(_, ixs): &(_, Vec<usize>)| !ixs.is_empty())
        .collect()
}

#[cfg(test)]
#[path = "../tests/unit/projects.rs"]
mod tests;
