use std::collections::HashMap;
use std::sync::Arc;

use adk_rust::serde_json::{self, json};
use adk_rust::tool::FunctionTool;
use aether_core::Skill;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::gate::Gate;

pub(crate) const NAME: &str = "load_skill";

#[derive(Serialize, Deserialize, JsonSchema)]
struct Args {
    /// The skill's name, exactly as listed.
    name: String,
}

pub(crate) fn tool(skills: &[Skill], gate: Arc<Gate>) -> FunctionTool {
    let skills: Arc<HashMap<String, String>> =
        Arc::new(skills.iter().map(|s| (s.name.clone(), s.instructions.clone())).collect());
    FunctionTool::new(
        NAME,
        "Load a skill's full instructions. Call it before answering when a request matches a \
         listed skill, then follow what it returns.",
        move |_, args| {
            let (skills, gate) = (skills.clone(), gate.clone());
            async move {
                let Ok(Args { name }) = serde_json::from_value(args) else {
                    return Ok(json!({ "error": "missing name" }));
                };
                let Some(name) = gate.approve(NAME, "Load a skill", name).await else {
                    return Ok(json!({ "error": "The user declined loading this skill." }));
                };
                Ok(match skills.get(&name) {
                    Some(instructions) => json!({ "name": name, "instructions": instructions }),
                    None => json!({ "error": format!("no skill named {name}") }),
                })
            }
        },
    )
    .with_parameters_schema::<Args>()
    .with_read_only(true)
}
