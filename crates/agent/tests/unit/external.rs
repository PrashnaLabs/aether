use super::ExternalState::{Idle, Waiting, Working};
use super::*;

const PS: &str = "    1     0 01-01:16:52 Tue Oct 13 20:45:09 2026     /sbin/launchd
 6414     1 01-01:05:10 Tue Oct 13 20:47:20 2026     /Users/x/.opencode/bin/opencode serve --service
 7700     1 01-01:06:00 Tue Oct 13 20:46:30 2026     login -pf x
 7707  7700 01-01:05:00 Tue Oct 13 20:47:30 2026     -zsh
10537  7707 01-01:03:35 Tue Oct 13 20:48:55 2026     opencode
 7728  7700    06:00:00 Wed Oct  4 16:00:00 2026     -zsh
90078  7728    05:50:07 Wed Oct  4 16:02:23 2026     /Users/x/.local/bin/claude
99016     1    01:31:00 Wed Oct 14 20:21:40 2026     /Applications/Google Chrome.app/Contents/MacOS/Google Chrome
99042 99016    01:30:40 Wed Oct 14 20:21:50 2026     /Users/x/.local/bin/claude --chrome-native-host
  500  7707    10:00 Wed Oct 14 21:40:00 2026     target/debug/aether
  501   500    09:00 Wed Oct 14 21:41:00 2026     claude -p --input-format stream-json
  502   500    08:00 Wed Oct 14 21:42:00 2026     sh -c pi
  503   502    08:00 Wed Oct 14 21:42:00 2026     pi
  504   500    08:00 Wed Oct 14 21:42:00 2026     pi-rpc
 2349     1 01-01:13:59 Tue Oct 13 20:47:14 2026     /opt/homebrew/bin/tmux new-session -d -s main
 3000  2349    02:00 Wed Oct 14 21:49:00 2026     claude
 4000     1    03:00 Wed Oct 14 21:48:00 2026     /Applications/Visual Studio Code.app/Contents/MacOS/Electron --type=x
 4001  4000    02:30 Wed Oct 14 21:48:30 2026     node /x/pi-coding-agent/dist/cli.js
 4002  4000    02:30 Wed Oct 14 21:48:30 2026     node /x/pi-coding-agent/dist/cli.js --mode rpc
";

#[test]
fn ps_keeps_tuis_and_drops_helpers_print_and_own_children() {
    let rows = parse_ps(PS);
    let claude = rows.iter().find(|r| r.pid == 90078).unwrap();
    assert_eq!(
        (claude.ppid, claude.etime.as_str(), claude.started.as_str(), claude.command.as_str()),
        (7728, "05:50:07", "Wed Oct  4 16:02:23 2026", "/Users/x/.local/bin/claude")
    );
    let kept: Vec<(Provider, u32, String)> =
        agent_rows(&rows, 500).into_iter().map(|(p, r, origin)| (p, r.pid, origin)).collect();
    assert_eq!(
        kept,
        [
            (Provider::OpenCode, 10537, "Terminal".into()),
            (Provider::Claude, 90078, "Terminal".into()),
            (Provider::Claude, 3000, "tmux".into()),
            (Provider::Pi, 4001, "Visual Studio Code".into()),
        ]
    );
}

#[test]
fn lsof_cwd_maps_pids_to_dirs() {
    let cwds = parse_lsof_cwd("p90078\nfcwd\nn/Users/x/a\np10537\nfcwd\nn/Users/x/b\n");
    assert_eq!(
        cwds,
        HashMap::from([(90078, PathBuf::from("/Users/x/a")), (10537, PathBuf::from("/Users/x/b"))])
    );
}

#[test]
fn claude_registry_maps_status() {
    let waiting = r#"{"pid":1,"sessionId":"s1","cwd":"/a","name":"Fix bug","status":"waiting","waitingFor":"input needed","tmux":"aether:@11.%13"}"#;
    assert_eq!(
        claude_entry(waiting, 1),
        Some((
            "s1".into(),
            "/a".into(),
            "Fix bug".into(),
            Waiting("input needed".into()),
            Some("aether:@11.%13".into())
        ))
    );
    assert_eq!(claude_entry(waiting, 2), None);
    let busy = claude_entry(r#"{"pid":1,"sessionId":"s1","cwd":"/a","status":"busy"}"#, 1).unwrap();
    assert_eq!((busy.3, busy.4), (Working, None));
    assert_eq!(
        claude_entry(r#"{"pid":1,"sessionId":"s1","cwd":"/a","status":"idle"}"#, 1).unwrap().3,
        Idle
    );
    assert_eq!(claude_entry("not json {", 1), None);
}

#[test]
fn pi_tail_state() {
    let stop = "{\"type\":\"session\"}\n{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"stop\"}}\n";
    assert_eq!(pi_state(stop, 10), Idle);
    assert_eq!(pi_state(stop, 1), Working);
    let tool = r#"{"type":"message","message":{"role":"assistant","stopReason":"toolUse"}}"#;
    assert_eq!(pi_state(tool, 10), Working);
    let result = format!("{tool}\n{}", r#"{"type":"message","message":{"role":"toolResult"}}"#);
    assert_eq!(pi_state(&result, 10), Working);
    let aborted = format!(
        "partial line\"}}\n{}\n{{\"type\":\"context_edit\"}}",
        r#"{"type":"message","message":{"role":"assistant","stopReason":"aborted"}}"#
    );
    assert_eq!(pi_state(&aborted, 10), Idle);
    assert_eq!(
        pi_state("{\"type\":\"session\"}\n{\"type\":\"message\",\"message\":{\"role\":\"assi", 10),
        Working
    );
    assert_eq!(etime_secs("01-02:03:04"), 93784);
    assert_eq!(etime_secs("01:46"), 106);
}

#[test]
fn opencode_states_from_api() {
    let sessions = r#"{"data":[
            {"id":"ses_a","title":"T","location":{"directory":"/a"},"time":{"updated":1}},
            {"id":"ses_b","title":"U","location":{"directory":"/b"},"time":{"updated":2}},
            {"id":"ses_c","parentID":"ses_a","title":"child","location":{"directory":"/a"},"time":{"updated":3}}]}"#;
    let active = r#"{"data":{"ses_a":{}}}"#;
    let permissions = r#"{"data":[{"id":"per_1","sessionID":"ses_b"}]}"#;
    assert_eq!(
        opencode_states(sessions, active, permissions),
        [
            ("ses_b".into(), "/b".into(), "U".into(), Waiting("approval".into()), Some(2)),
            ("ses_a".into(), "/a".into(), "T".into(), Working, Some(1)),
        ]
    );
}

fn agent(provider: Provider, cwd: &str, session: &str) -> ExternalAgent {
    ExternalAgent {
        provider,
        pid: Some(1),
        started: "s".into(),
        elapsed: "01:00".into(),
        cwd: cwd.into(),
        session: session.into(),
        title: String::new(),
        state: Idle,
        exact: false,
        origin: "Terminal".into(),
    }
}

#[test]
fn merge_matches_opencode_tuis_and_drops_own_sessions() {
    let own = HashSet::from(["own".to_string(), "ses_own".to_string(), String::new()]);
    let agents = vec![
        agent(Provider::Claude, "/x/mine", "own"),
        agent(Provider::Claude, "/x/theirs", "s2"),
        agent(Provider::OpenCode, "/a", ""),
        agent(Provider::OpenCode, "/c", ""),
        agent(Provider::OpenCode, "/d", ""),
    ];
    // TUIs are 60 s old at now = 1_000_000 ms, so they started at ~939_000
    let sessions = vec![
        ("ses_own".into(), "/c".into(), "aether".into(), Working, None),
        ("ses_a".into(), "/a".into(), "T".into(), Working, Some(990_000)),
        ("ses_b".into(), "/b".into(), String::new(), Waiting("approval".into()), None),
        ("ses_old".into(), "/a".into(), "old".into(), Idle, Some(500_000)),
        ("ses_stale".into(), "/d".into(), "stale".into(), Idle, Some(500_000)),
    ];
    let got = merge(agents.clone(), Some(sessions), &own, 1_000_000);
    let summary: Vec<_> = got
        .iter()
        .map(|a| (a.pid, a.session.as_str(), a.title.as_str(), a.state.clone(), a.exact))
        .collect();
    assert_eq!(
        summary,
        [
            (Some(1), "own", "mine", Idle, false),
            (Some(1), "s2", "theirs", Idle, false),
            (Some(1), "ses_a", "T", Working, true),
            (Some(1), "", "c", Idle, true),
            (Some(1), "", "d", Idle, true),
            (None, "ses_b", "b", Waiting("approval".into()), true),
        ]
    );
    assert_eq!(got[5].origin, "OpenCode service");
    let lost = vec![agent(Provider::OpenCode, "", "")];
    let got = merge(
        lost,
        Some(vec![("ses_e".into(), "".into(), String::new(), Idle, None)]),
        &own,
        1_000_000,
    );
    assert_eq!((got.len(), got[0].session.as_str()), (1, ""));
    let offline = merge(agents, None, &own, 1_000_000);
    assert_eq!(offline.iter().filter(|a| a.provider == Provider::OpenCode && !a.exact).count(), 3);
}

#[test]
fn pi_session_arg_parses_path_flags() {
    let want = Some(PathBuf::from("/x/y.jsonl"));
    assert_eq!(pi_session_arg("pi --session /x/y.jsonl foo"), want.clone());
    assert_eq!(pi_session_arg("pi -s /x/y.jsonl"), want.clone());
    assert_eq!(
        pi_session_arg("node /x/pi-coding-agent/dist/cli.js --session /x/y.jsonl"),
        want.clone()
    );
    assert_eq!(pi_session_arg("pi --session=/x/y.jsonl"), want);
    assert_eq!(
        pi_session_arg("pi --session relative.jsonl"),
        Some(PathBuf::from("relative.jsonl"))
    );
    assert_eq!(pi_session_arg("pi"), None);
    assert_eq!(pi_session_arg("pi --resume"), None);
    assert_eq!(pi_session_arg("pi --sessionx /x/y.jsonl"), None);
    assert_eq!(pi_session_arg("pi --session abc"), None);
}

#[test]
fn pi_session_id_parses_bare_ids() {
    assert_eq!(pi_session_id("pi --session abc"), Some("abc".into()));
    assert_eq!(pi_session_id("pi --session=/x/y.jsonl"), None);
    assert_eq!(pi_session_id("pi --session-id abc"), Some("abc".into()));
    assert_eq!(pi_session_id("pi"), None);
}

#[test]
fn pi_session_prefers_session_arg_over_newest_file() {
    let root = std::env::temp_dir().join(format!("aether-pi-exact-{}-a", std::process::id()));
    let dir = root.join("--w--");
    std::fs::create_dir_all(&dir).unwrap();
    let stop = r#"{"type":"message","message":{"role":"assistant","stopReason":"stop"}}"#;
    let now = SystemTime::now();
    let file = |name: &str, age: u64| {
        let path = dir.join(name);
        std::fs::write(&path, stop).unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(now - Duration::from_secs(age))
            .unwrap();
        path
    };
    let old = file("2020-01-01T00-00-00-000Z_old-id.jsonl", 100_000);
    file("newer.jsonl", 10);
    let cmd = format!("pi --session {}", old.display());
    let mut a = agent(Provider::Pi, "/w", "");
    pi_session(&mut a, &root, &HashSet::new(), &cmd);
    assert_eq!((a.session.as_str(), a.exact), (old.to_string_lossy().as_ref(), true));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn pi_session_resolves_bare_id_to_bucket_file() {
    let root = std::env::temp_dir().join(format!("aether-pi-exact-{}-b", std::process::id()));
    let dir = root.join("--w--");
    std::fs::create_dir_all(&dir).unwrap();
    let id = "01a1b1f1-a601-723a-bc93-f0b7df1fc391";
    let path = dir.join(format!("2026-11-06T17-39-26-337Z_{id}.jsonl"));
    std::fs::write(&path, format!("{{\"type\":\"session\",\"version\":3,\"id\":\"{id}\",\"cwd\":\"/w\"}}\n{{\"type\":\"message\",\"message\":{{\"role\":\"assistant\",\"stopReason\":\"stop\"}}}}")).unwrap();
    let cmd = format!("pi --session {id}");
    let mut a = agent(Provider::Pi, "/w", "");
    pi_session(&mut a, &root, &HashSet::new(), &cmd);
    assert_eq!((a.session.as_str(), a.exact), (path.to_string_lossy().as_ref(), true));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn pi_session_picks_newest_outside_file() {
    let root = std::env::temp_dir().join(format!("aether-pi-scan-{}", std::process::id()));
    let dir = root.join("--w--");
    std::fs::create_dir_all(&dir).unwrap();
    let stop = r#"{"type":"message","message":{"role":"assistant","stopReason":"stop"}}"#;
    let now = SystemTime::now();
    let file = |name: &str, age: u64| {
        let path = dir.join(name);
        std::fs::write(&path, stop).unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(now - Duration::from_secs(age))
            .unwrap();
        path.to_string_lossy().into_owned()
    };
    let (mine, theirs) = (file("mine.jsonl", 10), file("theirs.jsonl", 20));
    file("before-start.jsonl", 1000);
    let mut a = agent(Provider::Pi, "/w", "");
    pi_session(&mut a, &root, &HashSet::from([mine.clone()]), "pi");
    assert_eq!((a.session.as_str(), a.state.clone()), (theirs.as_str(), Idle));
    pi_session(&mut a, &root, &HashSet::new(), "pi");
    assert_eq!(a.session, mine);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn signal_plan_checks_identity() {
    let mut a = agent(Provider::Claude, "/a", "s");
    a.started = "Wed Oct  4 16:02:23 2026".into();
    let (l, c) = (Some("Wed Oct  4 16:02:23 2026"), Some("/x/bin/claude"));
    assert_eq!(signal_plan(&a, false, l, c), Ok(Some("INT")));
    assert_eq!(signal_plan(&a, true, l, c), Ok(Some("TERM")));
    assert_eq!(
        signal_plan(&a, false, Some("Wed Oct  4 16:02:24 2026"), c),
        Err("process changed".into())
    );
    assert_eq!(signal_plan(&a, false, l, Some("zsh")), Err("process changed".into()));
    assert_eq!(signal_plan(&a, false, None, None), Err("process changed".into()));
    a.pid = None;
    assert_eq!(signal_plan(&a, false, l, c), Err("process changed".into()));
    let mut p = agent(Provider::Pi, "/a", "s");
    p.started = "t".into();
    assert_eq!(
        signal_plan(&p, false, Some("t"), Some("node /x/pi-coding-agent/cli.js")),
        Ok(Some("TERM"))
    );
    assert_eq!(
        signal_plan(&p, false, Some("t"), Some("node other.js")),
        Err("process changed".into())
    );
    assert_eq!(signal_plan(&p, false, Some("t"), Some("claude")), Err("process changed".into()));
    let mut codex = agent(Provider::Codex, "/a", "s");
    codex.started = "t".into();
    assert_eq!(signal_plan(&codex, false, Some("t"), Some("/bin/codex")), Ok(Some("INT")));
    assert_eq!(signal_plan(&codex, true, Some("t"), Some("/bin/codex")), Ok(Some("TERM")));
    let mut agy = agent(Provider::Antigravity, "/a", "s");
    agy.started = "t".into();
    assert_eq!(signal_plan(&agy, false, Some("t"), Some("agy")), Ok(Some("TERM")));
    assert_eq!(
        signal_plan(&agy, false, Some("t"), Some("/usr/local/bin/antigravity")),
        Ok(Some("TERM"))
    );
    let mut o = agent(Provider::OpenRouter, "/a", "s");
    o.started = "t".into();
    assert!(signal_plan(&o, false, Some("t"), Some("claude")).is_err());
    assert!(signal_plan(&agent(Provider::OpenCode, "/a", "ses_../x"), false, None, None).is_err());
    let oc = agent(Provider::OpenCode, "/a", "ses_a");
    assert_eq!(signal_plan(&oc, true, None, None), Ok(None));
    assert!(signal_plan(&agent(Provider::OpenCode, "/a", ""), false, None, None).is_err());
}

#[test]
fn ps_identity_splits_lstart_and_command() {
    let out = " Tue Oct 13 20:45:16 2026 /Users/x/.local/bin/claude\n";
    assert_eq!(
        split_ps_identity(out),
        (Some("Tue Oct 13 20:45:16 2026"), Some("/Users/x/.local/bin/claude"))
    );
    assert_eq!(
        split_ps_identity("Wed Oct  4 16:02:23 2026 node a b").0,
        Some("Wed Oct  4 16:02:23 2026")
    );
    assert_eq!(split_ps_identity(""), (None, None));
    assert!(
        valid_session_id("ses_Ab9_x")
            && !valid_session_id("ses_a/../b")
            && !valid_session_id("abc")
    );
}

#[test]
fn claude_transcript_keeps_plain_text_only() {
    let t = [
        r#"{"type":"user","message":{"role":"user","content":"hi"}}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello"},{"type":"tool_use","name":"x","input":{}}]}}"#,
        r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"out"}]}}"#,
        r#"{"type":"user","isMeta":true,"message":{"content":"meta"}}"#,
        r#"{"type":"assistant","isSidechain":true,"message":{"content":[{"type":"text","text":"side"}]}}"#,
        r#"{"type":"user","message":{"content":"<command-name>/clear</command-name>"}}"#,
        r#"{"type":"user","message":{"content":"<local-command-stdout>x</local-command-stdout>"}}"#,
        r#"{"type":"user","isCompactSummary":true,"message":{"content":"summary"}}"#,
        r#"{"type":"ai-title","aiTitle":"T"}"#,
        "not json",
    ]
    .join("\n");
    assert_eq!(
        claude_transcript(&t),
        [(Role::User, "hi".into()), (Role::Assistant, "hello".into())]
    );
}

#[test]
fn pi_transcript_keeps_user_and_assistant_text() {
    let t = [
        r#"{"type":"session","id":"x"}"#,
        r#"{"type":"message","message":{"role":"user","content":[{"type":"text","text":"hi"}]}}"#,
        r#"{"type":"message","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hm"},{"type":"text","text":"hello"},{"type":"toolCall","name":"x"}]}}"#,
        r#"{"type":"message","message":{"role":"toolResult","content":[{"type":"text","text":"out"}]}}"#,
    ]
    .join("\n");
    assert_eq!(pi_transcript(&t), [(Role::User, "hi".into()), (Role::Assistant, "hello".into())]);
}

#[test]
fn codex_transcript_keeps_user_and_assistant_text() {
    let t = [
        r#"{"type":"session_meta","payload":{"id":"01a10f51-211c-7610-a20f-ac9f43c1c667"}}"#,
        r#"{"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"system"}]}}"#,
        r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>\n  <cwd>/x</cwd>\n</environment_context>"}]}}"#,
        r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<user_instructions>agents</user_instructions>"}]}}"#,
        r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}}"#,
        r#"{"type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"hm"}]}}"#,
        r#"{"type":"response_item","payload":{"type":"custom_tool_call","name":"x","input":"{}"}}"#,
        r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hello"}]}}"#,
        r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"again"},{"type":"input_image","image_url":"x"}]}}"#,
        "not json",
    ]
    .join("\n");
    assert_eq!(
        codex_transcript(&t),
        [
            (Role::User, "hi".into()),
            (Role::Assistant, "hello".into()),
            (Role::User, "again".into()),
        ]
    );
}

#[test]
fn codex_file_finds_rollout_by_thread_id() {
    let root = std::env::temp_dir().join(format!("aether-codex-lookup-{}", std::process::id()));
    let dir = root.join("sessions/2026/10/06");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rollout-2026-10-06T09-30-37-01a10f51-211c-7610-a20f-ac9f43c1c667.jsonl");
    std::fs::write(&path, "{}").unwrap();
    assert_eq!(
        codex_file(&root, "01a10f51-211c-7610-a20f-ac9f43c1c667"),
        Some(path.to_string_lossy().into_owned())
    );
    assert_eq!(codex_file(&root, "missing"), None);
    assert_eq!(codex_file(&root, "../escape"), None);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn agy_transcript_decodes_user_and_planner_steps() {
    let rows = [
        // type 14, a fixed64 to skip, then 19 { 2: "hi" }
        "14|080E1901020304050607089A010412026869",
        // 19 { 3 { 1: "yo" } } fallback
        "14|080E9A01061A040A02796F",
        // planner 20 { 1: "hello" }
        "15|080FA201070A0568656C6C6F",
        // tool-only planner placeholder, and a truncated blob
        "15|080F",
        "15|A20107",
    ]
    .join("\n");
    assert_eq!(
        agy_transcript(&rows),
        [
            (Role::User, "hi".to_string()),
            (Role::User, "yo".to_string()),
            (Role::Assistant, "hello".to_string()),
        ]
    );
}

#[test]
fn agy_rows_reads_with_and_without_a_wal() {
    let dir = std::env::temp_dir().join(format!("aether-agy-rows-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("c2f4e33d.db");
    let sql = "CREATE TABLE steps (idx INTEGER, step_type INTEGER, step_payload BLOB); \
               INSERT INTO steps VALUES (0, 14, X'9A010412026869'), (1, 132, X'00');";
    let made = std::process::Command::new("/usr/bin/sqlite3").arg(&db).arg(&sql).status();
    assert!(made.unwrap().success());
    assert_eq!(agy_rows(&db).as_deref(), Some("14|9A010412026869\n"));
    std::fs::write(db.with_extension("db-wal"), "not a wal").unwrap();
    assert_eq!(agy_transcript(&agy_rows(&db).unwrap()), [(Role::User, "hi".to_string())]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn opencode_transcript_keeps_text_parts() {
    let body = r#"{"data":[
            {"type":"assistant","id":"msg_3","content":[{"type":"tool","id":"d","name":"bash"}]},
            {"type":"assistant","id":"msg_2","content":[{"type":"reasoning","text":"hm"},{"type":"tool","id":"c","name":"bash"},{"type":"text","text":"hello"}]},
            {"type":"user","id":"msg_1","text":"hi"}]}"#;
    assert_eq!(
        opencode_transcript(body),
        [(Role::User, "hi".into()), (Role::Assistant, "hello".into())]
    );
}

#[test]
#[ignore]
fn live_scan_summary() {
    let agent = crate::Agent::new().unwrap();
    for a in futures::executor::block_on(agent.scan_external(HashSet::new())).unwrap_or_default() {
        let folder =
            a.cwd.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        println!(
            "{:?} pid={:?} folder={folder} state={:?} exact={} origin={}",
            a.provider, a.pid, a.state, a.exact, a.origin
        );
    }
}
