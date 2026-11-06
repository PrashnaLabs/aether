use super::*;
use aether_core::{Message, ToolCall, ToolDetail, ToolStatus, Update};
use gpui_kit::{Context, Modifiers, Render, VisualTestContext};

fn done(id: &str, detail: ToolDetail) -> Update {
    Update::Tool(ToolCall { status: ToolStatus::Done, ..ToolCall::new(id, "T", detail) })
}

#[test]
fn fold_line_counts_subagents_files_and_commands() {
    let mut m = Message::assistant();
    m.apply(done(
        "t",
        ToolDetail::Task {
            description: "d".into(),
            output: String::new(),
            agent: "explore".into(),
            prompt: String::new(),
            tools: 0,
            tokens: 0,
            activity: String::new(),
        },
    ));
    m.apply(Update::Tool(ToolCall {
        parent: Some("t".into()),
        status: ToolStatus::Done,
        ..ToolCall::new("r1", "Read", ToolDetail::Read { path: "a.rs".into(), range: None })
    }));
    m.apply(done("r2", ToolDetail::Read { path: "b.rs".into(), range: None }));
    m.apply(done("r3", ToolDetail::Read { path: "b.rs".into(), range: None }));
    m.apply(done(
        "c",
        ToolDetail::Command {
            command: "ls".into(),
            cwd: None,
            output: String::new(),
            exit_code: Some(0),
        },
    ));
    m.apply(done(
        "e",
        ToolDetail::Edit { path: "a.rs".into(), diff: String::new(), added: 1, removed: 0 },
    ));
    assert_eq!(
        fold_counts(&m.steps),
        " · 1 subagent · read 2 files · ran 1 command · edited 1 file"
    );
    assert_eq!(tool_counts(&m.steps), (5, 1));
    assert_eq!(fold_lead(Outcome::Worked, Some(401_000)), "Worked for 6m 41s");
    assert_eq!(fold_lead(Outcome::Stopped, Some(72_000)), "Stopped after 1m 12s");
    assert_eq!(fold_lead(Outcome::Failed, Some(5_000)), "Failed after 5s");
}

fn full_facts() -> FootFacts {
    FootFacts {
        outcome: Outcome::Worked,
        duration_ms: Some(401_000),
        tools: 31,
        subagents: 2,
        model: Some("DeepSeek V4.1 Flash".into()),
        tokens_in: 182_000,
        tokens_out: 12_400,
        cached: 165_620,
        cost: Some("$0.08".into()),
        at: Some("8:20 AM".into()),
    }
}

#[test]
fn footer_keeps_the_lead_and_moves_the_rest_to_details() {
    let f = full_facts();
    assert_eq!(footer_lead(&f), "Worked 6m 41s");
    assert_eq!(
        footer_details(&f),
        [
            ("Tool calls", "31".to_string()),
            ("Subagents", "2".to_string()),
            ("Model", "DeepSeek V4.1 Flash".to_string()),
            ("Input", "182k".to_string()),
            ("Output", "12.4k".to_string()),
            ("Cached", "91%".to_string()),
            ("Cost", "$0.08".to_string()),
        ]
    );
    let bare = FootFacts {
        outcome: Outcome::Failed,
        duration_ms: Some(123_000),
        tools: 1,
        subagents: 0,
        model: None,
        tokens_in: 0,
        tokens_out: 0,
        cached: 0,
        cost: None,
        at: None,
    };
    assert_eq!(footer_lead(&bare), "Failed after 2m 03s");
    assert_eq!(footer_details(&bare), [("Tool calls", "1".to_string())]);
    let none = FootFacts { duration_ms: None, tools: 0, ..bare };
    assert_eq!(footer_lead(&none), "Failed");
    assert!(footer_details(&none).is_empty());
}

struct Foot(fn() -> FootFacts);

impl Render for Foot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(TurnFooter::new("foot", &(self.0)()))
    }
}

#[gpui::test]
fn the_details_icon_opens_the_popover(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|_, _| Foot(full_facts));
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(*window, cx);
    let trigger = visual.debug_bounds("foot-details-trigger").unwrap();
    assert!(visual.debug_bounds("foot-details").is_none());
    visual.simulate_click(trigger.center(), Modifiers::none());
    visual.run_until_parked();
    assert!(visual.debug_bounds("foot-details").is_some());
}

#[gpui::test]
fn no_details_means_no_icon(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    let facts = || FootFacts {
        outcome: Outcome::Failed,
        duration_ms: None,
        tools: 0,
        subagents: 0,
        model: None,
        tokens_in: 0,
        tokens_out: 0,
        cached: 0,
        cost: None,
        at: None,
    };
    let window = cx.add_window(|_, _| Foot(facts));
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(*window, cx);
    assert!(visual.debug_bounds("foot-details-trigger").is_none());
}

fn step(id: &str, detail: ToolDetail, status: ToolStatus) -> Step {
    Step::Tool(ToolCall { status, ..ToolCall::new(id, "T", detail) })
}

fn cmd() -> ToolDetail {
    ToolDetail::Command { command: "ls".into(), cwd: None, output: String::new(), exit_code: None }
}

#[test]
fn unknown_time_leaves_the_lead_bare() {
    assert_eq!(fold_lead(Outcome::Worked, None), "Worked");
    assert_eq!(fold_lead(Outcome::Stopped, None), "Stopped");
    assert_eq!(fold_lead(Outcome::Failed, None), "Failed");
}

#[test]
fn fold_counts_skip_denied_and_count_searches_and_plain_tools() {
    let search = Step::Search { id: String::new(), query: "q".into(), results: Some(Ok(3)) };
    let steps = [
        step("1", cmd(), ToolStatus::Done),
        step("2", cmd(), ToolStatus::Denied),
        search.clone(),
        step("3", ToolDetail::Plain("ls".into()), ToolStatus::Done),
        step("4", ToolDetail::Web { target: "x".into(), output: String::new() }, ToolStatus::Done),
    ];
    assert_eq!(fold_counts(&steps), " · ran 1 command · searched 1 time · used 2 tools");
    assert_eq!(fold_counts(&[search.clone(), search]), " · searched 2 times");
    assert_eq!(fold_counts(&[]), "");
    assert_eq!(tool_counts(&steps), (3, 0));
}

#[test]
fn failed_calls_are_counted() {
    let steps = [
        step("1", cmd(), ToolStatus::Failed),
        step("2", cmd(), ToolStatus::Done),
        step("3", cmd(), ToolStatus::Failed),
    ];
    assert_eq!(fold_failed(&steps), 2);
}

#[test]
fn zero_tool_calls_are_left_out_of_the_footer() {
    let f = FootFacts {
        outcome: Outcome::Worked,
        duration_ms: Some(2_000),
        tools: 0,
        subagents: 0,
        model: None,
        tokens_in: 0,
        tokens_out: 0,
        cached: 0,
        cost: None,
        at: None,
    };
    assert_eq!(footer_lead(&f), "Worked 2s");
    assert!(footer_details(&f).is_empty());
}

#[test]
fn outcome_failed_beats_stopped_and_a_stopped_tool_means_stopped() {
    let mut m = Message::assistant();
    assert_eq!(Outcome::of(&m, false), Outcome::Worked);
    assert_eq!(Outcome::of(&m, true), Outcome::Stopped);
    m.steps.push(step("1", cmd(), ToolStatus::Stopped));
    assert_eq!(Outcome::of(&m, false), Outcome::Stopped);
    m.failed = true;
    assert_eq!(Outcome::of(&m, true), Outcome::Failed);
    let mut e = Message::assistant();
    e.error = Some("boom".into());
    assert_eq!(Outcome::of(&e, false), Outcome::Failed);
}

#[test]
fn summary_includes_thinking_hooks_and_distinct_skills() {
    let steps = vec![
        Step::Thought("inspect".into()),
        Step::Thinking { text: "plan".into(), ms: Some(100) },
        Step::Skill("design".into()),
        Step::Skill("design".into()),
        Step::Skill("review".into()),
        Step::Notice(aether_core::Notice::new("h", NoticeKind::Hook, "lint")),
    ];
    assert_eq!(fold_counts(&steps), " · 2 thinking steps · ran 1 hook · used 2 skills");
}
