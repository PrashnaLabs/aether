use super::*;
use futures::StreamExt;
use futures::channel::mpsc;
use serde_json::json;

use crate::gate::Gate;
use crate::orchestrator::OrchCall;
use crate::orchestrator::tool_specs;
use crate::tools::Tools;
use aether_core::Settings;

fn names(tools: &[Arc<dyn Tool>]) -> Vec<String> {
    tools.iter().map(|t| t.name().to_string()).collect()
}

fn pend() -> crate::orchestrator::OrchPending {
    std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0))
}

fn gate() -> Arc<Gate> {
    let (tx, _) = mpsc::unbounded();
    Arc::new(Gate::new(
        tx,
        Default::default(),
        Arc::default(),
        Settings::default(),
        Default::default(),
    ))
}

#[test]
fn lead_tools_cover_all_specs() {
    let (tx, _rx) = mpsc::unbounded();
    let tools = lead_tools("c".into(), tx, pend());
    let specs = tool_specs();
    let specs = specs.as_array().unwrap();
    assert_eq!(tools.len(), specs.len());
    for (tool, spec) in tools.iter().zip(specs) {
        assert_eq!(tool.name(), spec["name"]);
        assert_eq!(tool.description(), spec["description"]);
        assert_eq!(tool.parameters_schema().unwrap(), spec["inputSchema"]);
    }
}

#[test]
fn lead_tool_call_round_trips() {
    let (tx, mut rx) = mpsc::unbounded::<OrchCall>();
    let answer = std::thread::spawn(move || {
        let call = futures::executor::block_on(rx.next()).unwrap();
        assert_eq!((call.lead.as_str(), call.tool.as_str()), ("chat", "read_agent"));
        assert_eq!(call.args, json!({"id": 3}));
        call.reply.send(Ok(json!({"x": 1}))).unwrap();
    });
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let out = rt.block_on(ask("chat".into(), tx, pend(), "read_agent".into(), json!({"id": 3})));
    answer.join().unwrap();
    assert_eq!(out, Ok(json!({"x": 1})));
}

#[test]
fn lead_tool_reports_dropped_reply() {
    let (tx, mut rx) = mpsc::unbounded::<OrchCall>();
    let answer = std::thread::spawn(move || drop(futures::executor::block_on(rx.next())));
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let out = rt.block_on(ask("chat".into(), tx, pend(), "list_agents".into(), json!({})));
    answer.join().unwrap();
    assert_eq!(out, Err("Aether didn't answer".to_string()));
}

#[test]
fn non_lead_chat_gets_no_lead_tools() {
    let tools = Tools::new();
    let settings = Settings::default();
    let plain = names(&tools.enabled(&settings, &[], gate(), None));
    assert!(!plain.contains(&"list_agents".to_string()));
    let (tx, _rx) = mpsc::unbounded();
    let lead = names(&tools.enabled(&settings, &[], gate(), Some(("c".into(), tx, pend()))));
    assert!(lead.contains(&"list_agents".to_string()));
}

#[test]
fn full_pending_queue_is_busy_not_queued() {
    let (tx, mut rx) = mpsc::unbounded::<OrchCall>();
    let pending = pend();
    pending.store(crate::orchestrator::MAX_PENDING, std::sync::atomic::Ordering::SeqCst);
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let out = rt.block_on(ask("chat".into(), tx, pending.clone(), "list_agents".into(), json!({})));
    assert_eq!(out, Err(crate::orchestrator::BUSY.to_string()));
    assert!(rx.try_recv().is_err(), "nothing queued");
}
