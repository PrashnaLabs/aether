use gpui_kit::{KeyboardClickEvent, MouseClickEvent};

use super::*;

#[test]
fn space_applies_a_stash_while_enter_and_the_mouse_open_it() {
    let key = |button| ClickEvent::Keyboard(KeyboardClickEvent { button, ..Default::default() });
    assert!(applies(&key(KeyboardButton::Space)));
    assert!(!applies(&key(KeyboardButton::Enter)));
    assert!(!applies(&ClickEvent::Mouse(MouseClickEvent::default())));
}

#[test]
fn dropping_all_stashes_counts_them() {
    assert_eq!(clear_prompt(1), "Drop 1 stash?");
    assert_eq!(clear_prompt(3), "Drop all 3 stashes?");
    assert_eq!(clear_prompt(1200), "Drop all 1,200 stashes?");
}
