use super::*;
use aether_core::{Ask, Message, Provider};
use aether_ui::{ChatStatus, Heartbeat, Wait, heartbeat, stalled};

#[test]
fn only_opencode_offers_its_tui_for_the_live_session() {
    assert_eq!(
        tui_command(Provider::OpenCode, "ses_1").as_deref(),
        Some("opencode --session ses_1")
    );
    assert_eq!(tui_command(Provider::OpenCode, ""), None);
    assert_eq!(tui_command(Provider::Claude, "abc"), None);
}

#[test]
fn answering_a_long_pending_ask_never_reads_as_a_stall() {
    let (mut last, turn_at) = (1_000, 1_000);
    let ten_min = 601_000;
    hold_clock(&mut last, true, ten_min);
    let at = anchor(last, turn_at);
    let idle = idle_since(at, ten_min + 500);
    assert!(!stalled(idle, false));
    assert_eq!(heartbeat(idle), Heartbeat::Live);
}

#[test]
fn a_turn_with_no_events_is_timed_from_its_start_and_can_be_kept() {
    let at = anchor(0, 5_000);
    assert_eq!(at, 5_000);
    let idle = idle_since(at, 5_000 + 61_000);
    assert!(stalled(idle, false));
    assert_eq!(heartbeat(idle), Heartbeat::Silent(61_000));
    assert!(card_shown(true, 0, at));
    assert!(!card_shown(true, at, at));
    assert!(card_shown(true, at, at + 1));
}

#[test]
fn asks_map_to_what_the_user_owes() {
    use aether_core::Approval;
    let approval =
        Ask::Approval(Approval { source: Some("Designer".into()), ..Default::default() });
    assert_eq!(wait_of(&approval), (Wait::Approval, Some("Designer")));
    assert_eq!(
        wait_of(&Ask::Question { question: "q".into(), choices: vec![] }),
        (Wait::Answer, None)
    );
    assert_eq!(wait_of(&Ask::Questions(vec![])), (Wait::Answer, None));
    assert_eq!(wait_of(&Ask::Unknown), (Wait::Input, None));
}

#[test]
fn only_live_work_ticks() {
    use aether_core::{ToolCall, ToolDetail, ToolStatus, Update};
    let background = |id: &str, parent: Option<&str>| {
        Update::Tool(ToolCall {
            status: ToolStatus::Background,
            parent: parent.map(Into::into),
            ..ToolCall::new(id, "Run", ToolDetail::Plain(String::new()))
        })
    };
    let mut chat = crate::app::chat_with("t", &["a", "b"]);
    assert!(!has_live_work(false, &chat));
    assert!(has_live_work(true, &chat));
    chat.messages[1].apply(background("nested", Some("task")));
    assert!(!has_live_work(false, &chat));
    chat.messages[1].apply(background("b", None));
    chat.messages.push(Message::assistant());
    assert!(has_live_work(false, &chat));
    chat.messages[1].restored = true;
    assert!(!has_live_work(false, &chat));
}

#[test]
fn background_rows_in_any_reply_agree_across_sidebar_tray_and_ticker() {
    let mut chat = crate::app::chat_with("t", &["a", "b", "c", "d"]);
    let mut call = aether_core::ToolCall::new("bg", "dev", aether_core::ToolDetail::default());
    call.status = aether_core::ToolStatus::Background;
    chat.messages[1].apply(aether_core::Update::Tool(call));
    assert_eq!(chat.status(0), ChatStatus::Background(1), "sidebar");
    assert_eq!(chat.live_background().count(), 1, "tray");
    assert!(has_live_work(false, &chat), "ticker");
    chat.messages[1].restored = true;
    assert_eq!(chat.status(0), ChatStatus::Idle);
    assert_eq!(chat.live_background().count(), 0);
    assert!(!has_live_work(false, &chat));
}

#[test]
fn a_reattached_restored_reply_keeps_its_background_rows_live() {
    let mut chat = crate::app::chat_with("t", &["a", "b"]);
    chat.provider = Provider::OpenCode;
    chat.cli_session = "ses_1".into();
    let mut call = aether_core::ToolCall::new("bg", "dev", aether_core::ToolDetail::default());
    call.status = aether_core::ToolStatus::Background;
    let last = chat.messages.last_mut().unwrap();
    last.apply(aether_core::Update::Tool(call));
    last.finish();
    last.restored = true;
    assert_eq!(chat.status(0), ChatStatus::Background(1));
    assert!(has_live_work(false, &chat));
}

#[gpui_kit::test]
fn a_quiet_turn_keeps_its_bar_and_tucks_the_details_into_a_popover(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        let mut app = ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            aether_core::Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        );
        let mut chat = crate::app::chat_with("a", &["q", ""]);
        chat.id = 2;
        chat.cancel = Some(aether_agent::Cancel::inert());
        let now = now_ms();
        (chat.turn_at, chat.last_event_ms) = (now - 200_000, now - 90_000);
        app.chats.push(chat);
        app.select(1, window, cx);
        app
    });
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    let bounds = |visual: &mut gpui_kit::VisualTestContext, selector| {
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        visual.debug_bounds(selector)
    };
    let (warning, card) = ("s-1-live-stall", "s-1-live-stall-card");
    let trigger = bounds(&mut visual, warning).expect("the warning stands in for the spinner");
    assert!(bounds(&mut visual, card).is_none(), "details wait for a click");

    visual.simulate_click(trigger.center(), gpui_kit::Modifiers::default());
    assert!(bounds(&mut visual, card).is_some());

    let keep = bounds(&mut visual, "s-1-live-keep").expect("keep waiting");
    visual.simulate_click(keep.center(), gpui_kit::Modifiers::default());
    assert!(bounds(&mut visual, warning).is_none(), "back to the spinner once acknowledged");
    assert!(bounds(&mut visual, card).is_none());

    visual.update(|_, cx| app.update(cx, |app, _| app.chats[1].last_event_ms = now_ms() - 80_000));
    assert!(bounds(&mut visual, warning).is_some(), "a new quiet stretch warns again");
    assert!(bounds(&mut visual, card).is_none(), "without reopening by itself");
}
