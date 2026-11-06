use super::*;

#[test]
fn back_goes_to_main_then_closes() {
    assert_eq!(back_of(&PopView::Branch), Some(PopView::Main));
    assert_eq!(back_of(&PopView::Script(Some(2))), Some(PopView::Main));
    assert_eq!(back_of(&PopView::Main), None);
}

#[test]
fn machine_name_prefers_scutil_and_trims_local() {
    assert_eq!(
        machine_name(Some("Iris's MacBook Pro\n"), Some("iris-mbp.local")).as_deref(),
        Some("Iris's MacBook Pro")
    );
    assert_eq!(machine_name(Some("  "), Some("iris-mbp.local")).as_deref(), Some("iris-mbp"));
    assert_eq!(machine_name(None, None), None);
}

fn change(stat: aether_git::Stat) -> aether_git::Change {
    aether_git::Change {
        path: "a".into(),
        from: None,
        kind: aether_git::ChangeKind::Modified,
        staged: false,
        stat,
    }
}

#[test]
fn line_totals_sum_lines_and_skip_binary_unknown() {
    use aether_git::Stat;
    let changes = [
        change(Stat::Lines(10, 2)),
        change(Stat::Binary),
        change(Stat::Unknown),
        change(Stat::Lines(5995, 0)),
    ];
    assert_eq!(line_totals(&changes), (6005, 2));
}

#[test]
fn thousands_groups_digits() {
    assert_eq!(thousands(6005), "6,005");
    assert_eq!(thousands(1_234_567), "1,234,567");
    assert_eq!(thousands(2), "2");
}

#[test]
fn home_short_tildes_home_paths() {
    let home = Path::new("/Users/i");
    assert_eq!(home_short(Path::new("/Users/i/Code/x"), home), "~/Code/x");
    assert_eq!(home_short(Path::new("/Users/i"), home), "~");
    assert_eq!(home_short(Path::new("/opt/x"), home), "/opt/x");
}

#[test]
fn pop_rows_for_missing_folder_disable_git_and_scripts() {
    let r = pop_rows(false, true);
    assert!(!r.git && !r.scripts && r.reason == Some("Folder not found"));
    let r = pop_rows(true, false);
    assert!(!r.git && r.scripts);
    let r = pop_rows(true, true);
    assert!(r.git && r.scripts && r.reason.is_none());
}

#[test]
fn disabled_label_names_the_reason() {
    assert_eq!(
        disabled_label("Browse files", "Folder not found"),
        "Browse files, unavailable: Folder not found"
    );
}

#[test]
fn newest_chat_in_picks_latest_activity_in_path() {
    let chats = [(0, "/a".into(), 5), (1, "/b".into(), 9), (2, "/a".into(), 7)];
    assert_eq!(newest_chat_in(&chats, Path::new("/a")), Some(2));
    assert_eq!(newest_chat_in(&chats, Path::new("/c")), None);
}

#[test]
fn branch_block_reason_prefers_missing_folder() {
    assert_eq!(branch_block_reason(true, false), Some("Folder not found"));
    assert_eq!(branch_block_reason(true, true), Some("An agent is working in this folder"));
    assert_eq!(branch_block_reason(false, true), None);
}

#[test]
fn branch_view_rows_offer_create_only_for_new_names() {
    use PickRow::{Caption, Create, Local, Remote};
    let b = aether_git::Branches {
        current: Some("main".into()),
        local: vec!["main".into(), "feat/x".into()],
        remote: vec!["origin/main".into()],
    };
    assert_eq!(
        branch_view_rows(&b, ""),
        [
            Caption("Branches"),
            Local("main".into()),
            Local("feat/x".into()),
            Caption("Remote branches"),
            Remote("origin/main".into()),
        ]
    );
    assert_eq!(
        branch_view_rows(&b, " feat "),
        [Create, Caption("Branches"), Local("feat/x".into())]
    );
    assert_eq!(branch_view_rows(&b, "main")[0], Caption("Branches"));
}

#[test]
fn branch_row_label_names_state() {
    let label = |row: &PickRow, at| branch_row_label(row, "new", Some("main"), at);
    assert_eq!(label(&PickRow::Create, None), "Create branch new from main");
    assert_eq!(label(&PickRow::Local("main".into()), None), "main, current branch");
    assert_eq!(
        label(&PickRow::Local("fix".into()), Some("in fix")),
        "fix, checked out in fix, open its chat"
    );
    assert_eq!(label(&PickRow::Remote("origin/x".into()), None), "origin/x, remote branch");
    assert_eq!(
        branch_row_label(&PickRow::Create, "new", None, None),
        "Create branch new from HEAD"
    );
}

#[test]
fn view_title_for_sub_views_only() {
    assert_eq!(view_title(&PopView::Main), None);
    assert_eq!(view_title(&PopView::Location), Some("Location"));
    assert_eq!(view_title(&PopView::Apps), Some("Open in"));
    assert_eq!(view_title(&PopView::Branch), Some("Branch"));
}

#[test]
fn pop_step_threads_the_field_between_header_and_rows() {
    assert_eq!(pop_step(None, true, true), Some(Stop::Row(1)));
    assert_eq!(pop_step(None, true, false), Some(Stop::Row(0)));
    assert_eq!(pop_step(Some(0), true, true), Some(Stop::Field));
    assert_eq!(pop_step(Some(1), true, false), Some(Stop::Field));
    assert_eq!(pop_step(Some(1), true, true), Some(Stop::Row(2)));
    assert_eq!(pop_step(Some(1), false, false), Some(Stop::Row(0)));
    assert_eq!(pop_step(Some(0), false, false), None);
    assert_eq!(pop_step(None, false, true), Some(Stop::Row(0)));
    assert_eq!(pop_step(None, false, false), None);
}

#[test]
fn footer_error_keeps_first_detail_line() {
    assert_eq!(
        footer_error("Couldn't switch branches", "\nerror: local changes\n\tsrc/a.rs\n"),
        "Couldn't switch branches: error: local changes"
    );
    assert_eq!(footer_error("Couldn't switch branches", "  "), "Couldn't switch branches");
}

#[test]
fn refusal_reason_names_the_blocker() {
    assert_eq!(refusal_reason(true, true), "Another git command is running");
    assert_eq!(refusal_reason(false, true), "This folder is switching branches");
    assert_eq!(
        refusal_reason(false, false),
        "Its folder changed before git ran, so nothing was done"
    );
}

#[test]
fn view_title_names_publish_and_remote() {
    assert_eq!(view_title(&PopView::Publish), Some("Publish repository"));
    assert_eq!(view_title(&PopView::Remote), Some("Remote"));
}

#[test]
fn gh_state_from_auth_status() {
    assert_eq!(gh_state(false, false), GhState::Missing);
    assert_eq!(gh_state(true, false), GhState::LoggedOut);
    assert_eq!(gh_state(true, true), GhState::Ready);
}

#[test]
fn remote_row_states() {
    let remote = |name: &str, url: &str| Remote { name: name.into(), url: url.into() };
    let origin = [remote("origin", "git@github.com:iris/ac.git")];
    let empty = Status::default();
    let committed = Status {
        commit: Some("abc1234".into()),
        branch: Some("main".into()),
        ..Default::default()
    };
    let detached = Status { branch: None, ..committed.clone() };
    let no_commit = RemoteRow::Publish { blocked: Some("Make a first commit to publish") };
    let no_gh = RemoteRow::Publish { blocked: Some("Install GitHub CLI and run gh auth login") };
    assert_eq!(remote_row(false, None, &[], GhState::Ready), RemoteRow::Init);
    assert_eq!(remote_row(true, Some(&empty), &[], GhState::Ready), no_commit);
    assert_eq!(remote_row(true, Some(&empty), &[], GhState::Missing), no_commit);
    assert_eq!(remote_row(true, Some(&committed), &[], GhState::LoggedOut), no_gh);
    assert_eq!(remote_row(true, Some(&committed), &[], GhState::Missing), no_gh);
    assert_eq!(
        remote_row(true, Some(&detached), &[], GhState::Missing),
        RemoteRow::Publish { blocked: Some("Check out a branch to publish") }
    );
    assert_eq!(remote_row(true, Some(&empty), &[], GhState::Missing), no_commit);
    assert_eq!(
        remote_row(true, Some(&committed), &[], GhState::Ready),
        RemoteRow::Publish { blocked: None }
    );
    assert_eq!(
        remote_row(true, Some(&committed), &origin, GhState::Ready),
        RemoteRow::PublishBranch
    );
    let tracked = Status {
        upstream: Some("origin/main".into()),
        ahead_behind: Some((2, 1)),
        ..committed.clone()
    };
    let expected = RemoteRow::Remote {
        name: "origin".into(),
        slug: Some("iris/ac".into()),
        ahead: 2,
        behind: 1,
        gone: false,
    };
    assert_eq!(remote_row(true, Some(&tracked), &origin, GhState::Missing), expected);
    let both = [remote("fork", "https://github.com/x/ac"), origin[0].clone()];
    assert_eq!(remote_row(true, Some(&tracked), &both, GhState::Ready), expected);
    let gone = Status { ahead_behind: None, ..tracked.clone() };
    assert_eq!(
        remote_row(true, Some(&gone), &origin, GhState::Ready),
        RemoteRow::Remote {
            name: "origin".into(),
            slug: Some("iris/ac".into()),
            ahead: 0,
            behind: 0,
            gone: true,
        }
    );
}

#[test]
fn publish_args_shape() {
    assert_eq!(
        publish_args("ac", true, Path::new("/p")),
        ["repo", "create", "ac", "--private", "--source", "/p", "--remote", "origin", "--push"]
    );
    assert_eq!(publish_args("ac", false, Path::new("/p"))[3], "--public");
}

#[test]
fn compare_url_builds_github_compare() {
    assert_eq!(
        compare_url("https://github.com/o/r", "feat/x"),
        "https://github.com/o/r/compare/feat/x?expand=1"
    );
    assert_eq!(
        compare_url("https://github.com/o/r", "fix/a b#1?%"),
        "https://github.com/o/r/compare/fix/a%20b%231%3F%25?expand=1"
    );
}

#[test]
fn pr_branch_prefers_the_upstream_name() {
    assert_eq!(pr_branch(Some("origin/feat/y"), "origin", Some("x")).as_deref(), Some("feat/y"));
    assert_eq!(pr_branch(Some("other/y"), "origin", Some("x")).as_deref(), Some("x"));
    assert_eq!(pr_branch(None, "origin", Some("x")).as_deref(), Some("x"));
    assert_eq!(pr_branch(None, "origin", None), None);
}

#[test]
fn repo_name_ok_allows_github_charset() {
    for good in ["ac", "auto-clicker", "a.b_c-1", "iris/ac"] {
        assert!(repo_name_ok(good), "{good}");
    }
    for bad in ["", "-x", "a b", "a/b/c", "/ac", "iris/", "iris/-x", "-o/ac", "ác", "a;b"] {
        assert!(!repo_name_ok(bad), "{bad}");
    }
}

#[test]
fn publish_error_keeps_first_stderr_line() {
    assert_eq!(
        first_stderr_line("\n  GraphQL: Name already exists\nmore"),
        "GraphQL: Name already exists"
    );
    assert_eq!(first_stderr_line(""), "gh failed");
}

#[test]
fn sync_summary_and_spoken_form() {
    assert_eq!(sync_summary(Some((0, 0))), "Up to date");
    assert_eq!(sync_summary(Some((2, 1))), "\u{2191}2 \u{2193}1");
    assert_eq!(sync_summary(None), "Upstream gone");
    assert_eq!(sync_spoken(Some((0, 0))), "up to date");
    assert_eq!(sync_spoken(Some((2, 0))), "2 to push, 0 to pull");
    assert_eq!(sync_spoken(None), "upstream gone");
}

#[test]
fn script_default_name_skips_flags() {
    assert_eq!(script_default_name("pnpm dev"), "dev");
    assert_eq!(script_default_name("cargo run --release"), "run");
    assert_eq!(script_default_name("make && ./bin/serve -p 3"), "make");
    assert_eq!(script_default_name("   "), "script");
    assert_eq!(script_default_name("ls -la | wc"), "ls");
    assert_eq!(script_default_name("-x"), "script");
}

#[test]
fn script_runs_one_per_folder_and_name() {
    let (a, b) = (Path::new("/a"), Path::new("/b"));
    let mut runs = ScriptRuns::default();
    assert!(runs.start(a, "dev", 1));
    assert!(!runs.start(a, "dev", 2));
    assert_eq!(runs.running(a, "dev"), Some(1));
    assert!(runs.start(b, "dev", 2));
    runs.end(a, "dev");
    assert_eq!(runs.running(a, "dev"), None);
    assert_eq!(runs.running(b, "dev"), Some(2));
}

#[test]
fn runs_for_closed_chat_are_dropped() {
    let a = Path::new("/a");
    let mut runs = ScriptRuns::default();
    runs.start(a, "dev", 1);
    runs.start(a, "test", 2);
    assert_eq!(runs.drop_chat(1), vec![(a.to_path_buf(), "dev".to_string())]);
    assert_eq!(runs.running(a, "dev"), None);
    assert_eq!(runs.running(a, "test"), Some(2));
}

#[test]
fn ended_run_clears_only_its_owner() {
    let (a, b) = (Path::new("/a"), Path::new("/b"));
    let mut runs = ScriptRuns::default();
    runs.start(a, "dev", 1);
    runs.start(b, "dev", 2);
    runs.end_owned(3, "dev");
    runs.end_owned(1, "dev");
    assert_eq!(runs.running(a, "dev"), None);
    assert_eq!(runs.running(b, "dev"), Some(2));
}

#[test]
fn rerun_after_folder_move_drops_the_chats_old_entry() {
    let (a, b, c) = (Path::new("/a"), Path::new("/b"), Path::new("/c"));
    let mut runs = ScriptRuns::default();
    runs.start(a, "dev", 1);
    runs.start(a, "test", 1);
    runs.start(c, "dev", 2);
    assert!(runs.start(b, "dev", 1));
    assert_eq!(runs.running(a, "dev"), None);
    assert_eq!(runs.running(b, "dev"), Some(1));
    assert_eq!(runs.running(a, "test"), Some(1));
    assert_eq!(runs.running(c, "dev"), Some(2));
}

#[test]
fn script_names_are_unique_per_project() {
    let script = |name: &str| aether_core::ProjectScript {
        name: name.into(),
        command: "x".into(),
        on_new_worktree: false,
    };
    let list = [script("dev"), script("test")];
    assert!(script_name_taken(&list, "dev", None));
    assert!(!script_name_taken(&list, "build", None));
    assert!(!script_name_taken(&list, "dev", Some(0)));
    assert!(script_name_taken(&list, "dev", Some(1)));
}

#[test]
fn script_menu_label_names_the_script() {
    assert_eq!(script_menu_label("dev"), "Actions for dev");
}

#[test]
fn script_labels_say_run_or_stop() {
    let script = aether_core::ProjectScript {
        name: "dev".into(),
        command: "pnpm dev".into(),
        on_new_worktree: false,
    };
    assert_eq!(script_action(&script.name, false), "Run dev");
    assert_eq!(script_action(&script.name, true), "Stop dev");
    assert_eq!(script_row_label(&script, false), "Run dev, pnpm dev");
    let setup = aether_core::ProjectScript { on_new_worktree: true, ..script };
    assert_eq!(script_row_label(&setup, true), "Stop dev, pnpm dev, on new worktree");
}

#[test]
fn shrunk_rows_move_focus_to_the_last_one() {
    assert_eq!(row_after_shrink(Some(5), 3), Some(2));
    assert_eq!(row_after_shrink(Some(3), 3), Some(2));
    assert_eq!(row_after_shrink(Some(1), 3), None);
    assert_eq!(row_after_shrink(None, 3), None);
    assert_eq!(row_after_shrink(Some(2), 0), None);
}

#[test]
fn main_view_spoken_labels() {
    assert_eq!(branch_spoken("main", false), "Branch main");
    assert_eq!(branch_spoken("feat", true), "Branch feat, worktree");
    assert_eq!(copy_label("Project root", "~/x"), "Copy Project root ~/x");
    assert_eq!(choose_app_label(Some("Zed")), "Choose app, current: Zed");
    assert_eq!(choose_app_label(None), "Choose app");
}

#[test]
fn changes_spoken_reads_totals_or_nothing() {
    let status = aether_git::Status {
        changes: vec![change(aether_git::Stat::Lines(6005, 2))],
        ..Default::default()
    };
    assert_eq!(changes_spoken(Some(&status)), "Changes, 6,005 added, 2 removed");
    assert_eq!(changes_spoken(None), "Changes");
}

#[test]
fn git_trouble_tells_not_repo_from_failures() {
    use aether_git::StatusError;
    assert_eq!(git_trouble(None), None);
    assert_eq!(git_trouble(Some(&Ok(Status::default()))), None);
    assert_eq!(git_trouble(Some(&Err(StatusError::NotRepo))), Some("Not a git repository"));
    assert_eq!(git_trouble(Some(&Err(StatusError::Locked))), Some("Git is busy"));
    let missing = Err(StatusError::Failed("git: not found".into()));
    assert_eq!(git_trouble(Some(&missing)), Some("Couldn't run git"));
}

#[test]
fn publish_name_from_folder_fits_github() {
    assert_eq!(repo_name_from("My App"), "My-App");
    assert_eq!(repo_name_from("caf\u{e9}  app!"), "caf-app");
    assert_eq!(repo_name_from("aether.rs_2"), "aether.rs_2");
    assert!(repo_name_ok(&repo_name_from("My App")));
}

#[test]
fn runs_of_a_script_in_every_folder() {
    let (a, b) = (Path::new("/a"), Path::new("/b"));
    let mut runs = ScriptRuns::default();
    runs.start(a, "dev", 1);
    runs.start(b, "dev", 2);
    runs.start(a, "test", 1);
    let mut found = runs.runs_of("dev");
    found.sort();
    assert_eq!(found, vec![(a.to_path_buf(), 1), (b.to_path_buf(), 2)]);
    assert!(runs.runs_of("build").is_empty());
}
