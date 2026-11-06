//! Linked git worktrees that give an agent chat its own checkout.

use std::ffi::OsStr;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub use crate::CANCELED;
use crate::{self as git, Output, git_error};
use aether_core::home_dir;

const QUICK: Duration = Duration::from_secs(10);
const SLOW: Duration = Duration::from_secs(120);
const MAX_SLUG: usize = 48;
const BRANCH_PREFIX: &str = "aether/";
/// Base branches offered; the picker filters as you type.
const MAX_BASES: usize = 500;
/// Worktrees one repo lists; each is checked for changes.
const MAX_LISTED: usize = 200;
/// Changes checks run at once, and how long each may take.
const CHECKS_AT_ONCE: usize = 8;
const DIRTY: Duration = Duration::from_secs(3);

/// Local branches a new worktree can start from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bases {
    /// The default, then the checked-out branch, then the most recently
    /// committed; empty before the first commit.
    pub branches: Vec<String>,
    /// The branch `origin/HEAD` names, else the checked-out one.
    pub default: Option<String>,
    pub repo: PathBuf,
}

/// A worktree made for a chat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
    /// Where the chat runs: the project's subfolder inside the worktree, when it exists there.
    pub cwd: PathBuf,
    /// The project's subfolder, when the base branch doesn't have it.
    pub missing_subdir: Option<String>,
}

/// What removing a worktree would lose.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoveCheck {
    /// The folder is already gone; only git's record of it is left.
    pub missing: bool,
    /// Uncommitted and untracked paths.
    pub changes: Vec<String>,
    /// Checked-out branch; `None` when detached.
    pub branch: Option<String>,
    /// Commits on HEAD that no other branch or remote has.
    pub unmerged: usize,
    /// One of aether's, so its `aether/` branch may go with it.
    pub managed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoveError {
    /// Git kept the worktree (changes, submodules); `--force` would remove it.
    NeedsForce(String),
    Failed(String),
    /// The folder is gone but `git branch -d` refused; chats must still detach.
    BranchDeleteFailed(String),
}

/// A worktree, as Git records it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listed {
    pub path: PathBuf,
    /// Main checkout of the repo it belongs to.
    pub repo: PathBuf,
    pub branch: Option<String>,
    /// HEAD's commit; what a detached one shows.
    pub head: String,
    /// Git's lock reason, empty when none was given.
    pub locked: Option<String>,
    pub missing: bool,
    pub managed: bool,
    /// The repo's main checkout; never removed.
    pub main: bool,
    /// Has uncommitted changes to tracked files; `None` when not checked.
    pub dirty: Option<bool>,
}

impl Listed {
    /// What removing it does: Prune for a gone folder, else Remove; or why it can't.
    pub fn remove_action(&self) -> Result<&'static str, String> {
        match self.locked.as_deref() {
            _ if self.main => Err("The main worktree can't be removed".into()),
            Some("") => Err("Locked. Unlock it in a terminal to remove it".into()),
            Some(why) => Err(format!("Locked: {why}. Unlock it in a terminal to remove it")),
            None if self.missing => Ok("Prune worktree…"),
            None => Ok("Remove worktree…"),
        }
    }
}

/// What a new worktree checks out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Start {
    /// A local branch no worktree has checked out.
    Local(String),
    /// A remote branch (`origin/x`), as a new local branch tracking it.
    Remote(String),
    /// A new branch `name` at `base` (a full ref), else at HEAD.
    New { name: String, base: Option<String> },
}

/// Where new worktrees go; a path without spaces, so tools in them don't trip.
pub fn root() -> Option<PathBuf> {
    let home = home_dir();
    home.is_absolute().then(|| home.join(".aether/worktrees"))
}

/// Every folder aether has kept worktrees in, the current one first.
pub fn roots() -> Vec<PathBuf> {
    root().into_iter().chain(aether_core::config_dir().map(|dir| dir.join("worktrees"))).collect()
}

/// Local branches to offer as a base; `None` outside a work tree. Blocking.
pub fn bases(folder: &Path) -> Option<Bases> {
    let inside = run(folder, &["rev-parse", "--is-inside-work-tree"], QUICK).ok()?;
    if !inside.ok || inside.stdout.trim() != "true" {
        return None;
    }
    let refs = run(
        folder,
        &["for-each-ref", "--sort=-committerdate", "--format=%(refname)", "refs/heads"],
        QUICK,
    )
    .ok()
    .filter(|out| out.ok)?;
    let all: Vec<String> = refs
        .stdout
        .lines()
        .filter_map(|line| line.strip_prefix("refs/heads/"))
        .map(String::from)
        .collect();
    let read = |args: &[&str]| run(folder, args, QUICK).map(|out| out.stdout).unwrap_or_default();
    let origin = read(&["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"]);
    let current = read(&["symbolic-ref", "--quiet", "--short", "HEAD"]);
    let default = default_base(&origin, &current, &all);
    let branches = order_bases(all, default.as_deref(), current.trim());
    let listed =
        run(folder, &["worktree", "list", "--porcelain"], QUICK).ok().filter(|out| out.ok)?;
    let repo = entries(&listed.stdout).into_iter().next()?.path;
    let repo = repo.canonicalize().unwrap_or(repo);
    Some(Bases { branches, default, repo })
}

/// Makes `aether/<slug>` from local branch `base` in a new worktree under
/// `root`, named from `prompt`. Never reuses a branch or folder. Stops when
/// `cancel` is set. Blocking.
pub fn create(
    root: &Path,
    folder: &Path,
    prompt: &str,
    base: &str,
    cancel: &AtomicBool,
) -> Result<Worktree, String> {
    let top = run(folder, &["rev-parse", "--show-toplevel", "--show-prefix"], QUICK)
        .ok()
        .filter(|out| out.ok)
        .ok_or("This folder isn't in a git repository.")?
        .stdout;
    let mut lines = top.lines();
    let top = PathBuf::from(lines.next().unwrap_or_default());
    let prefix = lines.next().unwrap_or_default().trim_end_matches('/').to_string();
    let base_ref = format!("refs/heads/{base}");
    if !run(&top, &["rev-parse", "--verify", "--quiet", &format!("{base_ref}^{{commit}}")], QUICK)?
        .ok
    {
        return Err(format!("Base branch “{base}” wasn't found. Pick another base branch."));
    }
    let dir = root.join(repo_dir(&top));
    let dir = std::fs::create_dir_all(&dir)
        .and_then(|()| dir.canonicalize())
        .map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    let mut retried = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(CANCELED.into());
        }
        let (path, branch) = free_name(&top, &dir, prompt)?;
        let args = ["worktree", "add", "-b", &branch, "--"]
            .map(OsStr::new)
            .into_iter()
            .chain([path.as_os_str(), base_ref.as_ref()])
            .collect::<Vec<_>>();
        let out = match git::run_until(&top, &args, SLOW, cancel) {
            Err(e) if e == CANCELED => {
                return Err(format!("{CANCELED} Branch {branch} may be left over."));
            }
            other => other?,
        };
        if out.ok {
            let sub = path.join(&prefix);
            let (cwd, missing_subdir) = match () {
                _ if prefix.is_empty() => (path.clone(), None),
                _ if sub.is_dir() => (sub, None),
                _ => (path.clone(), Some(prefix)),
            };
            return Ok(Worktree { path, branch, cwd, missing_subdir });
        }
        // someone made that branch or folder since we looked; pick again once
        if !retried && out.stderr.contains("already exists") {
            retried = true;
            continue;
        }
        return Err(git_error(&out));
    }
}

/// A slug free as a branch, a branch prefix, a registered worktree and a folder.
fn free_name(top: &Path, dir: &Path, prompt: &str) -> Result<(PathBuf, String), String> {
    let refs =
        run(top, &["for-each-ref", "--format=%(refname)", "refs/heads/aether"], QUICK)?.stdout;
    let listed = entries(&run(top, &["worktree", "list", "--porcelain"], QUICK)?.stdout);
    let slug = free_slug(&slug(prompt), |s| {
        let branch = format!("refs/heads/{BRANCH_PREFIX}{s}");
        let path = dir.join(s);
        // a branch `aether/x/y` also blocks `aether/x`
        refs.lines().any(|r| r == branch || r.starts_with(&format!("{branch}/")))
            || listed.iter().any(|e| e.path == path)
            || path.exists()
    })
    .ok_or("Every name for this worktree is taken. Remove some old worktrees, then retry.")?;
    Ok((dir.join(&slug), format!("{BRANCH_PREFIX}{slug}")))
}

/// What removing the worktree at `path` would lose. Blocking.
pub fn check_remove(roots: &[PathBuf], path: &Path) -> Result<RemoveCheck, String> {
    Ok(RemoveCheck { managed: true, ..check_at(&managed(roots, path)?)? })
}

/// [`check_remove`] for one of aether's worktrees, else for a linked worktree
/// of the repo at `project`, never its main one. Blocking.
pub fn check_any(roots: &[PathBuf], project: &Path, path: &Path) -> Result<RemoveCheck, String> {
    match managed(roots, path) {
        Ok(_) => check_remove(roots, path),
        Err(_) => check_at(&linked(project, path)?),
    }
}

fn check_at(path: &Path) -> Result<RemoveCheck, String> {
    if gone(path)? {
        return Ok(RemoveCheck { missing: true, ..RemoveCheck::default() });
    }
    let status = run(path, &["status", "--porcelain", "--untracked-files=normal"], QUICK)?;
    if !status.ok {
        return Err(git_error(&status));
    }
    let branch = run(path, &["symbolic-ref", "--quiet", "--short", "HEAD"], QUICK)?;
    let branch = branch.ok.then(|| branch.stdout.trim().to_string());
    let exclude = branch.as_ref().map(|b| format!("--exclude={b}"));
    let mut args = vec!["rev-list", "--count", "HEAD", "--not"];
    args.extend(exclude.as_deref());
    args.extend(["--branches", "--remotes"]);
    let unmerged = run(path, &args, QUICK)?.stdout.trim().parse().unwrap_or(0);
    let changes = changed_paths(&status.stdout);
    Ok(RemoveCheck { missing: false, changes, branch, unmerged, managed: false })
}

/// Removes the worktree at `path` (its branch stays). `project` is the repo it
/// came from, used when the folder is already gone. Blocking.
pub fn remove(
    roots: &[PathBuf],
    path: &Path,
    project: &Path,
    force: bool,
) -> Result<(), RemoveError> {
    let path = managed(roots, path).map_err(RemoveError::Failed)?;
    remove_at(&path, project, force)
}

/// Removes the linked worktree at `path` of the repo at `project` with `git
/// worktree remove`, never its main one or its branch. One `--force` drops
/// changes; a locked worktree stays. Blocking.
pub fn remove_linked(project: &Path, path: &Path, force: bool) -> Result<(), RemoveError> {
    let path = linked(project, path).map_err(RemoveError::Failed)?;
    remove_at(&path, project, force)
}

fn remove_at(path: &Path, project: &Path, force: bool) -> Result<(), RemoveError> {
    let failed = |e: String| RemoveError::Failed(e);
    if gone(path).map_err(failed)? {
        let Some(entry) = listed_entry(project, path).map_err(failed)? else {
            return Ok(());
        };
        if let Some(why) = entry.locked {
            let why = if why.is_empty() { String::new() } else { format!(" ({why})") };
            return Err(failed(format!(
                "Git has this worktree locked{why}. Unlock it with git first."
            )));
        }
        // only this record, never `prune`: that would also drop worktrees on a drive that's just unplugged
        let args = ["worktree".as_ref(), "remove".as_ref(), "--".as_ref(), path.as_os_str()];
        let out = git::run(project, &args, SLOW).map_err(failed)?;
        if !out.ok {
            return Err(failed(git_error(&out)));
        }
        if listed_entry(project, path).map_err(failed)?.is_some() {
            return Err(failed("Git still lists this worktree after removing it. Check it with `git worktree list`.".into()));
        }
        return Ok(());
    }
    let common = run(path, &["rev-parse", "--path-format=absolute", "--git-common-dir"], QUICK)
        .map_err(failed)?;
    if !common.ok {
        return Err(failed(git_error(&common)));
    }
    let mut args: Vec<&OsStr> = ["-c", "status.showUntrackedFiles=normal", "worktree", "remove"]
        .iter()
        .map(OsStr::new)
        .collect();
    if force {
        args.push("--force".as_ref());
    }
    args.extend(["--".as_ref(), path.as_os_str()]);
    // run from the shared .git so it works even if the project folder moved
    let out = git::run(Path::new(common.stdout.trim()), &args, SLOW).map_err(failed)?;
    match (out.ok, force) {
        (true, _) => Ok(()),
        (false, false) if out.stderr.contains("--force") => {
            Err(RemoveError::NeedsForce(git_error(&out)))
        }
        (false, _) => Err(failed(git_error(&out))),
    }
}

/// Removes the worktree at `path`, then deletes its branch when asked. The
/// branch stays by default; it only goes when the worktree did, and only an
/// `aether/` branch ever goes, so a worktree on `main` never loses its branch.
/// A worktree that isn't aether's goes through [`remove_linked`] and keeps its branch.
pub fn remove_worktree_with_branch(
    roots: &[PathBuf],
    path: &Path,
    project: &Path,
    force: bool,
    delete_branch: bool,
) -> Result<(), RemoveError> {
    if managed(roots, path).is_err() {
        return remove_linked(project, path, force);
    }
    let branch = check_remove(roots, path).ok().and_then(|check| check.branch);
    remove(roots, path, project, force)?;
    if delete_branch && branch.as_deref().is_some_and(|b| b.starts_with(BRANCH_PREFIX)) {
        let branch = branch.unwrap_or_default();
        let flag = if force { "-D" } else { "-d" };
        let out = run(project, &["branch", flag, &branch], QUICK).map_err(RemoveError::Failed)?;
        if !out.ok {
            return Err(RemoveError::BranchDeleteFailed(git_error(&out)));
        }
    }
    Ok(())
}

/// Git's record of the worktree at `path`, asking the repo at `project`.
fn listed_entry(project: &Path, path: &Path) -> Result<Option<Entry>, String> {
    let out = run(project, &["worktree", "list", "--porcelain"], QUICK)?;
    if !out.ok {
        return Err(git_error(&out));
    }
    Ok(entries(&out.stdout).into_iter().find(|e| e.path == path))
}

/// Linked Git worktrees, found through managed roots and the supplied repositories. Blocking.
pub fn list(roots: &[PathBuf], repos: &[PathBuf]) -> Vec<Listed> {
    let mut repos = repos.to_vec();
    for slot in roots.iter().flat_map(|root| children(root)).flat_map(|dir| children(&dir)) {
        // a slot's shared .git names the repo it belongs to
        if let Ok(out) =
            run(&slot, &["rev-parse", "--path-format=absolute", "--git-common-dir"], QUICK)
            && out.ok
        {
            repos.push(PathBuf::from(out.stdout.trim()));
        }
    }
    repos.sort();
    repos.dedup();
    let mut found: Vec<Listed> = Vec::new();
    for repo in repos {
        let Some(out) =
            run(&repo, &["worktree", "list", "--porcelain"], QUICK).ok().filter(|out| out.ok)
        else {
            continue;
        };
        let listed = entries(&out.stdout);
        let Some(main) = listed.first().map(|e| e.path.clone()) else {
            continue;
        };
        let main = main.canonicalize().unwrap_or(main);
        for entry in listed.into_iter().skip(1) {
            let listed = to_listed(roots, &main, entry);
            if !found.iter().any(|f| f.path == listed.path) {
                found.push(listed);
            }
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

/// Every worktree of the repo `folder` is in, the main one first, up to
/// [`MAX_LISTED`], each checked for changes. Blocking.
pub fn list_repo(roots: &[PathBuf], folder: &Path) -> Result<Vec<Listed>, String> {
    let out = run(folder, &["worktree", "list", "--porcelain"], QUICK)?;
    if !out.ok {
        return Err(git_error(&out));
    }
    let listed = entries(&out.stdout);
    let Some(main) = listed.first().map(|e| e.path.clone()) else {
        return Ok(Vec::new());
    };
    let main = main.canonicalize().unwrap_or(main);
    let mut found: Vec<Listed> =
        listed.into_iter().take(MAX_LISTED).map(|e| to_listed(roots, &main, e)).collect();
    found[0].main = true;
    for chunk in found.chunks_mut(CHECKS_AT_ONCE) {
        std::thread::scope(|scope| {
            for tree in chunk.iter_mut().filter(|tree| !tree.missing) {
                scope.spawn(move || tree.dirty = dirty(&tree.path));
            }
        });
    }
    Ok(found)
}

fn to_listed(roots: &[PathBuf], main: &Path, entry: Entry) -> Listed {
    let path = entry.path.canonicalize().unwrap_or(entry.path);
    Listed {
        missing: gone(&path).unwrap_or(false),
        managed: managed(roots, &path).is_ok(),
        path,
        repo: main.to_path_buf(),
        branch: entry.branch,
        head: entry.head,
        locked: entry.locked,
        ..Listed::default()
    }
}

/// Whether `path` is gone. Only not-found counts: an unreadable folder isn't,
/// though one on an unplugged drive still looks gone.
fn gone(path: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(true),
        Err(e) => Err(format!("Couldn't read {}: {e}", path.display())),
    }
}

/// Whether the worktree at `path` has uncommitted changes to tracked files;
/// `None` when git didn't say in time.
fn dirty(path: &Path) -> Option<bool> {
    let args = ["--no-optional-locks", "status", "--porcelain", "--untracked-files=no"];
    let out = run(path, &args, DIRTY).ok().filter(|out| out.ok)?;
    Some(!out.stdout.trim().is_empty())
}

/// `path` resolved, when it's a linked worktree of the repo at `project` (by
/// git's record when its folder is gone); never the main worktree.
fn linked(project: &Path, path: &Path) -> Result<PathBuf, String> {
    let out = run(project, &["worktree", "list", "--porcelain"], QUICK)?;
    if !out.ok {
        return Err(git_error(&out));
    }
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let resolved = real(path);
    match entries(&out.stdout).iter().position(|e| real(&e.path) == resolved) {
        Some(0) => Err(format!(
            "{} is the repository's main worktree, so it wasn't touched.",
            path.display()
        )),
        Some(_) => Ok(resolved),
        None => Err(format!(
            "{} isn't a worktree of this repository, so it wasn't touched.",
            path.display()
        )),
    }
}

/// Checks `start` out in a new worktree in aether's folder for the repo
/// `folder` is in, under `root`, or in a new subfolder of `parent` named
/// from the branch; returns the new folder. Blocking.
pub fn add(
    root: &Path,
    folder: &Path,
    start: &Start,
    parent: Option<&Path>,
) -> Result<PathBuf, String> {
    let out = run(folder, &["worktree", "list", "--porcelain"], QUICK)
        .ok()
        .filter(|out| out.ok)
        .ok_or("This folder isn't in a git repository.")?;
    let listed = entries(&out.stdout);
    let main = listed.first().map(|e| e.path.clone()).ok_or("Git listed no worktrees.")?;
    let has = |full: &str| {
        run(&main, &["show-ref", "--verify", "--quiet", full], QUICK).is_ok_and(|out| out.ok)
    };
    let (hint, opts, point): (&str, Vec<&str>, String) = match start {
        Start::Local(branch) => {
            branch_name(&main, branch)?;
            if !has(&format!("refs/heads/{branch}")) {
                return Err(format!(
                    "{branch} isn't a branch here anymore; refresh and try again."
                ));
            }
            if let Some(at) = listed.iter().find(|e| e.branch.as_deref() == Some(branch)) {
                return Err(format!("{branch} is already checked out at {}.", at.path.display()));
            }
            (branch, Vec::new(), branch.clone())
        }
        Start::Remote(remote) => {
            let full = format!("refs/remotes/{remote}");
            if remote.starts_with('-') || remote.contains('\0') || !has(&full) {
                return Err(format!(
                    "{remote} isn't a remote branch here anymore; refresh and try again."
                ));
            }
            let remotes = run(&main, &["remote"], QUICK)?.stdout;
            let local = remotes
                .lines()
                .find_map(|name| remote.strip_prefix(name.trim())?.strip_prefix('/'))
                .ok_or_else(|| format!("{remote} isn't on a remote this repository has."))?;
            branch_name(&main, local)?;
            if has(&format!("refs/heads/{local}")) {
                return Err(format!(
                    "A local branch {local} already exists. Pick it under Branches instead."
                ));
            }
            (local, vec!["--track", "-b", local], full)
        }
        Start::New { name, base } => {
            branch_name(&main, name)?;
            let base = match base {
                Some(base)
                    if (base.starts_with("refs/heads/") || base.starts_with("refs/remotes/"))
                        && has(base) =>
                {
                    base.clone()
                }
                Some(base) => {
                    return Err(format!(
                        "{base} isn't a branch here anymore; refresh and try again."
                    ));
                }
                None => "HEAD".into(),
            };
            (name, vec!["--no-track", "-b", name], base)
        }
    };
    let path = match parent {
        Some(parent) => user_slot(parent, &listed, hint)?,
        None => {
            let dir = root.join(repo_dir(&main));
            let dir = std::fs::create_dir_all(&dir)
                .and_then(|()| dir.canonicalize())
                .map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
            let name = free_slug(&slug(hint), |s| {
                let path = dir.join(s);
                path.exists() || listed.iter().any(|e| e.path == path)
            })
            .ok_or(
                "Every name for this worktree is taken. Remove some old worktrees, then retry.",
            )?;
            dir.join(name)
        }
    };
    let all: Vec<&OsStr> = ["worktree", "add"]
        .into_iter()
        .chain(opts)
        .map(OsStr::new)
        .chain([OsStr::new("--"), path.as_os_str(), point.as_ref()])
        .collect();
    // from `folder`, so HEAD is its checkout's
    let out = git::run(folder, &all, SLOW)
        .and_then(|out| if out.ok { Ok(()) } else { Err(git_error(&out)) });
    if out.is_err() && parent.is_some() {
        let _ = std::fs::remove_dir(&path);
    }
    out.map(|()| path)
}

/// A new, empty `<slug of hint>` folder in `parent`; refused when it exists,
/// `parent` isn't writable, or it would nest inside one of the repo's worktrees.
fn user_slot(parent: &Path, listed: &[Entry], hint: &str) -> Result<PathBuf, String> {
    let parent =
        parent.canonicalize().map_err(|e| format!("Couldn't open {}: {e}", parent.display()))?;
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if let Some(tree) = listed.iter().find(|e| parent.starts_with(real(&e.path))) {
        return Err(format!(
            "{} is inside the worktree at {}. Pick a folder outside the repository.",
            parent.display(),
            tree.path.display()
        ));
    }
    let path = parent.join(slug(hint));
    // creating it checks free and writable at once; git fills an empty folder
    std::fs::create_dir(&path).map_err(|e| match e.kind() {
        ErrorKind::AlreadyExists => {
            format!("{} already exists. Pick another folder.", path.display())
        }
        ErrorKind::PermissionDenied => {
            format!("{} isn't writable. Pick another folder.", parent.display())
        }
        _ => format!("Couldn't create {}: {e}", path.display()),
    })?;
    Ok(path)
}

/// Errors unless `name` is a valid branch name as typed; `--branch` also
/// expands `@{-1}` and the like, so only one that comes back unchanged is.
fn branch_name(cwd: &Path, name: &str) -> Result<(), String> {
    let invalid = || format!("“{name}” isn't a valid branch name.");
    if name.is_empty() || name.starts_with('-') || name.contains('\0') {
        return Err(invalid());
    }
    let out = run(cwd, &["check-ref-format", "--branch", name], QUICK)?;
    if out.ok && out.stdout.trim() == name { Ok(()) } else { Err(invalid()) }
}

/// `git worktree prune --verbose`: drops git's records of worktrees whose
/// folders are gone (locked ones stay). `Ok` names the records it dropped. Blocking.
pub fn prune(folder: &Path) -> Result<Vec<String>, String> {
    let out = run(folder, &["worktree", "prune", "--verbose"], SLOW)?;
    if !out.ok {
        return Err(git_error(&out));
    }
    Ok(pruned(&format!("{}\n{}", out.stderr, out.stdout)))
}

/// Names in `git worktree prune --verbose` output: `Removing worktrees/x: why`.
fn pruned(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|line| line.strip_prefix("Removing "))
        .map(|rest| rest.split_once(": ").map_or(rest, |(name, _)| name))
        .map(|name| name.strip_prefix("worktrees/").unwrap_or(name).to_string())
        .collect()
}

/// Subfolders of `dir`; symlinks are skipped.
fn children(dir: &Path) -> impl Iterator<Item = PathBuf> + use<> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .map(|entry| entry.path())
}

fn run(cwd: &Path, args: &[&str], timeout: Duration) -> Result<Output, String> {
    let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    git::run(cwd, &args, timeout)
}

/// `path` resolved through symlinks, when it's one of aether's linked worktrees
/// (or a missing slot for one) under one of `roots`; never a root, a repo, or
/// anything outside them.
fn managed(roots: &[PathBuf], path: &Path) -> Result<PathBuf, String> {
    let outside =
        || format!("{} isn't one of aether's worktrees, so it wasn't touched.", path.display());
    let gone = |p: &Path| std::fs::symlink_metadata(p).is_err();
    let name = path.file_name().ok_or_else(outside)?;
    let resolved = match path.parent().and_then(|p| p.canonicalize().ok()) {
        Some(parent) => match parent.join(name) {
            slot if gone(&slot) => slot,
            slot => slot.canonicalize().map_err(|_| outside())?,
        },
        // its repo's folder under the root is gone too: nothing on disk to resolve or
        // delete, only git's record, so matching the path as written is enough
        None if path.is_absolute() && gone(path) && path.parent().is_some_and(gone) => {
            path.to_path_buf()
        }
        None => return Err(outside()),
    };
    if !roots
        .iter()
        .filter_map(|root| root.canonicalize().ok())
        .any(|root| within(&root, &resolved))
    {
        return Err(outside());
    }
    if resolved.exists() && !git::branch_info(&resolved).is_some_and(|info| info.worktree) {
        return Err(format!(
            "{} isn't a linked git worktree, so it wasn't touched.",
            resolved.display()
        ));
    }
    Ok(resolved)
}

/// Branch-safe name from a prompt: lowercase ASCII words joined by dashes.
pub fn slug(prompt: &str) -> String {
    let mut out = String::new();
    for word in prompt.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()) {
        let gap = usize::from(!out.is_empty());
        if out.len() + gap + word.len() > MAX_SLUG {
            if out.is_empty() {
                out = word[..MAX_SLUG].to_ascii_lowercase();
            }
            break;
        }
        if gap == 1 {
            out.push('-');
        }
        out.push_str(&word.to_ascii_lowercase());
    }
    if out.is_empty() { "new-worktree".into() } else { out }
}

/// `slug`, else `slug-2` … `slug-100`, whichever isn't `taken`.
fn free_slug(slug: &str, taken: impl Fn(&str) -> bool) -> Option<String> {
    (1..=100)
        .map(|n| match n {
            1 => slug.to_string(),
            n => {
                let suffix = format!("-{n}");
                let keep = slug[..slug.len().min(MAX_SLUG - suffix.len())].trim_end_matches('-');
                format!("{keep}{suffix}")
            }
        })
        .find(|candidate| !taken(candidate))
}

/// Folder for one repo's worktrees: its name plus a hash of its path.
pub fn repo_dir(toplevel: &Path) -> String {
    let name = toplevel.file_name().map_or_else(|| "repo".into(), |n| n.to_string_lossy());
    // FNV-1a: stable across Rust releases, unlike DefaultHasher
    let hash = toplevel
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .fold(0x811c_9dc5_u32, |h, b| (h ^ u32::from(*b)).wrapping_mul(0x0100_0193));
    format!("{name}-{hash:08x}")
}

/// Whether `path` is a worktree slot exactly two levels under `root`; both canonical.
fn within(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|rel| {
        rel.components().count() == 2 && rel.components().all(|c| matches!(c, Component::Normal(_)))
    })
}

/// Paths named in `git status --porcelain` output.
fn changed_paths(porcelain: &str) -> Vec<String> {
    porcelain
        .lines()
        .filter_map(|line| line.get(3..))
        .map(|path| path.rsplit(" -> ").next().unwrap_or(path).trim_matches('"').to_string())
        .filter(|path| !path.is_empty())
        .collect()
}

/// One worktree in `git worktree list --porcelain` output.
#[derive(Debug, Default, PartialEq, Eq)]
struct Entry {
    path: PathBuf,
    branch: Option<String>,
    head: String,
    locked: Option<String>,
}

/// Worktrees in `git worktree list --porcelain` output, the main checkout first.
fn entries(porcelain: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for line in porcelain.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            out.push(Entry { path: path.into(), ..Entry::default() });
        } else if let Some(entry) = out.last_mut() {
            if let Some(head) = line.strip_prefix("HEAD ") {
                entry.head = head.into();
            } else if let Some(branch) = line.strip_prefix("branch ") {
                entry.branch = Some(branch.strip_prefix("refs/heads/").unwrap_or(branch).into());
            } else if line == "locked" || line.starts_with("locked ") {
                entry.locked = Some(line["locked".len()..].trim().into());
            }
        }
    }
    out
}

/// The default, then the checked-out branch, then the rest by recency, up to [`MAX_BASES`].
fn order_bases(all: Vec<String>, default: Option<&str>, current: &str) -> Vec<String> {
    let mut front: Vec<String> = [default, Some(current)]
        .into_iter()
        .flatten()
        .filter(|name| all.iter().any(|b| b == name))
        .map(String::from)
        .collect();
    front.dedup();
    let rest: Vec<String> = all.into_iter().filter(|b| !front.contains(b)).collect();
    front.into_iter().chain(rest).take(MAX_BASES).collect()
}

/// `origin/HEAD`'s branch, else the current one, when it's a local branch.
fn default_base(origin_head: &str, current: &str, branches: &[String]) -> Option<String> {
    [origin_head.trim().strip_prefix("origin/"), Some(current.trim())]
        .into_iter()
        .flatten()
        .find(|name| branches.iter().any(|b| b == name))
        .map(String::from)
        .or_else(|| branches.first().cloned())
}

#[cfg(test)]
#[path = "../tests/unit/worktree.rs"]
mod tests;
