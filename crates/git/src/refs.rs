//! Stashes, branches and history: what the git panel lists besides changes.

use std::collections::HashSet;
use std::path::Path;

use crate::{QUICK, SLOW, git_error, report, run_args};

/// Commits the history lists from HEAD, and at most from the upstream.
const HISTORY: usize = 50;
const STASHES_CHANGED: &str = "Stashes changed; refresh and try again.";

/// A stash entry, `stash@{index}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stash {
    pub index: usize,
    /// The stash's commit; an op on `index` is refused once it's another one.
    pub sha: String,
    pub message: String,
    /// Branch it was made on; empty when git didn't say.
    pub branch: String,
    /// When it was made, in Unix seconds.
    pub time: i64,
}

/// Stashes, newest first; empty on any failure.
pub(crate) fn stashes(top: &Path) -> Vec<Stash> {
    let args = ["stash", "list", "--no-show-signature", "--format=%H%x00%ct%x00%gs"];
    run_args(top, &args, QUICK)
        .ok()
        .filter(|out| out.ok)
        .map(|out| parse_stashes(&out.stdout))
        .unwrap_or_default()
}

/// Reads `git stash list --format=%H%x00%ct%x00%gs` output: `On main: msg`
/// for a named stash, `WIP on main: a1b2c3d subject` for git's default.
fn parse_stashes(out: &str) -> Vec<Stash> {
    out.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let mut fields = line.splitn(3, '\0');
            let (sha, time, subject) = (fields.next()?, fields.next()?, fields.next()?);
            let (branch, message) = match subject.split_once(": ") {
                Some((head, rest)) => {
                    match (head.strip_prefix("On "), head.strip_prefix("WIP on ")) {
                        (Some(branch), _) => (branch, rest),
                        (_, Some(branch)) => (branch, subject),
                        _ => ("", subject),
                    }
                }
                None => ("", subject),
            };
            Some(Stash {
                index,
                sha: sha.into(),
                message: message.into(),
                branch: branch.into(),
                time: time.parse().ok()?,
            })
        })
        .collect()
}

/// `git stash push`, with untracked files when asked; an empty message is git's default.
pub(crate) fn stash_push(top: &Path, message: &str, untracked: bool) -> Result<String, String> {
    let message = message.replace('\0', "");
    let mut args = vec!["stash", "push"];
    if untracked {
        args.push("--include-untracked");
    }
    if !message.trim().is_empty() {
        args.extend(["-m", &message]);
    }
    let out = run_args(top, &args, SLOW)?;
    if !out.ok {
        return Err(report(&out));
    }
    if out.stdout.contains("No local changes to save") {
        return Err("There are no changes to stash.".into());
    }
    Ok("Stashed changes".into())
}

/// `git stash <verb> stash@{index}` (apply, pop or drop), once that's still
/// the stash `sha`: another stash made or dropped since shifts the index.
pub(crate) fn stash_act(top: &Path, verb: &str, index: usize, sha: &str) -> Result<String, String> {
    let name = format!("stash@{{{index}}}");
    let found =
        run_args(top, &["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")], QUICK)?;
    if !found.ok || sha.is_empty() || found.stdout.trim() != sha {
        return Err(STASHES_CHANGED.into());
    }
    let out = run_args(top, &["stash", verb, &name], SLOW)?;
    if out.ok { Ok(name) } else { Err(report(&out)) }
}

/// `git stash clear`, once the stashes are still exactly `shas`, newest first.
pub(crate) fn stash_clear(top: &Path, shas: &[String]) -> Result<String, String> {
    let listed: Vec<String> = stashes(top).into_iter().map(|s| s.sha).collect();
    if shas.is_empty() || listed != shas {
        return Err(STASHES_CHANGED.into());
    }
    let out = run_args(top, &["stash", "clear"], SLOW)?;
    if out.ok { Ok("Dropped all stashes".into()) } else { Err(report(&out)) }
}

/// Stash `sha`'s changes, untracked files too, as [`show`] prints them. Blocking.
pub fn stash_show(top: &Path, sha: &str) -> Result<String, String> {
    // stash show mangles --src-prefix; ^1 is the base, ^3 the untracked files
    let mut text = show(top, sha)?;
    let untracked =
        run_args(top, &["rev-parse", "--verify", "--quiet", &format!("{sha}^3")], QUICK)?;
    if untracked.ok {
        text.push_str(&show(top, untracked.stdout.trim())?);
    }
    Ok(text)
}

/// Local and remote-tracking branches, each most recently committed first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Branches {
    /// The checked-out branch; `None` when HEAD is detached.
    pub current: Option<String>,
    pub local: Vec<String>,
    /// `origin/x` names, without the remotes' `HEAD` aliases.
    pub remote: Vec<String>,
}

/// The repo's branches. Blocking.
pub fn branches(top: &Path) -> Result<Branches, String> {
    let args = [
        "for-each-ref",
        "--sort=-committerdate",
        "--format=%(HEAD)%00%(refname)%00%(symref)",
        "refs/heads",
        "refs/remotes",
    ];
    let out = run_args(top, &args, QUICK)?;
    if out.ok { Ok(parse_branches(&out.stdout)) } else { Err(git_error(&out)) }
}

/// Reads `for-each-ref --format=%(HEAD)%00%(refname)%00%(symref)` output.
fn parse_branches(out: &str) -> Branches {
    let mut found = Branches::default();
    for line in out.lines() {
        let mut fields = line.split('\0');
        let (Some(head), Some(name), Some(symref)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if let Some(local) = name.strip_prefix("refs/heads/") {
            if head == "*" {
                found.current = Some(local.into());
            }
            found.local.push(local.into());
        } else if let Some(remote) = name.strip_prefix("refs/remotes/")
            && symref.is_empty()
        {
            found.remote.push(remote.into());
        }
    }
    found
}

/// Makes a local branch tracking the remote branch `remote` (`origin/x`),
/// named as the remote's refspec says, and switches to it.
pub(crate) fn track(top: &Path, remote: &str) -> Result<String, String> {
    if remote.is_empty() || remote.starts_with('-') || remote.contains('\0') {
        return Err("Choose an existing remote branch.".into());
    }
    let full = format!("refs/remotes/{remote}");
    for args in [
        vec!["check-ref-format", &full],
        vec!["show-ref", "--verify", "--quiet", &full],
        vec!["switch", "--track", "--", &full],
    ] {
        let out = run_args(top, &args, SLOW)?;
        if !out.ok {
            return Err(match args[0] {
                "switch" => git_error(&out),
                _ => format!("{remote} isn't a remote branch here anymore; refresh and try again."),
            });
        }
    }
    let branch = run_args(top, &["symbolic-ref", "--quiet", "--short", "HEAD"], QUICK)?;
    Ok(format!("Switched to {}, tracking {remote}", branch.stdout.trim()))
}

/// Whether `sha`, a full commit id, names a commit here.
fn has_commit(top: &Path, sha: &str) -> Result<bool, String> {
    if !matches!(sha.len(), 40 | 64) || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(false);
    }
    let rev = format!("{sha}^{{commit}}");
    let out = run_args(top, &["rev-parse", "--verify", "--quiet", &rev], QUICK)?;
    Ok(out.ok && out.stdout.trim() == sha)
}

/// Makes branch `name` at `base` (a `refs/heads/` or `refs/remotes/` ref,
/// or a full commit id), else at HEAD, and switches to it. It tracks nothing,
/// so a push publishes it under its own name.
pub(crate) fn create_branch(top: &Path, name: &str, base: Option<&str>) -> Result<String, String> {
    let invalid = || format!("“{name}” isn't a valid branch name.");
    if name.is_empty() || name.starts_with('-') || name.contains('\0') {
        return Err(invalid());
    }
    // --branch also expands @{-1} and the like; only a name that comes back unchanged is one
    let checked = run_args(top, &["check-ref-format", "--branch", name], QUICK)?;
    if !checked.ok || checked.stdout.trim() != name {
        return Err(invalid());
    }
    let mut args = vec!["switch", "--no-track", "-c", name];
    if let Some(base) = base {
        if base.starts_with('-') {
            return Err("Invalid base ref.".into());
        }
        let branch = base.starts_with("refs/heads/") || base.starts_with("refs/remotes/");
        let known = match branch {
            true => {
                !base.contains('\0')
                    && run_args(top, &["show-ref", "--verify", "--quiet", base], QUICK)?.ok
            }
            false => has_commit(top, base)?,
        };
        if !known {
            return Err(match branch {
                true => format!("{base} isn't a branch here anymore; refresh and try again."),
                false => "That commit isn't here anymore; refresh and try again.".into(),
            });
        }
        args.extend(["--", base]);
    }
    let out = run_args(top, &args, SLOW)?;
    if out.ok { Ok(format!("Switched to new branch {name}")) } else { Err(git_error(&out)) }
}

/// Checks out commit `sha` (a full id) with HEAD detached. Never forced: git
/// refuses when local changes would be overwritten.
pub(crate) fn detach(top: &Path, sha: &str) -> Result<String, String> {
    if !has_commit(top, sha)? {
        return Err("That commit isn't here anymore; refresh and try again.".into());
    }
    let out = run_args(top, &["switch", "--detach", "--", sha], SLOW)?;
    if out.ok { Ok(format!("HEAD detached at {}", &sha[..7])) } else { Err(git_error(&out)) }
}

/// Where a commit is relative to the upstream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// On HEAD, not yet on the upstream.
    Outgoing,
    /// On the upstream, not yet on HEAD.
    Incoming,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub sha: String,
    pub short: String,
    pub subject: String,
    pub author: String,
    /// Commit time, in Unix seconds.
    pub time: i64,
    /// `None` when HEAD and its upstream both have it, or there's no upstream.
    pub flow: Option<Flow>,
}

/// The upstream's commits HEAD lacks, then HEAD's last commits; empty
/// before the first commit. Blocking.
pub fn history(top: &Path) -> Result<Vec<Commit>, String> {
    let has = |rev: &str| {
        run_args(top, &["rev-parse", "--verify", "--quiet", rev], QUICK).map(|out| out.ok)
    };
    if !has("HEAD")? {
        return Ok(Vec::new());
    }
    let upstream = has("@{u}")?;
    let (mut commits, outgoing) = if upstream {
        let ahead: HashSet<String> = log(top, "@{u}..HEAD")?.into_iter().map(|c| c.sha).collect();
        (log(top, "HEAD..@{u}")?, ahead)
    } else {
        (Vec::new(), HashSet::new())
    };
    for commit in &mut commits {
        commit.flow = Some(Flow::Incoming);
    }
    for mut commit in log(top, "HEAD")? {
        if outgoing.contains(&commit.sha) {
            commit.flow = Some(Flow::Outgoing);
        }
        commits.push(commit);
    }
    Ok(commits)
}

/// The newest [`HISTORY`] commits in `range`.
fn log(top: &Path, range: &str) -> Result<Vec<Commit>, String> {
    let max = HISTORY.to_string();
    let args = [
        "log",
        "--no-show-signature",
        "--no-color",
        "-n",
        &max,
        "--format=%H%x00%h%x00%ct%x00%an%x00%s",
        range,
        "--",
    ];
    let out = run_args(top, &args, QUICK)?;
    if out.ok { Ok(parse_log(&out.stdout)) } else { Err(git_error(&out)) }
}

/// Reads `git log --format=%H%x00%h%x00%ct%x00%an%x00%s` output.
fn parse_log(out: &str) -> Vec<Commit> {
    out.lines()
        .filter_map(|line| {
            let mut fields = line.splitn(5, '\0');
            let (sha, short, time) = (fields.next()?, fields.next()?, fields.next()?);
            let (author, subject) = (fields.next()?, fields.next()?);
            Some(Commit {
                sha: sha.into(),
                short: short.into(),
                subject: subject.into(),
                author: author.into(),
                time: time.parse().ok()?,
                flow: None,
            })
        })
        .collect()
}

/// Refuses anything but a commit id: 7 or more hex digits, never an option or ref.
fn commit_id(sha: &str) -> Result<(), String> {
    if sha.len() < 7 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("Refused the commit id “{sha}”."));
    }
    Ok(())
}

/// Commit `sha`'s whole message. Blocking.
pub fn commit_message(top: &Path, sha: &str) -> Result<String, String> {
    commit_id(sha)?;
    let args = ["log", "-1", "--no-show-signature", "--no-color", "--format=%B", sha, "--"];
    let out = run_args(top, &args, QUICK)?;
    if out.ok { Ok(out.stdout.trim_end().into()) } else { Err(git_error(&out)) }
}

/// Commit `sha`'s diff as `git show` prints it, with a/ and b/ prefixes. Blocking.
pub fn show(top: &Path, sha: &str) -> Result<String, String> {
    commit_id(sha)?;
    let args = [
        "show",
        "--root",
        "--format=",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--no-show-signature",
        // a merge's combined diff is usually empty; show what it brought in instead
        "--diff-merges=first-parent",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        sha,
        "--",
    ];
    let out = run_args(top, &args, QUICK)?;
    if out.ok { Ok(out.stdout) } else { Err(git_error(&out)) }
}

#[cfg(test)]
#[path = "../tests/unit/refs.rs"]
mod tests;
