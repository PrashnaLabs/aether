use crate::app::{WorktreeJob, chat_with};
use aether_core::Provider;

use super::*;

#[test]
fn review_retry_is_blocked_before_rewinding_a_shared_folder() {
    let mut switching = chat_with("draft", &[]);
    switching.provider = Provider::Claude;
    switching.cwd = "/code/app".into();
    switching.worktree_job = Some(WorktreeJob::Switching);
    let mut completed = chat_with("completed", &["question", "answer"]);
    completed.provider = Provider::Claude;
    completed.cwd = switching.cwd.clone();
    let mut chats = vec![switching, completed];
    assert_eq!(retry_request(&chats, 1, "/fallback"), None);
    assert_eq!(chats[1].messages.len(), 2);
    chats[0].worktree_job = None;
    assert_eq!(retry_request(&chats, 1, "/fallback"), Some((0, "question".into(), vec![])));
    let file = aether_core::Attachment {
        name: "a.png".into(),
        path: "/x/a.png".into(),
        mime: "image/png".into(),
        size: 1,
    };
    chats[1].messages[0].attachments = vec![file.clone()];
    assert_eq!(retry_request(&chats, 1, "/fallback"), Some((0, "question".into(), vec![file])));
}

fn file(name: &str) -> aether_core::Attachment {
    aether_core::Attachment {
        name: name.into(),
        path: format!("/x/{name}").into(),
        mime: "image/png".into(),
        size: 1,
    }
}

#[test]
fn an_edit_sets_the_draft_aside_and_gives_it_back() {
    let mut chat = chat_with("t", &[]);
    chat.pending = vec![file("keep.png")];
    stash_draft_for_edit(&mut chat, "half typed".into());
    assert!(chat.pending.is_empty());
    chat.pending = vec![file("copy.png")];
    assert_eq!(unstash(&mut chat), "half typed");
    assert_eq!(chat.pending, [file("keep.png")]);
    assert_eq!(unstash(&mut chat), "");
    assert!(chat.pending.is_empty());
}

#[test]
fn an_edit_follows_its_chat_id_through_deletes() {
    let mut chats = vec![chat_with("a", &[]), chat_with("b", &[]), chat_with("c", &[])];
    for (i, c) in chats.iter_mut().enumerate() {
        c.id = i as i64 + 1;
    }
    let editing = Some((3, 0));
    assert_eq!(edit_chat_ix(&chats, editing), Some(2));
    chats.remove(0);
    assert_eq!(edit_chat_ix(&chats, editing), Some(1));
    chats.remove(1);
    assert_eq!(edit_chat_ix(&chats, editing), None);
    assert_eq!(edit_chat_ix(&chats, None), None);
}

#[test]
fn only_a_different_chat_leaves_the_edit() {
    assert!(!leaves_edit(None, 1));
    assert!(!leaves_edit(Some((1, 4)), 1));
    assert!(leaves_edit(Some((1, 4)), 2));
}

#[gpui_kit::test]
fn an_emptied_chat_drops_its_old_title(cx: &mut gpui_kit::TestAppContext) {
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
        app.chats[app.active] = crate::app::chat_with("Old session", &["hi", "hello"]);
        app
    });
    window
        .update(cx, |app, _, cx| {
            app.clear_chat(cx);
            assert_eq!(app.chats[app.active].title.as_ref(), "New chat", "after /clear");
            app.chats[app.active] = crate::app::chat_with("Old session", &["hi", "hello"]);
            let ix = app.active;
            app.rewind(ix, 0, cx);
            assert_eq!(app.chats[ix].title.as_ref(), "New chat", "after editing the first message");
        })
        .unwrap();
}

#[test]
fn rewind_retains_only_sessions_with_surviving_history() {
    let mut chat = chat_with("t", &["question", "answer"]);
    for seen in [0, 1, 2] {
        chat.agent_sessions.push(aether_agent::AgentSession {
            provider: Provider::Codex,
            session: seen.to_string(),
            seen,
            context_tokens: None,
            context_window: None,
        });
    }
    chat.handoff_seen = Some(aether_agent::HandoffSeen {
        seen: 1,
        pending: true,
        context_tokens: Some(42),
        context_window: Some(200_000),
    });
    let handoff = chat.handoff_seen.clone();
    retain_sessions(&mut chat, 1);
    assert_eq!(chat.agent_sessions.iter().map(|s| s.seen).collect::<Vec<_>>(), [0, 1]);
    assert_eq!(chat.handoff_seen, handoff);
    retain_sessions(&mut chat, 0);
    assert!(chat.handoff_seen.is_none());
}
