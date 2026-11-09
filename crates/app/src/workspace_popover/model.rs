use std::collections::HashMap;
use std::path::{Path, PathBuf};

use aether_core::ProjectScript;
use aether_git::{Branches, Change, Remote, Stat, Status, StatusError, owner_repo};

use crate::git_panel::{PickMode, PickRow, pick_rows};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PopView {
    Main,
    Location,
    ChatLayout,
    Apps,
    Script(Option<usize>),
    Branch,
    Publish,
    Remote,
}

pub(crate) fn back_of(view: &PopView) -> Option<PopView> {
    (*view != PopView::Main).then_some(PopView::Main)
}

pub(crate) fn view_title(view: &PopView) -> Option<&'static str> {
    match view {
        PopView::Main => None,
        PopView::Location => Some("Location"),
        PopView::ChatLayout => Some("Chat layout"),
        PopView::Apps => Some("Open in"),
        PopView::Script(None) => Some("Add project script"),
        PopView::Script(Some(_)) => Some("Edit project script"),
        PopView::Branch => Some("Branch"),
        PopView::Publish => Some("Publish repository"),
        PopView::Remote => Some("Remote"),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    Row(usize),
    Field,
}

pub(crate) fn pop_step(at: Option<usize>, field: bool, down: bool) -> Option<Stop> {
    match (at, field, down) {
        (None, true, true) => Some(Stop::Row(1)),
        (None, true, false) => Some(Stop::Row(0)),
        (None, false, down) => down.then_some(Stop::Row(0)),
        (Some(0), true, true) | (Some(1), true, false) => Some(Stop::Field),
        (Some(at), _, true) => Some(Stop::Row(at + 1)),
        (Some(at), _, false) => at.checked_sub(1).map(Stop::Row),
    }
}

pub(crate) fn row_after_shrink(at: Option<usize>, len: usize) -> Option<usize> {
    at.filter(|&at| at >= len).and(len.checked_sub(1))
}

pub(crate) fn footer_error(text: &str, detail: &str) -> String {
    match detail.lines().map(str::trim).find(|line| !line.is_empty()) {
        Some(line) => format!("{text}: {line}"),
        None => text.to_string(),
    }
}

pub(crate) fn refusal_reason(busy: bool, switching: bool) -> &'static str {
    if busy {
        "Another git command is running"
    } else if switching {
        "This folder is switching branches"
    } else {
        "Its folder changed before git ran, so nothing was done"
    }
}

pub(crate) fn machine_name(scutil: Option<&str>, hostname: Option<&str>) -> Option<String> {
    let scutil = scutil.map(str::trim).filter(|s| !s.is_empty());
    let host = hostname
        .map(str::trim)
        .map(|h| h.strip_suffix(".local").unwrap_or(h))
        .filter(|h| !h.is_empty());
    scutil.or(host).map(str::to_string)
}

pub(crate) fn line_totals(changes: &[Change]) -> (u32, u32) {
    changes.iter().fold((0, 0), |(a, r), c| match c.stat {
        Stat::Lines(add, rem) => (a + add, r + rem),
        Stat::Binary | Stat::Unknown => (a, r),
    })
}

pub(super) fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub(crate) fn changes_spoken(status: Option<&Status>) -> String {
    match status {
        Some(s) => {
            let (added, removed) = line_totals(&s.changes);
            format!("Changes, {} added, {} removed", thousands(added), thousands(removed))
        }
        None => "Changes".into(),
    }
}

pub(crate) fn branch_spoken(branch: &str, worktree: bool) -> String {
    format!("Branch {branch}{}", if worktree { ", worktree" } else { "" })
}

pub(crate) fn copy_label(title: &str, short: &str) -> String {
    format!("Copy {title} {short}")
}

pub(crate) fn choose_app_label(current: Option<&str>) -> String {
    match current {
        Some(app) => format!("Choose app, current: {app}"),
        None => "Choose app".into(),
    }
}

pub(crate) fn git_trouble(read: Option<&Result<Status, StatusError>>) -> Option<&'static str> {
    match read? {
        Ok(_) => None,
        Err(StatusError::NotRepo) => Some("Not a git repository"),
        Err(StatusError::Locked) => Some("Git is busy"),
        Err(StatusError::Failed(_)) => Some("Couldn't run git"),
    }
}

pub(crate) fn home_short(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

pub(crate) struct PopRows {
    pub(crate) git: bool,
    pub(crate) scripts: bool,
    pub(crate) reason: Option<&'static str>,
}

pub(crate) fn pop_rows(folder_ok: bool, repo: bool) -> PopRows {
    PopRows {
        git: folder_ok && repo,
        scripts: folder_ok,
        reason: (!folder_ok).then_some("Folder not found"),
    }
}

#[derive(Default)]
pub(crate) struct ScriptRuns(HashMap<(PathBuf, String), i64>);

impl ScriptRuns {
    /// False when it's already running in that folder. A chat holds one run per
    /// name, so its entry from an older folder goes.
    pub(crate) fn start(&mut self, folder: &Path, name: &str, chat: i64) -> bool {
        if self.running(folder, name).is_some() {
            return false;
        }
        self.end_owned(chat, name);
        self.0.insert((folder.to_path_buf(), name.to_string()), chat);
        true
    }

    pub(crate) fn end(&mut self, folder: &Path, name: &str) {
        self.0.remove(&(folder.to_path_buf(), name.to_string()));
    }

    pub(crate) fn running(&self, folder: &Path, name: &str) -> Option<i64> {
        self.0.get(&(folder.to_path_buf(), name.to_string())).copied()
    }

    pub(crate) fn end_owned(&mut self, chat: i64, name: &str) {
        self.0.retain(|(_, n), c| !(*c == chat && n == name));
    }

    pub(crate) fn runs_of(&self, name: &str) -> Vec<(PathBuf, i64)> {
        self.0.iter().filter(|((_, n), _)| n == name).map(|((f, _), c)| (f.clone(), *c)).collect()
    }

    pub(crate) fn drop_chat(&mut self, chat: i64) -> Vec<(PathBuf, String)> {
        self.0.extract_if(|_, c| *c == chat).map(|(key, _)| key).collect()
    }
}

pub(crate) fn script_default_name(command: &str) -> String {
    let end = ["&&", ";", "|"].iter().filter_map(|p| command.find(p)).min();
    let first = &command[..end.unwrap_or(command.len())];
    first.split_whitespace().rev().find(|w| !w.starts_with('-')).unwrap_or("script").into()
}

pub(crate) fn script_action(name: &str, running: bool) -> String {
    format!("{} {name}", if running { "Stop" } else { "Run" })
}

pub(crate) fn script_menu_label(name: &str) -> String {
    format!("Actions for {name}")
}

pub(crate) fn script_name_taken(
    scripts: &[ProjectScript],
    name: &str,
    editing: Option<usize>,
) -> bool {
    scripts.iter().enumerate().any(|(i, s)| Some(i) != editing && s.name == name)
}

pub(crate) fn script_row_label(script: &ProjectScript, running: bool) -> String {
    let setup = if script.on_new_worktree { ", on new worktree" } else { "" };
    format!("{}, {}{setup}", script_action(&script.name, running), script.command)
}

/// Spoken label of a row that can't be used; gpui has no aria-disabled.
pub(crate) fn disabled_label(label: &str, reason: &str) -> String {
    format!("{label}, unavailable: {reason}")
}

pub(crate) fn newest_chat_in(chats: &[(usize, PathBuf, i64)], path: &Path) -> Option<usize> {
    chats.iter().filter(|(_, cwd, _)| cwd == path).max_by_key(|(_, _, at)| *at).map(|c| c.0)
}

pub(crate) fn branch_block_reason(agent_running: bool, folder_ok: bool) -> Option<&'static str> {
    if !folder_ok {
        Some("Folder not found")
    } else {
        agent_running.then_some("An agent is working in this folder")
    }
}

pub(crate) fn branch_view_rows(branches: &Branches, query: &str) -> Vec<PickRow> {
    let query = query.trim();
    let mut rows = pick_rows(branches, &PickMode::Switch, query);
    rows.retain(|row| match row {
        PickRow::CreateFrom => false,
        PickRow::Create => !query.is_empty() && !branches.local.iter().any(|b| b == query),
        _ => true,
    });
    rows
}

pub(crate) fn branch_row_label(
    row: &PickRow,
    query: &str,
    current: Option<&str>,
    at: Option<&str>,
) -> String {
    match row {
        PickRow::Create => {
            format!("Create branch {} from {}", query.trim(), current.unwrap_or("HEAD"))
        }
        PickRow::Local(b) if Some(b.as_str()) == current => format!("{b}, current branch"),
        PickRow::Local(b) => match at {
            Some(at) => format!("{b}, checked out {at}, open its chat"),
            None => b.clone(),
        },
        PickRow::Remote(b) => format!("{b}, remote branch"),
        _ => String::new(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GhState {
    Missing,
    LoggedOut,
    Ready,
}

pub(crate) fn gh_state(found: bool, auth_ok: bool) -> GhState {
    match (found, auth_ok) {
        (false, _) => GhState::Missing,
        (true, false) => GhState::LoggedOut,
        (true, true) => GhState::Ready,
    }
}

pub(crate) const NO_COMMIT: &str = "Make a first commit to publish";
pub(crate) const NO_BRANCH: &str = "Check out a branch to publish";
pub(crate) const NO_GH: &str = "Install GitHub CLI and run gh auth login";
pub(crate) const BAD_NAME: &str = "Use letters, digits, . _ -";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RemoteRow {
    Init,
    Publish { blocked: Option<&'static str> },
    PublishBranch,
    Remote { name: String, slug: Option<String>, ahead: u32, behind: u32, gone: bool },
}

pub(crate) fn remote_row(
    repo: bool,
    status: Option<&Status>,
    remotes: &[Remote],
    gh: GhState,
) -> RemoteRow {
    if !repo {
        return RemoteRow::Init;
    }
    if remotes.is_empty() {
        let blocked = match status.map(|s| (s.commit.is_some(), s.branch.is_some())) {
            None | Some((false, _)) => Some(NO_COMMIT),
            Some((true, false)) => Some(NO_BRANCH),
            Some((true, true)) => (gh != GhState::Ready).then_some(NO_GH),
        };
        return RemoteRow::Publish { blocked };
    }
    let Some(status) = status.filter(|s| s.upstream.is_some()) else {
        return RemoteRow::PublishBranch;
    };
    let upstream = status.upstream.as_deref().unwrap_or_default();
    let remote = remotes
        .iter()
        .filter(|r| upstream.strip_prefix(r.name.as_str()).is_some_and(|b| b.starts_with('/')))
        .max_by_key(|r| r.name.len())
        .unwrap_or(&remotes[0]);
    let (ahead, behind) = status.ahead_behind.unwrap_or_default();
    let gone = status.ahead_behind.is_none();
    RemoteRow::Remote {
        name: remote.name.clone(),
        slug: owner_repo(&remote.url),
        ahead,
        behind,
        gone,
    }
}

pub(crate) fn publish_args(name: &str, private: bool, top: &Path) -> Vec<String> {
    let visibility = if private { "--private" } else { "--public" };
    let top = top.display().to_string();
    ["repo", "create", name, visibility, "--source", &top, "--remote", "origin", "--push"]
        .map(String::from)
        .to_vec()
}

pub(crate) fn repo_name_ok(name: &str) -> bool {
    let part = |p: &str| {
        !p.is_empty()
            && !p.starts_with('-')
            && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    match name.split_once('/') {
        Some((owner, repo)) => part(owner) && part(repo),
        None => part(name),
    }
}

pub(crate) fn repo_name_from(folder: &str) -> String {
    let dashed = folder.split_whitespace().collect::<Vec<_>>().join("-");
    dashed.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')).collect()
}

pub(crate) fn pr_branch(
    upstream: Option<&str>,
    remote: &str,
    local: Option<&str>,
) -> Option<String> {
    let on_remote = upstream.and_then(|u| u.strip_prefix(remote)?.strip_prefix('/'));
    on_remote.or(local).map(str::to_string)
}

pub(crate) fn compare_url(https: &str, branch: &str) -> String {
    let mut path = String::new();
    for c in branch.chars() {
        match c {
            '%' => path.push_str("%25"),
            ' ' => path.push_str("%20"),
            '#' => path.push_str("%23"),
            '?' => path.push_str("%3F"),
            c => path.push(c),
        }
    }
    format!("{https}/compare/{path}?expand=1")
}

pub(crate) fn first_stderr_line(stderr: &str) -> String {
    stderr.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("gh failed").to_string()
}

pub(crate) fn sync_summary(ahead_behind: Option<(u32, u32)>) -> String {
    match ahead_behind {
        None => "Upstream gone".into(),
        Some((0, 0)) => "Up to date".into(),
        Some((ahead, behind)) => format!("\u{2191}{ahead} \u{2193}{behind}"),
    }
}

pub(crate) fn sync_spoken(ahead_behind: Option<(u32, u32)>) -> String {
    match ahead_behind {
        None => "upstream gone".into(),
        Some((0, 0)) => "up to date".into(),
        Some((ahead, behind)) => format!("{ahead} to push, {behind} to pull"),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workspace_popover/model.rs"]
mod tests;
