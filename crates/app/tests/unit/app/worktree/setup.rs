use std::path::PathBuf;

use crate::app::chat_with;

use super::*;

fn script(name: &str, setup: bool) -> ProjectScript {
    ProjectScript { name: name.into(), command: "x".into(), on_new_worktree: setup }
}

fn job(names: &[&str], current: usize, failed: Option<SetupFailure>) -> SetupJob {
    let queue = names.iter().map(|n| script(n, true)).collect();
    SetupJob { queue, current, text: "hi".into(), attachments: vec![], failed }
}

#[test]
fn setup_queue_keeps_only_on_new_worktree_in_order() {
    let scripts = [script("a", true), script("b", false), script("c", true)];
    let names: Vec<_> = setup_queue(&scripts).into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["a", "c"]);
}

#[test]
fn after_exit_advances_sends_or_fails() {
    use SetupFailure::*;
    assert_eq!(after_exit(&job(&["a", "b"], 0, None), Some(0)), SetupStep::Next(1));
    assert_eq!(after_exit(&job(&["a", "b"], 1, None), Some(0)), SetupStep::Send);
    assert_eq!(after_exit(&job(&["a", "b"], 0, None), Some(2)), SetupStep::Failed(Code(2)));
    assert_eq!(after_exit(&job(&["a", "b"], 1, None), None), SetupStep::Failed(Stopped));
}

#[test]
fn setup_banner_texts() {
    use SetupFailure::*;
    let banner = |failed| setup_banner(&job(&["install"], 0, failed));
    assert_eq!(banner(None), "Setting up worktree… (install)");
    assert_eq!(banner(Some(Code(1))), "Setup failed: install exited with code 1");
    assert_eq!(banner(Some(Stopped)), "Setup failed: install stopped");
    assert_eq!(banner(Some(NoStart)), "Setup failed: install couldn't start");
    assert_eq!(banner(Some(Blocked("Switching branches…"))), "Not sent: Switching branches…");
}

fn ended(key: &str, code: Option<i32>, stopped: bool, failed_to_start: bool) -> TerminalEvent {
    TerminalEvent::CommandEnded { key: key.to_string().into(), code, stopped, failed_to_start }
}

#[test]
fn only_the_awaited_script_moves_setup_on() {
    use SetupFailure::*;
    let running = job(&["install", "build"], 0, None);
    let step = |e| setup_step(&running, &e);
    assert_eq!(step(ended("dev", Some(0), false, false)), None);
    assert_eq!(step(ended("install", Some(0), false, false)), Some(SetupStep::Next(1)));
    assert_eq!(step(ended("install", Some(143), true, false)), Some(SetupStep::Failed(Stopped)));
    assert_eq!(step(ended("install", None, false, true)), Some(SetupStep::Failed(NoStart)));
    let failed = job(&["install"], 0, Some(Code(1)));
    assert_eq!(setup_step(&failed, &ended("install", Some(0), false, false)), None);
}

#[test]
fn canceled_prompt_goes_before_the_current_draft() {
    assert_eq!(rejoin("held", ""), "held");
    assert_eq!(rejoin("held", "  \n"), "held");
    assert_eq!(rejoin("held", "new"), "held\n\nnew");
}

#[test]
fn setup_stays_owed_until_sent_from_the_fresh_worktree() {
    let mut chat = chat_with("t", &[]);
    chat.fresh_worktree = Some(PathBuf::from("/w/fix"));
    assert!(!setup_owed(&chat));
    chat.worktree_path = "/w/other".into();
    assert!(!setup_owed(&chat));
    chat.worktree_path = "/w/fix".into();
    assert!(setup_owed(&chat));
    chat.fresh_worktree = None;
    assert!(!setup_owed(&chat));
}

#[test]
fn a_cancelled_job_restores_every_file_ahead_of_the_current_ones() {
    let file =
        |n: &str| Attachment { name: n.into(), path: n.into(), mime: "image/png".into(), size: 1 };
    let held: Vec<_> = (0..8).map(|i| file(&format!("h{i}"))).collect();
    let current: Vec<_> = (0..5).map(|i| file(&format!("c{i}"))).collect();
    let all = restored(held, current);
    assert_eq!(all.len(), 13);
    assert_eq!(all[0].name, "h0");
    assert_eq!(all[8].name, "c0");
}
