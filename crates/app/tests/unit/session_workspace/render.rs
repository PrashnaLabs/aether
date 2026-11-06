use super::*;
use aether_core::ChatLayout;

#[test]
fn next_chat_layout_cycles_all_three() {
    assert_eq!(next_chat_layout(ChatLayout::Tabbed), ChatLayout::Sidebar);
    assert_eq!(next_chat_layout(ChatLayout::Sidebar), ChatLayout::Split);
    assert_eq!(next_chat_layout(ChatLayout::Split), ChatLayout::Tabbed);
}

#[test]
fn only_split_disables_monocle() {
    assert!(chat_layout_allows_monocle(ChatLayout::Tabbed));
    assert!(chat_layout_allows_monocle(ChatLayout::Sidebar));
    assert!(!chat_layout_allows_monocle(ChatLayout::Split));
}
