use super::*;
use futures::StreamExt;
use futures::channel::mpsc;

fn answer(pending: &Pending, id: u64, answer: Answer) {
    let tx = pending.lock().unwrap().remove(&id).unwrap();
    tx.send(answer).unwrap();
}

#[test]
fn approvals_ask_edit_and_remember_always() {
    futures::executor::block_on(async {
        let (tx, mut updates) = mpsc::unbounded();
        let pending = Pending::default();
        let always_allowed: AlwaysAllowed = Default::default();
        let gate = Gate::new(
            tx.clone(),
            pending.clone(),
            Arc::default(),
            Settings::default(),
            always_allowed.clone(),
        );

        assert_eq!(gate.approve("load_skill", "Load", "tone".into()).await, Some("tone".into()));

        let first = gate.approve("web_search", "Search", "rust".into());
        let user = async {
            let Some(Update::Ask { id, .. }) = updates.next().await else { panic!() };
            answer(&pending, id, Answer::Approve { input: "rust gpui".into(), always: true });
        };
        let (approved, ()) = futures::join!(first, user);
        assert_eq!(approved, Some("rust gpui".into()));
        assert_eq!(
            gate.approve("web_search", "Search", "again".into()).await,
            Some("again".into())
        );

        let gate = Gate::new(
            gate.tx.clone(),
            pending.clone(),
            Arc::default(),
            Settings::default(),
            always_allowed.clone(),
        );
        assert_eq!(
            gate.approve("web_search", "Search", "next turn".into()).await,
            Some("next turn".into())
        );

        let other_chat: AlwaysAllowed = Default::default();
        let gate = Gate::new(
            gate.tx.clone(),
            pending.clone(),
            Arc::default(),
            Settings::default(),
            other_chat,
        );
        let denied = gate.approve("web_search", "Search", "x".into());
        let user = async {
            let Some(Update::Ask { id, .. }) = updates.next().await else { panic!() };
            answer(&pending, id, Answer::Deny);
        };
        assert_eq!(futures::join!(denied, user).0, None);
    });
}
