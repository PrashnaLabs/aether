use super::*;
use aether_core::{Message, PlanItem, PlanStatus, ToolCall, ToolDetail, ToolStatus, Update};

#[test]
fn composer_measurements_survive_paint_and_subsequent_renders() {
    let state = ComposerMeasurements::default();
    let render = state.clone();
    let paint_body = render.body.clone();
    let paint_tray = render.tray.clone();
    let paint_bounds = render.bounds.clone();
    let bounds =
        Bounds::new(gpui_kit::point(px(24.), px(600.)), gpui_kit::size(px(720.), px(120.)));
    paint_body.set(px(120.));
    paint_tray.set(px(32.));
    paint_bounds.set(bounds);

    let next_render = state.clone();
    assert_eq!(next_render.body.get(), px(120.), "collapse must use the painted body height");
    assert_eq!(next_render.tray.get(), px(32.), "tray must shrink from its painted height");
    assert_eq!(next_render.bounds.get(), bounds, "stable bounds must not trigger another refresh");
}

#[test]
fn send_button_default_state() {
    let btn = SendButton::new();
    assert!(!btn.streaming);
    assert!(!btn.loading);
    assert!(!btn.disabled);
}

#[test]
fn mode_folds_before_resting() {
    assert_eq!(composer_mode(true, true, true), ComposerMode::Folded);
    assert_eq!(composer_mode(true, false, true), ComposerMode::Folded);
    assert_eq!(composer_mode(true, true, false), ComposerMode::Resting);
    assert_eq!(composer_mode(true, false, false), ComposerMode::Full);
    assert_eq!(composer_mode(false, true, true), ComposerMode::Full);
}

#[test]
fn placeholder_names_the_agent() {
    assert_eq!(
        composer_placeholder(Some("Claude")),
        "Ask Claude anything, @tag files, / for commands"
    );
    assert_eq!(composer_placeholder(None), "Ask anything, @tag files, / for commands");
}

#[test]
fn fold_bar_shows_first_draft_line_or_reply_hint() {
    assert_eq!(
        fold_bar_text("\n  fix the queue\nsecond", Some("Codex")),
        ("fix the queue".to_string(), true)
    );
    assert_eq!(fold_bar_text("   ", Some("Codex")), ("Reply to Codex…".to_string(), false));
    assert_eq!(fold_bar_text("", None), ("Reply…".to_string(), false));
}

#[test]
fn context_label_rounds_and_hides_when_unused() {
    assert_eq!(context_label(82_000, 200_000), Some("41%".to_string()));
    assert_eq!(context_label(0, 200_000), None);
    assert_eq!(context_label(5, 0), None);
    assert_eq!(context_label(250_000, 200_000), Some("100%".to_string()));
}

#[test]
fn plan_progress_counts_done_and_names_the_active_item() {
    let item = |text: &str, status| PlanItem { text: text.into(), status };
    let items = [
        item("Audit", PlanStatus::Done),
        item("Draft", PlanStatus::Done),
        item("Rewrite Hero", PlanStatus::Active),
        item("Restyle", PlanStatus::Pending),
        item("Drop X", PlanStatus::Cancelled),
    ];
    let p = plan_progress(&items);
    assert_eq!(p, PlanProgress { done: 2, total: 4, current: Some("Rewrite Hero".into()) });
    assert_eq!(p.header(), "2 of 4");
}

#[test]
fn trays_read_the_newest_plan_and_running_background_rows() {
    let plan = |id: &str, text: &str| {
        Update::Tool(ToolCall::new(
            id,
            "Plan",
            ToolDetail::Plan {
                items: vec![PlanItem { text: text.into(), status: PlanStatus::Pending }],
            },
        ))
    };
    let cmd = |id: &str, status| {
        Update::Tool(ToolCall {
            status,
            ..ToolCall::new(
                id,
                "Run",
                ToolDetail::Command {
                    command: id.into(),
                    cwd: None,
                    output: String::new(),
                    exit_code: None,
                },
            )
        })
    };
    let mut first = Message::assistant();
    first.apply(plan("p1", "old"));
    first.apply(cmd("dev", ToolStatus::Background));
    let mut second = Message::assistant();
    second.apply(plan("p2", "new"));
    second.apply(cmd("test", ToolStatus::Done));
    let msgs = [Message::user("a"), first, Message::user("b"), second];
    assert_eq!(latest_plan(&msgs).unwrap()[0].text, "new");
    let rows = msgs.iter().enumerate().flat_map(|(ix, m)| background_rows(m).map(move |c| (ix, c)));
    assert_eq!(rows.map(|(ix, c)| (ix, c.id.as_str())).collect::<Vec<_>>(), [(1, "dev")]);
}

#[test]
fn queue_chips_and_placeholders_say_what_happens_next() {
    assert_eq!(chip_copy(false, true, "Pi"), ("Queued", "Sends when this turn ends".to_string()));
    assert_eq!(
        chip_copy(false, false, "Pi"),
        ("Queued", "Send now to run it".to_string()),
        "no turn will end: stopped, failed, blocked or idle"
    );
    assert_eq!(
        chip_copy(true, true, "Pi"),
        ("Delivering", "Waiting for Pi to pick it up".to_string())
    );
    assert_eq!(
        busy_placeholder(Some("OpenCode"), Busy::Working),
        "Type to steer OpenCode while it works"
    );
    assert_eq!(
        busy_placeholder(Some("OpenCode"), Busy::Waiting),
        "Answer above, or type to tell OpenCode what to do instead"
    );
    assert_eq!(
        busy_placeholder(Some("OpenCode"), Busy::Idle),
        composer_placeholder(Some("OpenCode"))
    );
}

fn plan_reply(statuses: &[PlanStatus]) -> Message {
    let items = statuses.iter().map(|&status| PlanItem { text: "x".into(), status }).collect();
    let mut m = Message::assistant();
    m.apply(Update::Tool(ToolCall::new("p", "Plan", ToolDetail::Plan { items })));
    m
}

#[test]
fn plan_tray_shows_for_unfinished_items_or_the_last_reply() {
    let done = [PlanStatus::Done, PlanStatus::Cancelled];
    let old = [plan_reply(&done), Message::user("next"), Message::assistant()];
    assert!(plan_tray_items(&old).is_none());
    let last = [Message::user("a"), plan_reply(&done)];
    assert!(plan_tray_items(&last).is_some());
    let open = [plan_reply(&[PlanStatus::Done, PlanStatus::Unknown]), Message::assistant()];
    assert!(plan_tray_items(&open).is_some());
    assert!(plan_tray_items(&[Message::user("a")]).is_none());
}

#[test]
fn dead_bar_needs_an_idle_dead_cli_and_an_unrestarted_error() {
    let mut failed = Message::assistant();
    failed.error = Some("exited".into());
    failed.ended_at_ms = Some(1000);
    let shown =
        |idle, alive, m: &Message, restarted| dead_agent_shown(idle, alive, Some(m), restarted);
    assert!(shown(true, false, &failed, 999));
    assert!(!shown(true, false, &failed, 1000));
    assert!(!shown(true, false, &failed, 2000));
    assert!(!shown(false, false, &failed, 0));
    assert!(!shown(true, true, &failed, 0));
    failed.error = Some("claude is not installed; install it".into());
    assert!(!shown(true, false, &failed, 0));
    let mut ok = Message::assistant();
    ok.ended_at_ms = Some(1000);
    assert!(!shown(true, false, &ok, 0));
    assert!(!dead_agent_shown(true, false, None, 0));
}
