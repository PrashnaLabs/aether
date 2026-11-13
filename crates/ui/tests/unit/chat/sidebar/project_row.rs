use super::*;

#[test]
fn project_aria_formats_collapsed_status_rollup() {
    let idle = project_aria("aether", 3, false, false, true, ChatStatus::Idle);
    assert_eq!(idle, "aether, 3 chats");

    let working =
        project_aria("aether", 2, false, false, true, ChatStatus::Working { since_ms: 0 });
    assert_eq!(working, "aether, 2 chats, working");

    let waiting = project_aria("aether", 1, false, false, true, ChatStatus::Waiting);
    assert_eq!(waiting, "aether, 1 chat, needs you");

    let expanded =
        project_aria("aether", 2, false, false, false, ChatStatus::Working { since_ms: 0 });
    assert_eq!(expanded, "aether, 2 chats");

    let missing = project_aria("broken", 0, false, true, true, ChatStatus::Idle);
    assert_eq!(missing, "broken, 0 chats, folder not found");
}

#[test]
fn project_chevron_and_folder_icons() {
    assert!(matches!(project_chevron_icon(true), IconName::ChevronRight));
    assert!(matches!(project_chevron_icon(false), IconName::ChevronDown));
    assert!(matches!(project_folder_icon(true), IconName::FolderClosed));
    assert!(matches!(project_folder_icon(false), IconName::FolderOpen));
}

#[test]
fn tilde_path_shortens_only_the_home_prefix() {
    let home = aether_core::home_dir();
    let inside = home.join("Code/datafuse");
    assert_eq!(tilde_path(&inside.display().to_string()), "~/Code/datafuse");
    assert_eq!(tilde_path(&home.display().to_string()), "~");
    assert_eq!(tilde_path("/opt/project"), "/opt/project");
}
