use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use aether_core::{Answer, Approval, Ask, MutexExt, Settings, ToolPolicy, Update};
use futures::channel::{mpsc::UnboundedSender, oneshot};

/// Answers the UI hasn't sent yet, by request id. Shared across turns.
pub(crate) type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Answer>>>>;

/// Tools allowed always for one chat, by tool name. Shared across turns.
pub(crate) type AlwaysAllowed = Arc<Mutex<HashSet<String>>>;

/// Lets a tool pause one turn until the user answers.
pub(crate) struct Gate {
    tx: UnboundedSender<Update>,
    pending: Pending,
    next_id: Arc<AtomicU64>,
    settings: Settings,
    always_allowed: AlwaysAllowed,
}

impl Gate {
    pub fn new(
        tx: UnboundedSender<Update>,
        pending: Pending,
        next_id: Arc<AtomicU64>,
        settings: Settings,
        always_allowed: AlwaysAllowed,
    ) -> Self {
        Self { tx, pending, next_id, settings, always_allowed }
    }

    /// Shows `ask` to the user and waits. `None` if the turn ends first.
    pub async fn ask(&self, ask: Ask) -> Option<Answer> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.guard().insert(id, tx);
        if self.tx.unbounded_send(Update::Ask { id, ask }).is_err() {
            self.pending.guard().remove(&id);
            return None;
        }
        rx.await.ok()
    }

    /// The input to run `tool` with, possibly edited by the user, or `None`
    /// if they declined. Tools set to Allow pass straight through.
    pub async fn approve(&self, tool: &str, title: &str, input: String) -> Option<String> {
        let asks = self.settings.tool_policy(tool) == ToolPolicy::Ask
            && !self.always_allowed.guard().contains(tool);
        if !asks {
            return Some(input);
        }
        let ask = Ask::Approval(Approval {
            tool: tool.into(),
            title: title.into(),
            input: input.clone(),
            ..Approval::default()
        });
        match self.ask(ask).await? {
            Answer::Approve { input, always } => {
                if always {
                    self.always_allowed.guard().insert(tool.into());
                }
                Some(input)
            }
            Answer::Pick(_) => Some(input),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/gate.rs"]
mod tests;
