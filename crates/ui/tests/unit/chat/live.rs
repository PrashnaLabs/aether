use super::*;

#[test]
fn clock_matches_the_prototype() {
    assert_eq!(clock(0), "0s");
    assert_eq!(clock(59_999), "59s");
    assert_eq!(clock(72_000), "1m 12s");
    assert_eq!(clock(123_000), "2m 03s");
    assert_eq!(clock(3_780_000), "1h 3m");
    assert_eq!(short_ms(400), "400ms");
    assert_eq!(short_ms(3_000), "3s");
    assert_eq!(clock(60_000), "1m 00s");
    assert_eq!(clock(3_599_000), "59m 59s");
    assert_eq!(clock(3_600_000), "1h 0m");
    assert_eq!(short_ms(999), "999ms");
    assert_eq!(short_ms(1000), "1s");
}

#[test]
fn since_is_none_without_a_start_and_never_negative() {
    assert_eq!(since(None, 5_000), None);
    assert_eq!(since(Some(9_000), 5_000).as_deref(), Some("0s"));
    assert_eq!(since(Some(1_000), 13_000).as_deref(), Some("12s"));
}

use aether_core::{
    Message, Notice, NoticeKind, NoticeState, ToolCall, ToolDetail, ToolStatus, Update,
};

#[test]
fn heartbeat_thresholds_match_the_prototype() {
    assert_eq!(heartbeat(3_999), Heartbeat::Live);
    assert_eq!(heartbeat(4_000), Heartbeat::Quiet(4_000));
    assert_eq!(heartbeat(30_000), Heartbeat::Silent(30_000));
    assert_eq!(Heartbeat::Live.text(), "live");
    assert_eq!(Heartbeat::Quiet(12_000).text(), "quiet 12s");
    assert_eq!(Heartbeat::Silent(65_000).text(), "no output 1m 05s");
}

#[test]
fn running_notice_holds_only_while_its_kind_runs() {
    let retry = |state| {
        Update::Notice(Notice {
            id: "retry-1".into(),
            kind: NoticeKind::RateLimit,
            text: String::new(),
            state,
            until_ms: None,
            url: None,
            detail: None,
        })
    };
    let retrying = [NoticeKind::Retry, NoticeKind::RateLimit];
    let mut m = Message::assistant();
    m.apply(retry(NoticeState::Running));
    assert!(running_notice(&m, &retrying));
    assert!(!running_notice(&m, &[NoticeKind::Compaction]));
    m.apply(retry(NoticeState::Done));
    assert!(!running_notice(&m, &retrying));
}

#[test]
fn stall_needs_sixty_quiet_seconds_and_nothing_pending() {
    assert!(!stalled(59_999, false));
    assert!(stalled(60_000, false));
    assert!(!stalled(600_000, true));
}

#[test]
fn phase_follows_what_the_reply_is_doing() {
    let running = |id: &str, detail| Update::Tool(ToolCall::new(id, "T", detail));
    let task = || ToolDetail::Task {
        description: "d".into(),
        output: String::new(),
        agent: "explore".into(),
        prompt: String::new(),
        tools: 0,
        tokens: 0,
        activity: String::new(),
    };
    let mut m = Message::assistant();
    assert_eq!(phase(&m, None), Phase::Starting);
    m.apply(Update::Thinking("hmm".into()));
    assert_eq!(phase(&m, None), Phase::Thinking);
    m.apply(Update::Delta("text".into()));
    assert_eq!(phase(&m, None), Phase::Writing);
    m.apply(running("c1", ToolDetail::Plain(String::new())));
    m.apply(running("c2", ToolDetail::Plain(String::new())));
    assert_eq!(phase(&m, None), Phase::Tools(2));
    m.apply(running("t1", task()));
    assert_eq!(phase(&m, None), Phase::Subagents(1));
    m.apply(Update::Notice(Notice {
        id: "retry-1".into(),
        kind: NoticeKind::Retry,
        text: "rate limited (429)".into(),
        state: NoticeState::Running,
        until_ms: Some(9),
        url: None,
        detail: None,
    }));
    assert_eq!(phase(&m, None), Phase::Retrying);
    assert_eq!(phase(&m, Some(Wait::Approval)), Phase::Waiting(Wait::Approval));
}

#[test]
fn a_rate_limit_warns_without_reading_as_a_retry() {
    let mut m = Message::assistant();
    m.apply(Update::Notice(Notice {
        id: "limit".into(),
        kind: NoticeKind::RateLimit,
        text: "5-hour limit at 90%".into(),
        state: NoticeState::Running,
        until_ms: Some(9),
        url: None,
        detail: None,
    }));
    m.apply(Update::Delta("text".into()));
    assert_eq!(phase(&m, None), Phase::Writing);
}

#[test]
fn labels_details_and_last_events_read_like_the_prototype() {
    assert_eq!(Phase::Starting.label("OpenCode"), "Starting OpenCode");
    assert_eq!(Phase::Tools(1).label(""), "Running 1 tool");
    assert_eq!(Phase::Subagents(2).label(""), "Running 2 subagents");
    assert_eq!(Phase::Waiting(Wait::Answer).label(""), "Waiting for your answer");
    assert_eq!(
        wait_detail(Some("Designer"), 2).as_deref(),
        Some("· Designer needs approval · 2 subagents still running")
    );
    assert_eq!(wait_detail(None, 0), None);
    let mut m = Message::assistant();
    assert_eq!(last_event(&m), "Session started");
    m.apply(Update::Tool(ToolCall::new(
        "c",
        "Run command",
        ToolDetail::Command {
            command: "pnpm install".into(),
            cwd: None,
            output: String::new(),
            exit_code: None,
        },
    )));
    assert_eq!(last_event(&m), "Run command `pnpm install` started");
    m.apply(Update::Tool(ToolCall {
        status: ToolStatus::Done,
        ..ToolCall::new(
            "c",
            "Run command",
            ToolDetail::Command {
                command: "pnpm install".into(),
                cwd: None,
                output: String::new(),
                exit_code: Some(0),
            },
        )
    }));
    assert_eq!(last_event(&m), "Run command `pnpm install` finished");
}

#[test]
fn compacting_working_and_search_wording() {
    let mut m = Message::assistant();
    m.apply(Update::Delta("hi".into()));
    m.apply(Update::Notice(Notice {
        id: "c".into(),
        kind: NoticeKind::Compaction,
        text: "Compacting context".into(),
        state: NoticeState::Running,
        until_ms: None,
        url: None,
        detail: None,
    }));
    assert_eq!(phase(&m, None), Phase::Compacting);
    m.apply(Update::Notice(Notice {
        id: "c".into(),
        kind: NoticeKind::Compaction,
        text: "Compacted".into(),
        state: NoticeState::Done,
        until_ms: None,
        url: None,
        detail: None,
    }));
    assert_eq!(phase(&m, None), Phase::Working);
    m.apply(Update::Search { id: "s".into(), query: "--color".into() });
    assert_eq!(last_event(&m), "Searching \"--color\"");
}
