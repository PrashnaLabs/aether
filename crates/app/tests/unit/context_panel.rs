use super::*;
use aether_core::{Message, Notice, NoticeState, ToolCall, ToolDetail, ToolStatus};

fn item(tokens: u64) -> Item {
    Item { tokens, ..Item::default() }
}

#[test]
fn estimated_breakdown_fills_messages_with_the_rest() {
    let inv = Inventory {
        memory: vec![item(100)],
        skills: vec![item(50)],
        mcp: vec![item(25)],
        ..Inventory::default()
    };
    let parts = breakdown(1_000, 10_000, &inv);
    let named: Vec<(&str, u64)> = parts.iter().map(|(n, t)| (n.as_str(), *t)).collect();
    assert_eq!(
        named,
        [
            ("Messages & system", 825),
            ("MCP tools", 25),
            ("Memory files", 100),
            ("Skills", 50),
            ("Custom agents", 0),
            (FREE, 9_000)
        ]
    );
}

#[test]
fn exact_breakdown_uses_reported_parts_and_buffer() {
    let inv = Inventory {
        parts: vec![("System prompt".into(), 2_000), ("Skills".into(), 1_000)],
        buffer: 500,
        ..Inventory::default()
    };
    let parts = breakdown(5_000, 10_000, &inv);
    assert_eq!(parts[0], (MESSAGES.to_string(), 2_000));
    assert_eq!(parts[3], (BUFFER.to_string(), 500));
    assert_eq!(parts[4], (FREE.to_string(), 4_500));
}

#[test]
fn breakdown_before_first_reply_counts_fixed_parts() {
    let inv = Inventory { memory: vec![item(300)], ..Inventory::default() };
    let parts = breakdown(0, 1_000, &inv);
    assert_eq!(parts[0].1, 0);
    assert_eq!(parts.last().unwrap(), &(FREE.to_string(), 700));
}

#[test]
fn sign_in_clears_after_success() {
    let mut reply = Message::assistant();
    let ask = |name: &str, text: &str| {
        Step::Notice(Notice::new(format!("mcp-{name}"), NoticeKind::McpAuth, text))
    };
    reply.steps.push(ask("postman", "postman needs you to sign in."));
    reply.steps.push(ask("figma", "figma needs you to sign in."));
    reply.steps.push(Step::Notice(Notice {
        state: NoticeState::Done,
        ..Notice::new("mcp-figma", NoticeKind::McpAuth, "Signed in to figma")
    }));
    let names = needs_sign_in(&[reply]);
    assert_eq!(names, HashSet::from(["postman".to_string()]));
}

#[test]
fn running_agents_are_live_task_rows_only() {
    let task = |id: &str, status| {
        let detail = ToolDetail::Task {
            description: id.into(),
            output: String::new(),
            agent: "general-purpose".into(),
            prompt: String::new(),
            tools: 2,
            tokens: 0,
            activity: String::new(),
        };
        let mut call = ToolCall::new(id, id, detail);
        call.status = status;
        Step::Tool(call)
    };
    let mut reply = Message::assistant();
    reply.steps.push(task("bg", ToolStatus::Background));
    reply.steps.push(task("run", ToolStatus::Running));
    reply.steps.push(task("done", ToolStatus::Stopped));
    let mut shell = ToolCall::new("sh", "sh", ToolDetail::default());
    shell.status = ToolStatus::Background;
    reply.steps.push(Step::Tool(shell));
    let mut old = reply.clone();
    old.restored = true;
    let messages = [old, reply];
    let ids: Vec<&str> = running_agents(&messages).iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["bg", "run"]);
}
