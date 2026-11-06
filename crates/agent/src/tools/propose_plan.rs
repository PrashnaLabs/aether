use std::sync::Arc;

use adk_rust::serde_json::{self, json};
use adk_rust::tool::FunctionTool;
use aether_core::{Answer, Ask, Plan};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::gate::Gate;

pub(crate) const NAME: &str = "propose_plan";

#[derive(Serialize, Deserialize, JsonSchema)]
struct Args {
    /// Short, concrete steps in the order you'll do them.
    steps: Vec<String>,
}

pub(crate) fn tool(gate: Arc<Gate>) -> FunctionTool {
    FunctionTool::new(
        NAME,
        "Show the user your plan and wait. They approve it (possibly edited) or reject it with \
         feedback.",
        move |_, args| {
            let gate = gate.clone();
            async move {
                let Ok(Args { steps }) = serde_json::from_value(args) else {
                    return Ok(json!({ "error": "missing steps" }));
                };
                Ok(match gate.ask(Ask::Plan(Plan { steps, ..Plan::default() })).await {
                    Some(Answer::Plan { approved: true, text }) => json!({
                        "approved": true,
                        "plan": text,
                        "next": "Carry out this plan now.",
                    }),
                    Some(Answer::Plan { approved: false, text }) => json!({
                        "approved": false,
                        "feedback": text,
                        "next": "Revise the plan with this feedback and propose it again.",
                    }),
                    _ => json!({ "error": "The user didn't respond to the plan." }),
                })
            }
        },
    )
    .with_parameters_schema::<Args>()
}
