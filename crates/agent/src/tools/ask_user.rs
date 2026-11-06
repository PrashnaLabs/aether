use std::sync::Arc;

use adk_rust::serde_json::{self, json};
use adk_rust::tool::FunctionTool;
use aether_core::{Answer, Ask};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::gate::Gate;

pub(crate) const NAME: &str = "ask_user";

#[derive(Serialize, Deserialize, JsonSchema)]
struct Args {
    /// The question, phrased for the user.
    question: String,
    /// Optional short answers the user can pick from.
    #[serde(default)]
    choices: Vec<String>,
}

pub(crate) fn tool(gate: Arc<Gate>) -> FunctionTool {
    FunctionTool::new(
        NAME,
        "Ask the user a question and wait for the answer. Use it when you need a decision or a \
         detail only the user can give, instead of guessing.",
        move |_, args| {
            let gate = gate.clone();
            async move {
                let Ok(Args { question, choices }) = serde_json::from_value(args) else {
                    return Ok(json!({ "error": "missing question" }));
                };
                Ok(match gate.ask(Ask::Question { question, choices }).await {
                    Some(Answer::Reply(answer)) => json!({ "answer": answer }),
                    _ => json!({ "error": "The user didn't answer." }),
                })
            }
        },
    )
    .with_parameters_schema::<Args>()
}
