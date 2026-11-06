use super::*;
use aether_core::{ToolCall, ToolDetail, ToolStatus};

#[test]
fn facts_line_joins_what_is_known() {
    assert_eq!(
        card_facts("general-purpose", Some("4m 31s".into()), 40, 135_600),
        "general-purpose · 4m 31s · 40 tools · 135.6k tok"
    );
    assert_eq!(card_facts("Explore", None, 0, 0), "Explore");
    assert_eq!(card_facts("", Some("2s".into()), 1, 0), "2s · 1 tool");
}

#[test]
fn nested_rows_show_the_last_five_and_count_the_rest() {
    assert_eq!(earlier(14, 3, false), (11, 0..3));
    assert_eq!(earlier(0, 12, false), (7, 7..12));
    assert_eq!(earlier(0, 12, true), (0, 0..12));
    assert_eq!(earlier(20, 12, true), (8, 0..12));
    assert_eq!(earlier_label(1), "+ 1 earlier tool call");
    assert_eq!(earlier_label(11), "+ 11 earlier tool calls");
}

#[test]
fn card_status_reads_the_row_and_its_children() {
    let row = |id: &str, status| ToolCall {
        status,
        started_ms: Some(1),
        ..ToolCall::new(id, "Run", ToolDetail::Plain(String::new()))
    };
    let waiting = row("c", ToolStatus::Waiting);
    assert_eq!(
        card_status(&row("t", ToolStatus::Running), std::slice::from_ref(&waiting)),
        CardStatus::Waiting
    );
    assert_eq!(card_status(&row("t", ToolStatus::Running), &[]), CardStatus::Running);
    assert_eq!(
        card_status(&ToolCall::new("t", "Run", ToolDetail::Plain(String::new())), &[]),
        CardStatus::Starting
    );
    assert_eq!(card_status(&row("t", ToolStatus::Background), &[]), CardStatus::Background);
    assert_eq!(
        card_status(&row("t", ToolStatus::Done), std::slice::from_ref(&waiting)),
        CardStatus::Done
    );
    assert_eq!(card_status(&row("t", ToolStatus::Stopped), &[]), CardStatus::Stopped);
    assert_eq!(waiting_line(&[waiting]).as_deref(), Some("Waiting for your approval below · Run"));
}

#[test]
fn timer_follows_status_not_the_turn() {
    let task = |status, started_ms, ms| ToolCall {
        status,
        started_ms,
        ms,
        ..ToolCall::new("t", "Task", ToolDetail::Plain(String::new()))
    };
    let bg = task(ToolStatus::Background, Some(1_000), None);
    assert_eq!(card_timer(&bg, CardStatus::Background, Some(8_000)).as_deref(), Some("7s"));
    assert_eq!(card_timer(&bg, CardStatus::Background, Some(500)).as_deref(), Some("0s"));
    let unknown = task(ToolStatus::Running, None, None);
    assert_eq!(card_timer(&unknown, CardStatus::Starting, Some(8_000)), None);
    let done = task(ToolStatus::Done, Some(1_000), Some(65_000));
    assert_eq!(card_timer(&done, CardStatus::Done, Some(99_000)).as_deref(), Some("1m 05s"));
    let quick = task(ToolStatus::Done, Some(1_000), Some(400));
    assert_eq!(card_timer(&quick, CardStatus::Done, Some(99_000)), None, "like a tool row");
    let failed = task(ToolStatus::Failed, Some(1_000), Some(400));
    assert_eq!(card_timer(&failed, CardStatus::Failed, Some(99_000)).as_deref(), Some("400ms"));
}

#[test]
fn waiting_child_wins_over_running_and_background() {
    let row = |status| ToolCall {
        status,
        started_ms: Some(1),
        ..ToolCall::new("t", "Run", ToolDetail::Plain(String::new()))
    };
    let waiting = [row(ToolStatus::Waiting)];
    assert_eq!(card_status(&row(ToolStatus::Background), &waiting), CardStatus::Waiting);
    assert_eq!(card_status(&row(ToolStatus::Running), &waiting), CardStatus::Waiting);
}

#[test]
fn streaming_window_keeps_the_start_of_a_growing_group() {
    let calls: Vec<_> = (0..12)
        .map(|i| ToolCall {
            status: ToolStatus::Done,
            ..ToolCall::new(
                i.to_string(),
                "Run",
                ToolDetail::Command {
                    command: "test".into(),
                    cwd: None,
                    output: String::new(),
                    exit_code: Some(0),
                },
            )
        })
        .collect();
    assert_eq!(activity_window(0, &calls[..7], false), (0, 0..7));
    assert_eq!(activity_window(0, &calls, false), (0, 0..12));
    let mut interrupted = calls;
    interrupted[5].status = ToolStatus::Failed;
    assert_eq!(activity_window(0, &interrupted, false), (6, 6..12));
}
