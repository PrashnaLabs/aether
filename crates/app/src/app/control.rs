use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use aether_core::{Provider, Role};
use aether_ui::ChatStatus;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::{Context, SharedString, Window};
use serde_json::{Value, json};

use super::{Bulk, CLIS, Chat, ChatApp, Delivery, now_ms, project_dir};
use crate::board::model::{BoardCard, Col, shown};

/// Which chats a chat-control tool acts on.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Filter {
    pub ids: Option<Vec<i64>>,
    /// `None`: the lead's project, or any project when `ids` are given. `Some("all")`: every project.
    pub project: Option<String>,
    pub older_than_ms: Option<i64>,
    pub state: Option<String>,
    pub provider: Option<Provider>,
    pub archived: bool,
    pub include_pinned: bool,
    /// Words that must all appear in the title, any case.
    pub search: String,
}

const STATES: [&str; 5] = ["running", "done", "failed", "empty", "not_running"];

/// "30m", "12h", "3d", "2w" (also seconds) to milliseconds.
pub(crate) fn parse_age(s: &str) -> Option<i64> {
    let s = s.trim();
    let unit = match s.chars().last()? {
        's' => 1_000,
        'm' => 60_000,
        'h' => 3_600_000,
        'd' => 86_400_000,
        'w' => 604_800_000,
        _ => return None,
    };
    let n: i64 = s[..s.len() - 1].trim().parse().ok().filter(|n| *n >= 0)?;
    n.checked_mul(unit)
}

pub(crate) fn parse_filter(args: &Value, archived_default: bool) -> Result<Filter, String> {
    let f = &args["filter"];
    let text = |k: &str| f[k].as_str().map(str::trim).filter(|s| !s.is_empty());
    let ids = match &f["ids"] {
        Value::Null => None,
        Value::Array(a) => Some(
            a.iter()
                .map(|v| v.as_i64().or_else(|| v.as_str()?.trim().parse().ok()))
                .collect::<Option<Vec<i64>>>()
                .ok_or(r#""ids" must be chat ids"#)?,
        ),
        _ => return Err(r#""ids" must be a list of chat ids"#.into()),
    };
    let older_than_ms = match text("older_than") {
        None => None,
        Some(s) => Some(parse_age(s).ok_or_else(|| {
            format!(r#"bad "older_than" "{s}"; use a number and m, h, d or w, like "3d""#)
        })?),
    };
    let state = match text("state") {
        None => None,
        Some(s) if STATES.contains(&s) => Some(s.to_string()),
        Some(s) => {
            return Err(format!(
                r#"unknown "state" "{s}"; use running, done, failed, empty or not_running"#
            ));
        }
    };
    let provider = match text("provider") {
        None => None,
        Some(s) => Some(
            CLIS.into_iter()
                .chain([Provider::OpenRouter])
                .find(|p| p.key() == s)
                .ok_or_else(|| format!(r#"unknown "provider" "{s}""#))?,
        ),
    };
    Ok(Filter {
        ids,
        project: text("project").map(str::to_string),
        older_than_ms,
        state,
        provider,
        archived: f["archived"].as_bool().unwrap_or(archived_default),
        include_pinned: f["include_pinned"].as_bool().unwrap_or(false),
        search: text("search").unwrap_or_default().to_lowercase(),
    })
}

/// Newest of creation (ids are millisecond times), last turn and last message.
pub(crate) fn last_active(chat: &Chat) -> i64 {
    let msgs = chat.messages.iter().flat_map(|m| [m.sent_at_ms, m.ended_at_ms]).flatten();
    msgs.chain([chat.id, chat.turn_at]).max().unwrap_or(0)
}

pub(crate) fn state_of(chat: &Chat, running: bool) -> &'static str {
    let last = chat.messages.iter().rev().find(|m| m.role == Role::Assistant);
    match last {
        _ if running => "running",
        None => "empty",
        Some(m) if m.failed => "failed",
        Some(_) => "done",
    }
}

pub(crate) fn age_label(ms: i64) -> String {
    let m = ms.max(0) / 60_000;
    match m {
        0 => "just now".into(),
        1..60 => format!("{m}m ago"),
        60..1440 => format!("{}h ago", m / 60),
        _ => format!("{}d ago", m / 1440),
    }
}

fn folder(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into())
}

fn titles(app: &ChatApp, ids: &[i64]) -> Vec<Value> {
    ids.iter()
        .filter_map(|&id| app.chat_ix(id))
        .map(|ix| json!({"id": app.chats[ix].id, "title": app.chats[ix].title.as_ref()}))
        .collect()
}

impl ChatApp {
    fn chat_running(&self, ix: usize) -> bool {
        let chat = &self.chats[ix];
        chat.busy()
            || chat.worktree_job.is_some()
            || self.locked(ix)
            || matches!(chat.status(now_ms()), ChatStatus::Background(_))
            || (chat.lead && self.running_children(ix) > 0)
    }

    /// Indexes of chats matching `f` for the lead at `lead_ix`.
    fn matching(&self, lead_ix: usize, f: &Filter) -> Vec<usize> {
        let root = &self.settings.workspace_root;
        let home = project_dir(&self.chats[lead_ix], root);
        let now = now_ms();
        (0..self.chats.len())
            .filter(|&ix| {
                let chat = &self.chats[ix];
                let dir = project_dir(chat, root);
                let in_project = match f.project.as_deref() {
                    Some("all") => true,
                    Some(p) => dir == Path::new(p) || folder(&dir).eq_ignore_ascii_case(p),
                    None => f.ids.is_some() || dir == home,
                };
                let state = state_of(chat, self.chat_running(ix));
                let title = chat.title.to_lowercase();
                in_project
                    && f.search.split_whitespace().all(|w| title.contains(w))
                    && chat.archived == f.archived
                    && f.ids.as_ref().is_none_or(|ids| ids.contains(&chat.id))
                    && f.provider.is_none_or(|p| chat.provider == p)
                    && f.older_than_ms.is_none_or(|age| now - last_active(chat) >= age)
                    && f.state
                        .as_deref()
                        .is_none_or(|s| s == state || (s == "not_running" && state != "running"))
            })
            .collect()
    }

    /// Splits matches into ids a bulk change may touch and `{id, title, reason}` skips.
    fn guard(&self, lead_ix: usize, f: &Filter, ixs: &[usize]) -> (Vec<i64>, Vec<Value>) {
        let (mut ok, mut skipped) = (Vec::new(), Vec::new());
        for &ix in ixs {
            let chat = &self.chats[ix];
            let reason = if ix == lead_ix {
                Some("it is you")
            } else if self.chat_running(ix) {
                Some("running")
            } else if chat.pinned && !f.include_pinned {
                Some("pinned")
            } else {
                None
            };
            match reason {
                Some(r) => {
                    skipped.push(json!({"id": chat.id, "title": chat.title.as_ref(), "reason": r}))
                }
                None => ok.push(chat.id),
            }
        }
        (ok, skipped)
    }

    pub(super) fn list_chats(&self, lead_ix: usize, args: &Value) -> Result<Value, String> {
        let f = parse_filter(args, false)?;
        let now = now_ms();
        let root = &self.settings.workspace_root;
        let chats: Vec<Value> = self
            .matching(lead_ix, &f)
            .into_iter()
            .map(|ix| {
                let c = &self.chats[ix];
                json!({
                    "id": c.id,
                    "title": c.title.as_ref(),
                    "project": folder(&project_dir(c, root)),
                    "provider": c.provider.key(),
                    "state": state_of(c, self.chat_running(ix)),
                    "last_active": age_label(now - last_active(c)),
                    "pinned": c.pinned,
                    "archived": c.archived,
                    "lead": c.lead,
                })
            })
            .collect();
        Ok(json!({"count": chats.len(), "chats": chats}))
    }

    /// Archive or delete after the user confirms in a dialog; the outcome arrives as a lead turn.
    pub(super) fn ask_bulk(
        &mut self,
        lead_ix: usize,
        bulk: Bulk,
        args: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let f = parse_filter(args, false)?;
        let (ids, skipped) = self.guard(lead_ix, &f, &self.matching(lead_ix, &f));
        let action = if bulk == Bulk::Delete { "delete" } else { "archive" };
        if ids.is_empty() {
            return Ok(json!({"status": "nothing_to_do", "action": action, "skipped": skipped}));
        }
        let lead = &self.chats[lead_ix];
        let (lead_id, lead_title) = (lead.id, lead.title.to_string());
        let reply = json!({
            "status": "waiting_for_user",
            "action": action,
            "count": ids.len(),
            "chats": titles(self, &ids),
            "skipped": skipped,
            "note": "The user confirms or declines in Aether; the outcome arrives as your next turn. Don't poll.",
        });
        let n = ids.len();
        let noun = if n == 1 { "chat" } else { "chats" };
        let mut names: Vec<String> = ids
            .iter()
            .take(10)
            .filter_map(|&id| self.chat_ix(id))
            .map(|ix| format!("• {}", self.chats[ix].title))
            .collect();
        if n > 10 {
            names.push(format!("…and {} more", n - 10));
        }
        let (verb, note) = match bulk {
            Bulk::Delete => (
                "Delete",
                "This removes them and their history. This can't be undone. Worktree folders stay on disk.",
            ),
            _ => ("Archive", "They move to the Archive. You can unarchive them any time."),
        };
        let text = format!("{}\n\n{note}", names.join("\n"));
        let title = format!("“{lead_title}” wants to {} {n} {noun}", verb.to_lowercase());
        let declined = format!(
            "[Aether] The user declined to {} {n} chat(s). Nothing changed.",
            verb.to_lowercase()
        );
        self.ask_user(
            (title, text, format!("{verb} {n}"), bulk == Bulk::Delete),
            move |this, window, cx| {
                this.finish_bulk(lead_id, bulk, f.clone(), ids.clone(), window, cx)
            },
            move |this, window, cx| this.tell_lead(lead_id, declined.clone(), window, cx),
            window,
            cx,
        );
        Ok(reply)
    }

    /// Applies an approved archive or delete to the chats that still qualify, then tells the lead.
    pub(super) fn finish_bulk(
        &mut self,
        lead_id: i64,
        bulk: Bulk,
        mut f: Filter,
        ids: Vec<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(lead_ix) = self.chat_ix(lead_id) else { return };
        f.ids = Some(ids.clone());
        let (ok, _) = self.guard(lead_ix, &f, &self.matching(lead_ix, &f));
        let done = titles(self, &ok);
        let changed: Vec<i64> = ids.iter().copied().filter(|id| !ok.contains(id)).collect();
        let changed = titles(self, &changed);
        self.apply_bulk(bulk, &ok, window, cx);
        let verb = if bulk == Bulk::Delete { "deleted" } else { "archived" };
        let mut text = format!(
            "[Aether] The user approved. {} {} chat(s): {}",
            capitalize(verb),
            done.len(),
            names_of(&done)
        );
        if !changed.is_empty() {
            text.push_str(&format!(
                "\nNot {verb}, changed since you asked (running, pinned or gone): {}",
                names_of(&changed)
            ));
        }
        self.tell_lead(lead_id, text, window, cx);
    }

    /// Confirm dialog (`title`, `text`, ok label, danger). Closing it counts as `no`.
    pub(crate) fn ask_user(
        &mut self,
        (title, text, ok, danger): (String, String, String, bool),
        yes: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        no: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        let (yes, no) = (Rc::new(yes), Rc::new(no));
        let answered = Rc::new(Cell::new(false));
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let answer = |pick: Rc<dyn Fn(&mut Self, &mut Window, &mut Context<Self>)>| {
                let (view, answered) = (view.clone(), answered.clone());
                move |window: &mut Window, cx: &mut gpui_kit::App| {
                    if !answered.replace(true) {
                        view.update(cx, |this, cx| pick(this, window, cx));
                    }
                }
            };
            let (on_yes, on_no, on_close) =
                (answer(yes.clone()), answer(no.clone()), answer(no.clone()));
            alert
                .confirm()
                .title(SharedString::from(title.clone()))
                .description(SharedString::from(text.clone()))
                .ok_text(SharedString::from(ok.clone()))
                .ok_variant(if danger { ButtonVariant::Danger } else { ButtonVariant::Primary })
                .on_ok(move |_, window, cx| {
                    on_yes(window, cx);
                    true
                })
                .on_cancel(move |_, window, cx| {
                    on_no(window, cx);
                    true
                })
                .on_close(move |_, window, cx| on_close(window, cx))
        });
    }

    pub(crate) fn tell_lead(
        &mut self,
        lead_id: i64,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(lead_ix) = self.chats.iter().position(|c| c.id == lead_id && c.lead) else {
            return;
        };
        self.hand_to_lead(lead_ix, Delivery { child: lead_id, reply_ix: None, text }, window, cx);
    }

    pub(super) fn unarchive_chats(
        &mut self,
        lead_ix: usize,
        args: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let f = parse_filter(args, true)?;
        let ids: Vec<i64> =
            self.matching(lead_ix, &f).into_iter().map(|ix| self.chats[ix].id).collect();
        let chats = titles(self, &ids);
        self.apply_bulk(Bulk::Unarchive, &ids, window, cx);
        Ok(json!({"unarchived": chats.len(), "chats": chats}))
    }

    pub(super) fn target(&self, args: &Value) -> Result<usize, String> {
        let id = args["id"]
            .as_i64()
            .or_else(|| args["id"].as_str().and_then(|s| s.trim().parse().ok()))
            .ok_or_else(|| r#"missing "id""#.to_string())?;
        self.chat_ix(id).ok_or_else(|| format!("chat {id} not found"))
    }

    pub(super) fn update_chat(
        &mut self,
        args: &Value,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let ix = self.target(args)?;
        let title = args["title"].as_str().map(str::trim).filter(|t| !t.is_empty());
        let pinned = args["pinned"].as_bool();
        if title.is_none() && pinned.is_none() {
            return Err(r#"nothing to change; pass "title" or "pinned""#.into());
        }
        let chat = &mut self.chats[ix];
        if let Some(t) = title {
            chat.title = SharedString::from(t.to_string());
        }
        if let Some(p) = pinned {
            chat.pinned = p;
        }
        let reply = json!({"id": chat.id, "title": chat.title.as_ref(), "pinned": chat.pinned});
        self.persist(ix);
        cx.notify();
        Ok(reply)
    }

    pub(super) fn stop_any_chat(
        &mut self,
        lead_ix: usize,
        args: &Value,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let ix = self.target(args)?;
        if ix == lead_ix {
            return Err("a lead can't stop itself".into());
        }
        let chat = &self.chats[ix];
        let busy = chat.cancel.is_some();
        if busy && chat.parent == Some(self.chats[lead_ix].id) {
            self.stopped_by_lead.insert(chat.id);
        }
        self.stop_chat(ix, cx);
        Ok(json!({"stopped": busy}))
    }

    pub(super) fn open_chat_tool(
        &mut self,
        args: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let ix = self.target(args)?;
        let id = self.chats[ix].id;
        self.board_open_chat(id, window, cx);
        Ok(json!({"opened": id}))
    }
}

impl ChatApp {
    /// This chat's messages `offset..offset+limit` in full, tool calls included.
    pub(super) fn read_history(&self, ix: usize, args: &Value) -> Result<Value, String> {
        let int = |k: &str, default: i64, lo: i64, hi: i64| {
            args[k].as_i64().unwrap_or(default).clamp(lo, hi) as usize
        };
        let ix = match args["id"].is_null() {
            true => ix,
            false => {
                let target = self.target(args)?;
                if target != ix && !self.chats[ix].lead {
                    return Err("only a lead can read another chat's history".into());
                }
                target
            }
        };
        let messages = &self.chats[ix].messages;
        let offset = match args["offset"].as_i64().unwrap_or(0) {
            n if n < 0 => messages.len().saturating_sub(n.unsigned_abs() as usize),
            n => n as usize,
        };
        let limit = int("limit", 5, 1, 20);
        let max = int("max_chars", 4_000, 500, 20_000);
        let items: Vec<Value> = messages
            .iter()
            .enumerate()
            .skip(offset)
            .take(limit)
            .map(|(i, m)| {
                let mut text: String = m.text.chars().take(max).collect();
                let more = m.text.chars().count().saturating_sub(max);
                if more > 0 {
                    text.push_str(&format!("… ({more} more chars)"));
                }
                let tools: Vec<String> = m
                    .steps
                    .iter()
                    .filter_map(|s| match s {
                        aether_core::Step::Tool(c) => super::chat::tool_record(c, max),
                        _ => None,
                    })
                    .collect();
                let role = if m.role == Role::User { "user" } else { "assistant" };
                let mut item = json!({"n": i, "role": role, "text": text});
                if !tools.is_empty() {
                    item["tools"] = tools.into();
                }
                if let Some(e) = &m.error {
                    item["error"] = e.lines().next().unwrap_or_default().into();
                }
                item
            })
            .collect();
        let next = offset + items.len();
        let mut reply = json!({"total": messages.len(), "messages": items});
        if next < messages.len() {
            reply["next_offset"] = next.into();
        }
        Ok(reply)
    }
}

/// Which cards delete_cards takes; `project` `None` is the lead's own.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CardFilter {
    pub ids: Option<Vec<i64>>,
    pub col: Option<Col>,
    pub project: Option<String>,
    pub by_agent: Option<bool>,
    pub search: String,
}

pub(crate) fn parse_card_filter(args: &Value) -> Result<CardFilter, String> {
    let f = &args["filter"];
    let text = |k: &str| f[k].as_str().map(str::trim).filter(|s| !s.is_empty());
    let ids = match &f["ids"] {
        Value::Null => None,
        Value::Array(a) => Some(
            a.iter()
                .map(|v| v.as_i64().or_else(|| v.as_str()?.trim().parse().ok()))
                .collect::<Option<Vec<i64>>>()
                .ok_or(r#""ids" must be card ids"#)?,
        ),
        _ => return Err(r#""ids" must be a list of card ids"#.into()),
    };
    let col = match text("col") {
        None => None,
        Some(s) => Some(Col::parse(s).ok_or_else(|| {
            format!(r#"unknown "col" "{s}"; use backlog, todo, doing, review or done"#)
        })?),
    };
    let by_agent = match text("added_by") {
        None => None,
        Some("agent") => Some(true),
        Some("user") => Some(false),
        Some(s) => return Err(format!(r#"unknown "added_by" "{s}"; use agent or user"#)),
    };
    Ok(CardFilter {
        ids,
        col,
        project: text("project").map(str::to_string),
        by_agent,
        search: text("search").unwrap_or_default().to_string(),
    })
}

fn card_names(cards: &[(i64, String)]) -> String {
    cards.iter().map(|(id, t)| format!("#{id} “{t}”")).collect::<Vec<_>>().join(", ")
}

impl ChatApp {
    /// Cards matching `f` for the lead at `lead_ix`.
    pub(super) fn matching_cards(&self, lead_ix: usize, f: &CardFilter) -> Vec<&BoardCard> {
        let home = self.lead_project(lead_ix);
        self.cards
            .iter()
            .filter(|c| match f.project.as_deref() {
                Some("all") => true,
                Some(p) => c.project == p || folder(Path::new(&c.project)).eq_ignore_ascii_case(p),
                None => f.ids.is_some() || c.project == home,
            })
            .filter(|c| f.ids.as_ref().is_none_or(|ids| ids.contains(&c.id)))
            .filter(|c| f.col.is_none_or(|col| c.col == col))
            .filter(|c| shown(c, &f.search, f.by_agent))
            .collect()
    }

    pub(super) fn delete_cards(
        &mut self,
        lead_ix: usize,
        args: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let f = parse_card_filter(args)?;
        let ids: Vec<i64> = self.matching_cards(lead_ix, &f).iter().map(|c| c.id).collect();
        if ids.is_empty() {
            return Ok(json!({"status": "nothing_to_do", "action": "delete_cards"}));
        }
        let cards: Vec<Value> = ids
            .iter()
            .filter_map(|id| self.cards.iter().find(|c| c.id == *id))
            .map(|c| json!({"id": c.id, "title": c.title, "col": c.col.key()}))
            .collect();
        let lead = &self.chats[lead_ix];
        let lead = (lead.id, lead.title.to_string());
        self.confirm_delete_cards(ids, Some(lead), window, cx);
        Ok(json!({
            "status": "waiting_for_user",
            "action": "delete_cards",
            "count": cards.len(),
            "cards": cards,
            "note": "The user confirms or declines in Aether; the outcome arrives as your next turn. Don't poll.",
        }))
    }

    pub(crate) fn finish_card_delete(
        &mut self,
        ids: &[i64],
        lead: Option<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let gone: Vec<(i64, String)> = ids
            .iter()
            .filter_map(|id| self.cards.iter().find(|c| c.id == *id))
            .map(|c| (c.id, c.title.clone()))
            .collect();
        for (id, _) in &gone {
            self.board_delete(*id);
            self.board_picks.remove(id);
        }
        if self.board_selected.is_some_and(|s| gone.iter().any(|(id, _)| *id == s)) {
            self.board_selected = None;
        }
        cx.notify();
        if let Some(lead) = lead {
            let text = format!(
                "[Aether] The user approved. Deleted {} card(s): {}",
                gone.len(),
                card_names(&gone)
            );
            self.tell_lead(lead, text, window, cx);
        }
    }

    /// Asks, then deletes the cards still on the board; a `lead` (id, title) hears the outcome.
    pub(crate) fn confirm_delete_cards(
        &mut self,
        ids: Vec<i64>,
        lead: Option<(i64, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let n = ids.len();
        let noun = if n == 1 { "card" } else { "cards" };
        let mut lines: Vec<String> = ids
            .iter()
            .filter_map(|id| self.cards.iter().find(|c| c.id == *id))
            .take(10)
            .map(|c| format!("• {}", c.title))
            .collect();
        if n > 10 {
            lines.push(format!("…and {} more", n - 10));
        }
        let text = format!("{}\n\nThis can't be undone. Linked chats stay.", lines.join("\n"));
        let title = match &lead {
            Some((_, who)) => format!("“{who}” wants to delete {n} {noun}"),
            None => format!("Delete {n} {noun}?"),
        };
        let lead_id = lead.map(|(id, _)| id);
        self.ask_user(
            (title, text, format!("Delete {n}"), true),
            move |this, window, cx| this.finish_card_delete(&ids, lead_id, window, cx),
            move |this, window, cx| {
                if let Some(lead) = lead_id {
                    let text = format!(
                        "[Aether] The user declined to delete {n} card(s). Nothing changed."
                    );
                    this.tell_lead(lead, text, window, cx);
                }
            },
            window,
            cx,
        );
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn names_of(chats: &[Value]) -> String {
    if chats.is_empty() {
        return "none".into();
    }
    chats
        .iter()
        .map(|c| format!("“{}” ({})", c["title"].as_str().unwrap_or_default(), c["id"]))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
#[path = "../../tests/unit/app/control.rs"]
mod tests;
