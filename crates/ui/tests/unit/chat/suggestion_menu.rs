use super::*;

#[test]
fn suggestion_menu_builder_sets_selected_and_max_height() {
    let menu = SuggestionMenu::new("Commands")
        .item("/clear", "Clear chat history")
        .item("/compact", "Compact history")
        .selected(1)
        .max_height(px(280.));

    assert_eq!(menu.title, "Commands");
    assert_eq!(menu.items.len(), 2);
    assert_eq!(menu.selected, 1);
    assert_eq!(menu.max_height, Some(px(280.)));
}
