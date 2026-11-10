use std::collections::HashMap;
use std::path::Path;

use aether_agent::orchestrator::{OrchCall, history_specs, tool_specs};
use aether_agent::usage::best_provider;
use aether_core::{AccessMode, Message, Provider, Role, ToolDetail};
use aether_driver::ProviderProbe;
use aether_driver::antigravity::AgyEntry;
use aether_ui::ChatStatus;
use gpui_kit::{Context, Window};
use serde_json::{Value, json};

use super::providers::{USAGE_STALE_MS, initial_model};
use super::{
    Bulk, CLIS, Chat, ChatApp, ChatViewMode, Delivery, Queued, chat_cwd, now_ms, project_dir,
};
use crate::board::model::{Col, lead_move, open_lines};
use crate::session_workspace::SplitAxis;

pub(crate) fn lead_capable(provider: Provider) -> bool {
    provider == Provider::OpenRouter || CLIS.contains(&provider)
}

pub(crate) fn shows_lead_chip(chat: &Chat) -> bool {
    chat.parent.is_none() && lead_capable(chat.provider)
}

pub(crate) fn spawned_child(detail: &ToolDetail) -> Option<i64> {
    let ToolDetail::Other { name, output, .. } = detail else { return None };
    let leaf = name.rsplit("__").next().unwrap_or(name);
    if leaf.strip_prefix("aether_").unwrap_or(leaf) != "spawn_agent" {
        return None;
    }
    serde_json::from_str::<Value>(output).ok()?.get("id")?.as_i64()
}

pub(super) fn needs_agy_registration(
    provider: Provider,
    entry: Option<&AgyEntry>,
    exe: Option<&Path>,
) -> bool {
    let live = entry.is_some_and(|e| !e.disabled && exe == Some(Path::new(&e.command)));
    provider == Provider::Antigravity && !live
}

#[derive(Debug)]
pub(super) struct SpawnArgs {
    pub(super) provider: Provider,
    pub(super) model: String,
    pub(super) effort: String,
    pub(super) title: String,
    pub(super) task: String,
    pub(super) worktree: bool,
    pub(super) nonce: String,
    pub(super) card: Option<i64>,
}

impl SpawnArgs {
    pub(super) fn parse(
        args: &Value,
        probes: &HashMap<Provider, ProviderProbe>,
    ) -> Result<Self, String> {
        let text = |k: &str| args[k].as_str().unwrap_or_default().trim().to_string();
        let need = |k: &str| {
            Some(text(k)).filter(|s| !s.is_empty()).ok_or_else(|| format!(r#"missing "{k}""#))
        };
        let key = text("provider");
        let provider = CLIS.into_iter().find(|p| p.key() == key).ok_or_else(|| {
            let keys = CLIS.map(Provider::key).join(", ");
            format!(r#"unknown provider "{key}"; use one of {keys}"#)
        })?;
        let probe = probes
            .get(&provider)
            .filter(|p| p.installed)
            .ok_or_else(|| format!("{key} is not installed"))?;
        let model = text("model");
        if !model.is_empty()
            && !probe.models.is_empty()
            && !probe.models.iter().any(|m| m.id == model)
        {
            return Err(format!(r#"unknown model "{model}" for {key}; call the list_agents tool"#));
        }
        let worktree = match &args["worktree"] {
            Value::Null => false,
            Value::Bool(b) => *b,
            _ => return Err(r#""worktree" must be true or false"#.into()),
        };
        Ok(Self {
            provider,
            model,
            effort: text("effort"),
            title: need("title")?,
            task: need("task")?,
            worktree,
            nonce: text("nonce"),
            card: args["card"].as_i64(),
        })
    }
}

fn rank(mode: AccessMode) -> u8 {
    match mode {
        AccessMode::ReadOnly => 0,
        AccessMode::Ask => 1,
        AccessMode::AutoEdit => 2,
        AccessMode::Auto => 3,
        AccessMode::Full => 4,
    }
}

pub(super) fn child_access(lead: AccessMode, child: Provider) -> Result<AccessMode, String> {
    AccessMode::for_provider(child)
        .iter()
        .copied()
        .filter(|&m| rank(m) <= rank(lead))
        .max_by_key(|&m| rank(m))
        .ok_or_else(|| format!("{} can't run as restricted as this lead ({lead:?})", child.key()))
}

pub(super) fn child_prompt(task: &str) -> String {
    format!(
        "{task}\n\nWhen you finish, end your reply with a short summary of what you did and which files you changed."
    )
}

fn starting(chat: &Chat) -> bool {
    chat.worktree_job.is_some() || chat.setup.as_ref().is_some_and(|j| j.failed.is_none())
}

pub(super) fn child_status(chat: &Chat) -> &'static str {
    let last = chat.messages.last();
    if last.and_then(Message::pending_request).is_some() {
        "needs_you"
    } else if chat.busy() {
        "running"
    } else if starting(chat) {
        "starting"
    } else if last.is_some_and(|m| m.failed) {
        "failed"
    } else {
        "idle"
    }
}

pub(crate) fn child_status_label(chat: &Chat) -> &'static str {
    match child_status(chat) {
        "needs_you" => "Needs you",
        "running" => "Running",
        "starting" => "Starting",
        "failed" => "Failed",
        _ if unread_done(chat) => "Done, unread",
        _ => "Done",
    }
}

/// Finished but the lead hasn't read its last reply yet.
pub(crate) fn unread_done(chat: &Chat) -> bool {
    if chat.busy() || starting(chat) || child_status(chat) == "needs_you" {
        return false;
    }
    chat.messages
        .iter()
        .rposition(|m| m.role == Role::Assistant && m.ended_at_ms.is_some())
        .is_some_and(|last| chat.lead_seen.is_none_or(|seen| last > seen))
}

fn crew_rank(chat: &Chat) -> u8 {
    match child_status(chat) {
        "needs_you" => 0,
        "failed" => 1,
        "running" | "starting" => 2,
        _ if unread_done(chat) => 3,
        _ => 4,
    }
}

pub(crate) fn clip(text: &str) -> String {
    match text.char_indices().nth(4000) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text.to_string(),
    }
}

fn reply_body(reply: &Message, failed: bool) -> String {
    match &reply.error {
        Some(error) if failed && reply.text.is_empty() => error.clone(),
        Some(error) if failed => format!("{error}\n\n{}", reply.text),
        _ => reply.text.clone(),
    }
}

fn delivery_head(child: &Chat, outcome: &str) -> String {
    format!(
        "[Aether child result] Agent {} \"{}\" ({}/{}) {outcome}.",
        child.id,
        child.title,
        child.provider.key(),
        child.provider_model,
    )
}

const SEP: &str = "\n\n---\n\n";

pub(super) fn unread(deliveries: Vec<Delivery>, chats: &[Chat]) -> Vec<Delivery> {
    deliveries
        .into_iter()
        .filter(|d| {
            let seen = chats.iter().find(|c| c.id == d.child).and_then(|c| c.lead_seen);
            d.reply_ix.is_none_or(|r| seen < Some(r))
        })
        .collect()
}

/// Texts of queued prompts to `chat` that a failure strands, in queue order.
/// Child-result deliveries for a lead are kept; they still get their turn.
pub(crate) fn stranded_texts(chat: &mut Chat) -> Vec<String> {
    let mut dropped = Vec::new();
    chat.queue.retain(|q| {
        if q.from_children {
            true
        } else {
            dropped.push(q.text.clone());
            false
        }
    });
    dropped
}

pub(super) fn delivery_text(child: &Chat, reply: &Message, outcome: &str) -> String {
    let body = reply_body(reply, outcome == "failed");
    format!(
        "{}\n\n{}\n\nUse the read_agent tool with id {} for more.",
        delivery_head(child, outcome),
        clip(&body),
        child.id,
    )
}

// ponytail: sync git on the UI thread (3s timeout), move off-thread if spawns stutter
pub(super) fn worktree_base(lead: &Chat, folder: &Path) -> Result<String, String> {
    let info = aether_git::branch_info(folder)
        .ok_or("the lead's folder isn't a git repository; spawn without worktree")?;
    if info.detached && lead.worktree_branch.is_empty() {
        return Err(
            "the lead's folder is on a detached HEAD; check out a branch or spawn without worktree"
                .into(),
        );
    }
    Ok(if lead.worktree_branch.is_empty() { info.branch } else { lead.worktree_branch.clone() })
}

impl ChatApp {
    pub(super) fn orchestrate(
        &mut self,
        call: OrchCall,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = match self.chats.iter().position(|c| c.id.to_string() == call.lead) {
            None => Err("lead chat not found".to_string()),
            Some(ix) => match call.tool.as_str() {
                "list_tools" if self.chats[ix].lead => Ok(tool_specs()),
                "list_tools" => {
                    let mut specs = aether_agent::orchestrator::board_specs();
                    specs
                        .as_array_mut()
                        .unwrap()
                        .extend(history_specs().as_array().unwrap().iter().cloned());
                    Ok(specs)
                }
                "read_history" => self.read_history(ix, &call.args),
                _ if !self.chats[ix].lead
                    && !matches!(call.tool.as_str(), "list_cards" | "add_card" | "move_card") =>
                {
                    Err("lead chat not found".to_string())
                }
                "list_agents" => Ok(self.list_agents(ix)),
                "spawn_agent" => self.spawn_agent(ix, &call.args, window, cx),
                "read_agent" => self.read_agent(ix, &call.args),
                "message_agent" => self.message_agent(ix, &call.args, window, cx),
                "stop_agent" => self.stop_agent(ix, &call.args, cx),
                "list_cards" => {
                    let project = self.lead_project(ix);
                    let cards: Vec<_> =
                        self.cards.iter().filter(|c| c.project == project).cloned().collect();
                    Ok(Value::String(open_lines(&cards)))
                }
                "add_card" => self.add_card(ix, &call.args),
                "move_card" => self.move_card(ix, &call.args),
                "list_chats" => self.list_chats(ix, &call.args),
                "archive_chats" => self.ask_bulk(ix, Bulk::Archive, &call.args, window, cx),
                "delete_chats" => self.ask_bulk(ix, Bulk::Delete, &call.args, window, cx),
                "unarchive_chats" => self.unarchive_chats(ix, &call.args, window, cx),
                "update_chat" => self.update_chat(&call.args, cx),
                "stop_chat" => self.stop_any_chat(ix, &call.args, cx),
                "open_chat" => self.open_chat_tool(&call.args, window, cx),
                "delete_cards" => self.delete_cards(ix, &call.args, window, cx),
                tool => Err(format!(r#"unknown tool "{tool}""#)),
            },
        };
        let _ = call.reply.send(result);
    }

    pub(super) fn list_agents(&self, lead_ix: usize) -> Value {
        let agents: Vec<Value> = CLIS
            .iter()
            .filter_map(|p| {
                let probe = self.probes.get(p).filter(|probe| probe.installed)?;
                let models: Vec<Value> = probe
                    .models
                    .iter()
                    .map(|m| json!({"id": m.id, "efforts": m.efforts.clone().unwrap_or_default()}))
                    .collect();
                Some(json!({"provider": p.key(), "label": p.label(), "models": models}))
            })
            .collect();
        let lead = self.chats[lead_ix].id;
        let children: Vec<Value> = self
            .chats
            .iter()
            .filter(|c| c.parent == Some(lead) && !c.archived)
            .map(|c| {
                json!({
                    "id": c.id,
                    "title": c.title.as_ref(),
                    "provider": c.provider.key(),
                    "model": c.provider_model,
                    "status": child_status(c),
                })
            })
            .collect();
        json!({"agents": agents, "children": children})
    }

    fn add_card(&mut self, lead_ix: usize, args: &Value) -> Result<Value, String> {
        let text = |k: &str| args[k].as_str().unwrap_or_default().trim().to_string();
        let title = text("title");
        if title.is_empty() {
            return Err(r#"missing "title""#.into());
        }
        let col = match text("col").as_str() {
            "" => Col::Todo,
            s => Col::parse(s).filter(|&c| c != Col::Done).ok_or_else(|| {
                format!(r#"unknown column "{s}"; use backlog, todo, doing or review"#)
            })?,
        };
        let id = self.board_add(
            title.clone(),
            text("body"),
            col,
            self.lead_project(lead_ix),
            None,
            true,
        );
        Ok(Value::String(format!("Created #{id} in {}: {title}", col.key())))
    }

    /// Board project of a lead: the folder it started in, even from a worktree.
    pub(super) fn lead_project(&self, lead_ix: usize) -> String {
        project_dir(&self.chats[lead_ix], &self.settings.workspace_root).display().to_string()
    }

    fn move_card(&mut self, lead_ix: usize, args: &Value) -> Result<Value, String> {
        let id = args["id"]
            .as_i64()
            .or_else(|| args["id"].as_str().and_then(|s| s.trim().parse().ok()))
            .ok_or_else(|| r#"missing "id""#.to_string())?;
        let to = args["col"].as_str().unwrap_or_default();
        let project = self.lead_project(lead_ix);
        let card = self
            .cards
            .iter_mut()
            .find(|c| c.id == id && c.project == project)
            .ok_or_else(|| format!("card #{id} not found"))?;
        let msg = lead_move(card, to)?;
        let col = card.col;
        self.board_move(id, col, None);
        Ok(Value::String(msg))
    }

    pub(super) fn own_child(&self, lead_ix: usize, args: &Value) -> Result<usize, String> {
        let id = args["id"]
            .as_i64()
            .or_else(|| args["id"].as_str().and_then(|s| s.trim().parse().ok()))
            .ok_or_else(|| r#"missing "id""#.to_string())?;
        let lead = self.chats[lead_ix].id;
        self.chats
            .iter()
            .position(|c| c.id == id && c.parent == Some(lead) && !c.archived)
            .ok_or_else(|| format!("agent {id} isn't one of this lead's children"))
    }

    pub(super) fn read_agent(&mut self, lead_ix: usize, args: &Value) -> Result<Value, String> {
        let ix = self.own_child(lead_ix, args)?;
        let chat = &self.chats[ix];
        let last = args["last"].as_i64().unwrap_or(1).clamp(1, 10) as usize;
        let mut replies: Vec<String> = chat
            .messages
            .iter()
            .rev()
            .filter(|m| m.role == Role::Assistant)
            .take(last)
            .map(|m| clip(&reply_body(m, m.failed)))
            .collect();
        replies.reverse();
        let status = child_status(chat);
        let mut v = json!({
            "id": chat.id,
            "title": chat.title.as_ref(),
            "provider": chat.provider.key(),
            "model": chat.provider_model,
            "status": status,
            "cwd": chat_cwd(chat, &self.settings.workspace_root).display().to_string(),
            "branch": chat.worktree_branch,
            "replies": replies,
        });
        if chat.busy() || starting(chat) {
            v["note"] = "still running; this is partial output. Its final result will be sent to you automatically — end your turn instead of polling.".into();
        } else {
            let chat = &mut self.chats[ix];
            chat.lead_seen = chat
                .messages
                .iter()
                .rposition(|m| m.role == Role::Assistant && m.ended_at_ms.is_some());
            self.persist(ix);
            self.drop_read_deliveries(lead_ix);
        }
        Ok(v)
    }

    pub(super) fn message_agent(
        &mut self,
        lead_ix: usize,
        args: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let ix = self.own_child(lead_ix, args)?;
        let text = args["text"].as_str().unwrap_or_default().trim().to_string();
        if text.is_empty() {
            return Err(r#"missing "text""#.into());
        }
        let id = self.chats[ix].id;
        if self.chats[ix].messages.is_empty() && self.chats[ix].worktree_error.is_some() {
            return Err(format!("agent {id} has no worktree; spawn a new one"));
        }
        if self.locked(ix) {
            return Err(format!(
                "agent {id} is open in a terminal; it can't take messages from Aether right now"
            ));
        }
        let chat = &mut self.chats[ix];
        if chat.stopped && chat.busy() {
            return Err(format!(
                "agent {id} is stopping; message it after its \"was stopped\" result arrives"
            ));
        }
        let queued = Queued {
            text,
            attachments: vec![],
            delivering: false,
            from_children: false,
            deliveries: vec![],
        };
        if chat.busy() {
            chat.queue.push(queued);
            return Ok(json!({"status": "queued"}));
        }
        if let Some(reason) = self.worktree_blocks_send(ix) {
            return Err(reason.to_string());
        }
        let behind = self.chats[ix].queue.len();
        let Queued { text, attachments, .. } = if behind == 0 {
            queued
        } else {
            let queue = &mut self.chats[ix].queue;
            queue.push(queued);
            queue.remove(0)
        };
        let follow = self.follows(ix, cx);
        self.submit_as(ix, text, attachments, follow, window, cx);
        if behind == 0 {
            Ok(json!({"status": "sent"}))
        } else {
            Ok(json!({
                "status": "queued",
                "note": format!(
                    "agent {id} was idle with {behind} queued message(s); the oldest queued text was sent now, and your text is queued behind it"
                ),
            }))
        }
    }

    pub(super) fn stop_agent(
        &mut self,
        lead_ix: usize,
        args: &Value,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let ix = self.own_child(lead_ix, args)?;
        let busy = self.chats[ix].busy();
        if self.chats[ix].cancel.is_some() {
            self.stopped_by_lead.insert(self.chats[ix].id);
        }
        if busy {
            self.stop_chat(ix, cx);
        }
        Ok(json!({"stopped": busy}))
    }

    pub(super) fn enqueue_delivery(&mut self, lead_ix: usize, delivery: Delivery) {
        let queue = &mut self.chats[lead_ix].queue;
        match queue.last_mut() {
            Some(q) if q.from_children && !q.delivering => {
                q.text.push_str(SEP);
                q.text.push_str(&delivery.text);
                q.deliveries.push(delivery);
            }
            _ => queue.push(Queued {
                text: delivery.text.clone(),
                attachments: vec![],
                delivering: false,
                from_children: true,
                deliveries: vec![delivery],
            }),
        }
        self.persist(lead_ix);
    }

    pub(super) fn drop_read_deliveries(&mut self, lead_ix: usize) {
        let mut queue = std::mem::take(&mut self.chats[lead_ix].queue);
        queue.retain_mut(|q| {
            if q.deliveries.is_empty() || q.delivering {
                return true;
            }
            q.deliveries = unread(std::mem::take(&mut q.deliveries), &self.chats);
            q.text = q.deliveries.iter().map(|d| d.text.as_str()).collect::<Vec<_>>().join(SEP);
            !q.deliveries.is_empty()
        });
        self.chats[lead_ix].queue = queue;
        self.persist(lead_ix);
    }

    pub(super) fn deliver_to_lead(
        &mut self,
        child_ix: usize,
        reply_ix: usize,
        failed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let child = &self.chats[child_ix];
        let by_lead = self.stopped_by_lead.remove(&child.id);
        let Some(lead_ix) = self.chats.iter().position(|c| c.lead && Some(c.id) == child.parent)
        else {
            return;
        };
        let outcome = if child.stopped && by_lead {
            "was stopped by the lead"
        } else if child.stopped {
            "was stopped by the user"
        } else if failed {
            "failed"
        } else {
            "finished"
        };
        let text = delivery_text(child, &child.messages[reply_ix], outcome);
        let delivery = Delivery { child: child.id, reply_ix: Some(reply_ix), text };
        if let Some(delivery) = unread(vec![delivery], &self.chats).pop() {
            self.hand_to_lead(lead_ix, delivery, window, cx);
        }
    }

    pub(super) fn deliver_failure_to_lead(
        &mut self,
        child_ix: usize,
        body: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let child = &self.chats[child_ix];
        let Some(lead_ix) = self.chats.iter().position(|c| c.lead && Some(c.id) == child.parent)
        else {
            return;
        };
        let text = format!("{}\n\n{body}", delivery_head(child, "failed"));
        let delivery = Delivery { child: child.id, reply_ix: None, text };
        self.hand_to_lead(lead_ix, delivery, window, cx);
    }

    pub(super) fn hand_to_lead(
        &mut self,
        lead_ix: usize,
        delivery: Delivery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lead = &self.chats[lead_ix];
        if lead.busy()
            || lead.stopped
            || !lead.queue.is_empty()
            || self.locked(lead_ix)
            || self.worktree_blocks_send(lead_ix).is_some()
        {
            self.enqueue_delivery(lead_ix, delivery);
        } else {
            let follow = self.follows(lead_ix, cx);
            self.submit_as(lead_ix, delivery.text, vec![], follow, window, cx);
        }
    }

    fn follows(&self, ix: usize, cx: &Context<Self>) -> bool {
        ix != self.active || !self.scroller.read(cx).is_scrolled_up()
    }

    /// An omitted or "auto" provider becomes the installed one with the most quota left, ranked
    /// on snapshots under USAGE_STALE_MS old; with none, the lead's own provider (then the first
    /// usable one). Returns the rewritten args and the pick's tightest percent left, if ranked.
    pub(super) fn resolve_auto(
        &mut self,
        lead_ix: usize,
        args: &Value,
        cx: &mut Context<Self>,
    ) -> Result<(Value, Option<f32>), String> {
        if !matches!(args["provider"].as_str().map(str::trim), None | Some("" | "auto")) {
            return Ok((args.clone(), None));
        }
        self.refresh_usage_for_spawn(cx);
        let (access, lead_provider, now) =
            (self.chats[lead_ix].access, self.chats[lead_ix].provider, now_ms());
        let usable: Vec<Provider> = CLIS
            .into_iter()
            .filter(|p| self.probes.get(p).is_some_and(|probe| probe.installed))
            .filter(|&p| child_access(access, p).is_ok())
            .collect();
        let running = |p: Provider| {
            self.chats
                .iter()
                .filter(|c| !c.archived && c.provider == p && (c.busy() || starting(c)))
                .count()
        };
        let ranked = best_provider(usable.iter().filter_map(|&p| {
            let fresh = self.plan_usage_at.get(&p).is_some_and(|&at| now - at <= USAGE_STALE_MS);
            Some((p, self.provider_plan_usages.get(&p).filter(|_| fresh)?, running(p)))
        }));
        let (provider, left) = match ranked {
            Some((p, left)) => (p, Some(left)),
            None => {
                let first = usable.iter().find(|&&p| p == lead_provider).or(usable.first());
                (
                    *first
                        .ok_or("no installed agent provider can run as restricted as this lead")?,
                    None,
                )
            }
        };
        let mut args = args.clone();
        args["provider"] = provider.key().into();
        let model = args["model"].as_str().unwrap_or_default().trim();
        if model.is_empty() {
            let default = initial_model(&self.settings, provider);
            let known = self.probes.get(&provider).is_some_and(|probe| {
                probe.models.is_empty() || probe.models.iter().any(|m| m.id == default)
            });
            args["model"] = if known { default } else { String::new() }.into();
        }
        Ok((args, left))
    }

    pub(super) fn spawn_agent(
        &mut self,
        lead_ix: usize,
        args: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let (args, quota_left) = self.resolve_auto(lead_ix, args, cx)?;
        let a = SpawnArgs::parse(&args, &self.probes)?;
        let lead_id = self.chats[lead_ix].id;
        if !a.nonce.is_empty() {
            let key = (lead_id, a.nonce.clone());
            if let Some(prev) = self.spawn_nonces.get(&key).cloned() {
                let alive = prev["id"].as_i64().is_some_and(|id| {
                    self.chats
                        .iter()
                        .any(|c| c.id == id && c.parent == Some(lead_id) && !c.archived)
                });
                if alive {
                    return Ok(prev);
                }
                self.spawn_nonces.remove(&key);
            }
        }
        let max_children = self.settings.max_children;
        if self.running_children(lead_ix) >= max_children {
            return Err(format!(
                "this lead already has {max_children} running agents; wait for one to finish or stop one with the stop_agent tool"
            ));
        }
        let lead = &self.chats[lead_ix];
        if lead.cwd.is_empty() && self.settings.workspace_root.is_empty() {
            return Err("this lead has no project folder; set a workspace root in Settings or lead from a project chat".into());
        }
        let base = if a.worktree {
            Some(worktree_base(lead, &chat_cwd(lead, &self.settings.workspace_root))?)
        } else {
            None
        };
        let project = self.lead_project(lead_ix);
        if let Some(id) = a.card
            && !self.cards.iter().any(|c| c.id == id && c.project == project)
        {
            return Err(format!("card #{id} not found"));
        }
        let ix = self.create_child(lead_ix, &a)?;
        if let Some(reason) = self.worktree_blocks_send(ix) {
            self.chats.remove(ix);
            return Err(reason.to_string());
        }
        self.persist(ix);
        let child_id = self.chats[ix].id;
        let card = match a.card {
            Some(id) => {
                self.board_link(id, child_id);
                id
            }
            None => self.board_add(
                a.title.clone(),
                a.task.clone(),
                Col::Doing,
                project,
                Some(child_id),
                true,
            ),
        };
        let mut reply = json!({
            "id": child_id,
            "card": card,
            "title": a.title,
            "provider": a.provider.key(),
            "model": a.model,
        });
        if let Some(left) = quota_left {
            reply["quota_left"] = left.into();
        }
        match base {
            Some(base) => {
                let (prompt, text) = (a.title.clone(), child_prompt(&a.task));
                self.send_in_new_worktree(ix, base, prompt, text, vec![], window, cx);
                reply["worktree"] = "creating".into();
            }
            None => self.submit(ix, child_prompt(&a.task), vec![], window, cx),
        }
        if !a.nonce.is_empty() {
            self.spawn_nonces.insert((lead_id, a.nonce.clone()), reply.clone());
        }
        Ok(reply)
    }

    pub(super) fn crew(&self, lead_ix: usize) -> Vec<usize> {
        let lead = self.chats[lead_ix].id;
        let mut kids: Vec<usize> = (0..self.chats.len())
            .filter(|&i| {
                let child = &self.chats[i];
                child.parent == Some(lead) && !child.archived && (child.busy() || starting(child))
            })
            .collect();
        kids.sort_by_key(|&i| (crew_rank(&self.chats[i]), self.chats[i].id));
        kids
    }

    pub(super) fn open_chat(&mut self, chat_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.chat_ix(chat_id) else { return };
        let model = &mut self.workspace.model;
        let layout = model.windows.get(&0).filter(|w| w.root.is_some());
        if let Some(layout) = layout.filter(|_| model.pane_for(chat_id).is_none()) {
            let focused = layout.focused;
            let other = layout.panes().into_iter().find(|p| Some(*p) != focused);
            let viewport = window.viewport_size();
            let size = focused
                .and_then(|p| self.workspace.bounds.get(&p).copied())
                .unwrap_or([f32::from(viewport.width), f32::from(viewport.height) - 120.]);
            if focused.is_some_and(|p| model.can_split(p, SplitAxis::Horizontal, size).is_ok()) {
                model.open(chat_id, 0);
            } else if let Some(pane) = other {
                model.panes.insert(pane, chat_id);
                model.focus(pane);
                self.workspace.views.remove(&pane);
            }
        }
        self.select(ix, window, cx);
    }

    pub(super) fn running_children(&self, lead_ix: usize) -> usize {
        let lead = self.chats[lead_ix].id;
        self.chats.iter().filter(|c| c.parent == Some(lead) && (c.busy() || starting(c))).count()
    }

    pub(super) fn create_child(&mut self, lead_ix: usize, a: &SpawnArgs) -> Result<usize, String> {
        let lead = &self.chats[lead_ix];
        let access = child_access(lead.access, a.provider)?;
        let (parent, cwd, project) = (lead.id, lead.cwd.clone(), lead.project.clone());
        let id = crate::session::allocate_chat_id(
            &mut self.chat_id_highwater,
            self.chats.iter().map(|c| c.id),
            now_ms(),
        );
        let session = self.next_session_id();
        self.chats.push(Chat {
            id,
            session,
            title: a.title.clone().into(),
            messages: Vec::new(),
            pinned: false,
            archived: false,
            provider: a.provider,
            provider_model: a.model.clone(),
            cancel: None,
            open_steps: HashMap::new(),
            run: 0,
            epoch: 0,
            unseen: ChatStatus::Idle,
            cwd,
            view: ChatViewMode::Chat,
            access,
            effort: a.effort.clone(),
            cli_session: String::new(),
            queue: Vec::new(),
            stopped: false,
            external_pid: None,
            turn_at: 0,
            project,
            worktree_path: String::new(),
            worktree_branch: String::new(),
            new_worktree: None,
            new_worktree_existing: None,
            worktree_job: None,
            worktree_error: None,
            worktree_info: None,
            setup: None,
            fresh_worktree: None,
            draft: String::new(),
            pending: Vec::new(),
            edit_stash: None,
            last_event_ms: 0,
            saved_ms: 0,
            mode_in_effect: None,
            health: None,
            stall_kept_ms: 0,
            restarted_ms: 0,
            bg_live: std::cell::Cell::new(None),
            parent: Some(parent),
            lead: false,
            lead_seen: None,
            handoff_from: None,
        });
        Ok(self.chats.len() - 1)
    }
}
