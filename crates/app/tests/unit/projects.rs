use super::*;
use chrono::Utc;

fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap().timestamp_millis()
}

#[test]
fn groups_by_folder_newest_first() {
    let p = PathBuf::from;
    let groups = group_projects(vec![
        (0, p("/a"), 100),
        (1, p("/b"), 300),
        (2, p("/a"), 200),
        (3, p("/a/"), 50),
        (4, p("/c"), 200),
    ]);
    assert_eq!(
        groups,
        [
            Project { folder: p("/b"), chats: vec![1] },
            Project { folder: p("/c"), chats: vec![4] },
            Project { folder: p("/a"), chats: vec![2, 0, 3] },
        ]
    );
    assert!(group_projects(Vec::new()).is_empty());
}

#[test]
fn shared_names_get_their_parent() {
    let folders: Vec<PathBuf> = ["/c/client/app", "/c/server/app", "/c/web", "/app"]
        .into_iter()
        .map(PathBuf::from)
        .collect();
    let mut names: Vec<String> = ["app", "app", "web", "app"].map(String::from).to_vec();
    disambiguate(&mut names, &folders);
    assert_eq!(names, ["client/app", "server/app", "web", "app"]);
}

#[test]
fn rollup_prefers_needs_you_then_working_failed_done() {
    use ChatStatus::*;
    let working = Working { since_ms: 0 };
    assert_eq!(rollup([Idle, Done, working]), working);
    assert_eq!(rollup([Done, Failed, Idle]), Failed);
    assert_eq!(rollup([working, Waiting, Failed]), Waiting);
    assert_eq!(rollup([Quiet { since_ms: 0 }, Waiting]), Waiting);
    assert_eq!(rollup([Done, Background(1)]), Background(1));
    assert_eq!(rollup([Idle, Done]), Done);
    assert_eq!(rollup([]), Idle);
}

#[test]
fn relative_time_steps_from_now_to_dates() {
    let now = at(2026, 10, 15, 12, 0);
    let t = |ms| relative_time(ms, now, &Utc);
    assert_eq!(t(now - 30_000), "now");
    assert_eq!(t(now + 90_000), "now");
    assert_eq!(t(now - 5 * 60_000), "5m");
    assert_eq!(t(now - 3 * 3_600_000 - 59_000), "3h");
    assert_eq!(t(at(2026, 10, 14, 8, 0)), "Yesterday");
    assert_eq!(t(at(2026, 10, 12, 23, 0)), "Oct 12");
    assert_eq!(t(at(2025, 10, 2, 9, 0)), "Oct 2, 2025");
}

#[test]
fn busy_label_shows_minutes_once_past_one() {
    assert_eq!(busy_label("Working", 0, 59_000), "Working");
    assert_eq!(busy_label("Working", 0, 125_000), "Working · 2m");
    assert_eq!(busy_label("Waiting", 0, 2 * 3_600_000), "Waiting · 2h");
}

#[test]
fn listed_paths_toggle_once_and_ignore_trailing_slashes() {
    let mut list = vec!["/a/".to_string()];
    assert!(listed(&list, Path::new("/a")));
    set_listed(&mut list, Path::new("/b"), true);
    set_listed(&mut list, Path::new("/b"), true);
    assert_eq!(list, ["/a/", "/b"]);
    set_listed(&mut list, Path::new("/a"), false);
    assert_eq!(list, ["/b"]);
    assert!(!listed(&list, Path::new("/a")));
}

#[test]
fn chat_section_categorization_by_time() {
    let now = at(2026, 10, 15, 12, 0);
    let cat = |ms| categorize_time(ms, now, &Utc);

    assert_eq!(cat(now - 30_000), ChatSection::Today);
    assert_eq!(cat(at(2026, 10, 15, 1, 0)), ChatSection::Today);
    assert_eq!(cat(at(2026, 10, 14, 23, 0)), ChatSection::Yesterday);
    assert_eq!(cat(at(2026, 10, 13, 10, 0)), ChatSection::Previous7Days);
    assert_eq!(cat(at(2026, 10, 8, 12, 0)), ChatSection::Previous7Days);
    assert_eq!(cat(at(2026, 10, 7, 12, 0)), ChatSection::Previous30Days);
    assert_eq!(cat(at(2026, 9, 20, 12, 0)), ChatSection::Previous30Days);
    assert_eq!(cat(at(2026, 9, 1, 12, 0)), ChatSection::Older);
    assert_eq!(cat(at(2025, 10, 1, 12, 0)), ChatSection::Older);
}

#[test]
fn saved_projects_without_chats_sort_last_alphabetically() {
    let active = vec![(PathBuf::from("/w/old"), 10), (PathBuf::from("/w/new"), 20)];
    let saved = vec!["/w/zeta".to_string(), "/w/new".to_string(), "/w/alpha".to_string()];
    let list = project_list(active, &saved, &[]);
    let folders: Vec<_> = list.iter().map(|p| p.folder.to_str().unwrap()).collect();
    assert_eq!(folders, ["/w/new", "/w/old", "/w/alpha", "/w/zeta"]);
    assert_eq!(list[2].last_activity, None);
}

#[test]
fn hidden_projects_stay_out_and_adding_one_unhides_it() {
    let mut s = Settings::default();
    s.hidden_projects = vec!["/w/a".into()];
    assert!(project_list(vec![(PathBuf::from("/w/a"), 5)], &[], &s.hidden_projects).is_empty());
    add_project(&mut s, Path::new("/w/a"));
    assert_eq!(s.projects, vec!["/w/a".to_string()]);
    assert!(s.hidden_projects.is_empty());
}

#[test]
fn short_path_uses_a_tilde_inside_home() {
    assert_eq!(
        short_path(Path::new("/Users/i/code/aether"), Path::new("/Users/i")),
        "~/code/aether"
    );
    assert_eq!(short_path(Path::new("/opt/x"), Path::new("/Users/i")), "/opt/x");
}

#[test]
fn saved_folders_join_the_groups_as_empty_projects() {
    let groups = vec![Project { folder: "/w/a".into(), chats: vec![0] }];
    let merged = with_saved(groups, &["/w/b".to_string(), "/w/a".to_string()]);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[1].folder, PathBuf::from("/w/b"));
    assert!(merged[1].chats.is_empty());
}

#[test]
fn saved_trailing_slash_duplicate_of_an_active_folder_is_one_entry() {
    let list = project_list(vec![(PathBuf::from("/w/a"), 1)], &["/w/a/".to_string()], &[]);
    assert_eq!(list.len(), 1);
    let merged = with_saved(vec![], &["/w/b".to_string(), "/w/b/".to_string()]);
    assert_eq!(merged.len(), 1);
}

#[test]
fn switcher_lists_projects_then_add_project() {
    let list = vec![
        ProjectEntry { folder: "/Users/i/aether".into(), last_activity: Some(2) },
        ProjectEntry { folder: "/Users/i/dotfiles".into(), last_activity: None },
    ];
    let names = HashMap::from([(PathBuf::from("/Users/i/aether"), "aether".to_string())]);
    let opts = project_switcher_options(
        &list,
        &names,
        Path::new("/Users/i/aether"),
        Path::new("/Users/i"),
    );
    let titles: Vec<_> = opts.iter().map(|o| o.title.to_string()).collect();
    assert_eq!(titles, ["aether", "dotfiles", "Add project…"]);
    assert!(opts[0].checked && !opts[1].checked);
    assert_eq!(opts[1].description.as_ref().map(|d| d.to_string()), Some("~/dotfiles".to_string()));
    assert_eq!(opts[0].mark.as_ref().map(|m| m.to_string()), Some("aether".to_string()));
}

#[test]
fn minimal_sections_group_by_time_or_project_and_order_rows() {
    use aether_core::{SidebarGrouping, SidebarOrdering};
    let day = 86_400_000;
    let now = 100 * day + day / 2;
    let chats = [
        Listed { ix: 0, activity: now - 40 * day, title: "Old", project: "beta" },
        Listed { ix: 1, activity: now - 1_000, title: "b fresh", project: "alpha" },
        Listed { ix: 2, activity: now - 2_000, title: "A fresh", project: "beta" },
    ];
    let utc = chrono::Utc;
    let updated =
        minimal_sections(&chats, SidebarGrouping::Updated, SidebarOrdering::Newest, now, &utc);
    assert_eq!(updated, [("Today".to_string(), vec![1, 2]), ("Older".to_string(), vec![0])]);
    let oldest =
        minimal_sections(&chats, SidebarGrouping::Updated, SidebarOrdering::Oldest, now, &utc);
    assert_eq!(oldest, [("Older".to_string(), vec![0]), ("Today".to_string(), vec![2, 1])]);
    let named =
        minimal_sections(&chats, SidebarGrouping::Project, SidebarOrdering::Name, now, &utc);
    assert_eq!(named, [("alpha".to_string(), vec![1]), ("beta".to_string(), vec![2, 0])]);
}

#[test]
fn focus_sections_put_what_needs_you_first() {
    use aether_ui::ChatStatus;
    let chats = [
        (0, ChatStatus::Idle, 5),
        (1, ChatStatus::Working { since_ms: 0 }, 4),
        (2, ChatStatus::Waiting, 3),
        (3, ChatStatus::Done, 9),
        (4, ChatStatus::Idle, 7),
        (5, ChatStatus::Quiet { since_ms: 0 }, 2),
    ];
    assert_eq!(
        focus_sections(&chats),
        [("Needs you", vec![3, 2]), ("Working", vec![1, 5]), ("Recent", vec![4, 0])]
    );
}
