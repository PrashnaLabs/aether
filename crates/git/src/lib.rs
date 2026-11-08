mod hunk;
mod refs;
pub mod worktree;

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use aether_core::find_binary;

pub use hunk::{HunkOp, apply_hunk};
pub use refs::{
    Branches, Commit, Flow, Stash, branches, commit_message, history, show, stash_show,
};
pub use worktree::{Bases, Listed, RemoveCheck, RemoveError, Worktree};

const TIMEOUT: Duration = Duration::from_secs(3);
/// Most stdout a git command keeps; longer output is cut here.
pub const STDOUT_CAP: u64 = 1 << 20;
/// Most stdout [`ls_files`] keeps: a big repo lists far past [`STDOUT_CAP`].
const LS_FILES_CAP: u64 = 64 << 20;
const STDERR_CAP: u64 = 256 << 10;
const QUICK: Duration = Duration::from_secs(10);
const SLOW: Duration = Duration::from_secs(120);
/// Untracked files whose lines are counted, and the bytes read doing it.
const COUNTED_FILES: usize = 200;
const COUNTED_BYTES: u64 = 1 << 20;
/// Most of a failed commit or push shown to the user.
const REPORT_LINES: usize = 40;
const REPORT_BYTES: usize = 8 << 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchInfo {
    /// Branch name, or the short commit when detached.
    pub branch: String,
    pub detached: bool,
    /// The folder is a linked worktree, not the repo's main checkout.
    pub worktree: bool,
}

/// Branch checked out where `cwd` lives; `None` outside a repo or on any failure.
/// Blocking: run it off the UI thread.
pub fn branch_info(cwd: &Path) -> Option<BranchInfo> {
    let args = [
        "rev-parse",
        "--abbrev-ref",
        "HEAD",
        "--path-format=absolute",
        "--git-dir",
        "--git-common-dir",
    ];
    let mut info = parse(&git(cwd, &args)?)?;
    if info.detached
        && let Some(sha) = git(cwd, &["rev-parse", "--short", "HEAD"])
    {
        info.branch = sha.trim().to_string();
    }
    Some(info)
}

/// Reads `rev-parse --abbrev-ref HEAD --git-dir --git-common-dir` output.
fn parse(out: &str) -> Option<BranchInfo> {
    let mut lines = out.lines().map(str::trim);
    let mut next = || lines.next().filter(|line| !line.is_empty());
    let (head, git_dir, common_dir) = (next()?, next()?, next()?);
    Some(BranchInfo {
        branch: head.to_string(),
        detached: head == "HEAD",
        worktree: Path::new(git_dir) != Path::new(common_dir),
    })
}

/// The git to run, found once; `None` turns git off for the session.
fn binary() -> Option<&'static Path> {
    static GIT: OnceLock<Option<PathBuf>> = OnceLock::new();
    GIT.get_or_init(|| {
        let git = find_binary("git", "")?;
        // without the developer tools this is a shim that opens an install dialog on every call
        let shim = cfg!(target_os = "macos") && git == Path::new("/usr/bin/git");
        (!shim || developer_tools()).then_some(git)
    })
    .as_deref()
}

/// Whether `xcode-select` points at an install that ships git.
fn developer_tools() -> bool {
    Command::new("/usr/bin/xcode-select")
        .arg("-p")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())
        .is_some_and(|out| {
            Path::new(String::from_utf8_lossy(&out.stdout).trim()).join("usr/bin/git").is_file()
        })
}

/// Stdout of a successful `git args…` in `cwd`, or `None` after `TIMEOUT`.
fn git(cwd: &Path, args: &[&str]) -> Option<String> {
    let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    run(cwd, &args, TIMEOUT).ok().filter(|out| out.ok).map(|out| out.stdout)
}

/// Stdout of `git ls-files args…` in `cwd`, cut at [`LS_FILES_CAP`]; `None` outside a
/// repo or on any failure. Blocking: run it off the UI thread.
pub fn ls_files(cwd: &Path, args: &[&str]) -> Option<String> {
    let args: Vec<&OsStr> = ["ls-files"].iter().chain(args).map(OsStr::new).collect();
    let out = run_with(cwd, &args, QUICK, &AtomicBool::new(false), &[], LS_FILES_CAP);
    out.ok().filter(|out| out.ok).map(|out| out.stdout)
}

/// What a git command printed, cut at the output caps.
#[derive(Debug)]
pub(crate) struct Output {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// What a cancelled run returns.
pub const CANCELED: &str = "Canceled.";

/// Runs git in `cwd`, stopping it after `timeout`. Blocking: run it off the UI thread.
pub(crate) fn run(cwd: &Path, args: &[&OsStr], timeout: Duration) -> Result<Output, String> {
    run_until(cwd, args, timeout, &AtomicBool::new(false))
}

/// [`run`] that also stops once `cancel` is set, returning [`CANCELED`].
pub(crate) fn run_until(
    cwd: &Path,
    args: &[&OsStr],
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<Output, String> {
    run_with(cwd, args, timeout, cancel, &[], STDOUT_CAP)
}

/// [`run_until`] with extra environment variables, keeping `cap` bytes of stdout.
fn run_with(
    cwd: &Path,
    args: &[&OsStr],
    timeout: Duration,
    cancel: &AtomicBool,
    envs: &[(&str, &str)],
    cap: u64,
) -> Result<Output, String> {
    let git = binary()
        .ok_or("Git isn't available. Install git or the Xcode Command Line Tools, then retry.")?;
    let mut child = aether_core::command(git)
        .current_dir(cwd)
        // an inherited GIT_DIR would point every command at some other repo
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env("LC_ALL", "C")
        // nobody can answer a prompt: fail instead of waiting on a terminal or a dialog
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_ASKPASS")
        .env_remove("SSH_ASKPASS")
        .envs(envs.iter().copied())
        .args(["-c", "core.quotePath=false"])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't run git in {}: {e}", cwd.display()))?;
    let stdout = drain(child.stdout.take(), cap);
    let stderr = drain(child.stderr.take(), STDERR_CAP);
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline && !cancel.load(Ordering::Relaxed) => {
                std::thread::sleep(Duration::from_millis(10))
            }
            _ => {
                stop(child);
                return Err(if cancel.load(Ordering::Relaxed) {
                    CANCELED.into()
                } else {
                    format!(
                        "Git took longer than {} s and was stopped, so it may have left partial changes behind.",
                        timeout.as_secs()
                    )
                });
            }
        }
    };
    // a hook's leftover child can keep the pipes open, so don't wait on them for long
    let collect = |rx: Receiver<Vec<u8>>| {
        String::from_utf8_lossy(&rx.recv_timeout(Duration::from_secs(1)).unwrap_or_default())
            .into_owned()
    };
    Ok(Output { ok: status.success(), stdout: collect(stdout), stderr: collect(stderr) })
}

/// SIGTERM first so git can clean up after itself (a half-made worktree, lock
/// files), then SIGKILL if it hasn't exited after a short grace.
fn stop(mut child: Child) {
    aether_core::signal_process(child.id(), "TERM", false);
    let grace = Instant::now() + Duration::from_secs(2);
    while Instant::now() < grace {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    // reaping can block on a hung mount; don't hold the caller for it
    std::thread::spawn(move || child.wait());
}

/// Reads `pipe` to the end on a thread, keeping the first `cap` bytes.
fn drain(pipe: Option<impl Read + Send + 'static>, cap: u64) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    if let Some(mut pipe) = pipe {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let _ = pipe.by_ref().take(cap).read_to_end(&mut kept);
            let _ = std::io::copy(&mut pipe, &mut std::io::sink());
            let _ = tx.send(kept);
        });
    }
    rx
}

/// Git's complaint, trimmed to a few lines for the UI.
pub(crate) fn git_error(out: &Output) -> String {
    let lines: Vec<&str> = out
        .stderr
        .lines()
        .map(|line| line.trim_start_matches("fatal: ").trim_start_matches("error: ").trim())
        .filter(|line| !line.is_empty())
        .collect();
    match lines.len() {
        0 => "Git failed without saying why.".into(),
        1..=3 => lines.join("\n"),
        _ => format!("{}\n…", lines[..3].join("\n")),
    }
}

/// What a commit or push printed when it failed, as git wrote it (hooks
/// print to both streams), cut to [`REPORT_LINES`] and [`REPORT_BYTES`].
fn report(out: &Output) -> String {
    let text = [out.stderr.trim(), out.stdout.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        return "Git failed without saying why.".into();
    }
    let mut kept = String::new();
    for (n, line) in text.lines().enumerate() {
        if n == REPORT_LINES {
            kept.push_str("\n…");
            break;
        }
        if n > 0 {
            kept.push('\n');
        }
        let room = REPORT_BYTES.saturating_sub(kept.len());
        if line.len() > room {
            let end = (0..=room).rev().find(|&i| line.is_char_boundary(i)).unwrap_or(0);
            kept.push_str(&line[..end]);
            kept.push('…');
            break;
        }
        kept.push_str(line);
    }
    kept
}

/// The repo a chat folder is in, read in one pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// The work tree's top folder; paths in `changes` are relative to it.
    pub top: PathBuf,
    /// Checked-out branch; `None` when HEAD is detached.
    pub branch: Option<String>,
    /// HEAD's commit, abbreviated; `None` before the first commit.
    pub commit: Option<String>,
    pub upstream: Option<String>,
    /// Commits ahead of and behind `upstream`, when git could count them.
    pub ahead_behind: Option<(u32, u32)>,
    /// Staged changes and changes in the work tree, in git's order.
    pub changes: Vec<Change>,
    /// The list stopped at the output cap.
    pub truncated: bool,
    /// Where a branch without an upstream would be pushed.
    pub remote: Option<String>,
    /// HEAD has a parent commit, so it can be undone.
    pub parent: bool,
    pub stashes: Vec<Stash>,
}

/// One file's change in the index (`staged`) or the work tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    /// Where a rename or copy came from.
    pub from: Option<String>,
    pub kind: ChangeKind,
    pub staged: bool,
    pub stat: Stat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Untracked,
    Conflicted,
}

impl ChangeKind {
    fn from_code(code: char) -> Self {
        match code {
            'A' => Self::Added,
            'D' => Self::Deleted,
            'R' => Self::Renamed,
            'C' => Self::Copied,
            'T' => Self::TypeChanged,
            _ => Self::Modified,
        }
    }

    /// Git's one-letter code, `?` for untracked and `U` for conflicted.
    pub fn letter(self) -> char {
        match self {
            Self::Modified => 'M',
            Self::Added => 'A',
            Self::Deleted => 'D',
            Self::Renamed => 'R',
            Self::Copied => 'C',
            Self::TypeChanged => 'T',
            Self::Untracked => '?',
            Self::Conflicted => 'U',
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Modified => "Modified",
            Self::Added => "Added",
            Self::Deleted => "Deleted",
            Self::Renamed => "Renamed",
            Self::Copied => "Copied",
            Self::TypeChanged => "Type changed",
            Self::Untracked => "Untracked",
            Self::Conflicted => "Conflicted",
        }
    }
}

/// Lines added and removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stat {
    /// Not counted: an untracked file past the counting budget, or a non-file.
    #[default]
    Unknown,
    Lines(u32, u32),
    Binary,
}

/// Why a folder's changes couldn't be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatusError {
    /// Another git command holds the index; try again shortly.
    Locked,
    NotRepo,
    Failed(String),
}

/// Reads `git status --porcelain=2 --branch -z` output.
pub(crate) fn parse_status(out: &str) -> Status {
    let mut status = Status::default();
    let mut records = out.split('\0');
    while let Some(record) = records.next() {
        if let Some(header) = record.strip_prefix("# ") {
            match header.split_once(' ') {
                Some(("branch.oid", oid)) => {
                    status.commit = (oid != "(initial)").then(|| oid.chars().take(7).collect())
                }
                Some(("branch.head", head)) => {
                    status.branch = (head != "(detached)").then(|| head.to_string())
                }
                Some(("branch.upstream", upstream)) => status.upstream = Some(upstream.to_string()),
                Some(("branch.ab", ab)) => status.ahead_behind = parse_ahead_behind(ab),
                _ => {}
            }
            continue;
        }
        let Some((tag, rest)) = record.split_once(' ') else {
            continue;
        };
        // the path is the last field and may hold spaces
        let fields =
            |n: usize| Some(rest.splitn(n, ' ').collect::<Vec<_>>()).filter(|f| f.len() == n);
        let changes = &mut status.changes;
        match tag {
            "1" => {
                if let Some(f) = fields(8) {
                    add_pair(changes, f[0], f[7], None);
                }
            }
            "2" => {
                // -z puts the rename's origin in its own record
                let from = records.next().filter(|from| !from.is_empty());
                if let Some(f) = fields(9) {
                    add_pair(changes, f[0], f[8], from);
                }
            }
            "u" => {
                if let Some(f) = fields(10) {
                    changes.push(change(f[9], None, ChangeKind::Conflicted, false));
                }
            }
            "?" => changes.push(change(rest, None, ChangeKind::Untracked, false)),
            _ => {}
        }
    }
    status
}

fn change(path: &str, from: Option<&str>, kind: ChangeKind, staged: bool) -> Change {
    let from = from.filter(|_| matches!(kind, ChangeKind::Renamed | ChangeKind::Copied));
    Change { path: path.into(), from: from.map(String::from), kind, staged, stat: Stat::Unknown }
}

/// The staged (X) and work-tree (Y) halves of a status `XY` code; `.` is unchanged.
fn add_pair(changes: &mut Vec<Change>, xy: &str, path: &str, from: Option<&str>) {
    let mut codes = xy.chars();
    for (code, staged) in [(codes.next(), true), (codes.next(), false)] {
        if let Some(code) = code.filter(|c| *c != '.') {
            changes.push(change(path, from, ChangeKind::from_code(code), staged));
        }
    }
}

/// `+3 -1` from a `branch.ab` header.
fn parse_ahead_behind(ab: &str) -> Option<(u32, u32)> {
    let (ahead, behind) = ab.split_once(' ')?;
    Some((ahead.strip_prefix('+')?.parse().ok()?, behind.strip_prefix('-')?.parse().ok()?))
}

/// Reads `git diff --numstat -z` output: stats by (new) path.
pub(crate) fn parse_numstat(out: &str) -> HashMap<String, Stat> {
    let mut stats = HashMap::new();
    let mut records = out.split('\0');
    while let Some(record) = records.next() {
        let mut parts = record.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        // a rename leaves the path empty; the old and new paths follow as records
        let path = match path {
            "" => match (records.next(), records.next()) {
                (Some(_), Some(new)) if !new.is_empty() => new,
                _ => break,
            },
            path => path,
        };
        let stat = match (added.parse(), removed.parse()) {
            (Ok(added), Ok(removed)) => Stat::Lines(added, removed),
            _ if added == "-" => Stat::Binary,
            _ => Stat::Unknown,
        };
        stats.insert(path.to_string(), stat);
    }
    stats
}

/// The remote to publish a new branch to: `origin`, else the only one.
fn pick_remote(remotes: &str) -> Option<String> {
    let names: Vec<&str> = remotes.lines().map(str::trim).filter(|name| !name.is_empty()).collect();
    match names[..] {
        [only] => Some(only.to_string()),
        _ => names.iter().find(|name| **name == "origin").map(|name| name.to_string()),
    }
}

/// A path from git's own output, checked before it goes back on a command line.
fn safe_path(path: &str) -> Result<&str, String> {
    let bad = path.is_empty()
        || path.contains('\0')
        || Path::new(path).is_absolute()
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir));
    if bad {
        Err(format!("Refused the path “{path}”: it isn't inside the repository."))
    } else {
        Ok(path)
    }
}

/// Lines in an untracked file, reading at most `budget` bytes across calls.
/// Only regular files: a FIFO or device could block or never end.
fn count_lines(path: &Path, budget: &mut u64) -> Stat {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Stat::Unknown;
    };
    if !meta.is_file() || meta.len() > *budget {
        return Stat::Unknown;
    }
    let Ok(bytes) = std::fs::read(path) else {
        return Stat::Unknown;
    };
    *budget = budget.saturating_sub(bytes.len() as u64);
    // git's own test: a NUL in the first 8000 bytes means binary
    if bytes[..bytes.len().min(8000)].contains(&0) {
        return Stat::Binary;
    }
    let newlines = bytes.iter().filter(|b| **b == b'\n').count();
    let partial = bytes.last().is_some_and(|b| *b != b'\n');
    Stat::Lines((newlines + usize::from(partial)) as u32, 0)
}

pub fn switch_branch(cwd: &Path, branch: &str) -> Result<(), String> {
    if branch.is_empty() || branch.starts_with('-') || branch.contains('\0') {
        return Err("Choose an existing local branch.".into());
    }
    let local = format!("refs/heads/{branch}");
    for args in [
        vec!["check-ref-format", &local],
        vec!["show-ref", "--verify", &local],
        vec!["switch", "--no-guess", "--", branch],
    ] {
        let out = run_args(cwd, &args, SLOW)?;
        if !out.ok {
            return Err(git_error(&out));
        }
    }
    Ok(())
}

fn run_args(cwd: &Path, args: &[&str], timeout: Duration) -> Result<Output, String> {
    let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    run(cwd, &args, timeout)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

pub fn parse_remotes(out: &str) -> Vec<Remote> {
    let mut remotes: Vec<Remote> = Vec::new();
    for line in out.lines() {
        let Some((name, rest)) = line.split_once('\t') else { continue };
        let Some(url) = rest.strip_suffix(" (fetch)") else { continue };
        if !remotes.iter().any(|r| r.name == name) {
            remotes.push(Remote { name: name.into(), url: url.into() });
        }
    }
    remotes
}

pub fn remotes(top: &Path) -> Result<Vec<Remote>, String> {
    let out = run_args(top, &["remote", "-v"], QUICK)?;
    if !out.ok {
        return Err(git_error(&out));
    }
    Ok(parse_remotes(&out.stdout))
}

fn split_url(url: &str) -> Option<(&str, &str)> {
    let url = url.trim();
    let (host, path) = if let Some((scheme, rest)) = url.split_once("://") {
        let (auth, path) = rest.split_once('/')?;
        let host = auth.rsplit('@').next()?;
        let web = scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https");
        (if web { host } else { host.split(':').next()? }, path)
    } else {
        let (auth, path) = url.split_once(':')?;
        if auth.len() == 1 && path.starts_with('/') {
            return None;
        }
        (auth.rsplit('@').next()?, path)
    };
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    (!host.is_empty() && !host.contains('/') && path.contains('/')).then_some((host, path))
}

pub fn https_url(url: &str) -> Option<String> {
    split_url(url).map(|(host, path)| format!("https://{host}/{path}"))
}

pub fn owner_repo(url: &str) -> Option<String> {
    split_url(url).map(|(_, path)| path.into())
}

pub fn init(folder: &Path) -> Result<(), String> {
    let out = run_args(folder, &["init"], QUICK)?;
    if out.ok { Ok(()) } else { Err(git_error(&out)) }
}

/// The changes in the repo `cwd` is in, with line counts. Blocking.
pub fn status(cwd: &Path) -> Result<Status, StatusError> {
    let failed = StatusError::Failed;
    let found = run_args(cwd, &["rev-parse", "--show-toplevel", "--absolute-git-dir"], QUICK)
        .map_err(failed)?;
    let mut lines = found.stdout.lines();
    let (Some(top), Some(git_dir), true) = (lines.next(), lines.next(), found.ok) else {
        return Err(StatusError::NotRepo);
    };
    if Path::new(git_dir).join("index.lock").exists() {
        return Err(StatusError::Locked);
    }
    let top = PathBuf::from(top);
    let args = [
        "--no-optional-locks",
        "status",
        "--porcelain=2",
        "--branch",
        "-z",
        "--untracked-files=all",
        "--find-renames",
    ];
    let out = run_args(&top, &args, QUICK).map_err(failed)?;
    if !out.ok {
        return Err(failed(git_error(&out)));
    }
    let truncated = out.stdout.len() as u64 >= STDOUT_CAP;
    // a cut stream ends mid-record; keep whole ones
    let whole = if truncated {
        &out.stdout[..out.stdout.rfind('\0').map_or(0, |end| end + 1)]
    } else {
        &out.stdout[..]
    };
    let mut status = parse_status(whole);
    let numstat = |cached: bool| {
        let mut args = vec![
            "--no-optional-locks",
            "diff",
            "--numstat",
            "-z",
            "--find-renames",
            "--no-ext-diff",
            "--no-textconv",
        ];
        if cached {
            args.push("--cached");
        }
        run_args(&top, &args, QUICK)
            .ok()
            .filter(|out| out.ok)
            .map(|out| parse_numstat(&out.stdout))
            .unwrap_or_default()
    };
    let (staged, unstaged) = (numstat(true), numstat(false));
    let mut budget = COUNTED_BYTES;
    let mut counted = 0;
    for change in &mut status.changes {
        change.stat = match change.kind {
            ChangeKind::Untracked if counted < COUNTED_FILES => {
                counted += 1;
                count_lines(&top.join(&change.path), &mut budget)
            }
            ChangeKind::Untracked => Stat::Unknown,
            _ if change.staged => staged.get(&change.path).copied().unwrap_or_default(),
            _ => unstaged.get(&change.path).copied().unwrap_or_default(),
        };
    }
    if status.upstream.is_none() && status.branch.is_some() && status.commit.is_some() {
        status.remote =
            run_args(&top, &["remote"], QUICK).ok().and_then(|out| pick_remote(&out.stdout));
    }
    status.parent = status.commit.is_some()
        && run_args(&top, &["rev-parse", "--verify", "--quiet", "HEAD~1"], QUICK)
            .is_ok_and(|out| out.ok);
    status.stashes = refs::stashes(&top);
    status.top = top;
    status.truncated = truncated;
    Ok(status)
}

/// One file's unified diff: staged, in the work tree, or a whole untracked
/// file. `top` is the work tree's top folder. Blocking.
pub fn diff(top: &Path, change: &Change) -> Result<String, String> {
    let path = safe_path(&change.path)?;
    let mut args = vec![
        "--literal-pathspecs",
        "--no-optional-locks",
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        // hunk patches apply with -p1, so a user's diff.noprefix can't change the paths
        "--src-prefix=a/",
        "--dst-prefix=b/",
    ];
    if change.kind == ChangeKind::Untracked {
        let meta = std::fs::symlink_metadata(top.join(path))
            .map_err(|e| format!("Couldn't read {path}: {e}"))?;
        if !meta.is_file() && !meta.is_symlink() {
            return Err(format!("{path} isn't a regular file, so there's no diff to show."));
        }
        args.extend(["--no-index", "--", "/dev/null", path]);
        let out = run_args(top, &args, QUICK)?;
        // --no-index exits 1 when the files differ, which they always do here
        return if out.stdout.is_empty() { Err(git_error(&out)) } else { Ok(out.stdout) };
    }
    args.push("--find-renames");
    if change.staged {
        args.push("--cached");
    }
    // a conflict's own diff is a combined one; against HEAD it reads as a plain
    // diff that shows the conflict markers
    if change.kind == ChangeKind::Conflicted {
        args.push("HEAD");
    }
    args.extend(["--", path]);
    // both sides of a rename, or git shows only an added file
    if let Some(from) = &change.from {
        args.push(safe_path(from)?);
    }
    let out = run_args(top, &args, QUICK)?;
    if out.ok { Ok(out.stdout) } else { Err(git_error(&out)) }
}

/// A change to the repo the git panel asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitOp {
    Stage(Vec<String>),
    /// Stages every change but conflicts: staging one marks it resolved.
    StageAll,
    Unstage(Vec<String>),
    UnstageAll,
    /// Throws away work-tree changes: tracked files go back to their staged
    /// state, untracked ones are deleted.
    Discard(Vec<String>),
    /// [`GitOp::Discard`] for a whole section. It names the files the user
    /// confirmed, so one written since isn't lost with them.
    DiscardAll(Vec<String>),
    Commit {
        message: String,
        stage_all: bool,
    },
    /// Folds the staged changes into HEAD; an empty message keeps HEAD's.
    Amend {
        message: String,
    },
    /// Moves HEAD back a commit, keeping its changes staged. `Ok` carries
    /// the undone commit's message.
    UndoCommit,
    Push,
    Fetch,
    Pull,
    /// Pull, then push.
    Sync,
    /// Stashes the changes, untracked files too when asked; an empty
    /// message is git's default.
    Stash {
        message: String,
        include_untracked: bool,
    },
    /// Applies `stash@{index}`, refused unless its commit is still `sha`.
    StashApply {
        index: usize,
        sha: String,
    },
    /// [`GitOp::StashApply`], then drops the stash.
    StashPop {
        index: usize,
        sha: String,
    },
    StashDrop {
        index: usize,
        sha: String,
    },
    /// Drops every stash, refused unless they're still these commits, newest first.
    StashClear(Vec<String>),
    /// Switches to a local branch.
    Checkout(String),
    /// Switches to a new local branch tracking the remote branch `origin/x`.
    Track(String),
    /// Makes branch `name` at `base` (a full ref or commit id), else at HEAD, and switches to it.
    CreateBranch {
        name: String,
        base: Option<String>,
    },
    /// Checks out a commit, by its full id, with HEAD detached.
    Detach(String),
}

/// Paths per git command, far below any argument-length limit.
const PATH_CHUNK: usize = 100;

/// Runs `op` in the work tree whose top folder is `top`. `Ok` carries a
/// short note for the user (empty for staging and discarding). Discards only
/// the files a discard names, rewrites only commits no remote branch has,
/// never force-pushes, force-switches or skips hooks. Blocking.
pub fn apply(top: &Path, op: &GitOp) -> Result<String, String> {
    let unborn =
        || run_args(top, &["rev-parse", "--verify", "--quiet", "HEAD"], QUICK).map(|out| !out.ok);
    match op {
        GitOp::Stage(paths) => checked(with_paths(top, &["add"], paths)?),
        GitOp::StageAll => add_all(top),
        // before the first commit there's no HEAD to restore from; dropping the
        // index entry is the same thing, and --cached never touches the file
        GitOp::Unstage(paths) if unborn()? => {
            checked(with_paths(top, &["rm", "--cached", "--force", "--quiet"], paths)?)
        }
        GitOp::Unstage(paths) => checked(with_paths(top, &["restore", "--staged"], paths)?),
        GitOp::UnstageAll if unborn()? => checked(with_paths(
            top,
            &["rm", "--cached", "--force", "--quiet", "-r"],
            &[".".into()],
        )?),
        GitOp::UnstageAll => checked(with_paths(top, &["restore", "--staged"], &[".".into()])?),
        GitOp::Discard(paths) | GitOp::DiscardAll(paths) => discard(top, paths),
        GitOp::Commit { message, stage_all } => {
            let message = message.replace('\0', "");
            if message.trim().is_empty() {
                return Err("Write a commit message first.".into());
            }
            if *stage_all {
                add_all(top)?;
            }
            commit(top, &["commit", "-m", &message], "Committed")
        }
        GitOp::Amend { message } => {
            rewritable(top, unborn()?, "amended")?;
            let message = message.replace('\0', "");
            let mut args = vec!["commit", "--amend"];
            if message.trim().is_empty() {
                args.push("--no-edit");
            } else {
                args.extend(["-m", &message]);
            }
            commit(top, &args, "Amended")
        }
        GitOp::UndoCommit => undo(top, unborn()?),
        GitOp::Push => push(top),
        GitOp::Fetch => {
            let out = run_remote(top, &["fetch", "--prune"])?;
            if out.ok { Ok("Fetched".into()) } else { Err(report(&out)) }
        }
        GitOp::Pull => pull(top).map(|upstream| format!("Pulled from {upstream}")),
        GitOp::Sync => {
            let upstream = pull(top)?;
            push(top).map_err(|e| format!("Pulled from {upstream}, but the push failed.\n{e}"))?;
            Ok(format!("Synced with {upstream}"))
        }
        GitOp::Stash { message, include_untracked } => {
            refs::stash_push(top, message, *include_untracked)
        }
        GitOp::StashApply { index, sha } => {
            refs::stash_act(top, "apply", *index, sha).map(|name| format!("Applied {name}"))
        }
        GitOp::StashPop { index, sha } => {
            refs::stash_act(top, "pop", *index, sha).map(|name| format!("Popped {name}"))
        }
        GitOp::StashDrop { index, sha } => {
            refs::stash_act(top, "drop", *index, sha).map(|name| format!("Dropped {name}"))
        }
        GitOp::StashClear(shas) => refs::stash_clear(top, shas),
        GitOp::Checkout(branch) => {
            switch_branch(top, branch).map(|()| format!("Switched to {branch}"))
        }
        GitOp::Track(remote) => refs::track(top, remote),
        GitOp::CreateBranch { name, base } => refs::create_branch(top, name, base.as_deref()),
        GitOp::Detach(sha) => refs::detach(top, sha),
    }
}

fn checked(out: Output) -> Result<String, String> {
    if out.ok { Ok(String::new()) } else { Err(git_error(&out)) }
}

/// `git --literal-pathspecs <head…> -- <paths…>`, each path checked first.
fn with_paths(top: &Path, head: &[&str], paths: &[String]) -> Result<Output, String> {
    let mut args = vec!["--literal-pathspecs"];
    args.extend(head);
    args.push("--");
    for path in paths {
        args.push(safe_path(path)?);
    }
    run_args(top, &args, SLOW)
}

/// `add --all`, leaving conflicts out: marking one resolved takes its own click.
fn add_all(top: &Path) -> Result<String, String> {
    let unmerged = run_args(top, &["diff", "--name-only", "--diff-filter=U", "-z"], QUICK)?;
    if !unmerged.ok {
        return Err(git_error(&unmerged));
    }
    let unmerged: HashSet<&str> = unmerged.stdout.split('\0').filter(|p| !p.is_empty()).collect();
    if unmerged.is_empty() {
        return checked(run_args(top, &["add", "--all"], SLOW)?);
    }
    // add resolves conflicts even past an exclude pathspec, so name the rest
    let args = ["ls-files", "-z", "--modified", "--deleted", "--others", "--exclude-standard"];
    let listed = run_args(top, &args, QUICK)?;
    if !listed.ok {
        return Err(git_error(&listed));
    }
    let mut paths: Vec<String> = listed
        .stdout
        .split('\0')
        .filter(|path| !path.is_empty() && !unmerged.contains(path))
        .map(String::from)
        .collect();
    paths.sort();
    paths.dedup();
    for chunk in paths.chunks(PATH_CHUNK) {
        checked(with_paths(top, &["add", "--all"], chunk)?)?;
    }
    Ok(String::new())
}

/// Restores tracked files from the index and deletes untracked ones and
/// intent-to-add ones (`add -N`), whose index copy is empty. Clean gets file
/// paths and neither -d nor -x, so it never removes a folder or an ignored
/// file; a nested repo would take -ff.
fn discard(top: &Path, paths: &[String]) -> Result<String, String> {
    for chunk in paths.chunks(PATH_CHUNK) {
        let others = ["ls-files", "-z", "--others", "--exclude-standard"];
        let listed = with_paths(top, &others, chunk)?;
        if !listed.ok {
            return Err(git_error(&listed));
        }
        let intent = ["diff", "--name-only", "--no-renames", "--diff-filter=A", "-z"];
        let intent = with_paths(top, &intent, chunk)?;
        if !intent.ok {
            return Err(git_error(&intent));
        }
        let intent: HashSet<&str> = intent.stdout.split('\0').collect();
        let (intent, rest): (Vec<String>, Vec<String>) =
            chunk.iter().cloned().partition(|path| intent.contains(path.as_str()));
        let new: HashSet<&str> = listed.stdout.split('\0').collect();
        let (untracked, tracked): (Vec<String>, Vec<String>) =
            rest.into_iter().partition(|path| new.contains(path.as_str()));
        if !tracked.is_empty() {
            checked(with_paths(top, &["restore", "--worktree"], &tracked)?)?;
        }
        if !intent.is_empty() {
            checked(with_paths(top, &["rm", "--cached", "--force", "--quiet"], &intent)?)?;
        }
        let untracked: Vec<String> = untracked.into_iter().chain(intent).collect();
        if !untracked.is_empty() {
            checked(with_paths(top, &["clean", "--force", "--quiet"], &untracked)?)?;
        }
    }
    Ok(String::new())
}

/// Runs a commit; `Ok` is "`done` <sha>", a failure what git and hooks said.
fn commit(top: &Path, args: &[&str], done: &str) -> Result<String, String> {
    let out = run_args(top, args, SLOW)?;
    if !out.ok {
        return Err(report(&out));
    }
    let sha = run_args(top, &["rev-parse", "--short", "HEAD"], QUICK)
        .map(|out| out.stdout.trim().to_string());
    Ok(format!("{done} {}", sha.unwrap_or_default()).trim_end().to_string())
}

/// Amend and undo replace HEAD, so only while no remote branch has it.
fn rewritable(top: &Path, unborn: bool, done: &str) -> Result<(), String> {
    if unborn {
        return Err("There's no commit yet.".into());
    }
    let args = [
        "for-each-ref",
        "--contains",
        "HEAD",
        "--count=1",
        "--format=%(refname:short)",
        "refs/remotes",
    ];
    let out = run_args(top, &args, QUICK)?;
    if !out.ok {
        return Err(git_error(&out));
    }
    match out.stdout.lines().next() {
        Some(remote) => {
            Err(format!("The last commit is already on {remote}, so it can't be {done}."))
        }
        None => Ok(()),
    }
}

/// `reset --soft HEAD~1`; `Ok` is the undone commit's message.
fn undo(top: &Path, unborn: bool) -> Result<String, String> {
    rewritable(top, unborn, "undone")?;
    if !run_args(top, &["rev-parse", "--verify", "--quiet", "HEAD~1"], QUICK)?.ok {
        return Err("This is the first commit, so there's nothing to go back to.".into());
    }
    let message =
        run_args(top, &["log", "-1", "--no-show-signature", "--format=%B", "HEAD"], QUICK)?;
    if !message.ok {
        return Err(git_error(&message));
    }
    checked(run_args(top, &["reset", "--soft", "HEAD~1"], SLOW)?)?;
    Ok(message.stdout.trim_end().to_string())
}

/// The checked-out branch and its upstream, if it has one.
fn tracking(top: &Path, verb: &str) -> Result<(String, Option<String>), String> {
    let branch = run_args(top, &["symbolic-ref", "--quiet", "--short", "HEAD"], QUICK)?;
    if !branch.ok {
        return Err(format!("HEAD is detached. Check out a branch to {verb}."));
    }
    let upstream =
        run_args(top, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"], QUICK)?;
    Ok((branch.stdout.trim().to_string(), upstream.ok.then(|| upstream.stdout.trim().to_string())))
}

/// Runs a fetch, pull or push. Credentials come only from helpers and agents.
fn run_remote(top: &Path, args: &[&str]) -> Result<Output, String> {
    // an empty askPass also stops git falling back to SSH_ASKPASS
    let mut full = vec!["-c", "core.askPass="];
    full.extend(args);
    let envs: Vec<_> = batch_ssh(top).into_iter().collect();
    let full: Vec<&OsStr> = full.iter().map(OsStr::new).collect();
    run_with(top, &full, SLOW, &AtomicBool::new(false), &envs, STDOUT_CAP)
}

/// ssh asks for passphrases and host keys on the terminal; batch mode fails
/// fast instead. A user's own ssh command is left alone (`None`).
pub fn batch_ssh(top: &Path) -> Option<(&'static str, &'static str)> {
    let own_ssh = ["GIT_SSH_COMMAND", "GIT_SSH"].iter().any(|var| std::env::var_os(var).is_some())
        || run_args(top, &["config", "--get", "core.sshCommand"], QUICK).is_ok_and(|out| out.ok);
    (!own_ssh).then_some(("GIT_SSH_COMMAND", "ssh -o BatchMode=yes"))
}

/// Pushes the checked-out branch: to its upstream, or to `origin` (else the
/// only remote) as a new upstream.
fn push(top: &Path) -> Result<String, String> {
    let (branch, upstream) = tracking(top, "push")?;
    let mut args = vec!["push"];
    let remote;
    let target = match upstream {
        Some(upstream) => upstream,
        None => {
            remote = pick_remote(&run_args(top, &["remote"], QUICK)?.stdout).ok_or(
                "This repository has no remote to push to. Add one in the terminal, then push.",
            )?;
            args.extend(["--set-upstream", &remote, &branch]);
            format!("{remote}/{branch}")
        }
    };
    let out = run_remote(top, &args)?;
    if out.ok { Ok(format!("Pushed to {target}")) } else { Err(report(&out)) }
}

/// Pulls the upstream in, merging or rebasing as the user's pull.rebase and
/// pull.ff say; a divergence they don't settle fails with git's advice.
/// `Ok` is the upstream's name.
fn pull(top: &Path) -> Result<String, String> {
    let (_, upstream) = tracking(top, "pull")?;
    let upstream = upstream.ok_or("This branch has no upstream to pull from. Publish it first.")?;
    let out = run_remote(top, &["pull", "--no-edit"])?;
    if out.ok { Ok(upstream) } else { Err(report(&out)) }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
