use super::*;
use aether_core::{Request, RequestStatus};
use aether_git::{Change, ChangeKind, Flow, Stat};

fn card(id: i64, title: &str, place: &str, status: CardStatus, activity_ms: i64) -> Card {
    Card {
        ix: id as usize,
        id,
        title: title.into(),
        provider: Provider::Claude,
        model: "Claude Opus 5.5".into(),
        effort: String::new(),
        status,
        since_ms: 0,
        place: place.into(),
        added: 0,
        removed: 0,
        ahead: 0,
        activity_ms,
        text: String::new(),
        cwd: PathBuf::from("/p"),
        worktree: false,
        gone: false,
    }
}

fn keys(groups: &[(String, Vec<Card>)]) -> Vec<&str> {
    groups.iter().map(|(k, _)| k.as_str()).collect()
}

fn ids(cards: &[Card]) -> Vec<i64> {
    cards.iter().map(|c| c.id).collect()
}

fn commit(at_ms: i64) -> Activity {
    Activity::Commit { short: "abc".into(), subject: "s".into(), at_ms }
}

fn chat(at_ms: i64) -> Activity {
    Activity::Chat { ix: 0, title: "t".into(), verb: "finished", at_ms }
}

#[test]
fn idle_with_failed_last_message_is_failed() {
    assert_eq!(card_status(ChatStatus::Idle, true, ""), CardStatus::Failed);
    assert_eq!(card_status(ChatStatus::Idle, false, ""), CardStatus::Idle);
    assert_eq!(card_status(ChatStatus::Starting, false, ""), CardStatus::Working);
    assert_eq!(card_status(ChatStatus::Waiting, false, "Answer"), CardStatus::Waiting("Answer"));
}

#[test]
fn filters_partition_statuses() {
    let w = CardStatus::Waiting("Approve");
    let all = [CardStatus::Working, w, CardStatus::Failed, CardStatus::Done, CardStatus::Idle];
    let pick = |f| all.into_iter().filter(|s| in_filter(f, *s)).collect::<Vec<_>>();
    assert_eq!(pick(Filter::All).len(), 5);
    assert_eq!(pick(Filter::Running), [CardStatus::Working]);
    assert_eq!(pick(Filter::NeedsYou), [w, CardStatus::Failed]);
    assert_eq!(pick(Filter::Done), [CardStatus::Done, CardStatus::Idle]);
}

#[test]
fn arrange_groups_status_by_rank() {
    let cards = vec![
        card(1, "a", "x", CardStatus::Done, 3),
        card(2, "b", "x", CardStatus::Waiting("Approve"), 2),
        card(3, "c", "x", CardStatus::Working, 1),
    ];
    let out = arrange(cards, Filter::All, GroupBy::Status, SortBy::Activity);
    assert_eq!(keys(&out), ["Needs you", "Running", "Done"]);
}

#[test]
fn arrange_sorts_by_activity_then_groups_by_first_appearance() {
    let cards = vec![
        card(1, "a", "main", CardStatus::Done, 10),
        card(2, "b", "feat", CardStatus::Done, 30),
        card(3, "c", "main", CardStatus::Done, 20),
    ];
    let out = arrange(cards, Filter::All, GroupBy::Worktree, SortBy::Activity);
    assert_eq!(keys(&out), ["feat", "main"]);
    assert_eq!(ids(&out[1].1), [3, 1]);
}

#[test]
fn arrange_none_is_single_unkeyed_group() {
    let cards =
        vec![card(1, "a", "x", CardStatus::Done, 1), card(2, "b", "y", CardStatus::Working, 2)];
    let out = arrange(cards, Filter::Done, GroupBy::None, SortBy::Newest);
    assert_eq!(keys(&out), [""]);
    assert_eq!(ids(&out[0].1), [1]);
}

#[test]
fn sort_by_name_is_case_insensitive() {
    let cards = ["b", "A", "c"]
        .iter()
        .enumerate()
        .map(|(i, t)| card(i as i64, t, "x", CardStatus::Done, 0))
        .collect();
    let out = arrange(cards, Filter::All, GroupBy::None, SortBy::Name);
    let titles: Vec<_> = out[0].1.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, ["A", "b", "c"]);
}

#[test]
fn sort_by_status_ranks_then_activity() {
    let cards = vec![
        card(1, "a", "x", CardStatus::Done, 50),
        card(2, "b", "x", CardStatus::Working, 10),
        card(3, "c", "x", CardStatus::Working, 20),
    ];
    let out = arrange(cards, Filter::All, GroupBy::None, SortBy::Status);
    assert_eq!(ids(&out[0].1), [3, 2, 1]);
}

#[test]
fn recent_merges_and_caps_commits() {
    let commits = (0..6).map(|i| commit(200 - i * 10)).collect();
    let chats = (0..4).map(|i| chat(95 - i * 10)).collect();
    let out = recent(chats, commits, 8);
    assert_eq!(out.len(), 8);
    assert_eq!(out.iter().filter(|a| matches!(a, Activity::Commit { .. })).count(), 5);
    assert!(!out.contains(&commit(150)));
    assert!(out.windows(2).all(|w| w[0].at_ms() >= w[1].at_ms()));
}

#[test]
fn card_aria_skips_empty_parts() {
    let mut c = card(1, "Project Purpose", "feat/autoclicker", CardStatus::Working, 0);
    assert_eq!(
        card_aria(&c, "2m"),
        "Project Purpose, Working, feat/autoclicker, Claude Opus 5.5, 2m"
    );
    c.model.clear();
    assert_eq!(card_aria(&c, "2m"), "Project Purpose, Working, feat/autoclicker, 2m");
    c.status = CardStatus::Waiting("Answer");
    assert_eq!(card_aria(&c, "2m"), "Project Purpose, Needs an answer, feat/autoclicker, 2m");
    c.status = CardStatus::Waiting("Approve");
    assert_eq!(card_aria(&c, "2m"), "Project Purpose, Needs approval, feat/autoclicker, 2m");
}

#[test]
fn last_text_uses_first_nonempty_line() {
    let mut m = Message::assistant();
    m.text = "\n\nDone. All good\nmore".into();
    assert_eq!(last_text(Some(&m), CardStatus::Done), "Done. All good");
    assert_eq!(last_text(None, CardStatus::Working), "Working…");
    assert_eq!(last_text(None, CardStatus::Failed), "Failed");
    m.apply(aether_core::Update::Error("boom\ndetails".into()));
    assert_eq!(last_text(Some(&m), CardStatus::Failed), "boom");
}

#[test]
fn last_text_reads_errors_legacy_chats_saved_in_the_text() {
    let mut m = Message::assistant();
    m.text = "partial\n\n**Error:** claude exited (1)\npanic".into();
    m.failed = true;
    assert_eq!(last_text(Some(&m), CardStatus::Failed), "claude exited (1)");
}

#[test]
fn verbs_map_waiting_labels() {
    assert_eq!(verb(CardStatus::Waiting("Approve")), "needs approval");
    assert_eq!(verb(CardStatus::Waiting("Answer")), "needs an answer");
    assert_eq!(verb(CardStatus::Waiting("Review")), "needs review");
    assert_eq!(verb(CardStatus::Idle), "finished");
}

#[test]
fn last_text_waiting_shows_the_pending_ask() {
    let mut m = Message::assistant();
    let ask = Ask::Question { question: "Which database?".into(), choices: Vec::new() };
    m.requests.push(Request { id: 1, at: 0, ask, status: RequestStatus::Pending, asked_ms: None });
    assert_eq!(last_text(Some(&m), CardStatus::Waiting("Answer")), "Which database?");
    m.requests[0].ask = Ask::Plan(Default::default());
    assert_eq!(last_text(Some(&m), CardStatus::Waiting("Review")), "Review plan");
    m.requests[0].status = RequestStatus::Expired;
    assert_eq!(last_text(Some(&m), CardStatus::Waiting("Review")), "");
}

#[test]
fn last_text_truncates_multibyte_text_to_140_chars() {
    let mut m = Message::assistant();
    m.text = "\u{e9}".repeat(200);
    let out = last_text(Some(&m), CardStatus::Idle);
    assert_eq!(out.chars().count(), 140);
    assert!(out.chars().all(|c| c == '\u{e9}'));
}

#[test]
fn badges_by_status() {
    let min = 60_000;
    assert_eq!(badge(CardStatus::Working, 0, 2 * min).as_deref(), Some("Working · 2m"));
    assert_eq!(badge(CardStatus::Working, 0, 1_000).as_deref(), Some("Working"));
    assert_eq!(badge(CardStatus::Waiting("Approve"), 0, 0).as_deref(), Some("Needs approval"));
    assert_eq!(badge(CardStatus::Waiting("Answer"), 0, 0).as_deref(), Some("Needs an answer"));
    assert_eq!(badge(CardStatus::Waiting("Review"), 0, 0).as_deref(), Some("Needs review"));
    assert_eq!(badge(CardStatus::Failed, 0, 0).as_deref(), Some("Failed"));
    assert_eq!(badge(CardStatus::Done, 0, 0).as_deref(), Some("Done"));
    assert_eq!(badge(CardStatus::Idle, 0, 0), None);
}

fn project(repo: bool, trees: usize, remotes: &[(&str, &str)]) -> ProjectData {
    let change = Change {
        path: "a".into(),
        from: None,
        kind: ChangeKind::Modified,
        staged: false,
        stat: Stat::Lines(6005, 2),
    };
    ProjectData {
        folder: PathBuf::from("/p"),
        status: repo.then(|| Status {
            branch: Some("main".into()),
            changes: vec![change],
            ..Status::default()
        }),
        repo,
        worktrees: vec![Listed::default(); trees],
        remotes: remotes
            .iter()
            .map(|(name, url)| Remote { name: (*name).into(), url: (*url).into() })
            .collect(),
        commits: Vec::new(),
    }
}

#[test]
fn facts_for_a_published_repo() {
    let remotes = [("fork", "git@github.com:other/a.git"), ("origin", "git@github.com:me/a.git")];
    let d = project(true, 3, &remotes);
    assert_eq!(
        facts("~/Code/a".into(), false, Some(&d)),
        [
            (Fact::Path, "~/Code/a".to_string()),
            (Fact::Branch, "main · 3 worktrees".into()),
            (Fact::Changes, "uncommitted".into()),
            (Fact::Remote, "me/a".into()),
        ]
    );
}

#[test]
fn facts_for_an_unpublished_repo_with_one_worktree() {
    let mut d = project(true, 1, &[]);
    let out = facts("~/a".into(), false, Some(&d));
    assert_eq!(out[1], (Fact::Branch, "main · 1 worktree".into()));
    assert_eq!(out[3], (Fact::Remote, "not published".into()));
    d.status = None;
    let kinds: Vec<Fact> = facts("~/a".into(), false, Some(&d)).into_iter().map(|f| f.0).collect();
    assert_eq!(kinds, [Fact::Path, Fact::Branch, Fact::Remote]);
}

#[test]
fn facts_for_non_repo_missing_and_loading() {
    let path = || "~/a".to_string();
    let not_repo = project(false, 0, &[]);
    assert_eq!(
        facts(path(), false, Some(&not_repo)),
        [(Fact::Path, path()), (Fact::Warn, "not a git repository".into())]
    );
    let repo = project(true, 2, &[]);
    assert_eq!(
        facts(path(), true, Some(&repo)),
        [(Fact::Path, path()), (Fact::Warn, "Folder not found".into())]
    );
    assert_eq!(facts(path(), false, None), [(Fact::Path, path())]);
}

#[test]
fn tree_badges_count_cards_in_that_folder() {
    let mut cards = vec![
        card(1, "a", "x", CardStatus::Working, 0),
        card(2, "b", "x", CardStatus::Waiting("Approve"), 0),
        card(3, "c", "x", CardStatus::Done, 0),
        card(4, "d", "y", CardStatus::Working, 0),
    ];
    cards[3].cwd = PathBuf::from("/w");
    let trees = [tree("/p"), tree("/w")];
    let some = |s: &str| Some(s.to_string());
    assert_eq!(
        tree_badges(&cards, &trees, Path::new("/p")),
        [some("3 agents"), some("1 running"), some("1 waiting")]
    );
    assert_eq!(
        tree_badges(&cards, &trees, Path::new("/w")),
        [some("1 agent"), some("1 running"), None]
    );
    assert_eq!(tree_badges(&cards, &trees, Path::new("/none")), [None, None, None]);
}

fn tree(path: &str) -> Listed {
    Listed { path: path.into(), ..Listed::default() }
}

#[test]
fn tree_badges_take_subfolders_and_the_deepest_worktree() {
    let mut cards = vec![
        card(1, "a", "x", CardStatus::Done, 0),
        card(2, "b", "x", CardStatus::Working, 0),
        card(3, "c", "x", CardStatus::Done, 0),
        card(4, "d", "x", CardStatus::Done, 0),
    ];
    cards[0].cwd = "/repo/app".into();
    cards[1].cwd = "/repo/.wt/feat/app".into();
    cards[2].cwd = "/repo-other/app".into();
    cards[3].cwd = "/repo/.wt/feature".into();
    let trees = [tree("/repo"), tree("/repo/.wt/feat")];
    let some = |s: &str| Some(s.to_string());
    assert_eq!(tree_badges(&cards, &trees, Path::new("/repo")), [some("2 agents"), None, None]);
    assert_eq!(
        tree_badges(&cards, &trees, Path::new("/repo/.wt/feat")),
        [some("1 agent"), some("1 running"), None]
    );
}

#[test]
fn tree_label_reads_branch_path_and_counts() {
    let badges = [Some("1 agent".to_string()), Some("1 running".to_string()), None];
    assert_eq!(tree_label("feat/x", "~/w/x", false, &badges), "feat/x, ~/w/x, 1 agent, 1 running");
    assert_eq!(tree_label("old", "~/w/old", true, &[None, None, None]), "old, ~/w/old, missing");
}

#[test]
fn activity_sentences() {
    let chat = Activity::Chat { ix: 0, title: "Release notes".into(), verb: "finished", at_ms: 0 };
    assert_eq!(activity_text(&chat), "Release notes finished");
    let commit = Activity::Commit { short: "1a1b87a".into(), subject: "fix: x".into(), at_ms: 0 };
    assert_eq!(activity_text(&commit), "Committed 1a1b87a fix: x");
    assert_eq!(activity_label(&chat, "2m"), "Release notes finished, 2m");
}

#[test]
fn new_here_label_names_the_branch() {
    assert_eq!(new_here_aria("feat/x"), "New agent in feat/x");
}

#[test]
fn commit_activity_skips_incoming_and_uses_ms() {
    let c = |short: &str, time: i64, flow: Option<Flow>| Commit {
        sha: short.repeat(7),
        short: short.into(),
        subject: format!("s{short}"),
        author: "me".into(),
        time,
        flow,
    };
    let commits =
        [c("a", 30, Some(Flow::Incoming)), c("b", 20, Some(Flow::Outgoing)), c("c", 10, None)];
    assert_eq!(
        commit_activity(&commits),
        [
            Activity::Commit { short: "b".into(), subject: "sb".into(), at_ms: 20_000 },
            Activity::Commit { short: "c".into(), subject: "sc".into(), at_ms: 10_000 },
        ]
    );
}

#[test]
fn view_option_labels() {
    assert_eq!(Filter::ALL.map(Filter::label), ["All", "Running", "Needs you", "Done"]);
    assert_eq!(GroupBy::ALL.map(GroupBy::label), ["None", "Worktree", "Status", "Provider"]);
    assert_eq!(SortBy::ALL.map(SortBy::label), ["Recent activity", "Newest", "Name", "Status"]);
    assert_eq!(filter_aria(Filter::NeedsYou, 2), "Needs you, 2 agents");
    assert_eq!(filter_aria(Filter::All, 1), "All, 1 agent");
}
