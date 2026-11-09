use super::*;

#[test]
fn branch_rows_truncate_past_22_chars() {
    assert_eq!(LONG_BRANCH, 22);
    assert_eq!(middle_truncate("main", LONG_BRANCH), "main");
    let exact = "a".repeat(LONG_BRANCH);
    assert_eq!(middle_truncate(&exact, LONG_BRANCH), exact);
    let long = "feature/a-very-long-login-fix";
    assert!(long.chars().count() > LONG_BRANCH);
    let short = middle_truncate(long, LONG_BRANCH);
    assert_eq!(short.chars().count(), LONG_BRANCH);
    assert!(short.starts_with("feature/") && short.ends_with("login-fix"));
    assert!(short.contains('…'));
}

#[test]
fn long_branches_keep_both_ends() {
    assert_eq!(middle_truncate("main", 22), "main");
    assert_eq!(middle_truncate("feature/a-very-long-login-fix", 11), "featu…n-fix");
    assert_eq!(middle_truncate("feature/a-very-long-login-fix", 11).chars().count(), 11);
    assert_eq!(middle_truncate("ünïcödé-bränch-ñame", 7), "ünï…ame");
}

#[test]
fn project_initials() {
    assert_eq!(initials("auto-clicker"), "AC");
    assert_eq!(initials("my_cool app"), "MC");
    assert_eq!(initials("aether"), "AE");
    assert_eq!(initials("x"), "X");
    assert_eq!(initials("ünï-côde"), "ÜC");
    assert_eq!(initials("--"), "");
}

#[test]
fn project_hue_is_stable_and_spread() {
    let hue = project_hue("auto-clicker");
    assert_eq!(hue, project_hue("auto-clicker"));
    assert!((0. ..1.).contains(&hue));
    assert_ne!(hue, project_hue("aether"));
    let gap = (project_hue("yachtcloud-ycs") - project_hue("aether")).abs();
    assert!(gap.min(1. - gap) > 0.1, "neighbouring projects get distinct hues");
}

#[test]
fn aria_carries_hover_card_lines() {
    use aether_core::Provider;
    let line = ChatLine {
        provider: Provider::OpenCode,
        project: Some("auto-clicker".into()),
        model: "DeepSeek V4.1 Flash Free".into(),
        place: Some(RowPlace::Branch {
            name: "feat/autoclicker".into(),
            worktree: false,
            detached: false,
        }),
        time: "15h".into(),
        path: "/repo".into(),
        removed: false,
        changed: 0,
        added: 0,
        deleted: 0,
        ahead_behind: None,
        background: 0,
    };
    assert_eq!(
        aria("Project Purpose", ChatStatus::Done, false, Some(&line), None),
        "Project Purpose, new reply, auto-clicker, Branch feat/autoclicker, \
         OpenCode DeepSeek V4.1 Flash Free, 15h"
    );
    let bare = ChatLine { project: None, place: None, ..line };
    assert_eq!(
        aria("Greeting", ChatStatus::Idle, false, Some(&bare), None),
        "Greeting, No project, OpenCode DeepSeek V4.1 Flash Free, 15h"
    );
    assert_eq!(aria("Plain", ChatStatus::Idle, false, None, None), "Plain");
    assert_eq!(
        aria("Greeting", ChatStatus::Idle, true, Some(&bare), None),
        "Greeting, Lead, No project, OpenCode DeepSeek V4.1 Flash Free, 15h"
    );
}

#[test]
fn lead_aria_reads_spawned_count_and_rollup() {
    use super::super::crew::CrewSummary;
    let line = ChatLine {
        provider: aether_core::Provider::Claude,
        project: Some("aether".into()),
        model: "Opus".into(),
        place: None,
        time: "2m".into(),
        path: "/repo".into(),
        removed: false,
        changed: 0,
        added: 0,
        deleted: 0,
        ahead_behind: None,
        background: 0,
    };
    let summary = CrewSummary { needs_you: 1, failed: 0, working: 2, done: 12 };
    assert_eq!(
        aria("Title", ChatStatus::Idle, true, Some(&line), Some((summary, false, 4))),
        "Lead, Title, 4 spawned agents, 1 needs you, 2 working, 12 done, collapsed"
    );
    assert!(
        aria("Title", ChatStatus::Idle, true, Some(&line), Some((summary, true, 4)))
            .ends_with(", expanded")
    );
}

#[test]
fn child_aria_is_short() {
    assert_eq!(
        aria("Title", ChatStatus::Working { since_ms: 0 }, false, None, None),
        "Title, working"
    );
}

#[test]
fn project_label_falls_back_to_no_project() {
    assert_eq!(project_label(None).as_ref(), "No project");
    let named: SharedString = "my-repo".into();
    assert_eq!(project_label(Some(&named)).as_ref(), "my-repo");
}

#[test]
fn status_state_predicates_and_waiting_action() {
    assert!(ChatStatus::Waiting.is_waiting());
    assert!(!ChatStatus::Idle.is_waiting());
    assert!(!ChatStatus::Working { since_ms: 100 }.is_waiting());

    assert!(ChatStatus::Working { since_ms: 100 }.is_working());
    assert!(ChatStatus::Starting.is_working());
    assert!(!ChatStatus::Done.is_working());

    assert!(ChatStatus::Waiting.is_busy());
    assert!(ChatStatus::Working { since_ms: 100 }.is_busy());
    assert!(!ChatStatus::Done.is_busy());
    assert!(!ChatStatus::Idle.is_busy());
}
