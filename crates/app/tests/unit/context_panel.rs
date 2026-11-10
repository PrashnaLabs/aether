use super::*;
use aether_core::{Message, Notice, NoticeState};

fn item(tokens: u64) -> Item {
    Item { name: String::new(), path: PathBuf::new(), tokens }
}

#[test]
fn breakdown_fills_messages_with_the_rest() {
    let inv = Inventory { memory: vec![item(100)], skills: vec![item(50)], ..Inventory::default() };
    let parts = breakdown(1_000, 10_000, &inv);
    assert_eq!(
        parts,
        [
            (Part::Messages, 850),
            (Part::Memory, 100),
            (Part::Skills, 50),
            (Part::Agents, 0),
            (Part::Free, 9_000)
        ]
    );
}

#[test]
fn breakdown_before_first_reply_counts_estimates() {
    let inv = Inventory { memory: vec![item(300)], ..Inventory::default() };
    let parts = breakdown(0, 1_000, &inv);
    assert_eq!(parts[0], (Part::Messages, 0));
    assert_eq!(parts[4], (Part::Free, 700));
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
