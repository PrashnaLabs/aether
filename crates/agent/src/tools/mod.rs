pub(crate) mod ask_user;
pub(crate) mod lead;
pub(crate) mod load_skill;
pub(crate) mod propose_plan;
pub(crate) mod web_search;

use std::sync::Arc;

use adk_rust::prelude::Tool;
use aether_core::{Settings, Skill};

use futures::channel::mpsc::UnboundedSender;

use crate::gate::Gate;
use crate::orchestrator::{OrchCall, OrchPending};

/// Builds each message's tools around that message's approval gate.
pub(crate) struct Tools {
    http: reqwest::Client,
}

impl Tools {
    pub(crate) fn new() -> Self {
        Self { http: reqwest::Client::new() }
    }

    pub(crate) fn enabled(
        &self,
        settings: &Settings,
        skills: &[Skill],
        gate: Arc<Gate>,
        lead: Option<(String, UnboundedSender<OrchCall>, OrchPending, bool)>,
    ) -> Vec<Arc<dyn Tool>> {
        let mut tools: Vec<Arc<dyn Tool>> = vec![Arc::new(ask_user::tool(gate.clone()))];
        if settings.web_search {
            tools.push(Arc::new(web_search::tool(self.http.clone(), gate.clone())));
        }
        if !skills.is_empty() {
            tools.push(Arc::new(load_skill::tool(skills, gate.clone())));
        }
        if settings.plan_mode {
            tools.push(Arc::new(propose_plan::tool(gate)));
        }
        if let Some((key, calls, pending, is_lead)) = lead {
            tools.extend(lead::lead_tools(key, calls, pending).into_iter().filter(|tool| {
                is_lead
                    || matches!(
                        tool.name(),
                        "list_cards" | "read_card" | "add_card" | "move_card" | "update_card"
                    )
            }));
        }
        tools
    }
}
