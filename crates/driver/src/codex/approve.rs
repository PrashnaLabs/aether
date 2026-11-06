use serde_json::{Value, json};

use aether_core::{
    AccessMode, Answer, Approval, ApprovalKind, ApprovalOption, Ask, Choice, Field, FieldKind,
    Form, Question, ToolCall, ToolDetail, Update,
};

use crate::tool::now_ms;

use super::events::command_text;
use super::spawn::note_turn;
use super::{Naming, State};

/// Maps an MCP elicitation schema to a form, plus each field's key and JSON type for the reply.
pub(crate) fn schema_form(message: &str, schema: &Value) -> (Form, Vec<(String, String)>) {
    let required: Vec<&str> =
        schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let text = |v: &Value, key: &str| v[key].as_str().unwrap_or_default().to_string();
    let choice = |label: String, value: String| Choice { label, value, ..Choice::default() };
    let (mut fields, mut keys) = (vec![], vec![]);
    for (key, prop) in schema["properties"].as_object().into_iter().flatten() {
        let ty = prop["type"].as_str().unwrap_or("string");
        let named = |v: &Value| match v {
            Value::String(s) => choice(s.clone(), String::new()),
            v => choice(v.to_string(), String::new()),
        };
        let mut options: Vec<Choice> =
            prop["enum"].as_array().into_iter().flatten().map(named).collect();
        let names = prop["enumNames"].as_array().map(Vec::as_slice).unwrap_or_default();
        for (o, name) in options.iter_mut().zip(names) {
            o.value = std::mem::replace(&mut o.label, name.as_str().unwrap_or_default().into());
        }
        let titled = prop["oneOf"].as_array().into_iter().flatten();
        for one in titled {
            let value = text(one, "const");
            let label = Some(text(one, "title")).filter(|t| !t.is_empty()).unwrap_or(value.clone());
            options.push(choice(label, value));
        }
        let items = &prop["items"];
        let kind = match ty {
            "boolean" => FieldKind::Bool,
            "number" | "integer" => FieldKind::Number,
            "array" => {
                options = items["enum"].as_array().into_iter().flatten().map(named).collect();
                for one in items["anyOf"].as_array().into_iter().flatten() {
                    let value = text(one, "const");
                    let label = Some(text(one, "title")).filter(|t| !t.is_empty());
                    options.push(choice(label.unwrap_or(value.clone()), value));
                }
                FieldKind::Multi
            }
            _ if !options.is_empty() => FieldKind::Choice,
            _ => FieldKind::Text,
        };
        let default = match &prop["default"] {
            Value::Null => vec![],
            Value::Array(a) => {
                a.iter().map(|v| v.as_str().map_or(v.to_string(), str::to_string)).collect()
            }
            Value::String(s) => vec![s.clone()],
            v => vec![v.to_string()],
        };
        let label = Some(text(prop, "title")).filter(|t| !t.is_empty()).unwrap_or(key.clone());
        fields.push(Field {
            key: key.clone(),
            label,
            description: text(prop, "description"),
            kind,
            options,
            required: required.contains(&key.as_str()),
            default,
            ..Field::default()
        });
        keys.push((key.clone(), ty.to_string()));
    }
    (Form { message: message.to_string(), fields, ..Form::default() }, keys)
}

pub(crate) enum CodexAsk {
    Decision { session: bool, execpolicy: Option<Value>, network: (Option<Value>, Option<Value>) },
    Questions(Vec<String>),
    McpApproval,
    McpForm(Vec<(String, String)>),
    UrlForm,
    Permissions(Value),
}

pub(crate) fn typed(ty: &str, values: &[String]) -> Value {
    let first = values.first().cloned().unwrap_or_default();
    match ty {
        "boolean" => json!(first == "true"),
        "number" | "integer" => {
            first.parse::<serde_json::Number>().map_or_else(|_| json!(first), Value::Number)
        }
        "array" => json!(values),
        _ => json!(first),
    }
}

pub(crate) fn reply(s: &mut State, id: u64, answer: &Answer) -> Option<Value> {
    let (rpc, kind) = s.asks.remove(&id)?;
    let allowed = matches!(answer, Answer::Approve { .. } | Answer::Reply(_));
    let picked = match answer {
        Answer::Pick(id) => id.as_str(),
        _ => "",
    };
    if let Answer::DenyWith(note) = answer
        && s.active
        && s.turn_id.is_some()
    {
        let line = note_turn(s, note);
        s.outbox.push(line);
    }
    let declined = || json!({"action": "decline", "content": null, "_meta": null});
    let result = match (kind, answer) {
        (CodexAsk::Decision { session, execpolicy, network }, _) => {
            let decision = match (picked, execpolicy, network) {
                ("session", ..) if session => json!("acceptForSession"),
                ("execpolicy", Some(amendment), _) => {
                    json!({"acceptWithExecpolicyAmendment": {"execpolicy_amendment": amendment}})
                }
                ("network-allow", _, (Some(amendment), _))
                | ("network-deny", _, (_, Some(amendment))) => {
                    json!({"applyNetworkPolicyAmendment": {"network_policy_amendment": amendment}})
                }
                _ if allowed => json!("accept"),
                _ => json!("decline"),
            };
            json!({"decision": decision})
        }
        (CodexAsk::Questions(ids), Answer::Answers(values)) => {
            let answers: serde_json::Map<String, Value> = ids
                .into_iter()
                .zip(values)
                .map(|(id, labels)| (id, json!({"answers": labels})))
                .collect();
            json!({"answers": answers})
        }
        (CodexAsk::Questions(_), _) => json!({"answers": {}}),
        (CodexAsk::McpApproval, _) if allowed => {
            json!({"action": "accept", "content": {}, "_meta": null})
        }
        (CodexAsk::McpForm(keys), Answer::Form(values)) => {
            let content: serde_json::Map<String, Value> = values
                .iter()
                .map(|(key, vals)| {
                    let ty = keys.iter().find(|(k, _)| k == key).map_or("string", |(_, t)| t);
                    (key.clone(), typed(ty, vals))
                })
                .collect();
            json!({"action": "accept", "content": content, "_meta": null})
        }
        (CodexAsk::UrlForm, Answer::Form(_)) => {
            json!({"action": "accept", "content": null, "_meta": null})
        }
        (CodexAsk::McpApproval | CodexAsk::McpForm(_) | CodexAsk::UrlForm, _) => declined(),
        (CodexAsk::Permissions(req), _) if picked == "session" => {
            json!({"permissions": req, "scope": "session"})
        }
        (CodexAsk::Permissions(req), _) if allowed => json!({"permissions": req, "scope": "turn"}),
        (CodexAsk::Permissions(_), _) => json!({"permissions": {}, "scope": "turn"}),
    };
    Some(json!({"id": rpc, "result": result}))
}

pub(crate) fn server_request(s: &mut State, method: &str, v: &Value) -> Vec<Update> {
    let (id, p) = (v["id"].clone(), &v["params"]);
    let side = matches!(&s.naming, Naming::Side(side) if p["threadId"] == *side)
        || p["threadId"].as_str().is_some_and(|t| s.sides.contains_key(t));
    let (ask, kind) = match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" if side => {
            s.outbox.push(json!({"id": id, "result": {"decision": "decline"}}));
            return vec![];
        }
        "item/permissions/requestApproval" if side => {
            s.outbox.push(json!({"id": id, "result": {"permissions": {}, "scope": "turn"}}));
            return vec![];
        }
        "mcpServer/elicitation/request" if side => {
            let decline = json!({"action": "decline", "content": null, "_meta": null});
            s.outbox.push(json!({"id": id, "result": decline}));
            return vec![];
        }
        "item/tool/requestUserInput" if side => {
            s.outbox.push(json!({"id": id, "result": {"answers": {}}}));
            return vec![];
        }
        "item/commandExecution/requestApproval" => {
            let host = p["networkApprovalContext"]["host"].as_str();
            let reason = [
                p["reason"].as_str().map(str::to_string),
                host.map(|h| format!("Network access to {h}")),
            ];
            let reason: Vec<String> =
                reason.into_iter().flatten().filter(|r| !r.is_empty()).collect();
            let (options, kind) = decisions(&p["availableDecisions"], host);
            let ask = Ask::Approval(Approval {
                tool: "Bash".into(),
                title: "Run command".into(),
                input: command_text(p),
                reason: reason.join("\n"),
                tool_id: p["itemId"].as_str().map(str::to_string),
                kind: ApprovalKind::Command,
                cwd: p["cwd"].as_str().map(str::to_string),
                source: source(s, p),
                options,
                ..Approval::default()
            });
            (ask, kind)
        }
        "item/fileChange/requestApproval"
            if matches!(s.access, AccessMode::AutoEdit | AccessMode::Full) =>
        {
            s.outbox.push(json!({"id": id, "result": {"decision": "accept"}}));
            return vec![];
        }
        "item/fileChange/requestApproval" => {
            let item = p["itemId"].as_str().unwrap_or_default();
            let calls: Vec<&ToolCall> =
                (0..).map_while(|i| s.tools.get(&format!("{item}#{i}"))).collect();
            let paths: Vec<String> = calls.iter().map(|call| call.detail.summary()).collect();
            let diffs: Vec<String> = calls
                .iter()
                .filter_map(|call| match &call.detail {
                    ToolDetail::Edit { path, diff, .. } if !diff.is_empty() && calls.len() > 1 => {
                        Some(format!("{path}\n{diff}"))
                    }
                    ToolDetail::Edit { diff, .. } if !diff.is_empty() => Some(diff.clone()),
                    _ => None,
                })
                .collect();
            let ask = Ask::Approval(Approval {
                tool: "Edit".into(),
                title: "Edit file".into(),
                input: paths.join("\n"),
                reason: p["reason"].as_str().unwrap_or_default().to_string(),
                tool_id: calls.first().map(|call| call.id.clone()),
                kind: ApprovalKind::Edit,
                diff: Some(diffs.join("\n")).filter(|d| !d.is_empty()),
                source: source(s, p),
                options: vec![ApprovalOption {
                    id: "session".into(),
                    label: "Allow edits for the session".into(),
                }],
                ..Approval::default()
            });
            (ask, CodexAsk::Decision { session: true, execpolicy: None, network: (None, None) })
        }
        "item/tool/requestUserInput" => {
            let questions = p["questions"].as_array().map(Vec::as_slice).unwrap_or_default();
            if questions.is_empty() {
                s.outbox.push(json!({"id": id, "result": {"answers": {}}}));
                return vec![];
            }
            let text = |q: &Value, key: &str| q[key].as_str().unwrap_or_default().to_string();
            let ids = questions.iter().map(|q| text(q, "id")).collect();
            let asked = questions
                .iter()
                .map(|q| Question {
                    header: text(q, "header"),
                    question: text(q, "question"),
                    options: q["options"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|o| Choice {
                            label: text(o, "label"),
                            description: text(o, "description"),
                            ..Choice::default()
                        })
                        .collect(),
                    other: q["isOther"] == true,
                    secret: q["isSecret"] == true,
                    multi: false,
                })
                .collect();
            (Ask::Questions(asked), CodexAsk::Questions(ids))
        }
        "mcpServer/elicitation/request" if p["_meta"]["codex_approval_kind"] == "mcp_tool_call" => {
            let title = p["message"].as_str().unwrap_or("Approve app tool call?").to_string();
            let meta = &p["_meta"];
            let args = match (meta["tool_params_display"].as_str(), meta.get("tool_params")) {
                (Some(shown), _) => shown.to_string(),
                (None, Some(params)) => serde_json::to_string_pretty(params).unwrap_or_default(),
                (None, None) => {
                    let mut rest = meta.clone();
                    rest.as_object_mut().map(|m| m.remove("codex_approval_kind"));
                    serde_json::to_string_pretty(&rest).unwrap_or_default()
                }
            };
            let input = format!("{}\n{args}", p["serverName"].as_str().unwrap_or_default());
            let ask = Ask::Approval(Approval {
                tool: "mcp".into(),
                title,
                input,
                kind: ApprovalKind::Mcp,
                source: source(s, p),
                ..Approval::default()
            });
            (ask, CodexAsk::McpApproval)
        }
        "mcpServer/elicitation/request" if p["mode"] == "url" => {
            let server = p["serverName"].as_str().unwrap_or_default();
            let form = Form {
                title: format!("{server} needs you to open a link"),
                message: p["message"].as_str().unwrap_or_default().to_string(),
                url: p["url"].as_str().map(str::to_string),
                source: Some(server.to_string()),
                ..Form::default()
            };
            (Ask::Form(form), CodexAsk::UrlForm)
        }
        "mcpServer/elicitation/request"
            if matches!(p["mode"].as_str(), Some("form" | "openai/form" | "openaiForm")) =>
        {
            let server = p["serverName"].as_str().unwrap_or_default();
            let message = p["message"].as_str().unwrap_or_default();
            let (mut form, keys) = schema_form(message, &p["requestedSchema"]);
            form.title = server.to_string();
            form.source = Some(server.to_string());
            (Ask::Form(form), CodexAsk::McpForm(keys))
        }
        "item/permissions/requestApproval" => {
            let input = serde_json::to_string_pretty(&p["permissions"]).unwrap_or_default();
            let ask = Ask::Approval(Approval {
                tool: "permissions".into(),
                title: "Grant extra permissions".into(),
                input,
                reason: p["reason"].as_str().unwrap_or_default().to_string(),
                tool_id: p["itemId"].as_str().map(str::to_string),
                cwd: p["cwd"].as_str().map(str::to_string),
                source: source(s, p),
                options: vec![ApprovalOption {
                    id: "session".into(),
                    label: "Grant for the session".into(),
                }],
                ..Approval::default()
            });
            (ask, CodexAsk::Permissions(p["permissions"].clone()))
        }
        "mcpServer/elicitation/request" => {
            let decline = json!({"action": "decline", "content": null, "_meta": null});
            s.outbox.push(json!({"id": id, "result": decline}));
            let what = match p["mode"].as_str() {
                Some("openai/userVerification") => "a user verification Aether can't show".into(),
                mode => format!("a prompt Aether can't show ({})", mode.unwrap_or("unknown")),
            };
            return vec![Update::Warning(format!("Codex asked for {what}, so it was declined"))];
        }
        "currentTime/read" => {
            s.outbox.push(json!({"id": id, "result": {"currentTimeAt": now_ms() / 1000}}));
            return vec![];
        }
        _ => {
            let message = format!("Codex asked for {method}, which Aether doesn't support");
            s.outbox.push(json!({"id": id, "error": {"code": -32601, "message": message}}));
            return vec![Update::Warning(message)];
        }
    };
    s.next_ask += 1;
    s.asks.insert(s.next_ask, (id, kind));
    vec![Update::Ask { id: s.next_ask, ask }]
}

/// The spawn row's agent name when a child thread asks.
pub(crate) fn source(s: &State, p: &Value) -> Option<String> {
    let row = s.agents.get(p["threadId"].as_str()?)?;
    let named = |call: &ToolCall| match &call.detail {
        ToolDetail::Task { agent, .. } if !agent.is_empty() => agent.clone(),
        detail => detail.summary(),
    };
    Some(s.tools.get(row).map(named).filter(|n| !n.is_empty()).unwrap_or("subagent".into()))
}

pub(crate) fn decisions(available: &Value, host: Option<&str>) -> (Vec<ApprovalOption>, CodexAsk) {
    let default = json!(["accept", "acceptForSession", "decline"]);
    let list = available.as_array().or(default.as_array()).map(Vec::as_slice).unwrap_or_default();
    let (mut options, mut session, mut execpolicy, mut network) =
        (vec![], false, None, (None, None));
    let option = |id: &str, label: String| ApprovalOption { id: id.into(), label };
    for d in list {
        if d == "acceptForSession" {
            session = true;
            let label = match host {
                Some(host) => format!("Allow {host} for this conversation"),
                None => "Allow this command for the session".into(),
            };
            options.push(option("session", label));
        } else if let argv @ Value::Array(words) =
            &d["acceptWithExecpolicyAmendment"]["execpolicy_amendment"]
        {
            let words: Vec<&str> = words.iter().filter_map(Value::as_str).collect();
            options.push(option("execpolicy", format!("Always allow `{}`", words.join(" "))));
            execpolicy = Some(argv.clone());
        } else if let amendment @ Value::Object(_) =
            &d["applyNetworkPolicyAmendment"]["network_policy_amendment"]
        {
            let host = amendment["host"].as_str().unwrap_or_default();
            if amendment["action"] == "deny" {
                options
                    .push(option("network-deny", format!("Always block network access to {host}")));
                network.1 = Some(amendment.clone());
            } else {
                options.push(option(
                    "network-allow",
                    format!("Always allow network access to {host}"),
                ));
                network.0 = Some(amendment.clone());
            }
        }
    }
    (options, CodexAsk::Decision { session, execpolicy, network })
}
