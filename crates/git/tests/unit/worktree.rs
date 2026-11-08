use super::*;

#[test]
fn lists_ordinary_and_detached_worktrees_without_chats() {
    let base =
        std::env::temp_dir().join(format!("aether-worktree-selector-{}", std::process::id()));
    let repo = base.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    assert!(sh(&repo, &["init", "-q", "-b", "main"]));
    assert!(sh(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]));
    let ordinary = base.join("ordinary");
    let detached = base.join("detached");
    assert!(sh(&repo, &["worktree", "add", "-b", "review", ordinary.to_str().unwrap()]));
    assert!(sh(&repo, &["worktree", "add", "--detach", detached.to_str().unwrap()]));
    let found = list(&[], &[repo]);
    assert!(found.iter().any(|row| row.path == aether_core::canonicalize(&ordinary).unwrap()
        && row.branch.as_deref() == Some("review")));
    assert!(found.iter().any(
        |row| row.path == aether_core::canonicalize(&detached).unwrap() && row.branch.is_none()
    ));
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn review_resolves_repository_identity_for_nested_and_linked_folders() {
    let base = std::env::temp_dir().join(format!("aether-review-identity-{}", std::process::id()));
    let parent = base.join("parent");
    let nested = parent.join("nested");
    std::fs::create_dir_all(nested.join("sub")).unwrap();
    for repo in [&parent, &nested] {
        assert!(sh(repo, &["init", "-q", "-b", "main"]));
        assert!(sh(repo, &["commit", "-q", "--allow-empty", "-m", "init"]));
    }
    let parent = aether_core::canonicalize(&parent).unwrap();
    let nested = aether_core::canonicalize(&nested).unwrap();
    let linked = base.join("linked");
    assert!(sh(&nested, &["worktree", "add", "-b", "review", linked.to_str().unwrap()]));
    assert_eq!(bases(&parent).unwrap().repo, aether_core::canonicalize(&parent).unwrap());
    assert_eq!(
        bases(&nested.join("sub")).unwrap().repo,
        aether_core::canonicalize(&nested).unwrap()
    );
    assert_eq!(bases(&linked).unwrap().repo, aether_core::canonicalize(&nested).unwrap());
    let listed = list(&[], &[parent, nested]);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].repo, bases(&linked).unwrap().repo);
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn listed_worktrees_offer_removal_whoever_made_them() {
    let base = std::env::temp_dir().join(format!("aether-review-removal-{}", std::process::id()));
    let repo = base.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    assert!(sh(&repo, &["init", "-q", "-b", "main"]));
    assert!(sh(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]));
    let ordinary = base.join("ordinary");
    assert!(sh(&repo, &["worktree", "add", "-b", "ordinary", ordinary.to_str().unwrap()]));
    let found = list(&[], std::slice::from_ref(&repo));
    assert_eq!(found.len(), 1);
    assert!(!found[0].managed);
    assert_eq!(found[0].remove_action(), Ok("Remove worktree…"));
    std::fs::remove_dir_all(&ordinary).unwrap();
    let missing = list(&[], std::slice::from_ref(&repo));
    assert!(missing[0].missing);
    assert_eq!(missing[0].remove_action(), Ok("Prune worktree…"));
    let root = base.join("managed");
    std::fs::create_dir_all(&root).unwrap();
    let made = create(&root, &repo, "managed", "main", &AtomicBool::new(false)).unwrap();
    let found = list(&[root], &[repo]);
    let made = found.iter().find(|row| row.path == made.path).unwrap();
    assert!(made.managed && made.remove_action().is_ok());
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn slugs_are_short_lowercase_words() {
    assert_eq!(slug("Fix the login bug!"), "fix-the-login-bug");
    assert_eq!(slug("  refactor: Store::open()  "), "refactor-store-open");
    assert_eq!(slug("修复登录"), "new-worktree");
    assert_eq!(slug(""), "new-worktree");
    let long =
        slug("please make the sidebar remember collapsed groups across restarts and windows");
    assert!(long.len() <= MAX_SLUG, "{long}");
    assert_eq!(long, "please-make-the-sidebar-remember-collapsed");
    assert_eq!(slug(&"x".repeat(80)), "x".repeat(MAX_SLUG));
}

#[test]
fn collisions_get_a_numeric_suffix_within_the_limit() {
    assert_eq!(free_slug("fix", |_| false).as_deref(), Some("fix"));
    assert_eq!(free_slug("fix", |s| s == "fix" || s == "fix-2").as_deref(), Some("fix-3"));
    let long = "a".repeat(MAX_SLUG);
    let next = free_slug(&long, |s| s == long).unwrap();
    assert_eq!(next, format!("{}-2", "a".repeat(MAX_SLUG - 2)));
    assert_eq!(free_slug("fix", |_| true), None);
    let tried = std::cell::Cell::new(0);
    free_slug("fix", |_| {
        tried.set(tried.get() + 1);
        true
    });
    assert_eq!(tried.get(), 100);
}

#[test]
fn repo_folders_are_named_and_stable() {
    let a = repo_dir(Path::new("/code/app"));
    assert!(a.starts_with("app-") && a.len() == "app-".len() + 8, "{a}");
    assert_eq!(a, repo_dir(Path::new("/code/app")));
    assert_ne!(a, repo_dir(Path::new("/other/app")));
}

#[test]
fn only_slots_two_levels_under_the_root_are_managed() {
    let root = Path::new("/r/worktrees");
    assert!(within(root, Path::new("/r/worktrees/app-1234abcd/fix")));
    assert!(!within(root, root));
    assert!(!within(root, Path::new("/r/worktrees/app-1234abcd")));
    assert!(!within(root, Path::new("/r/worktrees/app-1234abcd/fix/deeper")));
    assert!(!within(root, Path::new("/r/other/app/fix")));
    assert!(!within(root, Path::new("/r/worktrees/../worktrees2/a/b")));
    assert!(!within(root, Path::new("/r/worktreesx/a/b")));
}

#[test]
fn porcelain_lists_changed_paths_and_worktrees() {
    let status = " M src/main.rs\n?? notes.txt\nR  old.rs -> new.rs\nA  \"with space.rs\"\n";
    assert_eq!(changed_paths(status), ["src/main.rs", "notes.txt", "new.rs", "with space.rs"]);
    assert!(changed_paths("").is_empty());
    let list = "worktree /r/app\nHEAD abc\nbranch refs/heads/main\n\nworktree /w/app-1/fix\nHEAD def\ndetached\nlocked\nprunable gitdir file points to non-existent location\n\nworktree /w/app-1/usb\nbranch refs/heads/aether/usb\nlocked on a usb drive\n";
    let entry = |path: &str, head: &str, branch: Option<&str>, locked: Option<&str>| Entry {
        path: path.into(),
        branch: branch.map(String::from),
        head: head.into(),
        locked: locked.map(String::from),
    };
    assert_eq!(
        entries(list),
        [
            entry("/r/app", "abc", Some("main"), None),
            entry("/w/app-1/fix", "def", None, Some("")),
            entry("/w/app-1/usb", "", Some("aether/usb"), Some("on a usb drive")),
        ]
    );
    assert!(entries("").is_empty());
}

#[test]
fn default_base_prefers_origin_head_then_the_current_branch() {
    let branches: Vec<String> = ["dev", "main", "feature/x"].map(String::from).to_vec();
    assert_eq!(default_base("origin/main\n", "dev\n", &branches).as_deref(), Some("main"));
    assert_eq!(default_base("", "dev\n", &branches).as_deref(), Some("dev"));
    assert_eq!(default_base("origin/gone", "dev", &branches).as_deref(), Some("dev"));
    assert_eq!(default_base("", "", &branches).as_deref(), Some("dev"));
    assert_eq!(default_base("origin/main", "trunk", &[]), None);
}

#[test]
fn bases_put_the_default_then_the_checked_out_branch_first() {
    let all: Vec<String> = ["dev", "x", "main", "wip"].map(String::from).to_vec();
    assert_eq!(order_bases(all.clone(), Some("main"), "wip"), ["main", "wip", "dev", "x"]);
    assert_eq!(order_bases(all.clone(), Some("main"), "main"), ["main", "dev", "x", "wip"]);
    assert_eq!(order_bases(all.clone(), None, ""), ["dev", "x", "main", "wip"]);
    let many: Vec<String> = (0..MAX_BASES + 5).map(|n| format!("b{n}")).collect();
    let current = format!("b{}", MAX_BASES + 4);
    let kept = order_bases(many, Some("b0"), &current);
    assert_eq!(kept.len(), MAX_BASES);
    assert_eq!(kept[..2], ["b0".to_string(), current]);
}

#[test]
fn git_errors_drop_the_fatal_prefix_and_stay_short() {
    let out = |stderr: &str| Output { ok: false, stdout: String::new(), stderr: stderr.into() };
    assert_eq!(
        git_error(&out(
            "fatal: '/w/x' contains modified or untracked files, use --force to delete it\n"
        )),
        "'/w/x' contains modified or untracked files, use --force to delete it"
    );
    assert_eq!(git_error(&out("")), "Git failed without saying why.");
    let noisy = "error: a\nerror: b\nerror: c\nerror: d\nerror: e\n";
    assert_eq!(git_error(&out(noisy)), "a\nb\nc\n…");
}

#[test]
fn remove_offers_branch_delete_but_keeps_branch_by_default() {
    let base = std::env::temp_dir().join(format!("aether-worktree-branch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (repo, root) = (base.join("repo"), base.join("worktrees"));
    let roots = [root.clone()];
    let go = AtomicBool::new(false);
    std::fs::create_dir_all(&root).unwrap();
    if !sh(&repo, &["init", "-q", "-b", "main"]) {
        return;
    }
    std::fs::create_dir_all(repo.join("sub")).unwrap();
    std::fs::write(repo.join("sub/f.txt"), "x").unwrap();
    assert!(sh(&repo, &["add", "sub"]) && sh(&repo, &["commit", "-qm", "init"]));
    let kept = |branch: &str| {
        sh(&repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])
    };

    let keep = create(&root, &repo, "keep me", "main", &go).unwrap();
    remove_worktree_with_branch(&roots, &keep.path, &repo, false, false).unwrap();
    assert!(!keep.path.exists());
    assert!(kept(&keep.branch), "branch stays without the flag");

    let drop = create(&root, &repo, "drop me", "main", &go).unwrap();
    remove_worktree_with_branch(&roots, &drop.path, &repo, false, true).unwrap();
    assert!(!drop.path.exists());
    assert!(!kept(&drop.branch), "branch goes with the flag");

    let dirty = create(&root, &repo, "dirty work", "main", &go).unwrap();
    std::fs::write(dirty.path.join("x.txt"), "x").unwrap();
    assert!(matches!(
        remove_worktree_with_branch(&roots, &dirty.path, &repo, false, true),
        Err(RemoveError::NeedsForce(_))
    ));
    assert!(dirty.path.join("x.txt").is_file());
    assert!(kept(&dirty.branch), "a failed remove deletes nothing");

    let unmerged = create(&root, &repo, "unmerged branch", "main", &go).unwrap();
    assert!(sh(&unmerged.path, &["commit", "-q", "--allow-empty", "-m", "wip"]));
    assert!(matches!(
        remove_worktree_with_branch(&roots, &unmerged.path, &repo, false, true),
        Err(RemoveError::BranchDeleteFailed(_))
    ));
    assert!(!unmerged.path.exists(), "the folder goes even when -d refuses");
    assert!(kept(&unmerged.branch));

    let plain_dir = root.join("repo-plain");
    let plain = plain_dir.join("slot");
    std::fs::create_dir_all(&plain_dir).unwrap();
    assert!(sh(&repo, &["worktree", "add", plain.to_str().unwrap(), "-b", "plain-branch"]));
    assert_eq!(check_remove(&roots, &plain).unwrap().branch.as_deref(), Some("plain-branch"));
    remove_worktree_with_branch(&roots, &plain, &repo, false, true).unwrap();
    assert!(!plain.exists());
    assert!(kept("plain-branch") && kept("main"), "only aether/ branches ever go");
    let _ = std::fs::remove_dir_all(&base);
}

/// Git with a fixed identity and no hooks or signing from the user's config.
fn sh(dir: &Path, args: &[&str]) -> bool {
    let mut all = vec![
        "-c",
        "user.name=a",
        "-c",
        "user.email=a@b",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "commit.gpgsign=false",
    ];
    all.extend(args);
    let all: Vec<&OsStr> = all.iter().map(OsStr::new).collect();
    git::run(dir, &all, SLOW).is_ok_and(|out| out.ok)
}

#[cfg(unix)]
#[test]
fn worktrees_round_trip_in_a_temp_repo() {
    let base = std::env::temp_dir().join(format!("aether-worktree-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (repo, root, old) = (base.join("repo"), base.join("worktrees"), base.join("old-root"));
    let roots = [root.clone(), old.clone()];
    let go = AtomicBool::new(false);
    std::fs::create_dir_all(repo.join("sub/deep")).unwrap();
    std::fs::create_dir_all(repo.join("scratch")).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&old).unwrap();
    if !sh(&repo, &["init", "-q", "-b", "main"]) {
        return;
    }
    std::fs::write(repo.join("sub/deep/f.txt"), "x").unwrap();
    assert!(
        sh(&repo, &["add", "sub"])
            && sh(&repo, &["commit", "-qm", "init"])
            && sh(&repo, &["branch", "dev"])
    );
    std::fs::write(repo.join("scratch/untracked.txt"), "x").unwrap();

    let offered = bases(&repo.join("sub")).unwrap();
    assert_eq!(offered.default.as_deref(), Some("main"));
    assert_eq!(offered.branches[0], "main");
    assert!(offered.branches.contains(&"dev".to_string()));
    assert_eq!(bases(&root), None);
    assert_eq!(bases(&repo.join(".git")), None);

    let stopped = AtomicBool::new(true);
    assert_eq!(create(&root, &repo, "x", "main", &stopped), Err(CANCELED.to_string()));
    let made = create(&root, &repo.join("sub/deep"), "Fix the login bug", "main", &go).unwrap();
    assert_eq!(made.branch, "aether/fix-the-login-bug");
    assert!(made.path.starts_with(aether_core::canonicalize(&root).unwrap()), "{made:?}");
    assert_eq!((made.cwd.clone(), made.missing_subdir.clone()), (made.path.join("sub/deep"), None));
    let again = create(&root, &repo, "Fix the login bug", "dev", &go).unwrap();
    assert_eq!(again.branch, "aether/fix-the-login-bug-2");
    assert_eq!(again.cwd, again.path);
    let loose = create(&old, &repo.join("scratch"), "scratch", "main", &go).unwrap();
    assert_eq!(
        (loose.cwd.clone(), loose.missing_subdir.as_deref()),
        (loose.path.clone(), Some("scratch"))
    );
    assert!(create(&root, &repo, "x", "nope", &go).unwrap_err().contains("nope"));
    assert!(create(&root, &root, "x", "main", &go).is_err());

    let clean = check_remove(&roots, &made.path).unwrap();
    let branch = Some("aether/fix-the-login-bug".to_string());
    let want = RemoveCheck { missing: false, changes: vec![], branch, unmerged: 0, managed: true };
    assert_eq!(clean, want);
    std::fs::write(made.path.join("naïve.txt"), "n").unwrap();
    assert_eq!(check_remove(&roots, &made.path).unwrap().changes, ["naïve.txt"]);
    assert!(sh(&again.path, &["commit", "-q", "--allow-empty", "-m", "wip"]));
    assert_eq!(check_remove(&roots, &again.path).unwrap().unmerged, 1);
    assert!(check_remove(&roots, &loose.path).is_ok());
    assert!(check_remove(&roots[..1], &loose.path).is_err());

    let slot = made.path.parent().unwrap().join("link");
    std::os::unix::fs::symlink(&repo, &slot).unwrap();
    for outside in [&repo, &root, &old, &slot] {
        assert!(check_remove(&roots, outside).is_err(), "{outside:?}");
        assert!(matches!(remove(&roots, outside, &repo, true), Err(RemoveError::Failed(_))));
    }
    assert!(repo.join("sub/deep/f.txt").is_file());

    let listed = list(&roots, &[]);
    let paths: Vec<&PathBuf> = listed.iter().map(|l| &l.path).collect();
    assert_eq!(paths.len(), 3, "{listed:?}");
    assert!(
        paths.contains(&&made.path) && paths.contains(&&again.path) && paths.contains(&&loose.path)
    );
    assert!(listed.iter().all(|l| l.repo == aether_core::canonicalize(&repo).unwrap()
        && !l.missing
        && l.locked.is_none()));

    assert!(sh(
        &repo,
        &["worktree", "lock", "--reason", "on a usb drive", loose.path.to_str().unwrap()]
    ));
    let locked = list(&roots, std::slice::from_ref(&repo))
        .into_iter()
        .find(|l| l.path == loose.path)
        .unwrap();
    assert_eq!(locked.locked.as_deref(), Some("on a usb drive"));
    assert!(matches!(remove(&roots, &loose.path, &repo, false), Err(RemoveError::Failed(_))));
    assert!(loose.path.is_dir());

    assert!(matches!(remove(&roots, &made.path, &repo, false), Err(RemoveError::NeedsForce(_))));
    assert!(made.path.join("naïve.txt").is_file());
    remove(&roots, &made.path, &repo, true).unwrap();
    remove(&roots, &again.path, &repo, false).unwrap();
    assert!(!made.path.exists() && !again.path.exists());
    assert!(sh(
        &repo,
        &["rev-parse", "--verify", "--quiet", "refs/heads/aether/fix-the-login-bug"]
    ));
    assert!(sh(
        &repo,
        &["rev-parse", "--verify", "--quiet", "refs/heads/aether/fix-the-login-bug-2"]
    ));

    let gone = create(&root, &repo, "gone", "main", &go).unwrap();
    let other = create(&root, &repo, "other", "main", &go).unwrap();
    std::fs::remove_dir_all(&gone.path).unwrap();
    std::fs::remove_dir_all(&other.path).unwrap();
    assert!(check_remove(&roots, &gone.path).unwrap().missing);
    assert!(
        list(&roots, std::slice::from_ref(&repo)).iter().any(|l| l.path == gone.path && l.missing)
    );
    let never = gone.path.parent().unwrap().join("never-made");
    remove(&roots, &never, &repo, false).unwrap();
    assert!(
        matches!(remove(&roots, &gone.path, &root, false), Err(RemoveError::Failed(_))),
        "unreadable list"
    );
    remove(&roots, &gone.path, &repo, false).unwrap();
    let left: Vec<PathBuf> =
        list(&roots, std::slice::from_ref(&repo)).into_iter().map(|l| l.path).collect();
    assert!(!left.contains(&gone.path), "{left:?}");
    assert!(left.contains(&other.path), "only the asked-for record goes: {left:?}");
    assert!(sh(&repo, &["worktree", "lock", other.path.to_str().unwrap()]));
    let locked = remove(&roots, &other.path, &repo, false);
    assert!(matches!(&locked, Err(RemoveError::Failed(e)) if e.contains("locked")), "{locked:?}");
    assert!(list(&roots, std::slice::from_ref(&repo)).iter().any(|l| l.path == other.path));
    assert!(sh(&repo, &["worktree", "unlock", other.path.to_str().unwrap()]));
    remove(&roots, &other.path, &repo, false).unwrap();

    assert!(sh(&repo, &["worktree", "unlock", loose.path.to_str().unwrap()]));
    let stray = create(&old, &repo, "stray", "main", &go).unwrap();
    let repo_dir = stray.path.parent().unwrap().to_path_buf();
    assert_eq!(Some(repo_dir.as_path()), loose.path.parent());
    std::fs::remove_dir_all(&repo_dir).unwrap();
    assert!(check_remove(&roots, &stray.path).unwrap().missing);
    assert!(
        list(&roots, std::slice::from_ref(&repo)).iter().any(|l| l.path == stray.path && l.missing)
    );
    assert!(check_remove(&roots, &old.join("elsewhere/x/y")).is_err());
    remove(&roots, &stray.path, &repo, false).unwrap();
    let left: Vec<PathBuf> =
        list(&roots, std::slice::from_ref(&repo)).into_iter().map(|l| l.path).collect();
    assert!(!left.contains(&stray.path) && left.contains(&loose.path), "{left:?}");
    let _ = std::fs::remove_dir_all(&base);
}

/// A fresh repo with one commit at `base/repo`, and a root for aether's worktrees.
fn temp_repo(name: &str) -> Option<(PathBuf, PathBuf, PathBuf)> {
    let base = std::env::temp_dir().join(format!("aether-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (repo, root) = (base.join("repo"), base.join("worktrees"));
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    if !sh(&repo, &["init", "-q", "-b", "main"]) {
        return None;
    }
    std::fs::write(repo.join("f.txt"), "x").unwrap();
    assert!(sh(&repo, &["add", "f.txt"]) && sh(&repo, &["commit", "-qm", "init"]));
    Some((
        aether_core::canonicalize(&base).unwrap(),
        aether_core::canonicalize(&repo).unwrap(),
        root,
    ))
}

#[test]
fn list_repo_shows_main_then_linked_detached_locked_and_missing() {
    let Some((base, repo, root)) = temp_repo("list-repo") else {
        return;
    };
    let spaced = base.join("wt ünï code");
    let (detached, locked, gone) = (base.join("detached"), base.join("locked"), base.join("gone"));
    assert!(sh(&repo, &["worktree", "add", "-b", "spaced", spaced.to_str().unwrap()]));
    assert!(sh(&repo, &["worktree", "add", "--detach", detached.to_str().unwrap()]));
    assert!(sh(&repo, &["worktree", "add", "-b", "locked", locked.to_str().unwrap()]));
    assert!(sh(&repo, &["worktree", "lock", "--reason", "usb", locked.to_str().unwrap()]));
    assert!(sh(&repo, &["worktree", "add", "-b", "gone", gone.to_str().unwrap()]));
    std::fs::remove_dir_all(&gone).unwrap();
    std::fs::write(spaced.join("f.txt"), "changed").unwrap();
    std::fs::write(detached.join("new.txt"), "untracked only").unwrap();

    let found = list_repo(std::slice::from_ref(&root), &spaced).unwrap();
    assert_eq!(found.len(), 5);
    let main = &found[0];
    assert!(main.main && main.path == repo && !main.managed);
    assert_eq!((main.branch.as_deref(), main.dirty), (Some("main"), Some(false)));
    assert!(found[1..].iter().all(|l| !l.main && l.repo == repo));
    let at = |path: &Path| found.iter().find(|l| l.path == path).unwrap();
    assert_eq!((at(&spaced).branch.as_deref(), at(&spaced).dirty), (Some("spaced"), Some(true)));
    assert!(at(&detached).branch.is_none() && at(&detached).head.len() == 40);
    assert_eq!(at(&detached).dirty, Some(false), "untracked files don't count");
    assert_eq!(at(&locked).locked.as_deref(), Some("usb"));
    assert!(at(&gone).missing && at(&gone).dirty.is_none());
    assert!(list_repo(&[root], &base).is_err());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn add_checks_out_local_remote_and_new_branches() {
    let Some((base, repo, root)) = temp_repo("add") else {
        return;
    };
    let roots = [root.clone()];
    let slot = aether_core::canonicalize(&root).unwrap().join(repo_dir(&repo));
    let branch_of = |path: &Path| git::branch_info(path).unwrap().branch;
    assert!(sh(&repo, &["branch", "dev"]));
    assert!(sh(&repo, &["remote", "add", "origin", "/nowhere"]));
    assert!(sh(&repo, &["update-ref", "refs/remotes/origin/feature/api", "HEAD"]));

    let dev = add(&root, &repo, &Start::Local("dev".into()), None).unwrap();
    assert_eq!(dev, slot.join("dev"));
    assert_eq!(branch_of(&dev), "dev");
    let listed = list_repo(&roots, &repo).unwrap();
    assert!(listed.iter().any(|l| l.path == dev && l.managed));
    let taken = add(&root, &repo, &Start::Local("dev".into()), None).unwrap_err();
    assert!(taken.contains("already checked out"), "{taken}");
    assert!(add(&root, &repo, &Start::Local("main".into()), None).is_err(), "main is checked out");
    assert!(add(&root, &repo, &Start::Local("nope".into()), None).is_err());

    let api = add(&root, &dev, &Start::Remote("origin/feature/api".into()), None).unwrap();
    assert_eq!(api, slot.join("feature-api"));
    assert_eq!(branch_of(&api), "feature/api");
    let upstream =
        git::run(&api, &["rev-parse", "--abbrev-ref", "@{u}"].map(OsStr::new), QUICK).unwrap();
    assert_eq!(upstream.stdout.trim(), "origin/feature/api");
    let again = add(&root, &repo, &Start::Remote("origin/feature/api".into()), None).unwrap_err();
    assert!(again.contains("already exists"), "{again}");

    let new = Start::New { name: "my fix".into(), base: None };
    assert!(add(&root, &repo, &new, None).unwrap_err().contains("isn't a valid branch name"));
    let from_dev = Start::New { name: "wip/ünï".into(), base: Some("refs/heads/dev".into()) };
    let wip = add(&root, &repo, &from_dev, None).unwrap();
    assert_eq!(branch_of(&wip), "wip/ünï");
    assert_eq!(wip, slot.join("wip-n"), "folder slugs stay ASCII");
    let head = add(&root, &repo, &Start::New { name: "wip/n".into(), base: None }, None).unwrap();
    assert_eq!(head, slot.join("wip-n-2"), "a taken folder gets a suffix");
    let bad = Start::New { name: "x".into(), base: Some("refs/tags/v1".into()) };
    assert!(add(&root, &repo, &bad, None).is_err());
    assert!(add(&root, &base, &Start::Local("dev".into()), None).is_err(), "not a repo");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn add_into_a_chosen_folder_makes_a_user_worktree_named_from_the_branch() {
    let Some((base, repo, root)) = temp_repo("add-parent") else {
        return;
    };
    let roots = [root.clone()];
    let parent = base.join("my wörk trees");
    std::fs::create_dir_all(&parent).unwrap();
    std::fs::create_dir_all(repo.join("sub")).unwrap();
    assert!(sh(&repo, &["branch", "dev"]));
    let has = |branch: &str| {
        sh(&repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])
    };

    let dev = add(&root, &repo, &Start::Local("dev".into()), Some(&parent)).unwrap();
    assert_eq!(dev, parent.join("dev"));
    assert_eq!(git::branch_info(&dev).unwrap().branch, "dev");
    let fix = Start::New { name: "fix/ünï".into(), base: None };
    let made = add(&root, &repo, &fix, Some(&parent)).unwrap();
    assert_eq!(made, parent.join("fix-n"));
    let listed = list_repo(&roots, &repo).unwrap();
    assert!(listed.iter().any(|l| l.path == made && !l.managed), "outside aether's root");

    let taken = Start::New { name: "fix-n".into(), base: None };
    let err = add(&root, &repo, &taken, Some(&parent)).unwrap_err();
    assert!(err.contains("already exists"), "{err}");
    assert!(!has("fix-n"));

    for inside in [repo.clone(), repo.join("sub"), dev.clone()] {
        let nest = Start::New { name: "nest".into(), base: None };
        let err = add(&root, &repo, &nest, Some(&inside)).unwrap_err();
        assert!(err.contains("is inside the worktree"), "{err}");
        assert!(!inside.join("nest").exists() && !has("nest"));
    }

    let other = base.join("other");
    std::fs::create_dir_all(&other).unwrap();
    let clash = Start::New { name: "dev".into(), base: None };
    assert!(add(&root, &repo, &clash, Some(&other)).is_err(), "branch dev exists");
    assert!(!other.join("dev").exists(), "a failed add leaves no folder behind");

    remove_worktree_with_branch(&roots, &made, &repo, false, true).unwrap();
    assert!(!made.exists() && has("fix/ünï"), "a user worktree's branch stays");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn removal_is_refused_for_main_and_locked_and_prunes_missing() {
    let at = |path: &str| Listed { path: path.into(), ..Listed::default() };
    let main = Listed { main: true, ..at("/code/app") };
    assert_eq!(main.remove_action(), Err("The main worktree can't be removed".into()));
    let locked = Listed { locked: Some(String::new()), ..at("/w/a") };
    assert_eq!(locked.remove_action(), Err("Locked. Unlock it in a terminal to remove it".into()));
    let why = Listed { locked: Some("usb".into()), missing: true, ..at("/w/b") };
    assert_eq!(
        why.remove_action(),
        Err("Locked: usb. Unlock it in a terminal to remove it".into())
    );
    let gone = Listed { missing: true, ..at("/w/c") };
    assert_eq!(gone.remove_action(), Ok("Prune worktree…"));
    let user = at("/w/d");
    assert_eq!(user.remove_action(), Ok("Remove worktree…"));
    assert_eq!(Listed { managed: true, ..user }.remove_action(), Ok("Remove worktree…"));
}

#[cfg(unix)]
#[test]
fn user_worktrees_remove_clean_refuse_dirty_force_once_and_keep_locked() {
    let Some((base, repo, root)) = temp_repo("remove-user") else {
        return;
    };
    let roots = [root];
    let kept = |branch: &str| {
        sh(&repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])
    };
    let make = |name: &str| {
        let path = base.join(format!("{name} wt"));
        assert!(sh(&repo, &["worktree", "add", "-b", name, path.to_str().unwrap()]));
        path
    };

    let clean = make("clean");
    let check = check_any(&roots, &repo, &clean).unwrap();
    assert_eq!((check.managed, check.branch.as_deref()), (false, Some("clean")));
    remove_worktree_with_branch(&roots, &clean, &repo, false, true).unwrap();
    assert!(!clean.exists() && kept("clean"), "a user branch never goes");

    let dirty = make("dirty");
    std::fs::write(dirty.join("f.txt"), "edited").unwrap();
    assert_eq!(check_any(&roots, &repo, &dirty).unwrap().changes, ["f.txt"]);
    assert!(matches!(remove_linked(&repo, &dirty, false), Err(RemoveError::NeedsForce(_))));
    assert!(dirty.join("f.txt").is_file());
    remove_linked(&repo, &dirty, true).unwrap();
    assert!(!dirty.exists() && kept("dirty"));

    let locked = make("locked");
    std::fs::write(locked.join("f.txt"), "edited").unwrap();
    assert!(sh(&repo, &["worktree", "lock", locked.to_str().unwrap()]));
    for force in [false, true] {
        assert!(matches!(remove_linked(&repo, &locked, force), Err(RemoveError::Failed(_))));
        assert!(locked.join("f.txt").is_file(), "one --force never removes a locked worktree");
    }

    let link = base.join("link");
    std::os::unix::fs::symlink(&repo, &link).unwrap();
    let sub = make("sub");
    std::fs::create_dir_all(sub.join("deeper")).unwrap();
    for outside in [&repo, &link, &sub.join("deeper"), &base] {
        assert!(check_any(&roots, &repo, outside).is_err(), "{outside:?}");
        assert!(matches!(remove_linked(&repo, outside, true), Err(RemoveError::Failed(_))));
    }
    assert!(repo.join("f.txt").is_file() && sub.join("deeper").is_dir());

    std::fs::remove_dir_all(&sub).unwrap();
    assert!(check_any(&roots, &repo, &sub).unwrap().missing);
    remove_linked(&repo, &sub, false).unwrap();
    assert!(!list_repo(&roots, &repo).unwrap().iter().any(|l| l.path == sub));
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn prune_drops_missing_records_but_keeps_locked_ones() {
    let Some((base, repo, root)) = temp_repo("prune") else {
        return;
    };
    let (gone, held) = (base.join("gone"), base.join("held"));
    for path in [&gone, &held] {
        assert!(sh(&repo, &["worktree", "add", "--detach", path.to_str().unwrap()]));
        std::fs::remove_dir_all(path).unwrap();
    }
    assert!(sh(&repo, &["worktree", "lock", held.to_str().unwrap()]));
    assert_eq!(prune(&repo).unwrap(), ["gone"]);
    let left: Vec<PathBuf> =
        list_repo(&[root], &repo).unwrap().into_iter().map(|l| l.path).collect();
    assert_eq!(left, [repo.clone(), held]);
    assert!(prune(&repo).unwrap().is_empty());
    assert_eq!(
        pruned("Removing worktrees/a b: gitdir file points to non-existent location\nother\n"),
        ["a b"]
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn only_a_folder_that_is_not_found_counts_as_gone() {
    use std::os::unix::fs::PermissionsExt;
    let base = std::env::temp_dir().join(format!("aether-wt-gone-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (shut, tree) = (base.join("shut"), base.join("shut/tree"));
    std::fs::create_dir_all(&tree).unwrap();
    assert_eq!(gone(&tree), Ok(false));
    assert_eq!(gone(&base.join("nothing")), Ok(true));
    std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o000)).unwrap();
    let (read, check, removed) = (gone(&tree), check_at(&tree), remove_at(&tree, &base, false));
    std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _ = std::fs::remove_dir_all(&base);
    // root reads through any mode
    if read == Ok(false) {
        return;
    }
    assert!(read.unwrap_err().contains("ermission denied"));
    assert!(check.is_err());
    assert!(matches!(removed, Err(RemoveError::Failed(_))));
}
