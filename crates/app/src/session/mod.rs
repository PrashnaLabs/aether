use crate::app::Chat;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ChatId(pub i64);

#[derive(Clone, Copy, Debug)]
pub(crate) struct CommandTarget {
    pub chat: ChatId,
    pub epoch: u64,
}

impl CommandTarget {
    pub(crate) fn resolve(self, chats: &[Chat]) -> Option<usize> {
        chats.iter().position(|c| c.id == self.chat.0 && c.epoch == self.epoch)
    }
}

pub(crate) fn allocate_chat_id(
    highwater: &mut i64,
    ids: impl Iterator<Item = i64>,
    now: i64,
) -> i64 {
    *highwater = highwater.saturating_add(1).max(ids.max().unwrap_or(0).saturating_add(1)).max(now);
    *highwater
}

#[cfg(test)]
#[path = "../../tests/unit/session/runtime.rs"]
mod tests;
