use serde::{Deserialize, Serialize};

use crate::lenient;

/// Something the agent needs from the user before it can continue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ask {
    /// Run `tool` with `input`, which the user may edit first.
    Approval(Approval),
    /// OpenRouter and old chats.
    Question {
        question: String,
        choices: Vec<String>,
    },
    Questions(Vec<Question>),
    Form(Form),
    Plan(Plan),
    Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub tool: String,
    pub title: String,
    /// Full command, path or payload. Never clipped.
    pub input: String,
    /// Why the CLI asks, when it says; empty otherwise.
    #[serde(default)]
    pub reason: String,
    /// Id of the tool row this approval is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: ApprovalKind,
    /// Unified diff of a proposed edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// The subagent or child session that asks. `None` for the main agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Scoped choices beyond allow once and deny, such as "Always allow `pnpm add *`".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ApprovalOption>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalKind {
    Command,
    Edit,
    Fetch,
    Mcp,
    #[default]
    Other,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalOption {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    #[serde(default)]
    pub header: String,
    pub question: String,
    #[serde(default)]
    pub options: Vec<Choice>,
    #[serde(default)]
    pub multi: bool,
    /// Offers a free-text "Something else" answer.
    #[serde(default)]
    pub other: bool,
    #[serde(default)]
    pub secret: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// What the CLI receives. Empty means `label`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Form {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub fields: Vec<Field>,
    /// A link to open first (MCP URL elicitation, OpenCode `external` field, sign-in).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The CLI drops the dialog after this long.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub key: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub kind: FieldKind,
    #[serde(default)]
    pub options: Vec<Choice>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Vec<String>,
    #[serde(default)]
    pub secret: bool,
    /// Also accepts typed text besides `options` (OpenCode `custom`).
    #[serde(default)]
    pub other: bool,
    /// Shown only while every rule holds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<When>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldKind {
    #[default]
    Text,
    Choice,
    Multi,
    Bool,
    Number,
    #[serde(other)]
    Unknown,
}

/// Holds when field `key`'s first value equals `value` (or differs, when `not`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct When {
    pub key: String,
    #[serde(default)]
    pub not: bool,
    pub value: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub markdown: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ApprovalOption>,
}

pub fn field_label(field: &Field) -> &str {
    if field.label.is_empty() { &field.key } else { &field.label }
}

pub fn field_visible(field: &Field, fields: &[Field], values: &[Vec<String>]) -> bool {
    field.when.iter().all(|rule| {
        let first = fields
            .iter()
            .position(|f| f.key == rule.key)
            .and_then(|i| values.get(i))
            .and_then(|v| v.first());
        (first == Some(&rule.value)) != rule.not
    })
}

/// `values[i]` belongs to `form.fields[i]`; hidden and empty fields are left out.
pub fn form_answer(form: &Form, values: &[Vec<String>]) -> Result<Answer, String> {
    let mut answer = Vec::new();
    for (i, field) in form.fields.iter().enumerate() {
        if !field_visible(field, &form.fields, values) {
            continue;
        }
        match values.get(i).filter(|v| !v.is_empty()) {
            Some(v) if field.kind == FieldKind::Number => match v[0].trim() {
                n if n.parse::<f64>().is_ok_and(f64::is_finite) => {
                    answer.push((field.key.clone(), vec![n.to_string()]))
                }
                _ => return Err(format!("{} must be a number", field_label(field))),
            },
            Some(v) => answer.push((field.key.clone(), v.clone())),
            None if field.required => return Err(format!("Fill in {}", field_label(field))),
            None => {}
        }
    }
    Ok(Answer::Form(answer))
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Answer {
    /// Run with this (possibly edited) input. `always` stops asking for the tool.
    Approve {
        input: String,
        always: bool,
    },
    Deny,
    Reply(String),
    /// The plan text if approved, otherwise the user's feedback.
    Plan {
        approved: bool,
        text: String,
    },
    /// One of the ask's `options`, by id.
    Pick(String),
    /// Deny, with a note for the agent.
    DenyWith(String),
    /// One entry per question: the picked labels plus any typed text. Empty means skipped.
    Answers(Vec<Vec<String>>),
    /// Field key and its values: `Choice::value` (or label), "true"/"false", typed text.
    Form(Vec<(String, Vec<String>)>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestStatus {
    Pending,
    Answered(Answer),
    /// The run ended before the user answered.
    Expired,
    /// Answered, cancelled or timed out on the CLI side.
    Withdrawn,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    /// Length of the reply text when asked; the card sits after it.
    pub at: usize,
    #[serde(deserialize_with = "lenient_ask")]
    pub ask: Ask,
    #[serde(deserialize_with = "lenient_status")]
    pub status: RequestStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked_ms: Option<i64>,
}

fn lenient_ask<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Ask, D::Error> {
    lenient(d, Ask::Unknown)
}

fn lenient_status<'de, D: serde::Deserializer<'de>>(d: D) -> Result<RequestStatus, D::Error> {
    lenient(d, RequestStatus::Expired)
}

#[cfg(test)]
#[path = "../tests/unit/request.rs"]
mod tests;
