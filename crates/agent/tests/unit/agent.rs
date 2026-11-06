use super::*;
use adk_rust::session::GetRequest;
use futures::StreamExt;
use std::sync::Mutex as StdMutex;

struct Fake {
    tx: UnboundedSender<Update>,
    log: Arc<StdMutex<Vec<String>>>,
    resumed: bool,
    dead: bool,
    reports: bool,
}

fn tool(id: &str, status: aether_core::ToolStatus) -> Update {
    let call = aether_core::ToolCall::new(id, "Run", Default::default());
    Update::Tool(aether_core::ToolCall { status, ..call })
}

fn log_images(log: &StdMutex<Vec<String>>, images: &[Attachment]) {
    if !images.is_empty() {
        let paths: Vec<String> = images.iter().map(|f| f.path.display().to_string()).collect();
        log.lock().unwrap().push(format!("images:{}", paths.join(",")));
    }
}

impl Driver for Fake {
    fn prompt(&mut self, text: String, images: Vec<Attachment>) {
        self.log.lock().unwrap().push(format!("prompt:{text}"));
        log_images(&self.log, &images);
        let _ = self.tx.unbounded_send(Update::Delta(text.clone()));
        if text == "boom" {
            panic!("driver died");
        }
        self.dead = text == "die";
        self.reports = text == "hold-reporter";
        if text == "ask" || text == "resolve" {
            let ask = aether_core::Ask::Approval(aether_core::Approval {
                tool: "Bash".into(),
                title: "ls".into(),
                input: "ls".into(),
                ..Default::default()
            });
            let _ = self.tx.unbounded_send(Update::Ask { id: 7, ask });
            if text == "resolve" {
                for id in [7, 99] {
                    let _ = self.tx.unbounded_send(Update::AskResolved(id));
                }
            }
        } else if text == "hold-wake" {
            let _ = self.tx.unbounded_send(Update::Wake);
            let _ = self.tx.unbounded_send(Update::Delta("after".into()));
        } else if text == "wake" {
            let _ = self.tx.unbounded_send(Update::Final(text));
            for u in [Update::Wake, Update::Delta("woke".into()), Update::Final(String::new())] {
                let _ = self.tx.unbounded_send(u);
            }
        } else if text == "late-ask" {
            let _ = self.tx.unbounded_send(Update::Final(text));
            let ask = aether_core::Ask::Approval(aether_core::Approval {
                tool: "Bash".into(),
                title: "ls".into(),
                input: "ls".into(),
                ..Default::default()
            });
            let _ = self.tx.unbounded_send(Update::Ask { id: 9, ask });
        } else if text == "late-tool" {
            let _ = self.tx.unbounded_send(Update::Final(text));
            let _ = self.tx.unbounded_send(tool("t9", aether_core::ToolStatus::Done));
            let _ = self.tx.unbounded_send(Update::Warning("w".into()));
            let _ = self.tx.unbounded_send(Update::SteerRefused("s".into()));
        } else if text == "late-error" {
            let _ = self.tx.unbounded_send(Update::Final(text));
            let _ = self.tx.unbounded_send(Update::Error("boom".into()));
        } else if text == "bg" || text == "bg-done" {
            let status = if text == "bg" {
                aether_core::ToolStatus::Background
            } else {
                aether_core::ToolStatus::Done
            };
            let _ = self.tx.unbounded_send(tool("bg1", status));
            let _ = self.tx.unbounded_send(Update::Final(text));
        } else if !text.starts_with("hold") {
            let late = text == "titled";
            let _ = self.tx.unbounded_send(Update::Final(text));
            if late {
                let _ = self.tx.unbounded_send(Update::Title("T".into()));
            }
        }
    }
    fn steer(&mut self, text: String, images: Vec<Attachment>) -> bool {
        self.log.lock().unwrap().push(format!("steer:{text}"));
        log_images(&self.log, &images);
        if text == "late" {
            // the running turn ends just before the steer reaches the driver
            let _ = self.tx.unbounded_send(Update::Final(String::new()));
            return false;
        }
        if self.reports {
            let _ = self.tx.unbounded_send(Update::Steered(format!("cli:{text}")));
            let _ = self.tx.unbounded_send(Update::Final(text));
        }
        true
    }
    fn reports_steered(&self) -> bool {
        self.reports
    }
    fn stop_tool(&mut self, id: &str) -> bool {
        self.log.lock().unwrap().push(format!("stop:{id}"));
        true
    }
    fn notice_action(&mut self, action: &str) {
        self.log.lock().unwrap().push(format!("action:{action}"));
    }
    fn set_options(&mut self, model: &str, effort: &str) -> bool {
        self.log.lock().unwrap().push(format!("options:{model}:{effort}"));
        effort != "respawn"
    }
    fn cancel(&mut self) {
        self.log.lock().unwrap().push("cancel".into());
        let _ = self.tx.unbounded_send(Update::Final(String::new()));
    }
    fn answer(&mut self, id: u64, answer: Answer) {
        let allow = answer_allows(&answer);
        self.log.lock().unwrap().push(format!("answer:{id}:{allow}"));
        let _ = self.tx.unbounded_send(Update::Final(String::new()));
    }
    fn resumed(&self) -> bool {
        self.resumed
    }
    fn alive(&self) -> bool {
        !self.dead
    }
    fn pid(&self) -> Option<u32> {
        Some(std::process::id())
    }
}

fn fake_agent(
    resumed: bool,
    idle: Duration,
) -> (Agent, Arc<StdMutex<Vec<String>>>, Arc<AtomicU64>) {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let spawns = Arc::new(AtomicU64::new(0));
    let (l, s) = (log.clone(), spawns.clone());
    let factory: Factory = Arc::new(move |_, _, tx| {
        s.fetch_add(1, Ordering::SeqCst);
        Box::new(Fake { tx, log: l.clone(), resumed, dead: false, reports: false })
    });
    (Agent::with_factory(factory, idle).unwrap(), log, spawns)
}

fn cli_settings() -> Settings {
    Settings { provider: Provider::Claude, claude_binary: "/bin/sh".into(), ..Settings::default() }
}

fn chat(effort: &str) -> ChatSession {
    ChatSession {
        key: "1".into(),
        cwd: "/tmp".into(),
        access: aether_core::AccessMode::Ask,
        effort: effort.into(),
        resume: String::new(),
        lead: false,
    }
}

fn drain(agent: &Agent, mut reply: Reply) -> Vec<Update> {
    agent.rt.block_on(async {
        let mut out = Vec::new();
        while let Some(u) = reply.updates.next().await {
            out.push(u)
        }
        out
    })
}

#[test]
fn live_session_is_reused_and_seeded_only_once() {
    let (agent, log, spawns) = fake_agent(false, Duration::from_secs(600));
    let s = cli_settings();
    let history = [(Role::User, "earlier".to_string())];
    let first =
        drain(&agent, agent.send("x", "hi".into(), vec![], &s, "", vec![], &history, &chat("")));
    assert!(matches!(first.last(), Some(Update::Final(_))));
    drain(&agent, agent.send("x", "again".into(), vec![], &s, "", vec![], &history, &chat("")));
    assert_eq!(spawns.load(Ordering::SeqCst), 1);
    let log = log.lock().unwrap();
    assert!(log[0].starts_with("prompt:") && log[0].contains("earlier") && log[0].contains("hi"));
    assert_eq!(log[1], "prompt:again");
}

#[test]
fn resumed_session_is_not_seeded() {
    let (agent, log, _) = fake_agent(true, Duration::from_secs(600));
    let history = [(Role::User, "earlier".to_string())];
    drain(
        &agent,
        agent.send("x", "hi".into(), vec![], &cli_settings(), "", vec![], &history, &chat("")),
    );
    assert_eq!(log.lock().unwrap()[0], "prompt:hi");
}

#[test]
fn access_change_respawns() {
    let (agent, _, spawns) = fake_agent(true, Duration::from_secs(600));
    drain(&agent, agent.send("x", "a".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")));
    let full = ChatSession { access: aether_core::AccessMode::Full, ..chat("") };
    drain(&agent, agent.send("x", "b".into(), vec![], &cli_settings(), "", vec![], &[], &full));
    assert_eq!(spawns.load(Ordering::SeqCst), 2);
}

#[test]
fn steer_into_a_running_turn_stays_in_its_reply() {
    let (agent, log, _) = fake_agent(true, Duration::from_secs(600));
    let s = cli_settings();
    let old = agent.send("x", "hold".into(), vec![], &s, "", vec![], &[], &chat(""));
    let fallback = agent.steer("more".into(), vec![], &s, &[], &chat(""));
    old.cancel.cancel();
    assert_eq!(
        drain(&agent, old),
        [
            Update::Delta("hold".into()),
            Update::Steered("more".into()),
            Update::Final(String::new())
        ]
    );
    assert_eq!(drain(&agent, fallback), []);
    assert_eq!(log.lock().unwrap()[1], "steer:more");
}

#[test]
fn late_steer_runs_as_its_own_reply() {
    let (agent, _, _) = fake_agent(true, Duration::from_secs(600));
    let s = cli_settings();
    let old = agent.send("x", "hold".into(), vec![], &s, "", vec![], &[], &chat(""));
    let fallback = agent.steer("late".into(), vec![], &s, &[], &chat(""));
    assert_eq!(drain(&agent, old).last(), Some(&Update::Final(String::new())));
    assert_eq!(
        drain(&agent, fallback),
        [Update::Delta("late".into()), Update::Final("late".into())]
    );
}

#[test]
fn asks_outside_a_turn_open_a_side_turn() {
    let (agent, log, _) = fake_agent(true, Duration::from_secs(600));
    let mut side = agent.side_updates().unwrap();
    drain(
        &agent,
        agent.send("x", "late-ask".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")),
    );
    let Some(SideUpdate::Turn(key, reply)) = agent.rt.block_on(side.next()) else { panic!() };
    assert_eq!(key, "1");
    let mut updates = reply.updates;
    assert!(matches!(agent.rt.block_on(updates.next()), Some(Update::Ask { .. })));
    assert!(!log.lock().unwrap().iter().any(|l| l.starts_with("answer:")), "not auto-denied");
}

#[test]
fn tool_updates_outside_a_turn_go_to_meta() {
    let (agent, _, _) = fake_agent(true, Duration::from_secs(600));
    let mut side = agent.side_updates().unwrap();
    drain(
        &agent,
        agent.send("x", "late-tool".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")),
    );
    let Some(SideUpdate::Meta(_, update)) = agent.rt.block_on(side.next()) else { panic!() };
    let Update::Tool(call) = *update else { panic!() };
    assert_eq!(call.id, "t9");
    let Some(SideUpdate::Meta(_, update)) = agent.rt.block_on(side.next()) else { panic!() };
    assert_eq!(*update, Update::Warning("w".into()));
    let Some(SideUpdate::Meta(_, update)) = agent.rt.block_on(side.next()) else { panic!() };
    assert_eq!(*update, Update::SteerRefused("s".into()));
}

#[test]
fn errors_outside_a_turn_arrive_as_warnings() {
    let (agent, _, _) = fake_agent(true, Duration::from_secs(600));
    let mut side = agent.side_updates().unwrap();
    drain(
        &agent,
        agent.send("x", "late-error".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")),
    );
    let Some(SideUpdate::Meta(_, update)) = agent.rt.block_on(side.next()) else { panic!() };
    assert_eq!(*update, Update::Warning("boom".into()));
}

/// A driver with no turn running: like the real ones, cancel does nothing.
struct Idle(UnboundedSender<Update>);

impl Driver for Idle {
    fn prompt(&mut self, text: String, _: Vec<Attachment>) {
        if text.starts_with("hold") {
            let _ = self.0.unbounded_send(Update::Delta(text));
            return;
        }
        let _ = self.0.unbounded_send(Update::Final(text));
        let _ = self.0.unbounded_send(Update::Delta("late".into()));
    }
    fn steer(&mut self, _: String, _: Vec<Attachment>) -> bool {
        false
    }
    fn cancel(&mut self) {}
    fn resumed(&self) -> bool {
        true
    }
}

#[test]
fn stopping_a_side_turn_the_driver_never_started_ends_it() {
    let factory: Factory = Arc::new(|_, _, tx| Box::new(Idle(tx)));
    let agent = Agent::with_factory(factory, Duration::from_secs(600)).unwrap();
    let mut side = agent.side_updates().unwrap();
    drain(&agent, agent.send("x", "a".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")));
    let Some(SideUpdate::Turn(_, reply)) = agent.rt.block_on(side.next()) else { panic!() };
    reply.cancel.cancel();
    let (done, got) = std::sync::mpsc::channel();
    std::thread::spawn(move || done.send(futures::executor::block_on(reply.updates.collect())));
    let updates: Vec<Update> =
        got.recv_timeout(Duration::from_secs(5)).expect("side turn never ended");
    assert_eq!(updates, [Update::Delta("late".into()), Update::Final(String::new())]);
}

#[test]
fn a_steer_into_an_orphan_side_turn_is_not_ended_by_its_stop() {
    let factory: Factory = Arc::new(|_, _, tx| Box::new(Idle(tx)));
    let agent = Agent::with_factory(factory, Duration::from_secs(600)).unwrap();
    let mut side = agent.side_updates().unwrap();
    let s = cli_settings();
    drain(&agent, agent.send("x", "a".into(), vec![], &s, "", vec![], &[], &chat("")));
    let Some(SideUpdate::Turn(..)) = agent.rt.block_on(side.next()) else { panic!() };
    let mut real = agent.steer("hold".into(), vec![], &s, &[], &chat(""));
    assert_eq!(agent.rt.block_on(real.updates.next()), Some(Update::Delta("hold".into())));
    real.cancel.cancel();
    std::thread::sleep(Duration::from_millis(200));
    assert!(real.updates.try_recv().is_err(), "a stale orphan flag ended the real turn");
}

#[test]
fn a_stop_the_driver_cannot_do_warns_in_the_turn() {
    let factory: Factory = Arc::new(|_, _, tx| Box::new(Idle(tx)));
    let agent = Agent::with_factory(factory, Duration::from_secs(600)).unwrap();
    let mut reply =
        agent.send("x", "hold".into(), vec![], &cli_settings(), "", vec![], &[], &chat(""));
    assert_eq!(agent.rt.block_on(reply.updates.next()), Some(Update::Delta("hold".into())));
    agent.stop_tool("1", "bg1");
    agent.stop_tool("nope", "bg1");
    assert_eq!(
        agent.rt.block_on(reply.updates.next()),
        Some(Update::Warning("This agent can't stop background work from Aether".into()))
    );
}

#[test]
fn a_stopped_turn_releases_its_holds() {
    let (agent, _, _) = fake_agent(true, Duration::ZERO);
    let reply = agent.send("x", "ask".into(), vec![], &cli_settings(), "", vec![], &[], &chat(""));
    reply.cancel.cancel();
    drain(&agent, reply);
    agent.reap();
    assert!(agent.live.lock().unwrap().is_empty());
}

#[test]
fn an_open_ask_keeps_the_session_until_answered() {
    let (agent, _, _) = fake_agent(true, Duration::ZERO);
    let mut reply =
        agent.send("x", "ask".into(), vec![], &cli_settings(), "", vec![], &[], &chat(""));
    agent.rt.block_on(reply.updates.next());
    let Some(Update::Ask { id, .. }) = agent.rt.block_on(reply.updates.next()) else { panic!() };
    agent.live.lock().unwrap()["1"].turn.lock().unwrap().take();
    agent.reap();
    assert_eq!(agent.live.lock().unwrap().len(), 1);
    agent.answer(id, Answer::Deny);
    agent.reap();
    assert!(agent.live.lock().unwrap().is_empty());
}

#[test]
fn resolved_asks_reach_the_app_by_global_id() {
    let (agent, _, _) = fake_agent(true, Duration::from_secs(600));
    let mut reply =
        agent.send("x", "resolve".into(), vec![], &cli_settings(), "", vec![], &[], &chat(""));
    agent.rt.block_on(reply.updates.next());
    let Some(Update::Ask { id, .. }) = agent.rt.block_on(reply.updates.next()) else { panic!() };
    assert_eq!(agent.rt.block_on(reply.updates.next()), Some(Update::AskResolved(id)));
    assert!(agent.cli_asks.lock().unwrap().is_empty());
    assert!(agent.live.lock().unwrap()["1"].holds.lock().unwrap().is_empty());
    reply.cancel.cancel();
    assert_eq!(drain(&agent, reply), [Update::Final(String::new())]);
}

#[test]
fn a_wake_during_a_turn_stays_in_it() {
    let (agent, _, _) = fake_agent(true, Duration::from_secs(600));
    let mut side = agent.side_updates().unwrap();
    let reply =
        agent.send("x", "hold-wake".into(), vec![], &cli_settings(), "", vec![], &[], &chat(""));
    reply.cancel.cancel();
    assert_eq!(
        drain(&agent, reply),
        [
            Update::Delta("hold-wake".into()),
            Update::Delta("after".into()),
            Update::Final(String::new())
        ]
    );
    assert!(side.try_recv().is_err());
}

#[test]
fn reaper_keeps_sessions_with_background_work() {
    let (agent, _, _) = fake_agent(true, Duration::ZERO);
    drain(
        &agent,
        agent.send("x", "bg".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")),
    );
    agent.reap();
    assert_eq!(agent.live.lock().unwrap().len(), 1);
    drain(
        &agent,
        agent.send("x", "bg-done".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")),
    );
    agent.reap();
    assert!(agent.live.lock().unwrap().is_empty());
}

#[test]
fn health_reports_pid_and_liveness() {
    let (agent, _, _) = fake_agent(true, Duration::from_secs(600));
    assert!(agent.health("1").is_none());
    drain(&agent, agent.send("x", "a".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")));
    let h = agent.health("1").unwrap();
    assert_eq!((h.pid, h.alive), (Some(std::process::id()), true));
}

#[test]
fn approvals_reach_the_driver_with_its_own_id() {
    let (agent, log, _) = fake_agent(true, Duration::from_secs(600));
    let mut reply =
        agent.send("x", "ask".into(), vec![], &cli_settings(), "", vec![], &[], &chat(""));
    agent.rt.block_on(reply.updates.next());
    let Some(Update::Ask { id, .. }) = agent.rt.block_on(reply.updates.next()) else { panic!() };
    agent.answer(id, Answer::Deny);
    drain(&agent, reply);
    assert_eq!(log.lock().unwrap()[1], "answer:7:false");
}

#[test]
fn updates_sent_while_the_driver_starts_reach_the_first_reply() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let factory: Factory = Arc::new(move |_, _, tx: UnboundedSender<Update>| {
        let _ = tx.unbounded_send(Update::Session("s".into()));
        Box::new(Fake { tx, log: log.clone(), resumed: true, dead: false, reports: false })
    });
    let agent = Agent::with_factory(factory, Duration::from_secs(600)).unwrap();
    let out = drain(
        &agent,
        agent.send("x", "hi".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")),
    );
    assert_eq!(out[0], Update::Session("s".into()));
}

#[test]
fn steer_while_idle_prompts_and_respawns_on_new_options() {
    let (agent, log, spawns) = fake_agent(true, Duration::from_secs(600));
    drain(&agent, agent.send("x", "a".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")));
    drain(&agent, agent.steer("b".into(), vec![], &cli_settings(), &[], &chat("")));
    assert_eq!(spawns.load(Ordering::SeqCst), 1);
    let out =
        drain(&agent, agent.steer("c".into(), vec![], &cli_settings(), &[], &chat("respawn")));
    assert_eq!(out.last(), Some(&Update::Final("c".into())));
    assert_eq!(spawns.load(Ordering::SeqCst), 2);
    assert_eq!(*log.lock().unwrap(), ["prompt:a", "prompt:b", "options::respawn", "prompt:c"]);
}

#[test]
fn stale_asks_do_not_reach_a_reopened_session() {
    let (agent, log, _) = fake_agent(true, Duration::from_secs(600));
    let mut reply =
        agent.send("x", "ask".into(), vec![], &cli_settings(), "", vec![], &[], &chat(""));
    agent.rt.block_on(reply.updates.next());
    let Some(Update::Ask { id, .. }) = agent.rt.block_on(reply.updates.next()) else { panic!() };
    agent.close_session("1");
    drop(reply);
    drain(&agent, agent.send("x", "a".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")));
    agent.answer(id, Answer::Deny);
    drain(&agent, agent.send("x", "b".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")));
    assert_eq!(*log.lock().unwrap(), ["prompt:ask", "prompt:a", "prompt:b"]);
}

#[test]
fn a_dead_driver_is_respawned_on_the_next_send() {
    let (agent, _, spawns) = fake_agent(true, Duration::from_secs(600));
    drain(
        &agent,
        agent.send("x", "boom".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")),
    );
    let out = drain(
        &agent,
        agent.send("x", "a".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")),
    );
    assert_eq!(out.last(), Some(&Update::Final("a".into())));
    assert_eq!(spawns.load(Ordering::SeqCst), 2);
}

#[test]
fn a_driver_reporting_dead_is_rebuilt_with_the_latest_resume_id() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let l = log.clone();
    let factory: Factory =
        Arc::new(move |_, opts: DriverStartOptions, tx: UnboundedSender<Update>| {
            l.lock().unwrap().push(format!("spawn:{}", opts.resume));
            Box::new(Fake { tx, log: l.clone(), resumed: true, dead: false, reports: false })
        });
    let agent = Agent::with_factory(factory, Duration::from_secs(600)).unwrap();
    drain(
        &agent,
        agent.send("x", "die".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")),
    );
    let resumed = ChatSession { resume: "r1".into(), ..chat("") };
    let out = drain(
        &agent,
        agent.send("x", "a".into(), vec![], &cli_settings(), "", vec![], &[], &resumed),
    );
    assert_eq!(out.last(), Some(&Update::Final("a".into())));
    assert_eq!(*log.lock().unwrap(), ["spawn:", "prompt:die", "spawn:r1", "prompt:a"]);
}

#[test]
fn live_sessions_tracks_open_sessions() {
    let (agent, _, _) = fake_agent(true, Duration::from_secs(600));
    assert!(agent.live_sessions().is_empty());
    drain(&agent, agent.send("x", "a".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")));
    let key = chat("").key;
    assert!(agent.live_sessions().contains(&key));
    agent.close_session(&key);
    assert!(!agent.live_sessions().contains(&key));
}

#[test]
fn idle_sessions_are_reaped_but_running_ones_kept() {
    let (agent, _, spawns) = fake_agent(true, Duration::ZERO);
    let mut running =
        agent.send("x", "hold".into(), vec![], &cli_settings(), "", vec![], &[], &chat(""));
    agent.rt.block_on(running.updates.next());
    agent.reap();
    assert_eq!(agent.live.lock().unwrap().len(), 1);
    running.cancel.cancel();
    drain(&agent, running);
    agent.reap();
    assert!(agent.live.lock().unwrap().is_empty());
    drain(&agent, agent.send("x", "a".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")));
    assert_eq!(spawns.load(Ordering::SeqCst), 2);
}

#[test]
fn forked_sessions_start_with_their_history() {
    let agent = Agent::new().unwrap();
    let history = [(Role::User, "Hi".to_string()), (Role::Assistant, "Hello!".to_string())];
    agent.fork_session("fork-1", &history).unwrap();
    let session = agent
        .rt
        .block_on(agent.sessions.get(GetRequest {
            app_name: APP.into(),
            user_id: USER.into(),
            session_id: "fork-1".into(),
            num_recent_events: None,
            after: None,
        }))
        .unwrap();
    let events = session.events().all();
    let authors: Vec<&str> = events.iter().map(|e| e.author.as_str()).collect();
    assert_eq!(authors, [USER, AGENT]);
}

fn missing_chat() -> ChatSession {
    ChatSession {
        key: "m".into(),
        cwd: "/tmp".into(),
        access: aether_core::AccessMode::Ask,
        effort: String::new(),
        resume: String::new(),
        lead: false,
    }
}

#[test]
fn cli_send_with_missing_binary_errors_not_panics() {
    let agent = Agent::new().unwrap();
    let settings = Settings {
        provider: Provider::Claude,
        claude_binary: "/definitely/not/here-xyz".into(),
        ..Settings::default()
    };
    let Reply { mut updates, .. } =
        agent.send("cli-missing", "hi".into(), vec![], &settings, "", vec![], &[], &missing_chat());
    let first = agent.rt.block_on(updates.next()).unwrap();
    assert!(matches!(first, Update::Error(ref e) if e.contains("claude")), "got {first:?}");
    assert!(agent.rt.block_on(updates.next()).is_none());
}

#[test]
fn opencode_send_with_missing_binary_and_no_hub_errors_and_closes() {
    if opencode::service_reachable() {
        return;
    }
    let agent = Agent::new().unwrap();
    let settings = Settings {
        provider: Provider::OpenCode,
        opencode_binary: "/definitely/not/here-xyz".into(),
        ..Settings::default()
    };
    let Reply { mut updates, .. } = agent.send(
        "opencode-missing",
        "hi".into(),
        vec![],
        &settings,
        "",
        vec![],
        &[],
        &missing_chat(),
    );
    let first = agent.rt.block_on(updates.next()).unwrap();
    assert!(matches!(first, Update::Error(ref e) if e.contains("opencode")), "got {first:?}");
    assert!(agent.rt.block_on(updates.next()).is_none());
}

#[test]
fn cli_prompt_carries_the_transcript_as_context() {
    let history = [(Role::User, "hi".to_string()), (Role::Assistant, "yo".to_string())];
    let prompt = context_prompt(&history, "again");
    assert!(prompt.contains("hi") && prompt.contains("yo") && prompt.contains("again"));
    assert_eq!(context_prompt(&[], "bare"), "bare");
}

fn png(path: &str) -> Attachment {
    Attachment { name: "a.png".into(), path: path.into(), mime: "image/png".into(), size: 1 }
}

#[test]
fn cli_prompt_lists_files_the_agent_cannot_take_inline() {
    let (agent, log, _) = fake_agent(true, Duration::from_secs(600));
    let s = Settings {
        provider: Provider::Antigravity,
        antigravity_binary: "/bin/sh".into(),
        ..Settings::default()
    };
    let files = vec![png("/x/a.png")];
    drain(&agent, agent.send("x", "hi".into(), files, &s, "", vec![], &[], &chat("")));
    assert_eq!(*log.lock().unwrap(), ["prompt:hi\n\nAttached files:\n- /x/a.png (image/png)"]);
}

#[test]
fn claude_takes_images_inline_on_prompt_and_steer() {
    let (agent, log, _) = fake_agent(true, Duration::from_secs(600));
    let s = cli_settings();
    let mut old =
        agent.send("x", "hold".into(), vec![png("/x/a.png")], &s, "", vec![], &[], &chat(""));
    agent.rt.block_on(old.updates.next());
    drain(&agent, agent.steer("more".into(), vec![png("/x/b.png")], &s, &[], &chat("")));
    assert_eq!(
        *log.lock().unwrap(),
        ["prompt:hold", "images:/x/a.png", "steer:more", "images:/x/b.png"]
    );
}

#[test]
fn cli_answers_resume_or_stop_the_driver() {
    assert!(answer_allows(&Answer::Approve { input: "ls".into(), always: false }));
    assert!(answer_allows(&Answer::Reply("yes".into())));
    assert!(answer_allows(&Answer::Plan { approved: true, text: String::new() }));
    assert!(!answer_allows(&Answer::Deny));
    assert!(!answer_allows(&Answer::DenyWith("why".into())));
    assert!(answer_allows(&Answer::Pick("rule-0".into())));
    assert!(!answer_allows(&Answer::Plan { approved: false, text: "no".into() }));
}

#[test]
fn cli_cancel_keeps_partial_text_and_settles_without_error() {
    let mut reply = aether_core::Message::assistant();
    reply.apply(aether_core::Update::Delta("partial".into()));
    reply.apply(aether_core::Update::Final(String::new()));
    reply.finish();
    assert_eq!(reply.text, "partial");
    assert!(!reply.failed);
}

#[test]
fn a_title_sent_after_the_turn_reaches_the_side_channel() {
    let (agent, _, _) = fake_agent(true, Duration::from_secs(600));
    let mut side = agent.side_updates().unwrap();
    assert!(agent.side_updates().is_none());
    let out = drain(
        &agent,
        agent.send("x", "titled".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")),
    );
    assert_eq!(out.last(), Some(&Update::Final("titled".into())));
    let Some(SideUpdate::Meta(key, update)) = agent.rt.block_on(side.next()) else { panic!() };
    assert_eq!((key.as_str(), *update), ("1", Update::Title("T".into())));
}

#[test]
fn a_turn_the_cli_starts_itself_reaches_the_app_as_a_new_reply() {
    let (agent, _, _) = fake_agent(true, Duration::from_secs(600));
    let mut side = agent.side_updates().unwrap();
    let out = drain(
        &agent,
        agent.send("x", "wake".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")),
    );
    assert_eq!(out, [Update::Delta("wake".into()), Update::Final("wake".into())]);
    let Some(SideUpdate::Turn(key, reply)) = agent.rt.block_on(side.next()) else { panic!() };
    assert_eq!(key, "1");
    assert_eq!(drain(&agent, reply), [Update::Delta("woke".into()), Update::Final(String::new())]);
}

#[test]
fn a_steer_that_misses_the_turn_streams_into_its_own_reply() {
    let (agent, log, _) = fake_agent(true, Duration::from_secs(600));
    let s = cli_settings();
    let mut old = agent.send("x", "hold".into(), vec![], &s, "", vec![], &[], &chat(""));
    assert_eq!(agent.rt.block_on(old.updates.next()), Some(Update::Delta("hold".into())));
    let new = agent.steer("late".into(), vec![], &s, &[], &chat(""));
    assert_eq!(agent.rt.block_on(old.updates.next()), Some(Update::Final(String::new())));
    assert_eq!(agent.rt.block_on(old.updates.next()), None);
    assert_eq!(drain(&agent, new), [Update::Delta("late".into()), Update::Final("late".into())]);
    assert_eq!(*log.lock().unwrap(), ["prompt:hold", "steer:late", "prompt:late"]);
}

#[test]
fn model_and_effort_switch_in_place_when_the_driver_can() {
    let (agent, log, spawns) = fake_agent(true, Duration::from_secs(600));
    let s = cli_settings();
    drain(&agent, agent.send("x", "a".into(), vec![], &s, "", vec![], &[], &chat("")));
    drain(&agent, agent.send("x", "b".into(), vec![], &s, "", vec![], &[], &chat("high")));
    assert_eq!(spawns.load(Ordering::SeqCst), 1);
    drain(&agent, agent.send("x", "c".into(), vec![], &s, "", vec![], &[], &chat("respawn")));
    assert_eq!(spawns.load(Ordering::SeqCst), 2);
    let log = log.lock().unwrap();
    assert_eq!(log[1], "options::high");
    assert_eq!(log[3], "options::respawn");
}

#[test]
fn attach_spawns_without_prompting_and_driver_events_open_a_side_turn() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let spawns = Arc::new(AtomicU64::new(0));
    let (l, s) = (log.clone(), spawns.clone());
    let factory: Factory = Arc::new(move |_, _, tx: UnboundedSender<Update>| {
        s.fetch_add(1, Ordering::SeqCst);
        let _ = tx.unbounded_send(Update::Wake);
        let _ = tx.unbounded_send(Update::Delta("resumed".into()));
        Box::new(Fake { tx, log: l.clone(), resumed: true, dead: false, reports: false })
    });
    let agent = Agent::with_factory(factory, Duration::from_secs(600)).unwrap();
    let mut side = agent.side_updates().unwrap();
    agent.attach(&cli_settings(), &chat(""), vec![]);
    agent.attach(&cli_settings(), &chat(""), vec![]);
    let Some(SideUpdate::Turn(key, mut reply)) = agent.rt.block_on(side.next()) else { panic!() };
    assert_eq!(key, "1");
    assert_eq!(agent.rt.block_on(reply.updates.next()), Some(Update::Delta("resumed".into())));
    assert_eq!(spawns.load(Ordering::SeqCst), 1);
    assert!(log.lock().unwrap().is_empty(), "no prompt sent");
}

#[test]
fn only_the_first_driver_of_an_attach_reattaches() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let l = log.clone();
    let factory: Factory =
        Arc::new(move |_, opts: DriverStartOptions, tx: UnboundedSender<Update>| {
            let bg = opts.background.join(",");
            l.lock().unwrap().push(format!("spawn:{}:{bg}", opts.reattach));
            Box::new(Fake { tx, log: l.clone(), resumed: true, dead: false, reports: false })
        });
    let agent = Agent::with_factory(factory, Duration::from_secs(600)).unwrap();
    let s = cli_settings();
    agent.attach(&s, &chat(""), vec!["bg1".into()]);
    drain(&agent, agent.send("x", "die".into(), vec![], &s, "", vec![], &[], &chat("")));
    drain(&agent, agent.send("x", "a".into(), vec![], &s, "", vec![], &[], &chat("")));
    let other = ChatSession { key: "2".into(), ..chat("") };
    drain(&agent, agent.send("x", "b".into(), vec![], &s, "", vec![], &[], &other));
    assert_eq!(
        *log.lock().unwrap(),
        ["spawn:true:bg1", "prompt:die", "spawn:false:", "prompt:a", "spawn:false:", "prompt:b"]
    );
}

#[test]
fn a_prompt_that_beats_the_attach_drops_its_settle() {
    let log = Arc::new(StdMutex::new(Vec::new()));
    let l = log.clone();
    let (open, gate) = std::sync::mpsc::channel::<()>();
    let gate = StdMutex::new(gate);
    let factory: Factory =
        Arc::new(move |_, opts: DriverStartOptions, tx: UnboundedSender<Update>| {
            if opts.reattach {
                gate.lock().unwrap().recv().unwrap();
                let notice = aether_core::Notice {
                    id: "reattach".into(),
                    kind: NoticeKind::Info,
                    text: "finished".into(),
                    state: Default::default(),
                    until_ms: None,
                    url: None,
                    detail: None,
                };
                for u in [Update::Wake, Update::Notice(notice), Update::Final(String::new())] {
                    let _ = tx.unbounded_send(u);
                }
            }
            Box::new(Fake { tx, log: l.clone(), resumed: true, dead: false, reports: false })
        });
    let agent = Agent::with_factory(factory, Duration::from_secs(600)).unwrap();
    let mut side = agent.side_updates().unwrap();
    agent.attach(&cli_settings(), &chat(""), vec![]);
    let reply = agent.send("x", "hi".into(), vec![], &cli_settings(), "", vec![], &[], &chat(""));
    open.send(()).unwrap();
    assert_eq!(drain(&agent, reply), [Update::Delta("hi".into()), Update::Final("hi".into())]);
    assert!(side.try_recv().is_err());
}

#[test]
fn drivers_that_report_steers_are_not_marked_twice() {
    let (agent, _, _) = fake_agent(true, Duration::from_secs(600));
    let mut side = agent.side_updates().unwrap();
    let s = cli_settings();
    let old = agent.send("x", "hold-reporter".into(), vec![], &s, "", vec![], &[], &chat(""));
    agent.steer("more".into(), vec![], &s, &[], &chat(""));
    assert_eq!(
        drain(&agent, old),
        [
            Update::Delta("hold-reporter".into()),
            Update::Steered("cli:more".into()),
            Update::Final("more".into())
        ]
    );
    drain(&agent, agent.send("x", "b".into(), vec![], &s, "", vec![], &[], &chat("")));
    assert!(side.try_recv().is_err(), "a second Steered opened a side turn");
}

#[test]
fn stop_tool_and_notice_actions_reach_the_driver_in_order() {
    let (agent, log, _) = fake_agent(true, Duration::from_secs(600));
    drain(&agent, agent.send("x", "a".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")));
    agent.stop_tool("1", "bg1");
    agent.notice_action("1", "trust-project");
    drain(&agent, agent.send("x", "b".into(), vec![], &cli_settings(), "", vec![], &[], &chat("")));
    let log = log.lock().unwrap();
    let at = |l: &str| log.iter().position(|x| x == l).unwrap();
    assert!(
        at("stop:bg1") < at("action:trust-project") && at("action:trust-project") < at("prompt:b")
    );
}

#[test]
fn cli_sessions_get_their_chat_attachment_folder() {
    let (opts, _) = session_options(&cli_settings(), &chat(""), None);
    assert_eq!(opts.attachments, aether_core::attachments_dir().map(|d| d.join("1")));
}

#[test]
fn lead_session_gets_lead_tools_and_stable_token() {
    let (agent, _, _) = fake_agent(false, Duration::from_secs(600));
    let t = agent.lead_token("9");
    assert_eq!(t, agent.lead_token("9"));
    assert_eq!(t.len(), 32);
    assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    let settings = Settings::default();
    let on = ChatSession { key: "9".into(), lead: true, ..chat("") };
    let (opts, key_on) = session_options(&settings, &on, agent.lead_tools(&on));
    let want = aether_driver::LeadTools {
        token: t,
        sock: crate::orchestrator::socket_path().unwrap(),
        exe: std::env::current_exe().unwrap(),
        specs: crate::orchestrator::tool_specs(),
    };
    assert_eq!(opts.lead, Some(want));
    let off = ChatSession { key: "9".into(), ..chat("") };
    let (opts, key_off) = session_options(&settings, &off, agent.lead_tools(&off));
    assert_eq!(opts.lead, None);
    assert_ne!(key_on, key_off);
}
