use super::*;

#[test]
fn menu_bar_has_standard_menus_and_native_edit_items() {
    let menus = menus();
    let names: Vec<&str> = menus.iter().map(|m| m.name.as_ref()).collect();
    assert_eq!(names, ["Aether", "File", "Edit", "View", "Window", "Help"]);
    let edit = &menus[2].items;
    let native =
        edit.iter().filter(|i| matches!(i, MenuItem::Action { os_action: Some(_), .. })).count();
    assert_eq!(native, 6);
}
