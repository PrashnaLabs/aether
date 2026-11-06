use super::*;
use aether_core::{NoticeKind as K, NoticeState as S, Step, ToolCall, ToolDetail, ToolStatus};

#[test]
fn notice_leads_and_tones_by_kind_and_state() {
    assert_eq!(lead(K::Retry, S::Running), Some("Retrying"));
    assert_eq!(lead(K::Retry, S::Done), Some("Recovered"));
    assert_eq!(lead(K::Compaction, S::Done), Some("Compacted"));
    assert_eq!(lead(K::Hook, S::Failed), Some("Hook blocked"));
    assert_eq!(lead(K::Info, S::Done), None);
    assert_eq!(lead(K::Retry, S::Unknown), Some("Retry"));
    assert_eq!(lead(K::Compaction, S::Unknown), Some("Compaction"));
    assert_eq!(notice_tone(K::Hook, S::Failed), NoticeTone::Bad);
    assert_eq!(notice_tone(K::ModelChange, S::Done), NoticeTone::Warn);
    assert_eq!(notice_tone(K::Retry, S::Done), NoticeTone::Ok);
    assert_eq!(notice_tone(K::Hook, S::Done), NoticeTone::Muted);
}

#[test]
fn notice_body_drops_what_the_lead_already_says() {
    let body = |kind, state, text| notice_body(lead(kind, state), text);
    assert_eq!(body(K::Compaction, S::Running, "Compacting context"), "");
    assert_eq!(body(K::Compaction, S::Running, "Compacting context (manual)"), "(manual)");
    assert_eq!(body(K::Compaction, S::Failed, "Compaction failed: too big"), "too big");
    assert_eq!(body(K::Compaction, S::Done, "Compacted 90 → 12 tokens"), "90 → 12 tokens");
    assert_eq!(body(K::Compaction, S::Done, "Compacted context"), "context");
    assert_eq!(body(K::Retry, S::Running, "Retrying (2/10): 529"), "(2/10): 529");
    assert_eq!(body(K::Retry, S::Unknown, "Retrying (2/10)"), "Retrying (2/10)");
    assert_eq!(body(K::Info, S::Done, "Plain text"), "Plain text");
}

#[test]
fn only_a_finished_compaction_counts_as_compacted() {
    let notice = |state, text| Notice { state, ..Notice::new("c", K::Compaction, text) };
    assert!(compacted(&notice(S::Done, "Compacted 90 → 12 tokens")));
    assert!(compacted(&notice(S::Done, "Compacted context")));
    assert!(!compacted(&notice(S::Done, "Compacting context")));
    assert!(!compacted(&notice(S::Done, "Compaction stopped")));
    assert!(!compacted(&notice(S::Failed, "Compacted")));
    assert!(!compacted(&Notice::new("w", K::Warning, "Compacted")));
}

#[test]
fn compaction_counts_read_short() {
    assert_eq!(short_counts("671911 → 14549 tokens"), "671.9k → 14.5k tokens");
    assert_eq!(short_counts("671911 → ~14549 tokens"), "671.9k → ~14.5k tokens");
    assert_eq!(short_counts("context"), "context");
}

#[test]
fn countdown_rounds_up_and_ends_at_the_deadline() {
    assert_eq!(countdown(Some(10_000), 1_000).as_deref(), Some("9s"));
    assert_eq!(countdown(Some(10_000), 500).as_deref(), Some("10s"));
    assert_eq!(countdown(Some(10_000), 10_000), None);
    assert_eq!(countdown(None, 0), None);
}

#[test]
fn only_a_retry_counts_down_to_a_retry() {
    assert_eq!(countdown_verb(K::Retry), "retry in");
    assert_eq!(countdown_verb(K::RateLimit), "resets in");
    assert_eq!(countdown_verb(K::Warning), "resets in");
}

#[test]
fn notice_links_open_urls_or_run_actions() {
    assert_eq!(
        notice_link("https://figma.test/auth", K::McpAuth),
        Some(("Sign in".into(), NoticeLink::Open("https://figma.test/auth".into())))
    );
    assert_eq!(
        notice_link("aether:trust-project", K::Info),
        Some(("Trust this project".into(), NoticeLink::Action("trust-project".into())))
    );
    assert_eq!(
        notice_link("aether:open-settings", K::Info),
        Some(("Open settings".into(), NoticeLink::Action("open-settings".into())))
    );
    assert_eq!(notice_link("", K::Info), None);
}

#[test]
fn stopped_marker_says_what_was_cut_off_and_why() {
    let stopped = |id: &str, detail, error: &str| {
        Step::Tool(ToolCall {
            status: ToolStatus::Stopped,
            error: (!error.is_empty()).then(|| error.to_string()),
            ..ToolCall::new(id, "T", detail)
        })
    };
    let cmd = || ToolDetail::Command {
        command: "pnpm build".into(),
        cwd: None,
        output: String::new(),
        exit_code: None,
    };
    let task = || ToolDetail::Task {
        description: "d".into(),
        output: String::new(),
        agent: "explore".into(),
        prompt: String::new(),
        tools: 0,
        tokens: 0,
        activity: String::new(),
    };
    let steps = [stopped("c", cmd(), ""), stopped("a", task(), ""), stopped("b", task(), "")];
    assert_eq!(
        stopped_summary(&steps).as_deref(),
        Some("1 command and 2 subagents were interrupted")
    );
    assert_eq!(stopped_summary(&steps[..1]).as_deref(), Some("1 command was interrupted"));
    assert_eq!(stopped_summary(&[]), None);
    assert_eq!(stopped_lead(&steps, false), "Stopped by you");
    assert_eq!(
        stopped_lead(&[stopped("c", cmd(), "Interrupted: Aether closed")], false),
        "Interrupted when Aether closed"
    );
    assert_eq!(stopped_lead(&steps, true), "Interrupted");
    assert_eq!(stopped_lead(&[], false), "Stopped by you");
    let edit = ToolDetail::Edit { path: "a".into(), diff: String::new(), added: 0, removed: 0 };
    let mixed = [
        stopped("c", cmd(), ""),
        stopped("e", edit, ""),
        stopped("a", task(), ""),
        stopped("b", task(), ""),
    ];
    assert_eq!(
        stopped_summary(&mixed).as_deref(),
        Some("1 command, 1 edit and 2 subagents were interrupted")
    );
}

#[test]
fn wake_line_keeps_a_multiline_task_name_to_its_first_line() {
    let text =
        "Claude started a turn on its own: background task \"\n**Verdicts**\n- C1 done\" completed";
    assert_eq!(wake_line(text).1, "**Verdicts**…");
}

#[test]
fn wake_line_names_the_background_task_that_woke_the_agent() {
    assert_eq!(
        wake_line("Claude started a turn on its own: background task \"Rewrite guide\" completed"),
        ("Background task finished", "Rewrite guide".to_string(), true)
    );
    assert_eq!(
        wake_line("Claude started a turn on its own: background task \"say \"hi\"\" failed").0,
        "Background task failed"
    );
    assert_eq!(
        wake_line("Codex started a turn on its own"),
        ("Agent resumed on its own", String::new(), false)
    );
}
