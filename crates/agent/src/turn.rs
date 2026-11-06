use std::collections::HashMap;
use std::sync::Arc;

use adk_rust::GenerateContentConfig;
use adk_rust::prelude::*;
use aether_core::{
    Attachment, DIRECT_INLINE_BYTES, INLINE_TEXT_BYTES, Reasoning, Settings, Skill, Update, is_text,
};
use futures::StreamExt;
use futures::channel::mpsc::UnboundedSender;

use crate::agent::{AGENT, APP, USER};
use crate::events;

const BASE_PROMPT: &str = "You are a helpful assistant.";
const SEARCH_PROMPT: &str = "Use the web_search tool for recent events or facts you are unsure \
    of, and cite the URLs you used.";
const ASK_PROMPT: &str = "If you need a decision or a detail only the user can give, ask with \
    ask_user instead of guessing.";
const PLAN_PROMPT: &str = "Plan mode is on. Before doing the work, call propose_plan with a \
    short numbered plan and wait. Follow the approved plan; if it's rejected, revise it with the \
    feedback and propose it again.";
const SKILLS_PROMPT: &str = "You have skills: instructions for particular kinds of tasks. When a \
    request matches a skill below, call load_skill with its name before answering, then follow \
    what it returns.";
const INVOKED_PROMPT: &str = "The user invoked these skills for this message. Follow them:";
/// Long descriptions are cut in the catalog; the full skill loads on demand.
const MAX_DESCRIPTION: usize = 400;

/// Everything needed to answer one user message.
pub(crate) struct Turn {
    pub api_key: String,
    pub settings: Settings,
    pub sessions: Arc<InMemorySessionService>,
    pub session: String,
    pub tools: Vec<Arc<dyn Tool>>,
    pub skills: Vec<Skill>,
    pub invoked: Vec<Skill>,
}

impl Turn {
    pub async fn run(
        self,
        text: String,
        attachments: Vec<Attachment>,
        tx: &UnboundedSender<Update>,
    ) -> Result<()> {
        let content = tokio::task::spawn_blocking(move || user_content(text, &attachments))
            .await
            .map_err(|e| AdkError::agent(format!("attachment read failed: {e}")))??;
        let llm = OpenRouterClient::new(OpenRouterConfig::new(self.api_key, &self.settings.model))?;
        let agent = self
            .tools
            .into_iter()
            .fold(LlmAgentBuilder::new(AGENT), |agent, tool| agent.tool(tool))
            .model(Arc::new(llm))
            .generate_content_config(request_config(self.settings.reasoning)?)
            .instruction(system_prompt(&self.settings, &self.skills, &self.invoked))
            .build()?;
        let runner = Runner::builder()
            .app_name(APP)
            .agent(Arc::new(agent))
            .session_service(self.sessions)
            .build()?;
        let mut events = runner.run_str(USER, &self.session, content).await?;
        let mut inputs = HashMap::new();
        while let Some(event) = events.next().await {
            for update in events::to_updates(event, &mut inputs) {
                if tx.unbounded_send(update).is_err() {
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}

fn user_content(text: String, files: &[Attachment]) -> Result<Content> {
    let mut content = Content::new("user");
    // an empty text part next to files can be refused upstream
    if !text.is_empty() || files.is_empty() {
        content = content.with_text(text);
    }
    for f in files {
        let bytes =
            std::fs::read(&f.path).map_err(|e| AdkError::agent(format!("{}: {e}", f.name)))?;
        if bytes.len() as u64 > DIRECT_INLINE_BYTES {
            return Err(AdkError::agent(format!("{} is over 10 MB", f.name)));
        }
        let inline = is_text(&f.mime) && bytes.len() as u64 <= INLINE_TEXT_BYTES;
        content = match std::str::from_utf8(&bytes) {
            Ok(contents) if inline => {
                content.with_text(format!("File: {}\n```\n{contents}\n```", f.name))
            }
            _ => content.with_inline_data(&f.mime, bytes),
        };
    }
    Ok(content)
}

fn request_config(reasoning: Reasoning) -> Result<GenerateContentConfig> {
    let mut config = GenerateContentConfig::default();
    if let Some(effort) = reasoning.effort() {
        OpenRouterRequestOptions::default()
            .with_reasoning(OpenRouterReasoningConfig {
                effort: Some(effort.into()),
                ..Default::default()
            })
            .insert_into_config(&mut config)?;
    }
    Ok(config)
}

fn system_prompt(settings: &Settings, skills: &[Skill], invoked: &[Skill]) -> String {
    let mut prompt = format!("{BASE_PROMPT} {ASK_PROMPT}");
    if settings.web_search {
        prompt = format!("{prompt} {SEARCH_PROMPT}");
    }
    if settings.plan_mode {
        prompt = format!("{prompt}\n\n{PLAN_PROMPT}");
    }
    if !skills.is_empty() {
        prompt = format!("{prompt}\n\n{SKILLS_PROMPT}");
        for skill in skills {
            let description: String = skill.description.chars().take(MAX_DESCRIPTION).collect();
            prompt = format!("{prompt}\n- {}: {description}", skill.name);
        }
    }
    if !invoked.is_empty() {
        prompt = format!("{prompt}\n\n{INVOKED_PROMPT}");
        for skill in invoked {
            prompt = format!("{prompt}\n\n## {}\n{}", skill.name, skill.instructions.trim());
        }
    }
    let instructions = settings.instructions.trim();
    if !instructions.is_empty() {
        prompt = format!("{prompt}\n\nUser instructions:\n{instructions}");
    }
    prompt
}

#[cfg(test)]
#[path = "../tests/unit/turn.rs"]
mod tests;
