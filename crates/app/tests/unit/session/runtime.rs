use super::*;
use crate::app::chat_with;

#[test]
fn commands_keep_their_chat_after_delete() {
    let mut chats = vec![chat_with("a", &[]), chat_with("b", &[])];
    chats[0].id = 11;
    chats[1].id = 22;
    let b = CommandTarget { chat: ChatId(22), epoch: 0 };
    chats.remove(0);
    assert_eq!(b.resolve(&chats), Some(0));
    assert_eq!(CommandTarget { chat: ChatId(11), epoch: 0 }.resolve(&chats), None);
}

#[test]
fn stale_approval_after_rewind_is_rejected() {
    let mut chat = chat_with("a", &[]);
    let target = CommandTarget { chat: ChatId(chat.id), epoch: chat.epoch };
    chat.epoch += 1;
    assert_eq!(target.resolve(&[chat]), None);
}

#[test]
fn deleting_the_latest_chat_cannot_reuse_its_id() {
    let mut highwater = 0;
    let first = allocate_chat_id(&mut highwater, std::iter::empty(), 100);
    assert_eq!(first, 100);
    let next = allocate_chat_id(&mut highwater, std::iter::empty(), 100);
    assert_eq!(next, 101);
}
