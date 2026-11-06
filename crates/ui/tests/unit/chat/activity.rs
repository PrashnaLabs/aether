use super::*;
use aether_core::{Notice, ToolCall};

fn command(id: &str, status: ToolStatus, ms: Option<u64>) -> Step {
    Step::Tool(ToolCall {
        status,
        ms,
        ..ToolCall::new(
            id,
            "Run",
            ToolDetail::Command {
                command: "cargo test".into(),
                cwd: None,
                output: String::new(),
                exit_code: None,
            },
        )
    })
}

#[test]
fn attention_rows_never_enter_a_group() {
    for status in [ToolStatus::Failed, ToolStatus::Stopped, ToolStatus::Denied, ToolStatus::Waiting]
    {
        assert_eq!(activity_kind(&command("c", status, None)), None);
    }
    let mut hook = Notice::new("h", NoticeKind::Hook, "check");
    hook.state = NoticeState::Failed;
    assert_eq!(activity_kind(&Step::Notice(hook)), None);
    assert_eq!(
        activity_kind(&Step::Search {
            id: "s".into(),
            query: "q".into(),
            results: Some(Err("failed".into()))
        }),
        None
    );
    let Step::Tool(mut call) = command("c", ToolStatus::Done, None) else { unreachable!() };
    if let ToolDetail::Command { exit_code, .. } = &mut call.detail {
        *exit_code = Some(1);
    }
    assert_eq!(activity_kind(&Step::Tool(call)), None);
}

#[test]
fn duration_is_complete_tool_time_and_never_a_partial_sum() {
    let a = command("a", ToolStatus::Done, Some(300));
    let b = command("b", ToolStatus::Done, Some(700));
    let facts = ActivityFacts::new(ActivityKind::Commands, &[&a, &b], false);
    assert_eq!(facts.duration_ms, Some(1000));
    assert_eq!(facts.preview, "cargo test");
    assert_eq!(facts.count, 2);
    assert!(!facts.active);
    let unknown = command("c", ToolStatus::Done, None);
    assert_eq!(
        ActivityFacts::new(ActivityKind::Commands, &[&a, &unknown], false).duration_ms,
        None
    );
    let running = command("d", ToolStatus::Running, Some(50));
    let live = ActivityFacts::new(ActivityKind::Commands, &[&a, &running], true);
    assert!(live.active);
    assert_eq!(live.duration_ms, None);
}

#[test]
fn unfinished_thinking_is_only_active_in_a_live_reply() {
    let thought = Step::Thinking { text: "Inspect layout".into(), ms: None };
    assert!(ActivityFacts::new(ActivityKind::Thinking, &[&thought], true).active);
    assert!(!ActivityFacts::new(ActivityKind::Thinking, &[&thought], false).active);
    let skill = Step::Skill("impeccable".into());
    assert_eq!(ActivityFacts::new(ActivityKind::Skills, &[&skill], false).duration_ms, None);
}

#[gpui_kit::test]
fn a_group_keeps_its_state_while_its_tools_start_and_finish(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        let key = SharedString::from("edits");
        assert!(!activity_group_open(cx, &key));
        set_row_open(cx, key.clone(), true);
        assert!(activity_group_open(cx, &key));
    });
}
