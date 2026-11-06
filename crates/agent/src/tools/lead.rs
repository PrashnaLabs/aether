use std::sync::Arc;
use std::time::Duration;

use adk_rust::prelude::Tool;
use adk_rust::serde_json::{Value, json};
use adk_rust::{ToolContext, async_trait};
use futures::channel::mpsc::UnboundedSender;

use crate::orchestrator::{OrchCall, OrchPending, done_pending, tool_specs, try_queue};

const SILENT: &str = "Aether didn't answer";

struct LeadTool {
    name: String,
    description: String,
    schema: Value,
    key: String,
    calls: UnboundedSender<OrchCall>,
    pending: OrchPending,
}

#[async_trait]
impl Tool for LeadTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(self.schema.clone())
    }

    async fn execute(&self, _: Arc<dyn ToolContext>, args: Value) -> adk_rust::Result<Value> {
        let out = ask(
            self.key.clone(),
            self.calls.clone(),
            self.pending.clone(),
            self.name.clone(),
            args,
        )
        .await;
        Ok(out.unwrap_or_else(|error| json!({ "error": error })))
    }
}

async fn ask(
    lead: String,
    calls: UnboundedSender<OrchCall>,
    pending: OrchPending,
    tool: String,
    args: Value,
) -> Result<Value, String> {
    let (reply, rx) = std::sync::mpsc::channel();
    try_queue(&calls, &pending, OrchCall { lead, tool, args, reply })?;
    let out = tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(30)))
        .await
        .map_err(|_| SILENT.to_string())?;
    done_pending(&pending);
    out.map_err(|_| SILENT.to_string())?
}

pub(crate) fn lead_tools(
    key: String,
    calls: UnboundedSender<OrchCall>,
    pending: OrchPending,
) -> Vec<Arc<dyn Tool>> {
    let specs = tool_specs();
    specs
        .as_array()
        .into_iter()
        .flatten()
        .map(|spec| {
            let text = |field: &str| spec[field].as_str().unwrap_or_default().to_string();
            Arc::new(LeadTool {
                name: text("name"),
                description: text("description"),
                schema: spec["inputSchema"].clone(),
                key: key.clone(),
                calls: calls.clone(),
                pending: pending.clone(),
            }) as Arc<dyn Tool>
        })
        .collect()
}

#[cfg(test)]
#[path = "../../tests/unit/tools/lead.rs"]
mod tests;
