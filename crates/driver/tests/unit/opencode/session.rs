use super::*;
use aether_core::AccessMode;

#[test]
fn lost_resume_notice_only_when_a_resume_was_requested_but_not_possible() {
    let notice = lost_resume(true, false).unwrap();
    assert_eq!(notice.id, "info-resume");
    assert_eq!(notice.kind, aether_core::NoticeKind::Info);
    assert_eq!(notice.text, "Couldn't find the earlier OpenCode session, so this is a new one.");
    assert!(lost_resume(false, false).is_none());
    assert!(lost_resume(true, true).is_none());
    assert!(lost_resume(false, true).is_none());
}

#[test]
fn create_body_carries_location_and_model_ref() {
    let opts = DriverStartOptions {
        ephemeral: false,
        binary: String::new(),
        cwd: "/tmp/aether-no-such-dir-xyz/".into(),
        model: "anthropic/claude-x".into(),
        effort: "high".into(),
        access: AccessMode::Ask,
        resume: String::new(),
        reattach: false,
        attachments: None,
        background: Vec::new(),
        lead: None,
    };
    let location: std::path::PathBuf =
        std::path::Path::new("/tmp/aether-no-such-dir-xyz").components().collect();
    let dir = location.to_string_lossy().into_owned();
    let attachments =
        aether_core::attachments_dir().unwrap().join("*").to_string_lossy().into_owned();
    let files = serde_json::json!({"action": "external_directory", "resource": attachments, "effect": "allow"});
    assert_eq!(
        create_body("ses_1", &opts),
        serde_json::json!({
            "id": "ses_1",
            "location": {"directory": dir},
            "agent": "build",
            "permissions": [
                {"action": "edit", "resource": "*", "effect": "ask"},
                {"action": "shell", "resource": "*", "effect": "ask"},
                files,
            ],
            "model": {"providerID": "anthropic", "id": "claude-x", "variant": "high"},
        })
    );
    let bare = create_body("ses_1", &DriverStartOptions { model: String::new(), ..opts });
    assert!(bare.get("model").is_none());
    let shell = serde_json::json!([{"action": "shell", "resource": "*", "effect": "ask"}, files]);
    assert_eq!(rules(AccessMode::AutoEdit), shell);
    assert_eq!(rules(AccessMode::Full), serde_json::json!([files]));
}

#[test]
fn slash_commands_split_name_and_args() {
    assert_eq!(command("/review  main\nnow "), Some(("review", "main\nnow")));
    assert_eq!(command("/compact"), Some(("compact", "")));
    assert_eq!(command("/ hi"), None);
    assert_eq!(command("fix /etc/hosts"), None);
}

#[test]
fn minted_ids_match_opencode_format() {
    let id = new_session_id();
    assert_eq!((id.len(), &id[..4]), (30, "ses_"), "{id}");
    assert!(id[4..16].chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()), "{id}");
    assert!(id[13..16].eq("ffe"), "{id}");
    assert!(id[16..].chars().all(|c| c.is_ascii_alphanumeric()), "{id}");
    assert_ne!(id, new_session_id());
}

#[test]
fn sse_frames_split_on_blank_lines() {
    let mut buf =
        b"data: {\"type\":\"a\"}\n\n: heartbeat\n\ndata: {\"type\":\"b\"}\n\ndata: {\"ty".to_vec();
    let types: Vec<Value> = frames(&mut buf).into_iter().map(|v| v["type"].clone()).collect();
    assert_eq!(types, ["a", "b"]);
    assert_eq!(buf, b"data: {\"ty");
}

#[test]
fn unoffered_modes_never_allow_everything() {
    let effects = |a| -> Vec<(String, String)> {
        rules(a)
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["action"] != "external_directory")
            .map(|r| (r["action"].as_str().unwrap().into(), r["effect"].as_str().unwrap().into()))
            .collect()
    };
    let deny = vec![("edit".into(), "deny".into()), ("shell".into(), "deny".into())];
    assert_eq!(effects(AccessMode::ReadOnly), deny);
    assert_eq!(effects(AccessMode::Auto), effects(AccessMode::Ask));
    assert!(effects(AccessMode::Full).is_empty());
}

fn parents(pairs: &[(&str, &str)]) -> impl FnMut(&str) -> Result<Option<String>, ()> {
    let map: HashMap<String, String> =
        pairs.iter().map(|(c, p)| (c.to_string(), p.to_string())).collect();
    move |sid| Ok(map.get(sid).cloned())
}

#[test]
fn descends_walks_grandchildren() {
    let mut cache = HashMap::new();
    let mut lookups = parents(&[("ses_c", "ses_root"), ("ses_g", "ses_c")]);
    assert!(descends("ses_root", "ses_g", &mut cache, &mut lookups));
    assert_eq!(cache.get("ses_g"), Some(&true));
    assert_eq!(cache.get("ses_c"), Some(&true));
}

#[test]
fn descends_caches_unknown_as_foreign() {
    let mut cache = HashMap::new();
    assert!(!descends("ses_root", "ses_x", &mut cache, parents(&[])));
    assert!(!descends("ses_root", "ses_y", &mut cache, parents(&[("ses_y", "ses_other")])));
    assert_eq!(cache.get("ses_x"), Some(&false));
    let mut calls = 0;
    assert!(!descends("ses_root", "ses_x", &mut cache, |_| {
        calls += 1;
        Ok(None)
    }));
    assert_eq!(calls, 0, "cached answers skip the lookup");
}

#[test]
fn descends_does_not_cache_failed_lookups() {
    let mut cache = HashMap::new();
    assert!(!descends("ses_root", "ses_c", &mut cache, |_| Err(())));
    assert!(cache.is_empty());
    assert!(descends("ses_root", "ses_c", &mut cache, |_| Ok(Some("ses_root".into()))));
}

#[test]
fn descends_stops_on_cycles() {
    let mut cache = HashMap::new();
    assert!(!descends(
        "ses_root",
        "ses_a",
        &mut cache,
        parents(&[("ses_a", "ses_b"), ("ses_b", "ses_a")])
    ));
}

#[test]
fn descends_does_not_cache_past_the_depth_limit() {
    let names: Vec<String> = (0..10).map(|i| format!("ses_{i}")).collect();
    let mut pairs: Vec<(&str, &str)> = names.windows(2).map(|w| (&w[0][..], &w[1][..])).collect();
    pairs.push(("ses_9", "ses_root"));
    let mut cache = HashMap::new();
    assert!(!descends("ses_root", "ses_0", &mut cache, parents(&pairs)));
    assert!(!cache.contains_key("ses_0"));
}

#[test]
fn retry_lookup_recovers_from_transient_errors() {
    let mut cache = HashMap::new();
    let mut calls = 0;
    let found = descends("ses_root", "ses_c", &mut cache, |_| {
        retry_lookup(3, Duration::ZERO, || {
            calls += 1;
            if calls < 3 { Err(()) } else { Ok(Some("ses_root".into())) }
        })
    });
    assert!(found);
    assert_eq!(calls, 3);
    let mut calls = 0;
    let out = retry_lookup(3, Duration::ZERO, || {
        calls += 1;
        Err(())
    });
    assert_eq!((out, calls), (Err(()), 3));
}

#[test]
fn listing_is_scoped_by_the_encoded_session_directory() {
    assert_eq!(
        located("/api/form", "/Users/a b/é"),
        "/api/form?location%5Bdirectory%5D=%2FUsers%2Fa%20b%2F%C3%A9"
    );
}

#[test]
fn model_switch_resets_to_the_default_and_applies_effort() {
    let default = json!({"providerID": "opencode", "id": "big"});
    let current = json!({"providerID": "openrouter", "id": "glm"});
    assert_eq!(
        model_switch("", "", Some(&current), &default),
        Some(json!({"model": {"providerID": "opencode", "id": "big"}}))
    );
    assert_eq!(
        model_switch("openrouter/glm", "high", Some(&current), &default),
        Some(json!({"model": {"providerID": "openrouter", "id": "glm", "variant": "high"}}))
    );
    assert_eq!(model_switch("openrouter/glm", "", Some(&current), &default), None);
    assert_eq!(model_switch("", "", None, &default), None);
    assert_eq!(model_switch("garbage", "", Some(&current), &default), None);
}

#[test]
fn prompts_send_images_as_file_uris() {
    let img = Attachment {
        name: "a.png".into(),
        path: "/c/a.png".into(),
        mime: "image/png".into(),
        size: 1,
    };
    assert_eq!(
        prompt_body("look", &[img], Some("steer")),
        json!({"text": "look", "files": [{"uri": "file:///c/a.png"}], "delivery": "steer"})
    );
    assert_eq!(prompt_body("hi", &[], None), json!({"text": "hi"}));
}

#[test]
fn child_listing_walks_grandchildren_within_the_depth_limit() {
    let tree = |parent: &str| -> Result<Vec<(String, String)>, ()> {
        Ok(match parent {
            "root" => vec![("a".into(), "explore".into()), ("b".into(), "".into())],
            "a" => vec![("c".into(), "general".into())],
            "b" => return Err(()),
            "c" => vec![("root".into(), "cycle".into()), ("a".into(), "cycle".into())],
            _ => vec![],
        })
    };
    let found = list_children("root", tree);
    let ids: Vec<&str> = found.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["a", "b", "c"]);
    assert_eq!(found[2].1, "general");
    let chain = |parent: &str| -> Result<Vec<(String, String)>, ()> {
        let n: u32 = parent.trim_start_matches('s').parse().unwrap_or(0);
        Ok(vec![(format!("s{}", n + 1), String::new())])
    };
    assert_eq!(list_children("s0", chain).len(), 8);
}

#[test]
fn child_listing_is_scoped_by_the_root_directory() {
    assert_eq!(children_path("ses_1", None), "/api/session?parentID=ses_1");
    assert_eq!(
        children_path("ses_1", Some("/a b")),
        "/api/session?parentID=ses_1&location%5Bdirectory%5D=%2Fa%20b"
    );
}

#[test]
fn file_uris_are_percent_encoded() {
    let img = Attachment {
        name: "a".into(),
        path: "/c d/a#1.png".into(),
        mime: "image/png".into(),
        size: 1,
    };
    assert_eq!(prompt_body("x", &[img], None)["files"][0]["uri"], "file:///c%20d/a%231.png");
}

#[test]
fn served_model_reports_the_model_in_use() {
    let m = |p: &str, id: &str| json!({"providerID": p, "id": id});
    let mut seeded = Some(json!({"providerID": "new", "id": "n", "variant": "high"}));
    assert_eq!(served_model(Some(&m("old", "o")), &mut seeded).as_deref(), Some("new/n"));
    assert_eq!(seeded.unwrap()["variant"], "high");
    let mut seeded = Some(m("a", "b"));
    assert_eq!(served_model(None, &mut seeded).as_deref(), Some("a/b"));
    let mut seeded = None;
    assert_eq!(served_model(Some(&m("srv", "def")), &mut seeded).as_deref(), Some("srv/def"));
    assert_eq!(seeded, Some(m("srv", "def")));
    let mut seeded = None;
    assert_eq!(served_model(Some(&json!(null)), &mut seeded), None);
    assert_eq!(served_model(None, &mut seeded), None);
}

fn messages() -> Value {
    json!({"data": [
        {"role": "assistant", "content": [
            {"type": "text", "text": "started"},
            {"type": "tool", "id": "call_bg", "name": "task", "state": {"status": "completed",
                "input": {"description": "Scan", "subagent_type": "explore", "prompt": "go"},
                "metadata": {"sessionID": "ses_bg", "status": "running"}}},
            {"type": "tool", "id": "call_fg", "name": "task", "state": {"status": "completed",
                "input": {"description": "Old"}, "metadata": {"sessionID": "ses_fg"}}},
            {"type": "tool", "id": "call_gone", "name": "task", "state": {"status": "completed",
                "input": {"description": "Gone"}, "metadata": {"sessionID": "ses_gone", "status": "running"}}},
        ]},
    ]})
}

#[test]
fn relinking_rebuilds_running_children_and_settles_rows_whose_child_ended() {
    let mut s = State { root: "ses_1".into(), ..State::default() };
    let mut known = crate::tool::start("call_old", "task", &json!({"description": "Earlier"}));
    known.status = aether_core::ToolStatus::Background;
    s.background.insert("call_old".into(), known);
    s.children.insert("ses_old".into(), Child { task: "call_old".into(), agent: String::new() });
    let active = json!({"data": {"ses_bg": {}}});
    let out = relink(&mut s, &messages(), Some(&active), |_| None);
    let [Update::Tool(done)] = &out[..] else { panic!("{out:?}") };
    assert_eq!((done.id.as_str(), done.status), ("call_old", aether_core::ToolStatus::Done));
    assert_eq!(s.children["ses_bg"].task, "call_bg");
    assert_eq!(s.children["ses_bg"].agent, "explore");
    assert!(s.background.contains_key("call_bg"));
    assert!(!s.children.contains_key("ses_fg") && !s.children.contains_key("ses_gone"));
    assert_eq!(s.background.len(), 1);
    assert!(relink(&mut s, &messages(), Some(&active), |_| None).is_empty(), "idempotent");
}

#[test]
fn relinking_settles_restored_background_rows_whose_work_ended() {
    let part = |id: &str, name: &str, meta: Value| {
        json!({"type": "tool", "id": id, "name": name, "state": {"status": "completed",
            "input": {"description": id, "command": "sleep 9"}, "metadata": meta}})
    };
    let messages = json!({"data": [{"role": "assistant", "content": [
        part("call_ok", "task", json!({"sessionID": "ses_ok", "status": "running"})),
        part("call_cut", "task", json!({"sessionID": "ses_cut", "status": "running"})),
        part("call_live", "task", json!({"sessionID": "ses_live", "status": "running"})),
        part("call_sh", "shell", json!({"shellID": "sh_run", "status": "running"})),
        part("call_sh_done", "shell", json!({"shellID": "sh_done", "status": "running"})),
    ]}]});
    let restored = ["call_ok", "call_cut", "call_live", "call_sh", "call_sh_done", "call_far"];
    let mut s = State {
        root: "ses_1".into(),
        restored: restored.map(String::from).to_vec(),
        ..State::default()
    };
    let active = json!({"data": {"ses_live": {}}});
    let asked = std::cell::RefCell::new(vec![]);
    let ended = |w: &Work| {
        asked.borrow_mut().push(format!("{w:?}"));
        match w {
            Work::Child(sid) if sid == "ses_ok" => Some(ToolStatus::Done),
            Work::Child(_) => Some(ToolStatus::Stopped),
            Work::Shell(id) if id == "sh_done" => Some(ToolStatus::Done),
            Work::Shell(_) => None,
        }
    };
    let mut out: Vec<(String, ToolStatus)> = relink(&mut s, &messages, Some(&active), ended)
        .into_iter()
        .filter_map(|u| match u {
            Update::Tool(c) => Some((c.id, c.status)),
            _ => None,
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        out,
        [
            ("call_cut".to_string(), ToolStatus::Stopped),
            ("call_ok".to_string(), ToolStatus::Done),
            ("call_sh_done".to_string(), ToolStatus::Done),
        ]
    );
    assert!(!asked.borrow().iter().any(|w| w.contains("ses_live")), "a live child isn't looked up");
    assert_eq!(s.shells.get("sh_run").map(String::as_str), Some("call_sh"), "a live shell relinks");
    assert!(s.background.contains_key("call_sh") && s.background.contains_key("call_live"));
    assert!(s.restored.is_empty());
    assert!(relink(&mut s, &messages, Some(&active), ended).is_empty(), "settled once");
}

#[test]
fn a_failed_active_listing_settles_nothing() {
    let mut s =
        State { root: "ses_1".into(), restored: vec!["call_bg".into()], ..State::default() };
    let mut known = crate::tool::start("call_old", "task", &json!({"description": "Earlier"}));
    known.status = ToolStatus::Background;
    s.background.insert("call_old".into(), known);
    s.children.insert("ses_old".into(), Child { task: "call_old".into(), agent: String::new() });
    assert_eq!(relink(&mut s, &messages(), None, |_| Some(ToolStatus::Stopped)), []);
    assert!(s.background.contains_key("call_old"));
    assert_eq!(s.restored, ["call_bg"], "kept for the next relink");
}
