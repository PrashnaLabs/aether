use super::*;
use crate::orchestrator::forward;

fn rpc(
    line: &str,
    token: Option<&str>,
    call: &mut dyn FnMut(&str, Value) -> Result<Value, String>,
) -> Value {
    serde_json::from_str(&handle(line, token, call).expect("a response")).unwrap()
}
fn never(_: &str, _: Value) -> Result<Value, String> {
    panic!("no call expected")
}

#[test]
fn initialize_echoes_protocol_version_and_names_server() {
    let v = rpc(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        Some("t"),
        &mut never,
    );
    assert_eq!(v["id"], 1);
    assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(v["result"]["serverInfo"]["name"], "aether");
    assert!(v["result"]["capabilities"]["tools"].is_object());
}

#[test]
fn notifications_get_no_reply() {
    assert!(
        handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, Some("t"), &mut never)
            .is_none()
    );
}

#[test]
fn tools_list_is_empty_without_token() {
    let v = rpc(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, None, &mut never);
    assert_eq!(v["result"]["tools"], serde_json::json!([]));
    let mut down = |_: &str, _: Value| Err("Aether didn't answer".to_string());
    let v = rpc(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#, Some("t"), &mut down);
    let names: Vec<_> = v["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        [
            "list_agents",
            "spawn_agent",
            "request_worker",
            "read_agent",
            "message_agent",
            "stop_agent",
            "list_cards",
            "add_card",
            "read_card",
            "move_card",
            "update_card",
            "delete_cards",
            "read_history",
            "list_chats",
            "archive_chats",
            "delete_chats",
            "unarchive_chats",
            "update_chat",
            "stop_chat",
            "open_chat"
        ]
    );
}

#[test]
fn tools_list_asks_the_app_which_tools_this_chat_gets() {
    let mut asked = None;
    let v = rpc(r#"{"jsonrpc":"2.0","id":5,"method":"tools/list"}"#, Some("t"), &mut |tool, _| {
        asked = Some(tool.to_string());
        Ok(crate::orchestrator::history_specs())
    });
    assert_eq!(asked.as_deref(), Some("list_tools"));
    let names: Vec<_> =
        v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].clone()).collect();
    assert_eq!(names, ["read_history"]);
}

#[test]
fn tools_call_forwards_and_wraps_result_as_text() {
    let mut seen = None;
    let v = rpc(
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"spawn_agent","arguments":{"task":"x"}}}"#,
        Some("t"),
        &mut |tool, args| {
            seen = Some((tool.to_string(), args));
            Ok(serde_json::json!({"id": 7}))
        },
    );
    assert_eq!(seen.unwrap().0, "spawn_agent");
    assert_eq!(v["result"]["isError"], false);
    let text = v["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(serde_json::from_str::<Value>(text).unwrap()["id"], 7);
}

#[test]
fn tools_call_error_is_tool_error_not_rpc_error() {
    let v = rpc(
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"list_agents","arguments":{}}}"#,
        Some("t"),
        &mut |_, _| Err("Aether isn't running".into()),
    );
    assert_eq!(v["result"]["isError"], true);
    assert_eq!(v["result"]["content"][0]["text"], "Aether isn't running");
}

#[test]
fn tools_call_without_token_is_not_a_lead() {
    let v = rpc(
        r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"list_agents","arguments":{}}}"#,
        None,
        &mut never,
    );
    assert_eq!(v["result"]["isError"], true);
    assert_eq!(v["result"]["content"][0]["text"], "not an Aether lead");
}

#[test]
fn garbage_and_unknown_methods_answer_with_rpc_errors() {
    let v = rpc("not json", Some("t"), &mut never);
    assert_eq!(v["error"]["code"], -32700);
    let v = rpc(r#"{"jsonrpc":"2.0","id":6,"method":"nope"}"#, Some("t"), &mut never);
    assert_eq!(v["error"]["code"], -32601);
    let v = rpc(r#"{"jsonrpc":"2.0","id":8,"method":"ping"}"#, Some("t"), &mut never);
    assert_eq!(v["result"], serde_json::json!({}));
}

#[test]
fn forward_without_listener_says_aether_isnt_running() {
    let p = std::env::temp_dir().join(format!("aether-orch-none-{}.sock", std::process::id()));
    assert_eq!(
        forward(&p, "t", "list_agents", serde_json::json!({})).unwrap_err(),
        "Aether isn't running"
    );
}

#[cfg(unix)]
fn serve_once(name: &str, reply: &'static str) -> std::path::PathBuf {
    use std::io::{BufRead, BufReader, Write};
    let p = std::env::temp_dir().join(format!("aether-orch-{name}-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&p);
    let l = std::os::unix::net::UnixListener::bind(&p).unwrap();
    std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut req = String::new();
        BufReader::new(s.try_clone().unwrap()).read_line(&mut req).unwrap();
        let req: Value = serde_json::from_str(&req).unwrap();
        assert_eq!(req["token"], "t");
        assert_eq!(req["tool"], "list_agents");
        writeln!(s, "{reply}").unwrap();
    });
    p
}

#[cfg(unix)]
#[test]
fn forward_maps_server_replies() {
    let p = serve_once("ok", r#"{"ok":true,"result":{"n":1}}"#);
    assert_eq!(forward(&p, "t", "list_agents", serde_json::json!({})).unwrap()["n"], 1);
    let p = serve_once("err", r#"{"ok":false,"error":"nope"}"#);
    assert_eq!(forward(&p, "t", "list_agents", serde_json::json!({})).unwrap_err(), "nope");
    let p = serve_once("junk", "garbage");
    assert_eq!(
        forward(&p, "t", "list_agents", serde_json::json!({})).unwrap_err(),
        "Aether didn't answer"
    );
}

#[test]
fn agent_id_schema_is_a_plain_integer() {
    let specs = crate::orchestrator::tool_specs();
    for name in ["read_agent", "message_agent", "stop_agent"] {
        let spec = specs.as_array().unwrap().iter().find(|s| s["name"] == name).unwrap();
        assert_eq!(spec["inputSchema"]["properties"]["id"]["type"], "integer", "{name}");
    }
}

#[test]
fn spawn_agent_offers_an_optional_worktree_flag() {
    let specs = tool_specs();
    let spawn = specs.as_array().unwrap().iter().find(|t| t["name"] == "spawn_agent").unwrap();
    let flag = &spawn["inputSchema"]["properties"]["worktree"];
    assert_eq!(flag["type"], "boolean");
    assert!(flag["description"].as_str().unwrap().contains("uncommitted changes aren't included"));
    assert!(!spawn["inputSchema"]["required"].as_array().unwrap().contains(&"worktree".into()));
}

#[test]
fn spawn_and_read_say_results_arrive_automatically() {
    let specs = tool_specs();
    let desc = |name: &str| {
        let spec = specs.as_array().unwrap().iter().find(|t| t["name"] == name).unwrap();
        spec["description"].as_str().unwrap().to_string()
    };
    assert!(desc("spawn_agent").ends_with(
        "Its result is sent to you automatically when it finishes; don't poll read_agent or sleep."
    ));
    assert!(desc("read_agent").ends_with(
        "Use when you need a child's output early; finished results arrive automatically."
    ));
}

#[test]
fn spawn_agent_offers_an_optional_nonce() {
    let specs = tool_specs();
    let spawn = specs.as_array().unwrap().iter().find(|t| t["name"] == "spawn_agent").unwrap();
    assert!(spawn["inputSchema"]["properties"]["nonce"].is_object());
    assert!(!spawn["inputSchema"]["required"].as_array().unwrap().contains(&"nonce".into()));
}

#[test]
fn unknown_arguments_are_named() {
    use crate::orchestrator::check_args;
    assert_eq!(
        check_args("list_cards", &json!({"filter": {"search": "x"}, "bodies": true})),
        Ok(())
    );
    assert_eq!(
        check_args("list_cards", &json!({"search": "x"})),
        Err(r#"unknown argument "search"; expected one of: bodies, filter"#.into())
    );
    assert!(
        check_args("list_cards", &json!({"filter": {"serch": "x"}}))
            .unwrap_err()
            .starts_with(r#"unknown argument "filter.serch""#)
    );
    assert_eq!(check_args("list_tools", &json!({"anything": 1})), Ok(()));
    assert_eq!(check_args("list_agents", &Value::Null), Ok(()));
}
