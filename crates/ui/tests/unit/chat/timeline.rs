use super::*;
use aether_core::{
    Message, Notice, NoticeKind, NoticeState, ToolCall, ToolDetail, ToolStatus, Update,
};

fn row(id: &str, detail: ToolDetail, status: ToolStatus) -> Update {
    Update::Tool(ToolCall { status, ..ToolCall::new(id, "T", detail) })
}
fn read(id: &str) -> Update {
    row(id, ToolDetail::Read { path: format!("src/{id}.rs"), range: None }, ToolStatus::Done)
}
fn agent() -> ToolDetail {
    ToolDetail::Task {
        description: "audit".into(),
        output: String::new(),
        agent: "explore".into(),
        prompt: String::new(),
        tools: 0,
        tokens: 0,
        activity: String::new(),
    }
}
fn under(id: &str, parent: &str, detail: ToolDetail) -> Update {
    Update::Tool(ToolCall {
        parent: Some(parent.into()),
        status: ToolStatus::Done,
        ..ToolCall::new(id, "T", detail)
    })
}
fn view(live: bool, open: bool, stopped: bool, kept: &dyn Fn(&str) -> bool) -> ReplyView<'_> {
    ReplyView { live, open, stopped, agent: true, show_hooks: true, show_thinking: true, kept }
}
fn live(m: &Message) -> Vec<Block> {
    reply_blocks(m, &view(true, false, false, &|_| false))
}

#[test]
fn finished_reads_in_a_row_fold_into_one_block() {
    let mut m = Message::assistant();
    for id in ["a", "b", "c"] {
        m.apply(read(id));
    }
    m.apply(Update::Delta("x".into()));
    m.apply(read("d"));
    m.apply(row("e", ToolDetail::Read { path: "e".into(), range: None }, ToolStatus::Running));
    assert_eq!(
        live(&m),
        [
            Block::Reads(vec![0, 1, 2]),
            Block::Text(0..1),
            Block::Tool(3),
            Block::Tool(4),
            Block::Live
        ]
    );
}

#[test]
fn subagent_rows_nest_and_grandchildren_flatten_under_the_top_card() {
    let mut m = Message::assistant();
    m.apply(row("t1", agent(), ToolStatus::Running));
    m.apply(under("c1", "t1", ToolDetail::Plain(String::new())));
    m.apply(under("t2", "t1", agent()));
    m.apply(under("g1", "t2", ToolDetail::Plain(String::new())));
    m.apply(read("r"));
    m.apply(under("o1", "gone", ToolDetail::Plain(String::new())));
    assert_eq!(
        live(&m),
        [
            Block::Subagent { step: 0, children: vec![1, 2, 3] },
            Block::Tool(4),
            Block::Tool(5),
            Block::Live
        ]
    );
}

#[test]
fn a_parent_cycle_renders_top_level_instead_of_hanging() {
    let mut m = Message::assistant();
    m.apply(under("a", "b", agent()));
    m.apply(under("b", "a", agent()));
    m.apply(under("s", "s", agent()));
    assert_eq!(
        live(&m),
        [
            Block::Subagent { step: 0, children: vec![] },
            Block::Subagent { step: 1, children: vec![] },
            Block::Subagent { step: 2, children: vec![] },
            Block::Live,
        ]
    );
}

#[test]
fn a_steer_that_opens_a_reply_reads_as_a_user_message() {
    let mut m = Message::assistant();
    m.apply(Update::Steered("also dark mode".into()));
    m.apply(Update::Delta("ok".into()));
    m.apply(Update::Steered("and mobile".into()));
    assert_eq!(live(&m), [Block::SteerAsUser(0), Block::Text(0..2), Block::Steer(1), Block::Live]);
}

#[test]
fn splits_never_land_inside_an_open_fence() {
    assert_eq!(safe_split("p\n```rs\nx\n```\nq", 9), 14);
    assert_eq!(safe_split("~~~\nx\n~~~\ny", 5), 10);
    assert_eq!(safe_split("````\n```\nx\n```\n````\nz", 9), 20);
    assert_eq!(safe_split("1. step\n   ```sh\n   ls\n   ```\n2. next", 20), 30);
    assert_eq!(safe_split("```\nx", 4), 5);
    assert_eq!(safe_split("a\n```\nx\n```\nb", 1), 1);
    let mut m = Message::assistant();
    m.apply(Update::Delta("intro\n```rust\nlet a = 1;\n".into()));
    m.apply(row("t", ToolDetail::Plain(String::new()), ToolStatus::Done));
    m.apply(Update::Delta("```\nafter".into()));
    let close = m.text.find("after").unwrap();
    assert_eq!(
        live(&m),
        [Block::Text(0..close), Block::Tool(0), Block::Text(close..m.text.len()), Block::Live]
    );
}

#[test]
fn a_finished_reply_folds_to_its_answer_changes_and_footer() {
    let mut m = Message::assistant();
    m.apply(read("a"));
    m.apply(row(
        "e",
        ToolDetail::Edit {
            path: "src/x.rs".into(),
            diff: "@@ -1 +1 @@\n-a\n+b".into(),
            added: 1,
            removed: 1,
        },
        ToolStatus::Done,
    ));
    m.apply(Update::Delta("Done.".into()));
    m.finish();
    let text = Block::Text(0..m.text.len());
    assert_eq!(
        reply_blocks(&m, &view(false, false, false, &|_| false)),
        [Block::Fold, text.clone(), Block::Changes, Block::Footer]
    );
    assert_eq!(
        reply_blocks(&m, &view(false, false, false, &|id| id == "e")),
        [Block::Fold, Block::Tool(1), text.clone(), Block::Changes, Block::Footer]
    );
    assert_eq!(
        reply_blocks(&m, &view(false, true, false, &|_| false)),
        [Block::Fold, Block::Tool(0), Block::Tool(1), text, Block::Changes, Block::Footer]
    );
}

#[test]
fn work_before_text_folds_under_one_row() {
    let cmd = |status| {
        let detail = ToolDetail::Command {
            command: "ls".into(),
            cwd: None,
            output: String::new(),
            exit_code: Some(0),
        };
        row("c", detail, status)
    };
    let mut m = Message::assistant();
    m.apply(read("a"));
    m.apply(Update::Delta("Looking.".into()));
    m.apply(read("b"));
    m.apply(row("p", ToolDetail::Plain(String::new()), ToolStatus::Done));
    m.apply(Update::Delta(" Found it.".into()));
    m.apply(row("q", ToolDetail::Plain(String::new()), ToolStatus::Done));
    m.apply(cmd(ToolStatus::Running));
    let first = m.step_at(1);
    let worked = Block::Worked(vec![Block::Tool(1), Block::Tool(2)]);
    assert_eq!(
        live(&m),
        [
            Block::Tool(0),
            Block::Text(0..first),
            worked.clone(),
            Block::Text(first..m.text.len()),
            Block::Tool(3),
            Block::Tool(4),
            Block::Live,
        ],
        "one row stays flat; the run still going stays open"
    );
    m.apply(cmd(ToolStatus::Done));
    m.apply(Update::Delta(" Done.".into()));
    m.finish();
    let open = reply_blocks(&m, &view(false, true, false, &|_| false));
    assert!(open.contains(&worked));
    assert!(open.contains(&Block::Worked(vec![Block::Tool(3), Block::Tool(4)])));
}

#[test]
fn the_run_still_going_never_folds_after_earlier_folds() {
    // alternating kinds, so activity grouping leaves four separate rows to fold
    let plain = |id: &str| row(id, ToolDetail::Plain(String::new()), ToolStatus::Done);
    let mut m = Message::assistant();
    for id in ["a", "b", "c", "d"] {
        m.apply(if id == "b" || id == "d" { read(id) } else { plain(id) });
    }
    m.apply(Update::Delta("Next.".into()));
    m.apply(plain("e"));
    m.apply(read("f"));
    let blocks = live(&m);
    assert!(matches!(blocks[0], Block::Worked(_)));
    assert_eq!(blocks[2..], [Block::Tool(4), Block::Tool(5), Block::Live]);
}

#[test]
fn failed_rows_stay_outside_work_rows() {
    let mut m = Message::assistant();
    m.apply(row("a", ToolDetail::Plain(String::new()), ToolStatus::Done));
    m.apply(row("b", ToolDetail::Plain(String::new()), ToolStatus::Failed));
    m.apply(row("c", ToolDetail::Plain(String::new()), ToolStatus::Done));
    m.apply(Update::Delta("Hm.".into()));
    assert_eq!(
        live(&m),
        [Block::Tool(0), Block::Tool(1), Block::Tool(2), Block::Text(0..3), Block::Live]
    );
}

#[test]
fn work_label_says_thought_or_worked_and_for_how_long() {
    let mut m = Message::assistant();
    m.apply(Update::Thinking("hmm".into()));
    assert_eq!(work_label(&m, &[Block::Thought(0)]), "Thought");
    m.apply(row("a", ToolDetail::Plain(String::new()), ToolStatus::Done));
    assert!(work_label(&m, &[Block::Thought(0), Block::Tool(1)]).starts_with("Worked"));
}

#[test]
fn replies_without_steps_or_with_background_work_do_not_fold() {
    let mut plain = Message::assistant();
    plain.apply(Update::Delta("hi".into()));
    plain.finish();
    assert_eq!(
        reply_blocks(&plain, &view(false, false, false, &|_| false)),
        [Block::Text(0..2), Block::Footer]
    );
    let mut bg = Message::assistant();
    bg.apply(row(
        "dev",
        ToolDetail::Command {
            command: "pnpm dev".into(),
            cwd: None,
            output: String::new(),
            exit_code: None,
        },
        ToolStatus::Background,
    ));
    bg.finish();
    assert_eq!(reply_blocks(&bg, &view(false, false, false, &|_| false)), [Block::Footer]);
}

#[test]
fn stopped_rows_and_errors_add_markers_that_survive_the_fold() {
    let mut m = Message::assistant();
    m.apply(row("c", ToolDetail::Plain(String::new()), ToolStatus::Running));
    m.apply(Update::Error("OpenCode service stopped responding\npanic".into()));
    m.finish();
    assert_eq!(
        reply_blocks(&m, &view(false, false, false, &|_| false)),
        [Block::Fold, Block::Fatal, Block::Stopped, Block::Footer]
    );
    let mut cut = Message::assistant();
    cut.apply(Update::Delta("partial".into()));
    cut.finish();
    assert_eq!(
        reply_blocks(&cut, &view(false, false, true, &|_| false)),
        [Block::Text(0..7), Block::Stopped, Block::Footer]
    );
}

#[test]
fn startup_steps_show_until_work_begins_and_failures_stay() {
    let step = |id: &str, state| {
        Update::Notice(Notice {
            id: id.into(),
            kind: NoticeKind::Startup,
            text: id.into(),
            state,
            until_ms: None,
            url: None,
            detail: None,
        })
    };
    let mut m = Message::assistant();
    m.apply(step("startup-version", NoticeState::Done));
    m.apply(step("startup-session", NoticeState::Running));
    assert_eq!(live(&m), [Block::Startup(vec![0, 1]), Block::Live]);
    m.apply(Update::Delta("hi".into()));
    assert_eq!(live(&m), [Block::Text(0..2), Block::Live]);
    let mut failed = Message::assistant();
    failed.apply(step("startup-spawn", NoticeState::Failed));
    failed.apply(Update::Delta("hi".into()));
    assert_eq!(live(&failed), [Block::Notice(0), Block::Text(0..2), Block::Live]);
}

#[test]
fn a_long_reply_becomes_one_row_per_block() {
    let mut m = Message::assistant();
    for i in 0..300 {
        m.apply(row(&i.to_string(), ToolDetail::Plain(String::new()), ToolStatus::Done));
        m.apply(Update::Delta(format!("step {i}\n")));
    }
    let never = |_: &str| false;
    let rows = transcript_rows(&[Message::user("go"), m], &|ix| ReplyView {
        live: ix == 1,
        open: false,
        stopped: false,
        agent: true,
        show_hooks: true,
        show_thinking: true,
        kept: &never,
    });
    assert!(rows.len() > 600);
    assert_eq!(rows[0], TranscriptRow { message: 0, block: Block::User });
}

#[test]
fn handoff_divider_follows_the_last_handed_off_message() {
    let mut first = Message::user("go");
    first.handoff = Some(aether_core::Handoff {
        from: aether_core::Provider::Claude,
        from_model: "Sonnet".into(),
        to: aether_core::Provider::Pi,
        to_model: "Crouton Alpha".into(),
    });
    let never = |_: &str| false;
    let rows =
        transcript_rows(&[first, Message::user("hello")], &|_| view(false, false, false, &never));
    let blocks: Vec<_> = rows.into_iter().map(|r| (r.message, r.block)).collect();
    assert_eq!(blocks, [(0, Block::User), (0, Block::Handoff), (1, Block::User)]);
}

#[test]
fn gaps_separate_messages_more_than_steps() {
    let at = |message, block| TranscriptRow { message, block };
    assert_eq!(gap_before(None, &at(0, Block::User)), 0.);
    assert_eq!(gap_before(Some(&at(0, Block::User)), &at(1, Block::Text(0..1))), 32.);
    assert_eq!(gap_before(Some(&at(1, Block::Text(0..1))), &at(1, Block::Tool(0))), 12.);
    assert_eq!(gap_before(Some(&at(1, Block::Tool(0))), &at(1, Block::Tool(1))), 4.);
    assert_eq!(gap_before(Some(&at(1, Block::Tool(1))), &at(1, Block::Live)), 12.);
}

#[test]
fn requests_follow_steps_at_the_same_position() {
    let mut m = Message::assistant();
    m.apply(Update::Ask { id: 1, ask: aether_core::Ask::Unknown });
    m.apply(Update::Ask { id: 2, ask: aether_core::Ask::Unknown });
    m.apply(row("t", ToolDetail::Plain(String::new()), ToolStatus::Done));
    m.apply(Update::Delta("ok".into()));
    assert_eq!(
        live(&m),
        [Block::Tool(0), Block::Request(0), Block::Request(1), Block::Text(0..2), Block::Live]
    );
}

#[test]
fn a_folded_reply_keeps_steers_failures_and_opened_rows() {
    let mut m = Message::assistant();
    m.apply(Update::Steered("and mobile".into()));
    m.apply(Update::Notice(Notice {
        id: "hook".into(),
        kind: NoticeKind::Hook,
        text: "lint hook".into(),
        state: NoticeState::Failed,
        until_ms: None,
        url: None,
        detail: None,
    }));
    m.apply(row("t1", agent(), ToolStatus::Done));
    m.apply(under("c1", "t1", ToolDetail::Plain(String::new())));
    for id in ["a", "b", "c"] {
        m.apply(read(id));
    }
    m.apply(Update::Delta("Done.".into()));
    m.finish();
    let folded = |kept: &dyn Fn(&str) -> bool| reply_blocks(&m, &view(false, false, false, kept));
    let head = [Block::Fold, Block::SteerAsUser(0)];
    let card = Block::Subagent { step: 2, children: vec![3] };
    let tail = [Block::Text(0..5), Block::Footer];
    assert_eq!(
        folded(&|id| id == "t1" || id == "b"),
        [&head[..], &[card, Block::Tool(5)], &tail].concat()
    );
    assert_eq!(
        folded(&|id| id == "a" || id == "b"),
        [&head[..], &[Block::Reads(vec![4, 5])], &tail].concat()
    );
    let group = read_group_id("a");
    assert_eq!(
        folded(&|id| id == group),
        [&head[..], &[Block::Reads(vec![4, 5, 6])], &tail].concat(),
        "an opened read group stays whole"
    );
}

#[test]
fn growing_rows_keep_their_place_in_the_list() {
    let row = |message, block| TranscriptRow { message, block };
    let old = [row(0, Block::User), row(1, Block::Text(0..5)), row(1, Block::Reads(vec![1, 2]))];
    let new = [row(0, Block::User), row(1, Block::Text(0..9)), row(1, Block::Reads(vec![1, 2, 3]))];
    assert_eq!(row_splice(&old, &new), (vec![], vec![1, 2]), "grown rows remeasure in place");

    let old = [row(1, Block::Text(0..3)), row(1, Block::Reads(vec![1, 2]))];
    let new =
        [row(1, Block::Text(0..3)), row(1, Block::Tool(0)), row(1, Block::Reads(vec![1, 2, 3]))];
    assert_eq!(
        row_splice(&old, &new),
        (vec![(1..1, 1)], vec![2]),
        "a changed survivor keeps its new index"
    );
}

#[test]
fn a_finishing_reply_splices_only_around_its_survivors() {
    let mut m = Message::assistant();
    m.apply(row("a", ToolDetail::Plain(String::new()), ToolStatus::Done));
    m.apply(row("b", ToolDetail::Plain(String::new()), ToolStatus::Done));
    m.apply(Update::Delta("Done.".into()));
    let rows = |blocks: Vec<Block>| {
        let mut rows = vec![TranscriptRow { message: 0, block: Block::User }];
        rows.extend(blocks.into_iter().map(|block| TranscriptRow { message: 1, block }));
        rows
    };
    let kept = |id: &str| id == "b";
    let old = rows(reply_blocks(&m, &view(true, false, false, &kept)));
    m.finish();
    let new = rows(reply_blocks(&m, &view(false, false, false, &kept)));
    assert_eq!(new[2..4], old[1..3], "kept row and text survive the fold");
    assert_eq!(
        row_splice(&old, &new),
        (vec![(1..1, 1), (3..4, 1)], vec![]),
        "the fold line and footer splice in; survivors keep their place"
    );
}

#[test]
fn approvals_sit_below_their_row_or_subagent_card() {
    use aether_core::{Approval, Ask};
    let mut m = Message::assistant();
    m.apply(row("t1", agent(), ToolStatus::Running));
    m.apply(under("c1", "t1", ToolDetail::Plain(String::new())));
    m.apply(Update::Delta("text".into()));
    m.apply(Update::Ask {
        id: 7,
        ask: Ask::Approval(Approval {
            tool_id: Some("c1".into()),
            source: Some("Designer".into()),
            ..Approval::default()
        }),
    });
    m.apply(Update::Ask { id: 8, ask: Ask::Approval(Approval::default()) });
    assert_eq!(
        live(&m),
        [
            Block::Subagent { step: 0, children: vec![1] },
            Block::Request(0),
            Block::Text(0..4),
            Block::Request(1),
            Block::Live
        ]
    );
}

#[test]
fn an_approval_sits_below_its_own_top_level_row() {
    use aether_core::{Approval, Ask};
    let mut m = Message::assistant();
    m.apply(row("t1", ToolDetail::Plain(String::new()), ToolStatus::Waiting));
    m.apply(Update::Delta("abc".into()));
    let ask = Approval { tool_id: Some("t1".into()), ..Approval::default() };
    m.apply(Update::Ask { id: 1, ask: Ask::Approval(ask) });
    assert_eq!(live(&m), [Block::Tool(0), Block::Request(0), Block::Text(0..3), Block::Live]);
}

#[test]
fn activities_group_by_kind_without_crossing_text_or_attention_rows() {
    let mut m = Message::assistant();
    m.steps = vec![
        Step::Skill("design".into()),
        Step::Skill("review".into()),
        Step::Thinking { text: "a".into(), ms: Some(100) },
        Step::Thinking { text: "b".into(), ms: Some(200) },
        Step::Notice(Notice::new("h1", NoticeKind::Hook, "lint")),
        Step::Notice(Notice::new("h2", NoticeKind::Hook, "format")),
    ];
    assert_eq!(
        live(&m),
        [
            Block::Activity {
                kind: ActivityKind::Skills,
                steps: vec![Block::Skill(0), Block::Skill(1)]
            },
            Block::Activity {
                kind: ActivityKind::Thinking,
                steps: vec![Block::Thought(2), Block::Thought(3)]
            },
            Block::Activity {
                kind: ActivityKind::Hooks,
                steps: vec![Block::Notice(4), Block::Notice(5)]
            },
            Block::Live,
        ]
    );
    m.text = "boundary".into();
    m.step_positions = vec![0, 8, 8, 8, 8, 8];
    let rows = live(&m);
    assert_eq!(&rows[..3], &[Block::Skill(0), Block::Text(0..8), Block::Skill(1)]);
}

fn cmd(id: &str, status: ToolStatus) -> Update {
    row(
        id,
        ToolDetail::Command {
            command: "test".into(),
            cwd: None,
            output: String::new(),
            exit_code: None,
        },
        status,
    )
}

#[test]
fn opened_activity_survives_but_failed_commands_follow_the_turn_fold() {
    let mut m = Message::assistant();
    m.apply(cmd("a", ToolStatus::Done));
    m.apply(cmd("b", ToolStatus::Done));
    m.apply(cmd("failed", ToolStatus::Failed));
    m.apply(Update::Delta("Done".into()));
    let blocks = reply_blocks(&m, &view(false, false, false, &|id| id == "activity-0"));
    assert_eq!(
        blocks,
        [
            Block::Fold,
            Block::Activity {
                kind: ActivityKind::Commands,
                steps: vec![Block::Tool(0), Block::Tool(1)]
            },
            Block::Text(0..4),
            Block::Footer
        ]
    );
    let kept_child = reply_blocks(&m, &view(false, false, false, &|id| id == "b"));
    assert_eq!(kept_child, blocks);
}

#[test]
fn growing_activity_keeps_its_scroller_identity() {
    let group = |steps| TranscriptRow {
        message: 1,
        block: Block::Activity { kind: ActivityKind::Commands, steps },
    };
    let old = [group(vec![Block::Tool(0), Block::Tool(1)])];
    let new = [group(vec![Block::Tool(0), Block::Tool(1), Block::Tool(2)])];
    assert_eq!(row_splice(&old, &new), (vec![], vec![0]));
}

#[test]
fn opened_descendants_keep_their_subagent_visible_but_failures_follow_the_fold() {
    let mut m = Message::assistant();
    m.apply(row("task", agent(), ToolStatus::Done));
    m.apply(under("child", "task", ToolDetail::Plain("result".into())));
    let group_open =
        reply_blocks(&m, &view(false, false, false, &|id| id == "task-activity-child"));
    assert!(group_open.iter().any(|b| matches!(b, Block::Subagent { .. })));
    let Step::Tool(child) = &mut m.steps[1] else { unreachable!() };
    child.status = ToolStatus::Failed;
    let folded = reply_blocks(&m, &view(false, false, false, &|_| false));
    assert!(!folded.iter().any(|b| matches!(b, Block::Subagent { .. })));
    let expanded = reply_blocks(&m, &view(false, true, false, &|_| false));
    assert!(expanded.iter().any(|b| matches!(b, Block::Subagent { .. })));
}

#[test]
fn failed_searches_follow_the_fold_but_running_hooks_prevent_folding() {
    let mut m = Message::assistant();
    m.steps.push(Step::Search {
        id: "s".into(),
        query: "q".into(),
        results: Some(Err("offline".into())),
    });
    assert!(!reply_blocks(&m, &view(false, false, false, &|_| false)).contains(&Block::Search(0)));
    assert!(reply_blocks(&m, &view(false, true, false, &|_| false)).contains(&Block::Search(0)));
    let mut hook = Notice::new("h", NoticeKind::Hook, "checking");
    hook.state = NoticeState::Running;
    m.steps.push(Step::Notice(hook));
    assert!(!reply_blocks(&m, &view(false, false, false, &|_| false)).contains(&Block::Fold));
}

#[test]
fn hidden_hooks_drop_their_rows_but_keep_failed_ones() {
    let mut m = Message::assistant();
    let mut blocked = Notice::new("h2", NoticeKind::Hook, "blocked");
    blocked.state = NoticeState::Failed;
    m.steps = vec![
        Step::Notice(Notice::new("h1", NoticeKind::Hook, "lint")),
        Step::Thinking { text: "a".into(), ms: Some(100) },
        Step::Notice(blocked),
    ];
    let hidden = ReplyView { show_hooks: false, ..view(true, false, false, &|_| false) };
    assert_eq!(reply_blocks(&m, &hidden), [Block::Thought(1), Block::Notice(2), Block::Live]);
}

#[test]
fn the_live_row_stands_in_for_a_running_compaction() {
    let mut m = Message::assistant();
    let compacting = Notice {
        state: NoticeState::Running,
        ..Notice::new("c", NoticeKind::Compaction, "Compacting context")
    };
    m.steps = vec![Step::Notice(compacting.clone())];
    assert_eq!(live(&m), [Block::Live]);
    assert_eq!(
        reply_blocks(&m, &view(false, false, false, &|_| false)),
        [Block::Notice(0), Block::Footer]
    );
    m.steps = vec![Step::Notice(Notice { state: NoticeState::Done, ..compacting })];
    assert_eq!(live(&m), [Block::Notice(0), Block::Live]);
}

fn compact_reply() -> Message {
    let mut m = Message::assistant();
    let notice = |kind, text| Step::Notice(Notice::new("n", kind, text));
    m.steps = vec![
        notice(NoticeKind::Startup, "Resuming session"),
        notice(NoticeKind::Hook, "SessionStart hook: SessionStart:resume"),
        notice(NoticeKind::Compaction, "Compacting context"),
        notice(NoticeKind::Compaction, "Compacted 671911 → 14549 tokens"),
        notice(NoticeKind::Hook, "SessionStart hook: SessionStart:compact"),
        notice(NoticeKind::McpAuth, "Drive needs you to sign in"),
    ];
    m
}

#[test]
fn a_reply_that_only_compacted_is_one_row() {
    let m = compact_reply();
    let finished = |stopped| view(false, false, stopped, &|_| false);
    assert_eq!(reply_blocks(&m, &finished(false)), [Block::Notice(3)]);
    assert!(reply_blocks(&m, &finished(true)).contains(&Block::Stopped));
    assert!(live(&m).contains(&Block::Live));

    let mut failed = compact_reply();
    failed.steps.truncate(3);
    if let Step::Notice(n) = &mut failed.steps[2] {
        (n.state, n.text) = (NoticeState::Failed, "Compaction failed: too big".into());
    }
    assert_eq!(
        reply_blocks(&failed, &finished(false)),
        [Block::Fold, Block::Notice(2), Block::Footer],
        "a failure keeps its row and the reply's footer",
    );
}

#[test]
fn a_compaction_stays_visible_in_a_longer_reply() {
    let mut m = Message::assistant();
    m.apply(read("a"));
    m.apply(Update::Notice(Notice::new("c", NoticeKind::Compaction, "Compacted 90 → 12 tokens")));
    m.apply(read("b"));
    m.text = "answer".into();
    let finished = |open| reply_blocks(&m, &view(false, open, false, &|_| false));
    assert_eq!(finished(false), [Block::Fold, Block::Notice(1), Block::Text(0..6), Block::Footer]);
    assert!(finished(true).contains(&Block::Notice(1)), "never folded into a Worked group");
}

#[test]
fn commands_do_not_group_across_a_waiting_operation() {
    let mut m = Message::assistant();
    m.apply(cmd("a", ToolStatus::Done));
    m.apply(cmd("waiting", ToolStatus::Waiting));
    m.apply(cmd("b", ToolStatus::Done));
    m.apply(cmd("c", ToolStatus::Running));
    assert_eq!(
        live(&m),
        [
            Block::Tool(0),
            Block::Tool(1),
            Block::Activity {
                kind: ActivityKind::Commands,
                steps: vec![Block::Tool(2), Block::Tool(3)]
            },
            Block::Live
        ]
    );
}

#[test]
fn failed_commands_fold_with_the_work_log_and_remain_available_when_expanded() {
    let mut m = Message::assistant();
    m.apply(cmd("failed", ToolStatus::Failed));
    m.apply(Update::Delta("The final answer.".into()));
    m.finish();
    assert_eq!(
        reply_blocks(&m, &view(false, false, false, &|_| false)),
        [Block::Fold, Block::Text(0..m.text.len()), Block::Footer]
    );
    assert_eq!(
        reply_blocks(&m, &view(false, true, false, &|_| false)),
        [Block::Fold, Block::Tool(0), Block::Text(0..m.text.len()), Block::Footer]
    );
}

#[test]
fn final_answer_survives_stop_hooks_after_and_during_its_text() {
    let mut m = Message::assistant();
    m.apply(cmd("work", ToolStatus::Done));
    m.apply(Update::Delta("Final answer. ".into()));
    let split = m.text.len();
    m.apply(Update::Notice(Notice::new("post", NoticeKind::Hook, "postToolUse hook")));
    m.apply(Update::Delta("Validation passed.".into()));
    for id in ["stop1", "stop2"] {
        m.apply(Update::Notice(Notice::new(id, NoticeKind::Hook, "stop hook")));
    }
    m.finish();
    assert_eq!(
        reply_blocks(&m, &view(false, false, false, &|_| false)),
        [Block::Fold, Block::Text(0..split), Block::Text(split..m.text.len()), Block::Footer]
    );
}

#[test]
fn live_background_rows_leave_the_transcript_unless_they_hold_an_approval() {
    let mut m = Message::assistant();
    m.apply(row("bg", ToolDetail::Plain(String::new()), ToolStatus::Background));
    assert!(!live(&m).contains(&Block::Tool(0)));
    m.apply(row("bg", ToolDetail::Plain(String::new()), ToolStatus::Done));
    assert!(live(&m).contains(&Block::Tool(0)), "finished work returns with its result");
}

#[test]
fn a_folded_reply_keeps_the_wake_that_started_it() {
    let mut m = Message::assistant();
    m.apply(Update::Notice(Notice::new("wake-1", NoticeKind::Wake, "woke")));
    m.apply(read("a"));
    m.apply(Update::Delta("done".into()));
    m.finish();
    let blocks = reply_blocks(&m, &view(false, false, false, &|_| false));
    assert_eq!(blocks[..2], [Block::Fold, Block::Notice(0)]);
}

#[test]
fn an_opened_background_row_stays_in_the_side_panel() {
    let mut m = Message::assistant();
    m.apply(row("bg", ToolDetail::Plain(String::new()), ToolStatus::Background));
    let opened = |id: &str| id == "bg";
    assert!(!reply_blocks(&m, &view(true, false, false, &opened)).contains(&Block::Tool(0)));
}

#[test]
fn compact_detail_drops_thinking_rows() {
    let mut m = Message::assistant();
    m.steps = vec![
        Step::Thinking { text: "a".into(), ms: Some(100) },
        Step::Thought("b".into()),
        Step::Notice(Notice::new("n", NoticeKind::Wake, "woke")),
    ];
    let compact = ReplyView { show_thinking: false, ..view(true, false, false, &|_| false) };
    assert_eq!(reply_blocks(&m, &compact), [Block::Notice(2), Block::Live]);
}

fn sent(model: &str, effort: &str) -> aether_core::SentWith {
    aether_core::SentWith { model: model.into(), effort: effort.into() }
}

fn sent_user(model: &str, effort: &str) -> Message {
    let mut message = Message::user("hello");
    message.sent_with = Some(sent(model, effort));
    message
}

#[test]
fn model_switch_labels_show_only_changed_values() {
    let label = |from, to| {
        model_switch_label(&from, &to, &|m| format!("Model {m}"), &|_| Some("medium".into()))
    };
    assert_eq!(label(sent("a", "high"), sent("b", "high")).as_deref(), Some("Model a → Model b"));
    assert_eq!(
        label(sent("a", "medium"), sent("a", "xhigh")).as_deref(),
        Some("Medium → Extra High")
    );
    assert_eq!(
        label(sent("a", "medium"), sent("b", "high")).as_deref(),
        Some("Model a Medium → Model b High")
    );
    assert_eq!(label(sent("a", ""), sent("a", "medium")), None);
    assert_eq!(label(sent("a", ""), sent("a", "high")).as_deref(), Some("Medium → High"));
    assert_eq!(
        model_switch_label(&sent("a", ""), &sent("a", "high"), &str::to_string, &|_| None),
        None
    );
}

#[test]
fn model_switch_divider_precedes_only_a_changed_user_turn() {
    let messages = [
        sent_user("a", "medium"),
        Message::assistant(),
        sent_user("b", "high"),
        sent_user("b", "high"),
    ];
    let never = |_: &str| false;
    let rows = transcript_rows(&messages, &|_| view(false, false, false, &never));
    let switches: Vec<_> =
        rows.iter().filter(|r| matches!(r.block, Block::ModelSwitch(_))).collect();
    assert_eq!(switches.len(), 1);
    assert_eq!(switches[0].message, 2);
    let at = rows.iter().position(|r| matches!(r.block, Block::ModelSwitch(_))).unwrap();
    assert_eq!(rows[at + 1], TranscriptRow { message: 2, block: Block::User });
    let partial =
        transcript_rows_with(&messages, 2, &|_| view(false, false, false, &never), &|a, b| {
            model_switch_label(a, b, &str::to_string, &|_| None)
        });
    assert_eq!(partial, rows.into_iter().filter(|r| r.message >= 2).collect::<Vec<_>>());
}

#[test]
fn model_switch_divider_does_not_cross_handoffs_or_missing_snapshots() {
    let mut reply = Message::assistant();
    reply.handoff = Some(aether_core::Handoff {
        from: aether_core::Provider::Codex,
        from_model: "a".into(),
        to: aether_core::Provider::Claude,
        to_model: "b".into(),
    });
    let messages = [
        sent_user("a", "medium"),
        reply,
        sent_user("b", "high"),
        Message::user("old"),
        sent_user("c", "low"),
    ];
    let never = |_: &str| false;
    let rows = transcript_rows(&messages, &|_| view(false, false, false, &never));
    assert!(!rows.iter().any(|r| matches!(r.block, Block::ModelSwitch(_))));
    assert!(rows.iter().any(|r| r.block == Block::Handoff));
}

#[test]
fn effort_change_alone_places_a_divider() {
    let messages = [sent_user("a", "medium"), sent_user("a", "high")];
    let never = |_: &str| false;
    let rows = transcript_rows(&messages, &|_| view(false, false, false, &never));
    assert_eq!(
        rows,
        vec![
            TranscriptRow { message: 0, block: Block::User },
            TranscriptRow { message: 1, block: Block::ModelSwitch("Medium → High".into()) },
            TranscriptRow { message: 1, block: Block::User },
        ]
    );
}

#[test]
fn sent_settings_round_trip_and_old_messages_load_without_them() {
    let message = sent_user("a", "high");
    let mut value = serde_json::to_value(&message).unwrap();
    let restored: Message = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(restored.sent_with, message.sent_with);
    value.as_object_mut().unwrap().remove("sent_with");
    let restored: Message = serde_json::from_value(value).unwrap();
    assert_eq!(restored.sent_with, None);
}
