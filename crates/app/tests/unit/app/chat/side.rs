use super::*;
use aether_core::Role;

#[test]
fn btw_question_takes_the_text_after_the_command() {
    assert_eq!(btw_question("/btw x"), Some("x"));
    assert_eq!(btw_question("/side x y"), Some("x y"));
    assert_eq!(btw_question("  /btw  x  "), Some("x"));
    assert_eq!(btw_question("/btw"), None);
    assert_eq!(btw_question("/btw   "), None);
    assert_eq!(btw_question("/btwx"), None);
    assert_eq!(btw_question("/BTW x"), None);
    assert_eq!(btw_question("hi /btw x"), None);
}

#[test]
fn the_side_prompt_wraps_the_question() {
    let prompt = side_prompt("which file?");
    assert!(prompt.starts_with("Side question from the user."));
    assert!(prompt.ends_with("Question: which file?"));
}

#[test]
fn side_history_keeps_the_last_forty() {
    let all: Vec<(Role, String)> = (0..50).map(|i| (Role::User, i.to_string())).collect();
    let capped = side_history(all);
    assert_eq!(capped.len(), 40);
    assert_eq!(capped[0].1, "10");
    assert_eq!(side_history(vec![(Role::User, "a".into())]).len(), 1);
}

#[test]
fn only_three_cards_are_kept() {
    let mut cards = Vec::new();
    for n in 0..5 {
        push_card(&mut cards, SideCard::new(n, format!("q{n}")));
    }
    assert_eq!(cards.iter().map(|c| c.n).collect::<Vec<_>>(), [2, 3, 4]);
}

#[test]
fn empty_cards_say_thinking_or_no_answer() {
    let mut card = SideCard::new(1, "q".into());
    card.running = true;
    assert_eq!(empty_note(&card), Some("Thinking…"));
    card.running = false;
    assert_eq!(empty_note(&card), Some("No answer"));
    card.error = Some("boom".into());
    assert_eq!(empty_note(&card), None);
    card.error = None;
    card.answer = "a".into();
    assert_eq!(empty_note(&card), None);
}

#[gpui_kit::test]
fn a_btw_message_adds_a_card_and_leaves_the_chat_alone(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            aether_core::Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        )
    });
    window
        .update(cx, |app, window, cx| {
            let id = app.chats[app.active].id;
            let sent = app.chats[app.active].messages.len();
            app.composer.update(cx, |c, cx| c.set_value("/btw hi", window, cx));
            app.send(window, cx);
            assert_eq!(app.chats[app.active].messages.len(), sent);
            assert!(app.chats[app.active].queue.is_empty());
            assert!(app.composer.read(cx).value().is_empty());
            assert_eq!(app.side[&id].len(), 1);
            assert_eq!(app.side[&id][0].question, "hi");
            let n = app.side[&id][0].n;
            app.close_side(id, n, cx);
            assert!(app.side[&id].is_empty());
        })
        .unwrap();
}
