use std::path::PathBuf;
use std::process::Command;

use super::*;
use crate::{GitOp, apply, status};

#[test]
fn stash_lines_give_index_branch_message_and_time() {
    let out = "aaa\x001700000000\x00On main: fix login\n\
               bbb\x001690000000\x00WIP on feature/x: 1a2b3c4 wip subject\n\
               broken line\n\
               ccc\x00oops\x00On main: bad time\n\
               ddd\x001680000000\x00WIP on (no branch): 9f8e7d6 detached\n";
    let stash = |index, sha: &str, message: &str, branch: &str, time| Stash {
        index,
        sha: sha.into(),
        message: message.into(),
        branch: branch.into(),
        time,
    };
    assert_eq!(
        parse_stashes(out),
        [
            stash(0, "aaa", "fix login", "main", 1_700_000_000),
            stash(1, "bbb", "WIP on feature/x: 1a2b3c4 wip subject", "feature/x", 1_690_000_000),
            stash(4, "ddd", "WIP on (no branch): 9f8e7d6 detached", "(no branch)", 1_680_000_000),
        ]
    );
    assert!(parse_stashes("").is_empty());
}

#[test]
fn branch_refs_split_into_local_and_remote_keeping_order() {
    let out = " \0refs/heads/feature/x\0\n\
               *\0refs/heads/main\0\n\
               \x20\0refs/remotes/origin/HEAD\0refs/remotes/origin/main\n\
               \x20\0refs/remotes/origin/main\0\n\
               \x20\0refs/remotes/fork/topic\0\n\
               cut";
    assert_eq!(
        parse_branches(out),
        Branches {
            current: Some("main".into()),
            local: vec!["feature/x".into(), "main".into()],
            remote: vec!["origin/main".into(), "fork/topic".into()],
        }
    );
    assert_eq!(parse_branches(""), Branches::default());
}

#[test]
fn log_lines_keep_subjects_with_separators() {
    let out = "f00d\x00f00\x001700000000\x00Ada L\x00fix: a: b\nbad\n";
    assert_eq!(
        parse_log(out),
        [Commit {
            sha: "f00d".into(),
            short: "f00".into(),
            subject: "fix: a: b".into(),
            author: "Ada L".into(),
            time: 1_700_000_000,
            flow: None,
        }]
    );
}

fn sh(dir: &Path, args: &[&str]) -> String {
    let out = run_args(dir, args, QUICK).unwrap();
    assert!(out.ok, "git {args:?}: {}{}", out.stderr, out.stdout);
    out.stdout
}

fn configure(repo: &Path, name: &str) {
    for (key, value) in [
        ("user.name", name),
        ("user.email", "a@b"),
        ("core.hooksPath", "/dev/null"),
        ("commit.gpgsign", "false"),
    ] {
        sh(repo, &["config", key, value]);
    }
}

fn repo(name: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("aether-refs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let repo = base.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    sh(&repo, &["init", "-q", "-b", "main"]);
    configure(&repo, "a");
    (base, repo)
}

fn write(repo: &Path, path: &str, text: &str) {
    std::fs::write(repo.join(path), text).unwrap();
}

fn read(repo: &Path, path: &str) -> String {
    std::fs::read_to_string(repo.join(path)).unwrap()
}

/// Commits everything with both dates at `at`, so recency order is fixed.
fn commit_at(repo: &Path, message: &str, at: i64) {
    sh(repo, &["add", "--all"]);
    let date = format!("@{at} +0000");
    let ok = Command::new("git")
        .current_dir(repo)
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .args(["commit", "-qm", message])
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "commit {message}");
}

/// A bare `origin` with `repo`'s main pushed to it, and a second clone of it.
fn remote(base: &Path, repo: &Path) -> PathBuf {
    let bare = base.join("remote.git");
    sh(base, &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()]);
    sh(repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
    sh(repo, &["push", "-q", "-u", "origin", "main"]);
    let other = base.join("other");
    sh(base, &["clone", "-q", "-b", "main", bare.to_str().unwrap(), other.to_str().unwrap()]);
    configure(&other, "b");
    other
}

fn listed(repo: &Path) -> Vec<(usize, String, String)> {
    stashes(repo).into_iter().map(|s| (s.index, s.message, s.branch)).collect()
}

#[test]
fn stashes_push_list_apply_pop_and_drop() {
    let (base, repo) = repo("stash");
    write(&repo, "a.txt", "one\n");
    commit_at(&repo, "first", 1_700_000_000);
    assert!(stashes(&repo).is_empty());
    assert_eq!(
        apply(&repo, &GitOp::Stash { message: String::new(), include_untracked: false })
            .unwrap_err(),
        "There are no changes to stash."
    );

    write(&repo, "a.txt", "two\n");
    write(&repo, "new.txt", "new\n");
    let note =
        apply(&repo, &GitOp::Stash { message: "edit a".into(), include_untracked: false }).unwrap();
    assert_eq!(note, "Stashed changes");
    assert_eq!(read(&repo, "a.txt"), "one\n");
    assert_eq!(read(&repo, "new.txt"), "new\n");
    apply(&repo, &GitOp::Stash { message: String::new(), include_untracked: true }).unwrap();
    assert!(!repo.join("new.txt").exists());
    let found = status(&repo).unwrap().stashes;
    assert_eq!(found.len(), 2);
    assert!(found[0].message.starts_with("WIP on main: ") && found[0].branch == "main");
    assert_eq!((found[1].index, found[1].message.as_str()), (1, "edit a"));
    assert!(found[0].time > 0 && found[0].sha.len() >= 40);

    let apply_one = GitOp::StashApply { index: 1, sha: found[1].sha.clone() };
    assert_eq!(apply(&repo, &apply_one).unwrap(), "Applied stash@{1}");
    assert_eq!(read(&repo, "a.txt"), "two\n");
    assert_eq!(stashes(&repo).len(), 2);
    sh(&repo, &["checkout", "--", "a.txt"]);

    let pop = GitOp::StashPop { index: 0, sha: found[0].sha.clone() };
    assert_eq!(apply(&repo, &pop).unwrap(), "Popped stash@{0}");
    assert_eq!(read(&repo, "new.txt"), "new\n");
    assert_eq!(listed(&repo), [(0, "edit a".into(), "main".into())]);

    let drop = GitOp::StashDrop { index: 0, sha: found[1].sha.clone() };
    assert_eq!(apply(&repo, &drop).unwrap(), "Dropped stash@{0}");
    assert!(stashes(&repo).is_empty());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn stash_ops_refuse_a_stash_whose_index_moved() {
    let (base, repo) = repo("stash-moved");
    write(&repo, "a.txt", "one\n");
    commit_at(&repo, "first", 1_700_000_000);
    write(&repo, "a.txt", "old\n");
    apply(&repo, &GitOp::Stash { message: "old".into(), include_untracked: false }).unwrap();
    let old = stashes(&repo).remove(0);
    // a newer stash pushes "old" to stash@{1}
    write(&repo, "a.txt", "new\n");
    apply(&repo, &GitOp::Stash { message: "new".into(), include_untracked: false }).unwrap();
    for op in [
        GitOp::StashApply { index: old.index, sha: old.sha.clone() },
        GitOp::StashPop { index: old.index, sha: old.sha.clone() },
        GitOp::StashDrop { index: old.index, sha: old.sha.clone() },
        GitOp::StashDrop { index: 5, sha: old.sha.clone() },
        GitOp::StashDrop { index: 0, sha: String::new() },
    ] {
        assert_eq!(apply(&repo, &op).unwrap_err(), STASHES_CHANGED, "{op:?}");
    }
    assert_eq!(read(&repo, "a.txt"), "one\n");
    assert_eq!(listed(&repo), [(0, "new".into(), "main".into()), (1, "old".into(), "main".into())]);
    apply(&repo, &GitOp::StashDrop { index: 1, sha: old.sha }).unwrap();
    assert_eq!(listed(&repo), [(0, "new".into(), "main".into())]);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_conflicting_pop_reports_git_and_keeps_the_stash() {
    let (base, repo) = repo("stash-conflict");
    write(&repo, "a.txt", "one\n");
    commit_at(&repo, "first", 1_700_000_000);
    write(&repo, "a.txt", "stashed\n");
    apply(&repo, &GitOp::Stash { message: "mine".into(), include_untracked: false }).unwrap();
    write(&repo, "a.txt", "committed\n");
    commit_at(&repo, "second", 1_700_000_100);
    let stash = stashes(&repo).remove(0);
    let error = apply(&repo, &GitOp::StashPop { index: 0, sha: stash.sha }).unwrap_err();
    assert!(error.contains("CONFLICT"), "{error}");
    assert_eq!(stashes(&repo).len(), 1);
    let conflicted = status(&repo).unwrap();
    assert!(conflicted.changes.iter().any(|c| c.kind == crate::ChangeKind::Conflicted));
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn branches_list_local_and_remote_newest_first() {
    let (base, repo) = repo("branches");
    write(&repo, "a.txt", "a\n");
    commit_at(&repo, "main", 1_700_000_000);
    let _other = remote(&base, &repo);
    sh(&repo, &["switch", "-qc", "old"]);
    write(&repo, "a.txt", "old\n");
    commit_at(&repo, "old", 1_600_000_000);
    sh(&repo, &["switch", "-qc", "new", "main"]);
    write(&repo, "a.txt", "new\n");
    commit_at(&repo, "new", 1_800_000_000);
    sh(&repo, &["push", "-q", "origin", "new"]);
    sh(&repo, &["switch", "-q", "main"]);
    sh(&repo, &["fetch", "-q", "origin"]);
    sh(&repo, &["remote", "set-head", "origin", "main"]);
    assert_eq!(
        branches(&repo).unwrap(),
        Branches {
            current: Some("main".into()),
            local: vec!["new".into(), "main".into(), "old".into()],
            remote: vec!["origin/new".into(), "origin/main".into()],
        }
    );
    sh(&repo, &["checkout", "-q", "--detach"]);
    assert_eq!(branches(&repo).unwrap().current, None);
    assert!(branches(&base).is_err());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn new_branches_need_valid_names_and_track_nothing() {
    let (base, repo) = repo("create");
    write(&repo, "a.txt", "a\n");
    commit_at(&repo, "first", 1_700_000_000);
    let _other = remote(&base, &repo);
    sh(&repo, &["switch", "-qc", "prev"]);
    sh(&repo, &["switch", "-q", "main"]);
    for bad in ["", "bad name", "-x", "a..b", "@{-1}", "x.lock", "a\0b", "main"] {
        let op = GitOp::CreateBranch { name: bad.into(), base: None };
        assert!(apply(&repo, &op).is_err(), "{bad:?}");
        assert_eq!(sh(&repo, &["branch", "--show-current"]).trim(), "main", "{bad:?}");
    }
    let op = GitOp::CreateBranch { name: "feature/y".into(), base: None };
    assert_eq!(apply(&repo, &op).unwrap(), "Switched to new branch feature/y");
    assert_eq!(sh(&repo, &["branch", "--show-current"]).trim(), "feature/y");

    let from =
        |base: &str| GitOp::CreateBranch { name: "from-remote".into(), base: Some(base.into()) };
    for bad in ["origin/main", "main", "refs/tags/x", "refs/heads/missing", "-f"] {
        assert!(apply(&repo, &from(bad)).is_err(), "{bad:?}");
    }
    apply(&repo, &from("refs/remotes/origin/main")).unwrap();
    assert_eq!(sh(&repo, &["branch", "--show-current"]).trim(), "from-remote");
    let upstream = run_args(&repo, &["rev-parse", "--abbrev-ref", "@{u}"], QUICK).unwrap();
    assert!(!upstream.ok, "{}", upstream.stdout);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn checking_out_a_remote_branch_makes_a_tracking_one() {
    let (base, repo) = repo("track");
    write(&repo, "a.txt", "a\n");
    commit_at(&repo, "first", 1_700_000_000);
    let other = remote(&base, &repo);
    sh(&other, &["switch", "-qc", "feature/x"]);
    write(&other, "x.txt", "x\n");
    commit_at(&other, "x", 1_700_000_100);
    sh(&other, &["push", "-q", "origin", "feature/x"]);
    sh(&repo, &["fetch", "-q", "origin"]);

    assert_eq!(
        apply(&repo, &GitOp::Track("origin/feature/x".into())).unwrap(),
        "Switched to feature/x, tracking origin/feature/x"
    );
    assert_eq!(read(&repo, "x.txt"), "x\n");
    assert_eq!(sh(&repo, &["rev-parse", "--abbrev-ref", "@{u}"]).trim(), "origin/feature/x");
    let again = apply(&repo, &GitOp::Track("origin/feature/x".into())).unwrap_err();
    assert!(again.contains("already exists"), "{again}");
    for bad in ["", "-f", "origin/missing", "feature/x", "../x"] {
        assert!(apply(&repo, &GitOp::Track(bad.into())).is_err(), "{bad:?}");
    }
    assert_eq!(apply(&repo, &GitOp::Checkout("main".into())).unwrap(), "Switched to main");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn checkout_never_overwrites_local_changes() {
    let (base, repo) = repo("checkout-dirty");
    write(&repo, "a.txt", "main\n");
    commit_at(&repo, "first", 1_700_000_000);
    let other = remote(&base, &repo);
    sh(&repo, &["switch", "-qc", "side"]);
    write(&repo, "a.txt", "side\n");
    commit_at(&repo, "side", 1_700_000_100);
    sh(&repo, &["switch", "-q", "main"]);
    write(&other, "a.txt", "theirs\n");
    sh(&other, &["switch", "-qc", "theirs"]);
    commit_at(&other, "theirs", 1_700_000_200);
    sh(&other, &["push", "-q", "origin", "theirs"]);
    sh(&repo, &["fetch", "-q", "origin"]);

    write(&repo, "a.txt", "my edits\n");
    for op in [
        GitOp::Checkout("side".into()),
        GitOp::Track("origin/theirs".into()),
        GitOp::CreateBranch { name: "fresh".into(), base: Some("refs/heads/side".into()) },
    ] {
        let error = apply(&repo, &op).unwrap_err();
        assert!(error.contains("would be overwritten"), "{op:?}: {error}");
        assert_eq!(sh(&repo, &["branch", "--show-current"]).trim(), "main");
        assert_eq!(read(&repo, "a.txt"), "my edits\n");
    }
    for left in ["refs/heads/theirs", "refs/heads/fresh"] {
        assert!(!run_args(&repo, &["show-ref", "--verify", "--quiet", left], QUICK).unwrap().ok);
    }
    assert!(stashes(&repo).is_empty());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn history_marks_outgoing_and_incoming_commits() {
    let (base, repo) = repo("history");
    assert_eq!(history(&repo).unwrap(), []);
    write(&repo, "a.txt", "a\n");
    commit_at(&repo, "shared", 1_700_000_000);
    // no upstream yet: nothing is marked
    let alone = history(&repo).unwrap();
    assert_eq!(
        alone.iter().map(|c| (c.subject.as_str(), c.flow)).collect::<Vec<_>>(),
        [("shared", None)]
    );
    let other = remote(&base, &repo);
    write(&other, "b.txt", "b\n");
    commit_at(&other, "theirs", 1_700_000_100);
    sh(&other, &["push", "-q"]);
    sh(&repo, &["fetch", "-q"]);
    write(&repo, "c.txt", "c\n");
    commit_at(&repo, "mine", 1_700_000_200);

    let commits = history(&repo).unwrap();
    let marks: Vec<(&str, &str, Option<Flow>)> =
        commits.iter().map(|c| (c.subject.as_str(), c.author.as_str(), c.flow)).collect();
    assert_eq!(
        marks,
        [
            ("theirs", "b", Some(Flow::Incoming)),
            ("mine", "a", Some(Flow::Outgoing)),
            ("shared", "a", None),
        ]
    );
    let mine = &commits[1];
    assert_eq!(mine.time, 1_700_000_200);
    assert!(mine.sha.starts_with(&mine.short) && mine.short.len() >= 7);

    let text = show(&repo, &mine.sha).unwrap();
    assert!(
        text.starts_with("diff --git a/c.txt b/c.txt")
            && text.contains("+c")
            && !text.contains("\x1b["),
        "{text}"
    );
    for bad in ["", "abc", "--output=/tmp/x", "HEAD~1", "0123456789abcdef0123456789abcdef0123456z"]
    {
        assert!(show(&repo, bad).is_err(), "{bad:?}");
    }
    assert!(show(&repo, "0123456789abcdef0123456789abcdef01234567").is_err());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_merge_commit_shows_what_it_brought_in() {
    let (base, repo) = repo("merge-show");
    write(&repo, "a.txt", "a\n");
    commit_at(&repo, "first", 1_700_000_000);
    sh(&repo, &["switch", "-q", "-c", "side"]);
    write(&repo, "b.txt", "b\n");
    commit_at(&repo, "side", 1_700_000_100);
    sh(&repo, &["switch", "-q", "main"]);
    write(&repo, "c.txt", "c\n");
    commit_at(&repo, "main", 1_700_000_200);
    sh(&repo, &["merge", "-q", "--no-edit", "side"]);
    let merge = sh(&repo, &["rev-parse", "HEAD"]);
    let text = show(&repo, merge.trim()).unwrap();
    assert!(text.contains("+++ b/b.txt"), "{text}");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_stash_shows_its_changes_and_untracked_files() {
    let (base, repo) = repo("stash-show");
    write(&repo, "a.txt", "one\n");
    commit_at(&repo, "first", 1_700_000_000);
    write(&repo, "a.txt", "two\n");
    apply(&repo, &GitOp::Stash { message: "tracked".into(), include_untracked: false }).unwrap();
    write(&repo, "a.txt", "three\n");
    write(&repo, "new.txt", "new\n");
    apply(&repo, &GitOp::Stash { message: "both".into(), include_untracked: true }).unwrap();
    let [both, tracked] = <[Stash; 2]>::try_from(stashes(&repo)).unwrap();

    let text = stash_show(&repo, &both.sha).unwrap();
    assert!(
        text.starts_with("diff --git a/a.txt b/a.txt")
            && text.contains("+three")
            && text.contains("diff --git a/new.txt b/new.txt")
            && text.contains("+new"),
        "{text}"
    );
    let text = stash_show(&repo, &tracked.sha).unwrap();
    assert!(text.contains("+two") && !text.contains("new.txt"), "{text}");
    for bad in ["", "--output=/tmp/x", "stash@{0}"] {
        assert!(stash_show(&repo, bad).is_err(), "{bad:?}");
    }
    assert_eq!(stashes(&repo).len(), 2);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn clearing_stashes_needs_the_list_asked_about() {
    let (base, repo) = repo("stash-clear");
    write(&repo, "a.txt", "one\n");
    commit_at(&repo, "first", 1_700_000_000);
    for message in ["old", "new"] {
        write(&repo, "a.txt", &format!("{message}\n"));
        apply(&repo, &GitOp::Stash { message: message.into(), include_untracked: false }).unwrap();
    }
    let shas: Vec<String> = stashes(&repo).into_iter().map(|s| s.sha).collect();
    write(&repo, "a.txt", "newer\n");
    apply(&repo, &GitOp::Stash { message: "newer".into(), include_untracked: false }).unwrap();
    let mut reversed = shas.clone();
    reversed.reverse();
    for stale in [shas.clone(), reversed, Vec::new()] {
        assert_eq!(apply(&repo, &GitOp::StashClear(stale)).unwrap_err(), STASHES_CHANGED);
        assert_eq!(stashes(&repo).len(), 3);
    }
    let now: Vec<String> = stashes(&repo).into_iter().map(|s| s.sha).collect();
    assert_eq!(apply(&repo, &GitOp::StashClear(now)).unwrap(), "Dropped all stashes");
    assert!(stashes(&repo).is_empty());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn branches_start_at_existing_commits_and_detach_never_forces() {
    let (base, repo) = repo("at-commit");
    write(&repo, "a.txt", "one\n");
    commit_at(&repo, "first", 1_700_000_000);
    let first = sh(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    write(&repo, "a.txt", "two\n");
    commit_at(&repo, "second\n\nwhy it changed", 1_700_000_100);
    assert_eq!(commit_message(&repo, &first).unwrap(), "first");
    let head = sh(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    assert_eq!(commit_message(&repo, &head).unwrap(), "second\n\nwhy it changed");
    assert!(commit_message(&repo, "HEAD").is_err());

    let at =
        |name: &str, base: &str| GitOp::CreateBranch { name: name.into(), base: Some(base.into()) };
    for bad in [&first[..7], "0123456789abcdef0123456789abcdef01234567", "HEAD~1", "-f"] {
        assert!(apply(&repo, &at("old", bad)).is_err(), "{bad:?}");
        assert!(apply(&repo, &GitOp::Detach(bad.into())).is_err(), "{bad:?}");
    }
    assert_eq!(sh(&repo, &["branch", "--show-current"]).trim(), "main");
    assert!(apply(&repo, &at("bad name", &first)).is_err());
    assert_eq!(apply(&repo, &at("old", &first)).unwrap(), "Switched to new branch old");
    assert_eq!(read(&repo, "a.txt"), "one\n");
    assert!(!run_args(&repo, &["rev-parse", "--abbrev-ref", "@{u}"], QUICK).unwrap().ok);
    sh(&repo, &["switch", "-q", "main"]);

    write(&repo, "a.txt", "my edits\n");
    let error = apply(&repo, &GitOp::Detach(first.clone())).unwrap_err();
    assert!(error.contains("would be overwritten"), "{error}");
    assert_eq!(sh(&repo, &["branch", "--show-current"]).trim(), "main");
    assert_eq!(read(&repo, "a.txt"), "my edits\n");

    sh(&repo, &["checkout", "--", "a.txt"]);
    let note = apply(&repo, &GitOp::Detach(first.clone())).unwrap();
    assert_eq!(note, format!("HEAD detached at {}", &first[..7]));
    assert_eq!(sh(&repo, &["rev-parse", "HEAD"]).trim(), first);
    assert_eq!(branches(&repo).unwrap().current, None);
    let _ = std::fs::remove_dir_all(&base);
}
