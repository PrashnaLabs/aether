use super::*;

fn turn(
    d: &mut OpencodeDriver,
    rx: &mut futures::channel::mpsc::UnboundedReceiver<Update>,
    cancel: bool,
) -> Vec<Update> {
    let start = Instant::now();
    let mut deadline = start + Duration::from_secs(120);
    let mut out = Vec::new();
    loop {
        assert!(Instant::now() < deadline, "timed out: {out:?}");
        let u = match rx.try_recv() {
            Ok(u) => u,
            Err(futures::channel::mpsc::TryRecvError::Empty) => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(_) => panic!("channel closed: {out:?}"),
        };
        println!("{:>6}ms {u:?}", start.elapsed().as_millis());
        match &u {
            Update::Ask { id, .. } => d.answer(*id, Answer::Deny),
            Update::Delta(_) if cancel && !out.iter().any(|u| matches!(u, Update::Delta(_))) => {
                println!("-- cancel");
                d.cancel();
                deadline = Instant::now() + Duration::from_secs(15);
            }
            _ => {}
        }
        let done = matches!(u, Update::Final(_));
        out.push(u);
        if done {
            return out;
        }
    }
}

fn text(updates: &[Update]) -> String {
    updates
        .iter()
        .filter_map(|u| match u {
            Update::Delta(t) | Update::Final(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<String>()
        .to_lowercase()
}

fn session(updates: &[Update]) -> Option<String> {
    updates.iter().find_map(|u| match u {
        Update::Session(id) => Some(id.clone()),
        _ => None,
    })
}

#[test]
#[ignore]
fn live_opencode_smoke() {
    let dir = std::env::temp_dir().join(format!("aether-opencode-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let opts = |resume: &str| DriverStartOptions {
        ephemeral: false,
        compaction_window: None,
        binary: String::new(),
        cwd: dir.clone(),
        model: std::env::var("AETHER_OPENCODE_SMOKE_MODEL").unwrap_or_default(),
        effort: String::new(),
        access: AccessMode::Ask,
        resume: resume.into(),
        reattach: false,
        attachments: None,
        background: Vec::new(),
        lead: None,
    };
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let mut d = OpencodeDriver::spawn(opts(""), tx);
    assert!(!d.resumed());
    println!("-- turn 1");
    d.prompt("Reply with exactly: ok".into(), vec![]);
    let first = turn(&mut d, &mut rx, false);
    let id = session(&first).expect("session id");
    assert!(id.starts_with("ses_"), "{id}");
    assert!(text(&first).contains("ok"));
    drop(d);

    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let mut d = OpencodeDriver::spawn(opts(&id), tx);
    assert!(d.resumed());
    println!("-- turn 2 (resumed {id})");
    d.prompt("Reply with exactly: again".into(), vec![]);
    let second = turn(&mut d, &mut rx, false);
    assert_eq!(session(&second).as_deref(), Some(id.as_str()));
    assert!(text(&second).contains("again"));
    println!("-- turn 3 (cancel)");
    d.prompt("Count from 1 to 300, one number per line.".into(), vec![]);
    turn(&mut d, &mut rx, true);
    println!("-- turn 4");
    d.prompt("Reply with exactly: ok".into(), vec![]);
    assert!(text(&turn(&mut d, &mut rx, false)).contains("ok"));
    drop(d);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn side_request_targets_generate_with_preamble() {
    let (path, body) = side_request("ses_1", "why?");
    assert_eq!(path, "/api/session/ses_1/generate");
    assert_eq!(
        body["prompt"],
        "Answer this side question briefly from the conversation so far. Do not use tools.\n\nwhy?"
    );
}

#[test]
fn side_reply_reads_data_text() {
    let ok = side_reply(Ok(json!({"data": {"text": "because"}})));
    assert!(matches!(ok, Update::Final(t) if t == "because"));
}

#[test]
fn side_reply_errors_on_malformed_or_http_failure() {
    assert!(matches!(side_reply(Ok(json!({"data": {}}))), Update::Error(_)));
    assert!(matches!(side_reply(Ok(Value::Null)), Update::Error(_)));
    assert!(matches!(side_reply(Err("boom".into())), Update::Error(e) if e == "boom"));
}
