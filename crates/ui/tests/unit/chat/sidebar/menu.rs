use super::*;
use aether_core::Provider;

#[test]
fn removed_row_disables_reveal() {
    let line = |removed: bool| ChatLine {
        provider: Provider::Claude,
        project: None,
        model: SharedString::from(""),
        place: None,
        time: SharedString::from(""),
        path: SharedString::from("/repo"),
        removed,
        changed: 2,
        added: 0,
        deleted: 0,
        ahead_behind: None,
        background: 0,
    };
    assert!(reveal_disabled(&line(true)));
    assert!(!reveal_disabled(&line(false)));
}

#[test]
fn project_menu_lists_page_first() {
    let labels = project_menu_labels(true, true, true);
    assert_eq!(labels[0], "Open project page");
    assert_eq!(labels[1], "New agent chat here");
    assert_eq!(project_menu_labels(false, false, false)[0], "New agent chat here");
}
