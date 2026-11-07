use super::*;

fn info(branch: &str, detached: bool, worktree: bool) -> Option<BranchInfo> {
    Some(BranchInfo { branch: branch.into(), detached, worktree })
}

#[test]
fn parses_branch_worktree_and_detached_heads() {
    assert_eq!(parse("main\n/r/.git\n/r/.git\n"), info("main", false, false));
    assert_eq!(
        parse("feature/x-long\n/r/.git/worktrees/wt\n/r/.git\n"),
        info("feature/x-long", false, true)
    );
    assert_eq!(parse("HEAD\n/r/.git\n/r/.git\n"), info("HEAD", true, false));
    assert_eq!(parse(""), None);
    assert_eq!(parse("main\n"), None);
    assert_eq!(parse("main\n\n/r/.git\n"), None);
}

fn changed(path: &str, from: Option<&str>, kind: ChangeKind, staged: bool) -> Change {
    change(path, from, kind, staged)
}

#[test]
fn porcelain_v2_reads_branch_headers_and_every_record_kind() {
    let out = [
        "# branch.oid 82edbe8d3ed81f9856d834641a8fcdbce9cbfdcc",
        "# branch.head feature/x y",
        "# branch.upstream origin/feature/x",
        "# branch.ab +2 -1",
        "1 .M N... 100644 100644 100644 88768ef 88768ef b.bin",
        "1 MM N... 100644 100644 100644 2fa992c 5f146bd dir/with space/k.txt",
        "2 R. N... 100644 100644 100644 96cc558 1c5a36f R97 moved.txt",
        "old name.txt",
        "2 RM N... 100644 100644 100644 96cc558 1c5a36f R100 b.rs",
        "a.rs",
        "u UU N... 100644 100644 100644 100644 h1 h2 h3 conflict.rs",
        "1 D. N... 100644 000000 000000 abc 000 gone.txt",
        "1 .T N... 100644 100644 120000 abc abc link",
        "? new file.txt",
        "! ignored.txt",
        "",
    ]
    .join("\0");
    let status = parse_status(&out);
    assert_eq!(status.branch.as_deref(), Some("feature/x y"));
    assert_eq!(status.commit.as_deref(), Some("82edbe8"));
    assert_eq!(status.upstream.as_deref(), Some("origin/feature/x"));
    assert_eq!(status.ahead_behind, Some((2, 1)));
    use ChangeKind::*;
    assert_eq!(
        status.changes,
        [
            changed("b.bin", None, Modified, false),
            changed("dir/with space/k.txt", None, Modified, true),
            changed("dir/with space/k.txt", None, Modified, false),
            changed("moved.txt", Some("old name.txt"), Renamed, true),
            changed("b.rs", Some("a.rs"), Renamed, true),
            changed("b.rs", None, Modified, false),
            changed("conflict.rs", None, Conflicted, false),
            changed("gone.txt", None, Deleted, true),
            changed("link", None, TypeChanged, false),
            changed("new file.txt", None, Untracked, false),
        ]
    );
}

#[test]
fn porcelain_v2_knows_unborn_detached_and_upstream_gone() {
    let unborn = parse_status("# branch.oid (initial)\0# branch.head main\0? a\0");
    assert_eq!((unborn.branch.as_deref(), unborn.commit), (Some("main"), None));
    assert_eq!(unborn.changes, [changed("a", None, ChangeKind::Untracked, false)]);
    let detached = parse_status("# branch.oid 1234567890\0# branch.head (detached)\0");
    assert_eq!((detached.branch, detached.commit.as_deref()), (None, Some("1234567")));
    let gone =
        parse_status("# branch.oid 1234567890\0# branch.head x\0# branch.upstream origin/x\0");
    assert_eq!((gone.upstream.as_deref(), gone.ahead_behind), (Some("origin/x"), None));
    let cut = parse_status("2 R. N... 100644 100644 100644 a b R90 new.rs");
    assert_eq!(cut.changes, [changed("new.rs", None, ChangeKind::Renamed, true)]);
    assert_eq!(parse_status(""), Status::default());
    assert_eq!(parse_status("1 M. garbage\0").changes, []);
}

#[test]
fn numstat_reads_counts_binaries_and_renames() {
    let out = "3\t1\tsrc/a.rs\0-\t-\tlogo.png\0\
               1\t0\t\0old dir/a.txt\0new dir/a.txt\0\
               0\t0\tmode-only.sh\0";
    let stats = parse_numstat(out);
    assert_eq!(stats.len(), 4);
    assert_eq!(stats["src/a.rs"], Stat::Lines(3, 1));
    assert_eq!(stats["logo.png"], Stat::Binary);
    assert_eq!(stats["new dir/a.txt"], Stat::Lines(1, 0));
    assert_eq!(stats["mode-only.sh"], Stat::Lines(0, 0));
    assert!(parse_numstat("").is_empty());
    assert_eq!(parse_numstat("1\t0\t\0only-old\0").len(), 0);
}

#[test]
fn remotes_prefer_origin_then_a_lone_one() {
    assert_eq!(pick_remote("origin\nupstream\n").as_deref(), Some("origin"));
    assert_eq!(pick_remote("fork\n").as_deref(), Some("fork"));
    assert_eq!(pick_remote("fork\nupstream\n"), None);
    assert_eq!(pick_remote(""), None);
}

#[test]
fn only_relative_paths_inside_the_repo_go_back_to_git() {
    for ok in ["a.rs", "dir/b c.rs", "./x", "-n", ":(top)x", "f[1].txt"] {
        assert_eq!(safe_path(ok), Ok(ok));
    }
    for bad in ["", "/etc/passwd", "../x", "a/../../x", "a\0b"] {
        assert!(safe_path(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn reports_keep_hook_output_verbatim_but_bounded() {
    let out = |stdout: &str, stderr: &str| Output {
        ok: false,
        stdout: stdout.into(),
        stderr: stderr.into(),
    };
    assert_eq!(
        report(&out("On branch main\nnothing to commit\n", "")),
        "On branch main\nnothing to commit"
    );
    assert_eq!(
        report(&out("hook says hi", "error: lint failed\n")),
        "error: lint failed\nhook says hi"
    );
    assert_eq!(report(&out("", "")), "Git failed without saying why.");
    let long = (0..100).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n");
    let cut = report(&out(&long, ""));
    assert_eq!(cut.lines().count(), REPORT_LINES + 1);
    assert!(cut.ends_with("\n…"));
    let wide = report(&out(&"é".repeat(REPORT_BYTES), ""));
    assert!(
        wide.starts_with("éé")
            && wide.ends_with('…')
            && wide.len() <= REPORT_BYTES + '…'.len_utf8(),
        "{}",
        wide.len()
    );
}

#[test]
fn untracked_lines_count_within_the_budget() {
    let dir = std::env::temp_dir().join(format!("aether-count-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = |name: &str, bytes: &[u8]| {
        std::fs::write(dir.join(name), bytes).unwrap();
        dir.join(name)
    };
    let mut budget = 1000;
    assert_eq!(count_lines(&file("a", b"x\ny\n"), &mut budget), Stat::Lines(2, 0));
    assert_eq!(count_lines(&file("b", b"x\ny"), &mut budget), Stat::Lines(2, 0));
    assert_eq!(count_lines(&file("c", b""), &mut budget), Stat::Lines(0, 0));
    assert_eq!(count_lines(&file("d", b"PNG\0\x01"), &mut budget), Stat::Binary);
    assert_eq!(budget, 1000 - 4 - 3 - 5);
    assert_eq!(count_lines(&file("e", &[b'x'; 2000]), &mut budget), Stat::Unknown);
    assert_eq!(count_lines(&dir, &mut budget), Stat::Unknown);
    assert_eq!(count_lines(&dir.join("missing"), &mut budget), Stat::Unknown);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Git in a throwaway repo, with an identity and nothing from the user's hooks or signing.
fn sh(dir: &Path, args: &[&str]) -> bool {
    run_args(dir, args, QUICK).is_ok_and(|out| out.ok)
}

fn repo(base: &Path) -> Option<PathBuf> {
    let repo = base.join("repo");
    let _ = std::fs::remove_dir_all(base);
    std::fs::create_dir_all(&repo).unwrap();
    let isolated = sh(&repo, &["init", "-q", "-b", "main"])
        && [
            ("user.name", "a"),
            ("user.email", "a@b"),
            ("core.hooksPath", "/dev/null"),
            ("commit.gpgsign", "false"),
        ]
        .iter()
        .all(|(key, value)| sh(&repo, &["config", key, value]));
    isolated.then_some(repo)
}

fn paths(status: &Status, staged: bool) -> Vec<(String, char, Stat)> {
    status
        .changes
        .iter()
        .filter(|c| c.staged == staged)
        .map(|c| (c.path.clone(), c.kind.letter(), c.stat))
        .collect()
}

fn row(path: &str, letter: char, stat: Stat) -> (String, char, Stat) {
    (path.into(), letter, stat)
}

fn switch_repo(base: &Path) -> PathBuf {
    let repo = repo(base).expect("git must be available for branch switching tests");
    std::fs::write(repo.join("a.txt"), "main\n").unwrap();
    assert!(sh(&repo, &["add", "a.txt"]));
    assert!(sh(&repo, &["commit", "-qm", "main"]));
    assert!(sh(&repo, &["switch", "-qc", "feature/x"]));
    std::fs::write(repo.join("a.txt"), "feature\n").unwrap();
    assert!(sh(&repo, &["commit", "-qam", "feature"]));
    assert!(sh(&repo, &["switch", "-q", "main"]));
    repo
}

#[test]
fn switch_branch_checks_out_an_existing_local_branch() {
    let base = std::env::temp_dir().join(format!("aether-switch-local-{}", std::process::id()));
    let repo = switch_repo(&base);
    std::fs::write(repo.join("untracked.txt"), "keep\n").unwrap();
    switch_branch(&repo, "feature/x").unwrap();
    assert_eq!(branch_info(&repo), info("feature/x", false, false));
    assert_eq!(std::fs::read(repo.join("a.txt")).unwrap(), b"feature\n");
    assert_eq!(std::fs::read(repo.join("untracked.txt")).unwrap(), b"keep\n");
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn switch_branch_refuses_conflicting_dirty_files() {
    let base = std::env::temp_dir().join(format!("aether-switch-dirty-{}", std::process::id()));
    let repo = switch_repo(&base);
    std::fs::write(repo.join("a.txt"), "my edits\n").unwrap();
    let head = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    let error = switch_branch(&repo, "feature/x").unwrap_err();
    assert!(error.contains("would be overwritten"), "{error}");
    assert!(!error.starts_with("error: "), "{error}");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]).unwrap(), head);
    assert_eq!(branch_info(&repo), info("main", false, false));
    assert_eq!(std::fs::read(repo.join("a.txt")).unwrap(), b"my edits\n");
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn switch_branch_refuses_a_branch_in_another_worktree() {
    let base = std::env::temp_dir().join(format!("aether-switch-worktree-{}", std::process::id()));
    let repo = switch_repo(&base);
    let worktree = base.join("linked");
    assert!(sh(&repo, &["worktree", "add", "-q", worktree.to_str().unwrap(), "feature/x"]));
    let head = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    let error = switch_branch(&repo, "feature/x").unwrap_err();
    assert!(error.contains("already"), "{error}");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]).unwrap(), head);
    assert_eq!(branch_info(&repo), info("main", false, false));
    assert_eq!(std::fs::read(repo.join("a.txt")).unwrap(), b"main\n");
    assert_eq!(std::fs::read(worktree.join("a.txt")).unwrap(), b"feature\n");
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn switch_branch_refuses_invalid_or_nonlocal_targets() {
    let base = std::env::temp_dir().join(format!("aether-switch-invalid-{}", std::process::id()));
    let repo = switch_repo(&base);
    assert!(sh(&repo, &["tag", "tag-only", "feature/x"]));
    assert!(sh(&repo, &["update-ref", "refs/remotes/origin/remote-only", "feature/x"]));
    let head = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    std::fs::write(repo.join("untracked.txt"), "keep\n").unwrap();
    for branch in [
        "",
        "--detach",
        "-",
        "bad name",
        "a\0b",
        "@{-1}",
        "HEAD",
        "main~1",
        "refs/heads/feature/x",
        "missing",
        "tag-only",
        "origin/remote-only",
        "remote-only",
    ] {
        assert!(switch_branch(&repo, branch).is_err(), "{branch:?}");
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]).unwrap(), head, "{branch:?}");
        assert_eq!(branch_info(&repo), info("main", false, false), "{branch:?}");
        assert_eq!(std::fs::read(repo.join("a.txt")).unwrap(), b"main\n", "{branch:?}");
        assert_eq!(std::fs::read(repo.join("untracked.txt")).unwrap(), b"keep\n", "{branch:?}");
    }
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn stage_unstage_and_commit_round_trip_in_a_temp_repo() {
    let base = std::env::temp_dir().join(format!("aether-git-panel-{}", std::process::id()));
    let Some(repo) = repo(&base) else {
        return;
    };
    let write = |path: &str, text: &[u8]| {
        let path = repo.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write("sub/a b.txt", b"one\ntwo\n");
    write("f[1].txt", b"bracket\n");
    write("f1.txt", b"plain\n");
    let unborn = status(&repo.join("sub")).unwrap();
    assert_eq!(unborn.top, repo.canonicalize().unwrap());
    assert_eq!((unborn.branch.as_deref(), unborn.commit.as_deref()), (Some("main"), None));
    assert_eq!(
        paths(&unborn, false),
        [
            row("f1.txt", '?', Stat::Lines(1, 0)),
            row("f[1].txt", '?', Stat::Lines(1, 0)),
            row("sub/a b.txt", '?', Stat::Lines(2, 0)),
        ]
    );
    assert!(status(&base).is_err_and(|e| e == StatusError::NotRepo));
    let top = unborn.top.clone();

    // a bracket in a name is a name, not a pattern
    apply(&top, &GitOp::Stage(vec!["f[1].txt".into()])).unwrap();
    let one = status(&top).unwrap();
    assert_eq!(paths(&one, true), [row("f[1].txt", 'A', Stat::Lines(1, 0))]);
    apply(&top, &GitOp::StageAll).unwrap();
    write("f1.txt", b"plain\nmore\n");
    apply(&top, &GitOp::Unstage(vec!["f1.txt".into()])).unwrap();
    let before_first = status(&top).unwrap();
    assert_eq!(paths(&before_first, true).len(), 2);
    assert_eq!(paths(&before_first, false), [row("f1.txt", '?', Stat::Lines(2, 0))]);
    assert_eq!(std::fs::read(repo.join("f1.txt")).unwrap(), b"plain\nmore\n");
    apply(&top, &GitOp::UnstageAll).unwrap();
    assert!(paths(&status(&top).unwrap(), true).is_empty());

    assert_eq!(
        apply(&top, &GitOp::Commit { message: " \n".into(), stage_all: true }).unwrap_err(),
        "Write a commit message first."
    );
    let note =
        apply(&top, &GitOp::Commit { message: "first\n\nbody".into(), stage_all: true }).unwrap();
    assert!(note.starts_with("Committed ") && note.len() > "Committed ".len(), "{note}");
    let clean = status(&top).unwrap();
    assert!(clean.changes.is_empty(), "{clean:?}");
    assert_eq!(clean.commit.as_deref().map(str::len), Some(7));
    let nothing =
        apply(&top, &GitOp::Commit { message: "again".into(), stage_all: false }).unwrap_err();
    assert!(nothing.contains("nothing to commit"), "{nothing}");

    assert!(sh(&top, &["mv", "sub/a b.txt", "sub/renamed.txt"]));
    write("sub/renamed.txt", b"one\ntwo\nthree\n");
    assert!(sh(&top, &["add", "sub/renamed.txt"]));
    write("logo.png", b"\x89PNG\0\x01");
    assert!(sh(&top, &["add", "logo.png"]));
    write("f1.txt", b"plain\nchanged\n");
    let renamed = status(&top).unwrap();
    let rename = renamed.changes.iter().find(|c| c.kind == ChangeKind::Renamed).unwrap();
    assert_eq!(
        (rename.path.as_str(), rename.from.as_deref(), rename.stat),
        ("sub/renamed.txt", Some("sub/a b.txt"), Stat::Lines(1, 0))
    );
    assert_eq!(paths(&renamed, false), [row("f1.txt", 'M', Stat::Lines(1, 1))]);
    assert!(paths(&renamed, true).contains(&row("logo.png", 'A', Stat::Binary)));

    let text = diff(&top, rename).unwrap();
    assert!(text.contains("rename from sub/a b.txt") && text.contains("+three"), "{text}");
    let binary = renamed.changes.iter().find(|c| c.path == "logo.png").unwrap();
    assert!(diff(&top, binary).unwrap().contains("Binary files"));
    let work = renamed.changes.iter().find(|c| !c.staged).unwrap();
    let text = diff(&top, work).unwrap();
    assert!(
        text.contains("-more") && text.contains("+changed") && !text.contains("\x1b["),
        "{text}"
    );
    write("brand new.txt", b"hello\n");
    let untracked = change("brand new.txt", None, ChangeKind::Untracked, false);
    assert!(diff(&top, &untracked).unwrap().contains("+hello"));
    let outside = change("../escape", None, ChangeKind::Modified, false);
    assert!(diff(&top, &outside).is_err());
    assert!(apply(&top, &GitOp::Stage(vec!["/etc/hosts".into()])).is_err());

    apply(&top, &GitOp::Unstage(vec!["sub/renamed.txt".into(), "sub/a b.txt".into()])).unwrap();
    let split = status(&top).unwrap();
    assert!(
        split
            .changes
            .iter()
            .any(|c| c.path == "sub/a b.txt" && c.kind == ChangeKind::Deleted && !c.staged)
    );
    assert!(
        split
            .changes
            .iter()
            .any(|c| c.path == "sub/renamed.txt" && c.kind == ChangeKind::Untracked)
    );
    assert!(repo.join("sub/renamed.txt").is_file());
    apply(&top, &GitOp::Stage(vec!["sub/a b.txt".into()])).unwrap();
    let deleted = status(&top).unwrap();
    assert!(
        deleted
            .changes
            .iter()
            .any(|c| c.path == "sub/a b.txt" && c.kind == ChangeKind::Deleted && c.staged)
    );
    apply(&top, &GitOp::UnstageAll).unwrap();
    assert!(status(&top).unwrap().changes.iter().all(|c| !c.staged));
    assert_eq!(std::fs::read(repo.join("f1.txt")).unwrap(), b"plain\nchanged\n");

    let lock = top.join(".git/index.lock");
    std::fs::write(&lock, "").unwrap();
    assert_eq!(status(&top), Err(StatusError::Locked));
    std::fs::remove_file(&lock).unwrap();
    assert!(status(&top).is_ok());
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn failing_hooks_and_pushes_report_what_git_said() {
    let base = std::env::temp_dir().join(format!("aether-git-push-{}", std::process::id()));
    let Some(repo) = repo(&base) else {
        return;
    };
    let hooks = base.join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    std::fs::write(&hook, "#!/bin/sh\necho 'lint: 2 problems' >&2\nexit 1\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    assert!(sh(&repo, &["config", "core.hooksPath", hooks.to_str().unwrap()]));
    let blocked =
        apply(&repo, &GitOp::Commit { message: "x".into(), stage_all: true }).unwrap_err();
    assert!(blocked.contains("lint: 2 problems"), "{blocked}");
    assert!(sh(&repo, &["config", "core.hooksPath", "/dev/null"]));
    apply(&repo, &GitOp::Commit { message: "x".into(), stage_all: false }).unwrap();

    let no_remote = status(&repo).unwrap();
    assert_eq!((no_remote.upstream.as_deref(), no_remote.remote.as_deref()), (None, None));
    assert!(apply(&repo, &GitOp::Push).unwrap_err().contains("no remote"));

    let remote = base.join("remote.git");
    assert!(sh(&base, &["init", "-q", "--bare", remote.to_str().unwrap()]));
    assert!(sh(&repo, &["remote", "add", "origin", remote.to_str().unwrap()]));
    assert_eq!(status(&repo).unwrap().remote.as_deref(), Some("origin"));
    assert_eq!(apply(&repo, &GitOp::Push).unwrap(), "Pushed to origin/main");
    let tracked = status(&repo).unwrap();
    assert_eq!(
        (tracked.upstream.as_deref(), tracked.ahead_behind),
        (Some("origin/main"), Some((0, 0)))
    );
    std::fs::write(repo.join("b.txt"), "b\n").unwrap();
    apply(&repo, &GitOp::Commit { message: "y".into(), stage_all: true }).unwrap();
    assert_eq!(status(&repo).unwrap().ahead_behind, Some((1, 0)));
    assert_eq!(apply(&repo, &GitOp::Push).unwrap(), "Pushed to origin/main");

    assert!(sh(&repo, &["checkout", "-q", "--detach"]));
    assert!(apply(&repo, &GitOp::Push).unwrap_err().contains("detached"));

    assert!(sh(&repo, &["checkout", "-q", "-b", "side", "main~1"]));
    std::fs::write(repo.join("b.txt"), "side\n").unwrap();
    assert!(sh(&repo, &["add", "b.txt"]) && sh(&repo, &["commit", "-qm", "side"]));
    assert!(!sh(&repo, &["merge", "-q", "main"]));
    let conflicted = status(&repo).unwrap();
    let conflict = conflicted.changes.iter().find(|c| c.kind == ChangeKind::Conflicted).unwrap();
    assert_eq!((conflict.path.as_str(), conflict.staged), ("b.txt", false));
    let text = diff(&repo, conflict).unwrap();
    assert!(text.contains("<<<<<<<") && text.contains("@@ -"), "{text}");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn slow_git_stops_on_timeout_and_on_cancel() {
    let dir = std::env::temp_dir();
    let args: Vec<&OsStr> = ["-c", "alias.nap=!sleep 5", "nap"].map(OsStr::new).to_vec();
    if binary().is_none() {
        return;
    }
    let started = Instant::now();
    let timed_out = run(&dir, &args, Duration::from_millis(200)).unwrap_err();
    assert!(timed_out.contains("took longer"), "{timed_out}");
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        flag.store(true, Ordering::Relaxed);
    });
    assert_eq!(run_until(&dir, &args, Duration::from_secs(60), &cancel).unwrap_err(), CANCELED);
    assert!(started.elapsed() < Duration::from_secs(4), "{:?}", started.elapsed());
}

#[test]
fn reads_a_real_repo_and_ignores_plain_folders() {
    let root = std::env::temp_dir().join(format!("aether-git-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (repo, plain) = (root.join("repo"), root.join("plain"));
    std::fs::create_dir_all(&plain).unwrap();
    assert_eq!(branch_info(&plain), None);
    assert_eq!(branch_info(&root.join("missing")), None);
    let made = Command::new("git")
        .args(["init", "-q", "-b", "trunk"])
        .arg(&repo)
        .status()
        .is_ok_and(|s| s.success())
        && git(
            &repo,
            &[
                "-c",
                "user.name=a",
                "-c",
                "user.email=a@b",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "x",
            ],
        )
        .is_some();
    if made {
        assert_eq!(branch_info(&repo), info("trunk", false, false));
        let wt = root.join("wt");
        git(&repo, &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()]).unwrap();
        let detached = branch_info(&wt).unwrap();
        assert!(detached.detached && detached.worktree);
        assert!(detached.branch.len() >= 7 && detached.branch != "HEAD", "{detached:?}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

fn write(repo: &Path, path: &str, text: &str) {
    let path = repo.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read(repo: &Path, path: &str) -> String {
    std::fs::read_to_string(repo.join(path)).unwrap()
}

fn head_subject(repo: &Path) -> String {
    git(repo, &["log", "-1", "--format=%s"]).unwrap().trim().to_string()
}

/// A bare `origin` for `repo` with main pushed to it, plus a second clone of it.
fn remote_pair(base: &Path, repo: &Path) -> PathBuf {
    let remote = base.join("remote.git");
    assert!(sh(base, &["init", "-q", "--bare", "-b", "main", remote.to_str().unwrap()]));
    assert!(sh(repo, &["remote", "add", "origin", remote.to_str().unwrap()]));
    assert_eq!(apply(repo, &GitOp::Push).unwrap(), "Pushed to origin/main");
    let other = base.join("other");
    assert!(sh(
        base,
        &["clone", "-q", "-b", "main", remote.to_str().unwrap(), other.to_str().unwrap()]
    ));
    for (key, value) in [
        ("user.name", "b"),
        ("user.email", "b@c"),
        ("core.hooksPath", "/dev/null"),
        ("commit.gpgsign", "false"),
    ] {
        assert!(sh(&other, &["config", key, value]));
    }
    other
}

#[test]
fn discard_restores_tracked_files_and_deletes_only_named_untracked_ones() {
    let base = std::env::temp_dir().join(format!("aether-git-discard-{}", std::process::id()));
    let Some(repo) = repo(&base) else {
        return;
    };
    write(&repo, "a.txt", "one\n");
    write(&repo, "b.txt", "b\n");
    write(&repo, ".gitignore", "*.log\n");
    assert!(sh(&repo, &["add", "."]) && sh(&repo, &["commit", "-qm", "base"]));

    write(&repo, "a.txt", "two\n");
    assert!(sh(&repo, &["add", "a.txt"]));
    write(&repo, "a.txt", "three\n");
    std::fs::remove_file(repo.join("b.txt")).unwrap();
    write(&repo, "new dir/n.txt", "n\n");
    write(&repo, "f[1].txt", "bracket\n");
    write(&repo, "f1.txt", "keep\n");
    write(&repo, "x.log", "ignored\n");
    let named = ["a.txt", "b.txt", "new dir/n.txt", "f[1].txt"].map(String::from).to_vec();
    assert_eq!(apply(&repo, &GitOp::Discard(named)).unwrap(), "");

    // the work tree goes back to the index, so the staged edit survives
    assert_eq!(read(&repo, "a.txt"), "two\n");
    assert_eq!(read(&repo, "b.txt"), "b\n");
    assert!(!repo.join("new dir/n.txt").exists() && repo.join("new dir").is_dir());
    assert!(!repo.join("f[1].txt").exists());
    assert_eq!(read(&repo, "f1.txt"), "keep\n");
    assert_eq!(read(&repo, "x.log"), "ignored\n");
    let left = status(&repo).unwrap();
    assert_eq!(paths(&left, true), [row("a.txt", 'M', Stat::Lines(1, 1))]);
    assert_eq!(paths(&left, false), [row("f1.txt", '?', Stat::Lines(1, 0))]);
    assert!(apply(&repo, &GitOp::Discard(vec!["../outside".into()])).is_err());

    // a section discard drops what it lists, not what turned up since
    write(&repo, "a.txt", "four\n");
    write(&repo, "late.txt", "late\n");
    apply(&repo, &GitOp::DiscardAll(vec!["a.txt".into(), "f1.txt".into()])).unwrap();
    assert_eq!(read(&repo, "a.txt"), "two\n");
    assert!(!repo.join("f1.txt").exists());
    assert_eq!(read(&repo, "late.txt"), "late\n");
    let many: Vec<String> = (0..PATH_CHUNK + 5).map(|n| format!("many/{n}.txt")).collect();
    for path in &many {
        write(&repo, path, "x\n");
    }
    apply(&repo, &GitOp::DiscardAll(many)).unwrap();
    assert_eq!(paths(&status(&repo).unwrap(), false), [row("late.txt", '?', Stat::Lines(1, 0))]);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn discarding_an_intent_to_add_file_deletes_it() {
    let base = std::env::temp_dir().join(format!("aether-git-intent-{}", std::process::id()));
    let Some(repo) = repo(&base) else {
        return;
    };
    write(&repo, "a.txt", "a\n");
    assert!(sh(&repo, &["add", "."]) && sh(&repo, &["commit", "-qm", "base"]));
    write(&repo, "a.txt", "edited\n");
    write(&repo, "n.txt", "new\n");
    write(&repo, "keep.txt", "keep\n");
    assert!(sh(&repo, &["add", "-N", "n.txt", "keep.txt"]));
    let before = status(&repo).unwrap();
    assert!(paths(&before, false).contains(&row("n.txt", 'A', Stat::Lines(1, 0))));

    apply(&repo, &GitOp::Discard(vec!["a.txt".into(), "n.txt".into()])).unwrap();
    assert!(!repo.join("n.txt").exists());
    assert_eq!(read(&repo, "a.txt"), "a\n");
    assert_eq!(read(&repo, "keep.txt"), "keep\n");
    assert!(!git(&repo, &["ls-files"]).unwrap().contains("n.txt"));
    assert_eq!(paths(&status(&repo).unwrap(), false), [row("keep.txt", 'A', Stat::Lines(1, 0))]);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn conflicts_stay_out_of_stage_all_and_discard() {
    let base = std::env::temp_dir().join(format!("aether-git-conflict-{}", std::process::id()));
    let Some(repo) = repo(&base) else {
        return;
    };
    write(&repo, "a.txt", "base\n");
    assert!(sh(&repo, &["add", "."]) && sh(&repo, &["commit", "-qm", "base"]));
    assert!(sh(&repo, &["switch", "-qc", "side"]));
    write(&repo, "a.txt", "side\n");
    assert!(sh(&repo, &["commit", "-qam", "side"]));
    assert!(sh(&repo, &["switch", "-q", "main"]));
    write(&repo, "a.txt", "main\n");
    assert!(sh(&repo, &["commit", "-qam", "main"]));
    assert!(!sh(&repo, &["merge", "-q", "side"]));
    write(&repo, "u.txt", "u\n");

    apply(&repo, &GitOp::StageAll).unwrap();
    let staged = status(&repo).unwrap();
    assert_eq!(paths(&staged, true), [row("u.txt", 'A', Stat::Lines(1, 0))]);
    assert_eq!(paths(&staged, false), [row("a.txt", 'U', Stat::Lines(4, 0))]);
    assert!(apply(&repo, &GitOp::Discard(vec!["a.txt".into()])).is_err());
    assert!(read(&repo, "a.txt").contains("<<<<<<<"));
    let blocked =
        apply(&repo, &GitOp::Commit { message: "m".into(), stage_all: true }).unwrap_err();
    assert!(blocked.contains("unmerged"), "{blocked}");
    apply(&repo, &GitOp::Stage(vec!["a.txt".into()])).unwrap();
    assert!(status(&repo).unwrap().changes.iter().all(|c| c.kind != ChangeKind::Conflicted));
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn fetch_pull_and_sync_with_a_local_remote() {
    let base = std::env::temp_dir().join(format!("aether-git-sync-{}", std::process::id()));
    let Some(repo) = repo(&base) else {
        return;
    };
    write(&repo, "a.txt", "a\n");
    apply(&repo, &GitOp::Commit { message: "one".into(), stage_all: true }).unwrap();
    let other = remote_pair(&base, &repo);
    write(&other, "b.txt", "b\n");
    assert!(sh(&other, &["add", "."]) && sh(&other, &["commit", "-qm", "two"]));
    assert!(sh(&other, &["push", "-q"]));

    assert_eq!(apply(&repo, &GitOp::Fetch).unwrap(), "Fetched");
    assert_eq!(status(&repo).unwrap().ahead_behind, Some((0, 1)));
    assert_eq!(apply(&repo, &GitOp::Pull).unwrap(), "Pulled from origin/main");
    assert_eq!(status(&repo).unwrap().ahead_behind, Some((0, 0)));
    assert_eq!(read(&repo, "b.txt"), "b\n");

    // diverged: a fast-forward-only pull says why it stopped and leaves both sides be
    write(&other, "c.txt", "c\n");
    assert!(sh(&other, &["add", "."]) && sh(&other, &["commit", "-qm", "three"]));
    assert!(sh(&other, &["push", "-q"]));
    write(&repo, "d.txt", "d\n");
    apply(&repo, &GitOp::Commit { message: "four".into(), stage_all: true }).unwrap();
    assert!(sh(&repo, &["config", "pull.rebase", "false"]));
    assert!(sh(&repo, &["config", "pull.ff", "only"]));
    let head = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    let refused = apply(&repo, &GitOp::Sync).unwrap_err();
    assert!(refused.contains("fast-forward"), "{refused}");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]).unwrap(), head);
    assert_eq!(status(&repo).unwrap().ahead_behind, Some((1, 1)));

    assert!(sh(&repo, &["config", "pull.ff", "true"]));
    assert_eq!(apply(&repo, &GitOp::Sync).unwrap(), "Synced with origin/main");
    assert_eq!(status(&repo).unwrap().ahead_behind, Some((0, 0)));
    assert!(sh(&other, &["pull", "-q", "--no-rebase"]));
    assert_eq!(read(&other, "d.txt"), "d\n");

    assert!(sh(&repo, &["switch", "-qc", "solo"]));
    assert!(apply(&repo, &GitOp::Pull).unwrap_err().contains("no upstream"));
    assert!(sh(&repo, &["checkout", "-q", "--detach"]));
    assert!(apply(&repo, &GitOp::Sync).unwrap_err().contains("detached"));
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn amend_rewrites_only_commits_no_remote_has() {
    let base = std::env::temp_dir().join(format!("aether-git-amend-{}", std::process::id()));
    let Some(repo) = repo(&base) else {
        return;
    };
    assert_eq!(
        apply(&repo, &GitOp::Amend { message: "x".into() }).unwrap_err(),
        "There's no commit yet."
    );
    write(&repo, "a.txt", "a\n");
    apply(&repo, &GitOp::Commit { message: "first".into(), stage_all: true }).unwrap();
    write(&repo, "b.txt", "b\n");
    apply(&repo, &GitOp::Commit { message: "second".into(), stage_all: true }).unwrap();
    let note = apply(&repo, &GitOp::Amend { message: "second, better".into() }).unwrap();
    assert!(note.starts_with("Amended ") && note.len() > "Amended ".len(), "{note}");
    assert_eq!(head_subject(&repo), "second, better");
    assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]).unwrap().trim(), "2");

    write(&repo, "c.txt", "c\n");
    assert!(sh(&repo, &["add", "c.txt"]));
    apply(&repo, &GitOp::Amend { message: " \n".into() }).unwrap();
    assert_eq!(head_subject(&repo), "second, better");
    let files = git(&repo, &["show", "--name-only", "--format=", "HEAD"]).unwrap();
    assert_eq!(files.lines().collect::<Vec<_>>(), ["b.txt", "c.txt"]);

    remote_pair(&base, &repo);
    let head = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    let pushed = apply(&repo, &GitOp::Amend { message: "nope".into() }).unwrap_err();
    assert!(pushed.contains("already on origin"), "{pushed}");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]).unwrap(), head);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn undo_puts_the_last_commit_back_staged_and_returns_its_message() {
    let base = std::env::temp_dir().join(format!("aether-git-undo-{}", std::process::id()));
    let Some(repo) = repo(&base) else {
        return;
    };
    write(&repo, "a.txt", "a\n");
    apply(&repo, &GitOp::Commit { message: "first".into(), stage_all: true }).unwrap();
    assert!(!status(&repo).unwrap().parent);
    assert!(apply(&repo, &GitOp::UndoCommit).unwrap_err().contains("first commit"));
    assert_eq!(head_subject(&repo), "first");

    write(&repo, "b.txt", "b\n");
    let message = "second\n\nwhy it matters";
    apply(&repo, &GitOp::Commit { message: message.into(), stage_all: true }).unwrap();
    assert!(status(&repo).unwrap().parent);
    assert_eq!(apply(&repo, &GitOp::UndoCommit).unwrap(), message);
    assert_eq!(head_subject(&repo), "first");
    let undone = status(&repo).unwrap();
    assert_eq!(paths(&undone, true), [row("b.txt", 'A', Stat::Lines(1, 0))]);
    assert_eq!(read(&repo, "b.txt"), "b\n");

    apply(&repo, &GitOp::Commit { message: "again".into(), stage_all: false }).unwrap();
    remote_pair(&base, &repo);
    let pushed = apply(&repo, &GitOp::UndoCommit).unwrap_err();
    assert!(pushed.contains("already on origin") && pushed.contains("undone"), "{pushed}");
    assert_eq!(head_subject(&repo), "again");
    write(&repo, "c.txt", "c\n");
    apply(&repo, &GitOp::Commit { message: "local".into(), stage_all: true }).unwrap();
    assert_eq!(apply(&repo, &GitOp::UndoCommit).unwrap(), "local");
    assert_eq!(head_subject(&repo), "again");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn parse_remotes_keeps_fetch_lines_once() {
    let out = "origin\tgit@github.com:iris/ac.git (fetch)\norigin\tgit@github.com:iris/ac.git (push)\nup\thttps://github.com/x/y (fetch)\n";
    assert_eq!(
        parse_remotes(out),
        vec![
            Remote { name: "origin".into(), url: "git@github.com:iris/ac.git".into() },
            Remote { name: "up".into(), url: "https://github.com/x/y".into() }
        ]
    );
}

#[test]
fn https_url_converts_ssh_forms() {
    assert_eq!(
        https_url("git@github.com:iris/ac.git").as_deref(),
        Some("https://github.com/iris/ac")
    );
    assert_eq!(
        https_url("ssh://git@gitlab.com/a/b.git").as_deref(),
        Some("https://gitlab.com/a/b")
    );
    assert_eq!(https_url("https://github.com/a/b.git").as_deref(), Some("https://github.com/a/b"));
    assert_eq!(https_url("/srv/repos/x.git"), None);
}

#[test]
fn owner_repo_from_any_form() {
    assert_eq!(owner_repo("git@github.com:iris/ac.git").as_deref(), Some("iris/ac"));
    assert_eq!(owner_repo("ssh://git@gitlab.com/a/b.git").as_deref(), Some("a/b"));
    assert_eq!(owner_repo("https://github.com/a/b.git").as_deref(), Some("a/b"));
    assert_eq!(owner_repo("/srv/repos/x.git"), None);
}

#[test]
fn url_helpers_handle_edge_forms() {
    let cases = [
        ("https://user:token@github.com/o/r.git", Some("https://github.com/o/r")),
        ("ssh://git@host:2222/o/r.git", Some("https://host/o/r")),
        ("https://host:8443/o/r.git", Some("https://host:8443/o/r")),
        ("https://github.com/o/r/", Some("https://github.com/o/r")),
        ("https://github.com/o/r.git/", Some("https://github.com/o/r")),
        ("git@github.com:o/r", Some("https://github.com/o/r")),
        ("file:///srv/x.git", None),
        ("C:/a/b", None),
    ];
    for (url, want) in cases {
        assert_eq!(https_url(url).as_deref(), want, "{url}");
    }
    assert_eq!(owner_repo("https://user:token@github.com/o/r.git").as_deref(), Some("o/r"));
    assert_eq!(owner_repo("C:/a/b"), None);
}
