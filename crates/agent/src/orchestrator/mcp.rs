use std::io::{BufRead, Read, Write};

use serde_json::{Value, json};

use super::{SOCK_ENV, TOKEN_ENV, forward, socket_path, tool_specs};

pub fn handle(
    line: &str,
    token: Option<&str>,
    call: &mut dyn FnMut(&str, Value) -> Result<Value, String>,
) -> Option<String> {
    let Ok(req) = serde_json::from_str::<Value>(line) else {
        return Some(error(Value::Null, -32700, "parse error"));
    };
    let id = req.get("id")?.clone();
    let params = &req["params"];
    let result = match req["method"].as_str().unwrap_or("") {
        "initialize" => json!({
            "protocolVersion": params["protocolVersion"],
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "aether", "version": env!("CARGO_PKG_VERSION")}
        }),
        "ping" => json!({}),
        "tools/list" => json!({"tools": if token.is_some() { tool_specs() } else { json!([]) }}),
        "tools/call" => {
            let outcome = match token {
                Some(_) => call(params["name"].as_str().unwrap_or(""), params["arguments"].clone()),
                None => Err("not an Aether lead".to_string()),
            };
            let (text, is_error) = match outcome {
                Ok(v) => (v.to_string(), false),
                Err(e) => (e, true),
            };
            json!({"content": [{"type": "text", "text": text}], "isError": is_error})
        }
        _ => return Some(error(id, -32601, "method not found")),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string())
}

fn error(id: Value, code: i32, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
}

pub fn run_stdio() -> i32 {
    let token = std::env::var(TOKEN_ENV).ok().filter(|t| !t.is_empty());
    let sock = std::env::var_os(SOCK_ENV).map(std::path::PathBuf::from).or_else(socket_path);
    let mut call = |tool: &str, args: Value| match (&sock, &token) {
        (Some(sock), Some(token)) => forward(sock, token, tool, args),
        _ => Err("not an Aether lead".to_string()),
    };
    let mut out = std::io::stdout().lock();
    let mut stdin = std::io::stdin().lock();
    let mut line = String::new();
    loop {
        line.clear();
        match stdin.by_ref().take(1024 * 1024 + 1).read_line(&mut line) {
            Ok(0) => break,
            Ok(_) if line.len() <= 1024 * 1024 => {}
            _ => break,
        }
        let line = line.trim_end_matches(['\n', '\r']);
        if let Some(reply) = handle(line, token.as_deref(), &mut call)
            && writeln!(out, "{reply}").and_then(|_| out.flush()).is_err()
        {
            break;
        }
    }
    0
}

#[cfg(test)]
#[path = "../../tests/unit/orchestrator/mcp.rs"]
mod tests;
