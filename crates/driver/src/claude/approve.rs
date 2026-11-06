use serde_json::{Map, Value, json};

use aether_core::{
    Answer, Approval, ApprovalKind, ApprovalOption, Ask, Choice, Field, FieldKind, Form, Plan,
    Question, ToolDetail, Update,
};

use crate::tool::classify;

use super::events::claude_mode;
use super::{ClaudeAsk, Kind, State};

/// Our deny message; Claude echoes it as the call's error result.
pub(crate) const DENIED: &str = "Denied in Aether";

pub(crate) fn deny(message: &str) -> Value {
    json!({"behavior": "deny", "message": message})
}

pub(crate) fn control_response(request_id: &str, response: Value) -> Value {
    json!({
        "type": "control_response",
        "response": {"subtype": "success", "request_id": request_id, "response": response},
    })
}

pub(crate) const NOTES: [&str; 3] =
    ["(runs outside the sandbox)", "(runs in the background)", "(replaces every occurrence)"];

pub(crate) fn without_notes(mut text: &str) -> &str {
    text = text.trim_end();
    while let Some(rest) = NOTES.iter().find_map(|n| text.strip_suffix(n)) {
        text = rest.trim_end();
    }
    text
}

pub(crate) fn is_edit(tool: &str) -> bool {
    matches!(tool, "Write" | "Edit" | "MultiEdit" | "NotebookEdit")
}

pub(crate) fn approval_input(tool: &str, input: &Value, detail: &ToolDetail) -> String {
    let flag = |k: &str| input[k] == true;
    let mut out = match tool {
        "Bash" => input["command"].as_str().unwrap_or_default().to_string(),
        t if is_edit(t) => match detail {
            ToolDetail::Edit { path, .. } => path.clone(),
            _ => String::new(),
        },
        _ => serde_json::to_string_pretty(input).unwrap_or_default(),
    };
    let notes = match tool {
        "Bash" => {
            [(flag("dangerouslyDisableSandbox"), NOTES[0]), (flag("run_in_background"), NOTES[1])]
        }
        "Edit" => [(flag("replace_all"), NOTES[2]), (false, "")],
        _ => [(false, ""); 2],
    };
    for (_, note) in notes.into_iter().filter(|(on, _)| *on) {
        out.push('\n');
        out.push_str(note);
    }
    out
}

pub(crate) fn approval_kind(tool: &str) -> ApprovalKind {
    match tool {
        "Bash" => ApprovalKind::Command,
        t if is_edit(t) => ApprovalKind::Edit,
        "WebFetch" | "WebSearch" => ApprovalKind::Fetch,
        t if t.starts_with("mcp__") => ApprovalKind::Mcp,
        _ => ApprovalKind::Other,
    }
}

pub(crate) fn approval_options(suggestions: &[Value]) -> Vec<ApprovalOption> {
    let strings = |v: &Value| -> Vec<String> {
        v.as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_string)).collect()
    };
    suggestions
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let label = match s["type"].as_str()? {
                "addRules" => {
                    let rules: Vec<String> = s["rules"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|r| match r["ruleContent"].as_str().filter(|c| !c.is_empty()) {
                            Some(c) => Some(format!("`{}`", c.replace(":*", " *"))),
                            None => r["toolName"].as_str().map(str::to_string),
                        })
                        .collect();
                    (!rules.is_empty())
                        .then(|| format!("Allow {} for this chat", rules.join(", ")))?
                }
                "addDirectories" => {
                    let dirs = strings(&s["directories"]);
                    (!dirs.is_empty())
                        .then(|| format!("Allow edits in {} for this chat", dirs.join(", ")))?
                }
                "setMode" => {
                    let mode = s["mode"].as_str()?;
                    let mode = if mode == "acceptEdits" { "Accept edits" } else { mode };
                    format!("Allow and switch this chat to {mode}")
                }
                _ => return None,
            };
            Some(ApprovalOption { id: format!("s-{i}"), label })
        })
        .collect()
}

/// The picked suggestion, scoped to this session.
pub(crate) fn picked(s: &mut State, suggestions: &[Value], id: &str) -> Option<Value> {
    let mut rule = suggestions.get(id.strip_prefix("s-")?.parse::<usize>().ok()?)?.clone();
    rule["destination"] = "session".into();
    if let Some(mode) = rule["mode"].as_str().and_then(claude_mode) {
        s.requested = mode;
    }
    Some(rule)
}

pub(crate) fn choices(list: &Value, names: &Value) -> Vec<Choice> {
    let names = names.as_array();
    let option = |i: usize, value: &str, title: Option<&str>| {
        let label = title.or_else(|| names?.get(i)?.as_str()).unwrap_or(value);
        Choice { label: label.into(), value: value.into(), ..Choice::default() }
    };
    list.as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(i, o)| match o.as_str() {
            Some(v) => Some(option(i, v, None)),
            None => Some(option(i, o["const"].as_str()?, o["title"].as_str())),
        })
        .collect()
}

pub(crate) fn elicitation_form(r: &Value) -> (Form, Vec<(String, String)>) {
    let schema = &r["requested_schema"];
    let required: Vec<&str> =
        schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let mut fields = Vec::new();
    let mut types = Vec::new();
    for (key, spec) in schema["properties"].as_object().into_iter().flatten() {
        let (kind, options) = match spec["type"].as_str() {
            Some("boolean") => (FieldKind::Bool, vec![]),
            Some("number" | "integer") => (FieldKind::Number, vec![]),
            Some("array") => {
                let items = &spec["items"];
                let list = if items["anyOf"].is_array() { &items["anyOf"] } else { &items["enum"] };
                (FieldKind::Multi, choices(list, &Value::Null))
            }
            _ if spec["oneOf"].is_array() => {
                (FieldKind::Choice, choices(&spec["oneOf"], &Value::Null))
            }
            _ if spec["enum"].is_array() => {
                (FieldKind::Choice, choices(&spec["enum"], &spec["enumNames"]))
            }
            _ => (FieldKind::Text, vec![]),
        };
        let text = |k: &str| spec[k].as_str().unwrap_or_default().to_string();
        let default = match &spec["default"] {
            Value::Null => vec![],
            Value::String(s) => vec![s.clone()],
            Value::Array(a) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            v => vec![v.to_string()],
        };
        fields.push(Field {
            key: key.clone(),
            label: text("title"),
            description: text("description"),
            kind,
            options,
            required: required.contains(&key.as_str()),
            default,
            ..Field::default()
        });
        types.push((key.clone(), spec["type"].as_str().unwrap_or("string").to_string()));
    }
    let text = |k: &str| r[k].as_str().unwrap_or_default().to_string();
    let url = (r["mode"] == "url").then(|| text("url")).filter(|u| !u.is_empty());
    let form = Form {
        title: text("title"),
        message: text("message"),
        fields,
        url,
        source: r["mcp_server_name"].as_str().map(str::to_string),
        ..Form::default()
    };
    (form, types)
}

pub(crate) fn allow_with(input: Value, rule: Option<Value>) -> Value {
    let mut allow = json!({"behavior": "allow", "updatedInput": input});
    if let Some(rule) = rule {
        allow["updatedPermissions"] = json!([rule]);
    }
    allow
}

pub(crate) fn reply(s: &mut State, id: u64, answer: &Answer) -> Option<Value> {
    let ask = s.asks.remove(&id)?;
    let note = |text: &str| if text.trim().is_empty() { deny(DENIED) } else { deny(text) };
    let response = match ask.kind {
        Kind::Approval { tool, shown, suggestions } => match answer {
            Answer::Approve { input: edited, .. } => {
                let mut input = ask.input;
                let edited = edited.trim_end();
                let edited = without_notes(edited);
                if tool == "Bash" && !edited.is_empty() && edited != without_notes(&shown) {
                    input["command"] = edited.into();
                }
                allow_with(input, None)
            }
            Answer::Pick(pick) => match picked(s, &suggestions, pick) {
                Some(rule) => allow_with(ask.input, Some(rule)),
                None => deny(DENIED),
            },
            Answer::Reply(_) => allow_with(ask.input, None),
            Answer::DenyWith(text) => note(text),
            _ => deny(DENIED),
        },
        Kind::Questions(questions) => match answer {
            Answer::Answers(vals) => {
                let answers: Map<String, Value> = questions
                    .into_iter()
                    .enumerate()
                    .map(|(i, q)| (q, vals.get(i).map(|v| v.join(", ")).unwrap_or_default().into()))
                    .collect();
                allow_with(json!({"questions": ask.input["questions"], "answers": answers}), None)
            }
            Answer::DenyWith(text) => note(text),
            _ => deny(DENIED),
        },
        Kind::Plan { suggestions } => match answer {
            Answer::Plan { approved: true, .. } => allow_with(ask.input, None),
            Answer::Pick(pick) => match picked(s, &suggestions, pick) {
                Some(rule) => allow_with(ask.input, Some(rule)),
                None => deny(DENIED),
            },
            Answer::Plan { text, .. } | Answer::DenyWith(text) => note(text),
            _ => deny(DENIED),
        },
        Kind::Elicitation(types) => match answer {
            Answer::Form(values) => {
                let content: Map<String, Value> = values
                    .iter()
                    .map(|(key, vals)| {
                        let kind =
                            types.iter().find(|(k, _)| k == key).map_or("string", |(_, t)| t);
                        let first = vals.first().map(String::as_str).unwrap_or_default();
                        let value = match kind {
                            "boolean" => (first == "true").into(),
                            "number" | "integer" => {
                                serde_json::from_str(first).unwrap_or_else(|_| first.into())
                            }
                            "array" => vals.clone().into(),
                            _ => vals.join(", ").into(),
                        };
                        (key.clone(), value)
                    })
                    .collect();
                json!({"action": "accept", "content": content})
            }
            _ => json!({"action": "decline"}),
        },
    };
    Some(control_response(&ask.request_id, response))
}

pub(crate) fn control_request(s: &mut State, v: &Value) -> Vec<Update> {
    let (r, request_id) = (&v["request"], v["request_id"].as_str().unwrap_or_default());
    let subtype = r["subtype"].as_str().unwrap_or_default();
    let (kind, ask) = match subtype {
        "can_use_tool" => can_use_tool(s, r),
        "elicitation" => {
            let (form, types) = elicitation_form(r);
            (Kind::Elicitation(types), Ask::Form(form))
        }
        "request_user_dialog" => {
            let dialog = r["dialog_kind"].as_str().unwrap_or("unknown");
            s.outbox.push(control_response(request_id, json!({"behavior": "cancelled"})));
            return vec![Update::Warning(format!(
                "Claude asked for a {dialog} dialog Aether can't show yet"
            ))];
        }
        _ => {
            s.outbox.push(json!({
                "type": "control_response",
                "response": {"subtype": "error", "request_id": request_id, "error": "unsupported in aether"},
            }));
            let what = if subtype.is_empty() { String::new() } else { format!("{subtype} ") };
            return vec![Update::Warning(format!("Claude sent an unsupported {what}request"))];
        }
    };
    s.next_ask += 1;
    let entry = ClaudeAsk {
        request_id: request_id.to_string(),
        input: r["input"].clone(),
        kind,
        agent: r["agent_id"].as_str().map(str::to_string),
        card: ask.clone(),
    };
    s.asks.insert(s.next_ask, entry);
    vec![Update::Ask { id: s.next_ask, ask }]
}

pub(crate) fn can_use_tool(s: &State, r: &Value) -> (Kind, Ask) {
    let (tool, input) = (r["tool_name"].as_str().unwrap_or_default(), &r["input"]);
    let suggestions = r["permission_suggestions"].as_array().cloned().unwrap_or_default();
    match tool {
        "AskUserQuestion" => {
            let qs: Vec<&Value> = input["questions"].as_array().into_iter().flatten().collect();
            let text = |q: &Value, k: &str| q[k].as_str().unwrap_or_default().to_string();
            let asked = qs.iter().map(|q| text(q, "question")).collect();
            let questions = qs.iter().map(|q| Question {
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
                multi: q["multiSelect"] == true,
                other: true,
                ..Question::default()
            });
            (Kind::Questions(asked), Ask::Questions(questions.collect()))
        }
        "ExitPlanMode" => {
            let markdown = input["plan"].as_str().unwrap_or_default().to_string();
            let options = approval_options(&suggestions);
            (Kind::Plan { suggestions }, Ask::Plan(Plan { markdown, options, ..Plan::default() }))
        }
        _ => {
            let detail = classify(tool, input).1;
            let shown = approval_input(tool, input, &detail);
            let display = r["display_name"].as_str().unwrap_or(tool);
            let title = match r["description"].as_str().filter(|d| !d.is_empty()) {
                Some(d) => format!("{display} {d}"),
                None => display.to_string(),
            };
            let text = |k: &str| r[k].as_str().filter(|t| !t.is_empty());
            let reason = match (text("decision_reason"), text("blocked_path")) {
                (Some(why), Some(path)) => format!("{why} ({path})"),
                (Some(why), None) => why.to_string(),
                (None, Some(path)) => format!("Blocked path: {path}"),
                (None, None) => String::new(),
            };
            let diff = match detail {
                ToolDetail::Edit { diff, .. } if is_edit(tool) => Some(diff),
                _ => None,
            };
            let approval = Approval {
                tool: tool.to_string(),
                title,
                input: shown.clone(),
                reason,
                tool_id: r["tool_use_id"].as_str().map(str::to_string),
                kind: approval_kind(tool),
                diff,
                source: r["agent_id"].as_str().and_then(|a| s.agents.get(a)).cloned(),
                options: approval_options(&suggestions),
                ..Approval::default()
            };
            (Kind::Approval { tool: tool.to_string(), shown, suggestions }, Ask::Approval(approval))
        }
    }
}
