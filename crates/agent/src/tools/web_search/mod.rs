mod duckduckgo;

use std::sync::Arc;

use adk_rust::serde_json::{self, json};
use adk_rust::tool::FunctionTool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::gate::Gate;

pub(crate) const NAME: &str = "web_search";

#[derive(Serialize, Deserialize, JsonSchema)]
struct Args {
    /// What to search for.
    query: String,
}

pub(crate) fn tool(client: reqwest::Client, gate: Arc<Gate>) -> FunctionTool {
    FunctionTool::new(
        NAME,
        "Search the web with DuckDuckGo. Returns result titles, URLs and snippets.",
        move |_, args| {
            let (client, gate) = (client.clone(), gate.clone());
            async move {
                let Ok(Args { query }) = serde_json::from_value(args) else {
                    return Ok(json!({ "error": "missing query" }));
                };
                let Some(query) = gate.approve(NAME, "Search the web", query).await else {
                    return Ok(json!({ "error": "The user declined this search." }));
                };
                Ok(match duckduckgo::search(&client, &query).await {
                    Ok(results) => json!({ "results": results }),
                    Err(error) => json!({ "error": error }),
                })
            }
        },
    )
    .with_parameters_schema::<Args>()
    .with_read_only(true)
}
