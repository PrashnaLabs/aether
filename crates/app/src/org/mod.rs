pub(crate) mod model;
pub(crate) mod view;

use std::path::{Path, PathBuf};

use aether_core::{Message, Role, config_dir};
use gpui_kit::component::input::TextareaState;
use gpui_kit::{Context, Entity, Window};
use serde_json::{Value, json};

use crate::app::{CLIS, ChatApp, Page, Queued, SidebarView, now_ms};
use crate::board::model::Col;
use model::{
    Decision, Head, HireOutcome, HireSpec, NeedsYou, Org, OrgMessage, OrgSpec, Request,
    hire_checks, hire_outcome, memory_path, models, onboarding_prompt, parse_from, pool_cap,
    role_file_text, role_path, slug,
};

const REQUEST_NONCE: &str = "org-request-";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OrgSel {
    Head(String),
    Worker(i64),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum OrgTab {
    #[default]
    NeedsYou,
    Messages,
    Rules,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum OrgOverlay {
    #[default]
    None,
    /// Steps: 0 Template, 1 Staff, 2 Rules, 3 Review.
    Wizard {
        step: usize,
        draft: OrgSpec,
    },
    /// Role of the head being edited.
    HeadEditor(String),
    Hire(HireSpec),
}

pub(crate) struct OrgState {
    /// Every project's org, loaded at startup.
    pub orgs: Vec<Org>,
    pub selected: Option<OrgSel>,
    pub overlay: OrgOverlay,
    pub tab: OrgTab,
    pub goal: Entity<TextareaState>,
    /// Head role the goal goes to; `None` is the CEO.
    pub goal_target: Option<String>,
    /// Card wake-ups waiting for a window: (head chat, text).
    pub wake: Vec<(i64, String)>,
    /// Project the page shows, picked from its sidebar row; `None` follows the current project.
    pub project: Option<String>,
    /// Page and sidebar view to return to when the Org page closes.
    pub back: (Page, SidebarView),
    /// Last non-org chat shown; the sidebar's back row returns to it.
    pub back_chat: Option<i64>,
}

impl OrgState {
    pub(crate) fn new(orgs: Vec<Org>, goal: Entity<TextareaState>) -> Self {
        Self {
            orgs,
            selected: None,
            overlay: OrgOverlay::None,
            tab: OrgTab::NeedsYou,
            goal,
            goal_target: None,
            wake: Vec::new(),
            project: None,
            back: (Page::Chat, SidebarView::Projects),
            back_chat: None,
        }
    }
}

/// `<config>/org/<project-slug>/`.
pub(crate) fn org_dir(project: &str) -> Option<PathBuf> {
    Some(config_dir()?.join("org").join(slug(project)))
}

pub(crate) fn load_orgs() -> Vec<Org> {
    let Some(root) = config_dir().map(|d| d.join("org")) else { return vec![] };
    let Ok(dirs) = std::fs::read_dir(root) else { return vec![] };
    dirs.flatten()
        .filter_map(|d| std::fs::read_to_string(d.path().join("org.json")).ok())
        .filter_map(|text| serde_json::from_str(&text).ok())
        .collect()
}

fn write_new(path: &Path, text: &str) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn hire_args(spec: &HireSpec, nonce: Option<String>) -> Value {
    let mut args = json!({
        "provider": spec.provider.key(),
        "model": spec.model,
        "effort": spec.effort,
        "title": spec.title,
        "task": spec.task,
        "worktree": spec.worktree,
    });
    if let Some(card) = spec.card {
        args["card"] = card.into();
    }
    if let Some(nonce) = nonce {
        args["nonce"] = nonce.into();
    }
    args
}

/// The head's role file text, written with the default persona if missing.
fn role_text(dir: &Path, head: &Head) -> Result<String, String> {
    let path = role_path(dir, &head.role);
    write_new(&path, &role_file_text(head))?;
    write_new(&memory_path(dir, &head.role), "")?;
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

impl ChatApp {
    pub(crate) fn org_for_project(&self, project: &str) -> Option<&Org> {
        self.org.orgs.iter().find(|o| o.project == project)
    }

    /// Queues a wake-up for the reviewer head named by `review:<role>` on a card entering Review.
    pub(crate) fn org_card_review(&mut self, card: i64) {
        let Some(c) = self.cards.iter().find(|c| c.id == card) else { return };
        let Some(org) = self.org_for_project(&c.project) else { return };
        let Some(chat) = org.labelled(&c.labels, "review") else { return };
        let text = format!(
            "[Aether] Card #{card} \"{}\" is in Review and you are its reviewer. read_card {card}, then leave approve or concrete changes as a note with update_card.",
            c.title
        );
        self.org.wake.push((chat, text));
    }

    /// Queues wake-ups for the card's owning head and the CEO when a head leaves a note.
    pub(crate) fn org_card_note(&mut self, card: i64, author: i64, note: &str) {
        let Some(c) = self.cards.iter().find(|c| c.id == card) else { return };
        let Some(org) = self.org_for_project(&c.project) else { return };
        let Some(head) = org.head_of_chat(author) else { return };
        let text = format!(
            "[Aether] {} left a note on card #{card} \"{}\" ({}): {}",
            head.title,
            c.title,
            c.col.key(),
            note.chars().take(600).collect::<String>()
        );
        for chat in org.note_wake(&c.labels, author) {
            self.org.wake.push((chat, text.clone()));
        }
    }

    /// Delivers queued card wake-ups; board changes happen where no window is at hand.
    pub(crate) fn org_flush_wake(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (chat, text) in std::mem::take(&mut self.org.wake) {
            self.tell_lead(chat, text, window, cx);
        }
    }

    /// Project the page shows: the one picked from its sidebar row, else the current project.
    pub(crate) fn org_project(&self) -> Option<String> {
        self.org
            .project
            .clone()
            .or_else(|| self.current_project_folder().map(|p| p.display().to_string()))
    }

    pub(crate) fn org_current(&self) -> Option<&Org> {
        self.org_for_project(&self.org_project()?)
    }

    /// The org head whose chat is `chat`, in any project.
    pub(crate) fn org_head_by_chat(&self, chat: i64) -> Option<&Head> {
        self.org.orgs.iter().find_map(|o| o.head_of_chat(chat))
    }

    /// Project of the org whose head or worker chat `ix` is.
    pub(crate) fn chat_org(&self, ix: usize) -> Option<String> {
        let chat = self.chats.get(ix).filter(|_| self.settings.org_enabled)?;
        let of = |id: i64| id == chat.id || chat.parent == Some(id);
        let org = self.org.orgs.iter().find(|o| o.heads.iter().any(|h| h.chat.is_some_and(of)))?;
        Some(org.project.clone())
    }

    /// Chat ids of the org's heads and their workers.
    pub(crate) fn org_chats(&self, org: &Org) -> Vec<i64> {
        let heads: Vec<i64> = org.heads.iter().filter_map(|h| h.chat).collect();
        self.chats
            .iter()
            .filter(|c| heads.contains(&c.id) || c.parent.is_some_and(|p| heads.contains(&p)))
            .map(|c| c.id)
            .collect()
    }

    fn org_current_mut(&mut self) -> Option<&mut Org> {
        let project = self.org_project()?;
        self.org.orgs.iter_mut().find(|o| o.project == project)
    }

    pub(crate) fn org_save(&self, project: &str) -> Result<(), String> {
        let org = self.org_for_project(project).ok_or("no organization in this project")?;
        let dir = org_dir(project).ok_or("no config folder")?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(org).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("org.json"), json).map_err(|e| e.to_string())
    }

    pub(crate) fn open_org(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings.org_enabled {
            return;
        }
        if self.page == Page::Org {
            self.org_back(window, cx);
        } else {
            self.open_org_page(None, cx);
        }
    }

    /// Opens the Org page for `project`, or for the current project when `None`.
    pub(crate) fn open_org_page(&mut self, project: Option<String>, cx: &mut Context<Self>) {
        self.org_show(project);
        self.open_page(Page::Org, cx);
    }

    fn org_show(&mut self, project: Option<String>) {
        let shown = self.org_project();
        self.org.project = project;
        if self.org_project() != shown {
            self.org.overlay = OrgOverlay::None;
            self.org.selected = None;
        }
    }

    /// Org sidebar's +: hire a worker, or add a head when only the CEO exists.
    pub(crate) fn org_add(&mut self, cx: &mut Context<Self>) {
        self.open_page(Page::Org, cx);
        match self.org_current() {
            None => self.org_open_wizard(),
            Some(org) if org.heads.iter().all(|h| h.role == "CEO") => {
                self.org.overlay = OrgOverlay::HeadEditor(String::new())
            }
            Some(_) => self.org_open_hire(None),
        }
        cx.notify();
    }

    /// Org page for a project's sidebar row; `setup` opens the setup wizard.
    pub(crate) fn open_org_for(&mut self, folder: &Path, setup: bool, cx: &mut Context<Self>) {
        self.open_org_page(Some(folder.display().to_string()), cx);
        if setup {
            self.org_open_wizard();
        }
    }

    pub(crate) fn org_open_wizard(&mut self) {
        let s = &self.settings;
        let draft = OrgSpec::from_template(&s.org_template, s.org_workers_per_head);
        self.org.overlay = OrgOverlay::Wizard { step: 0, draft };
    }

    /// Leaves the Org page for the page it was opened from.
    pub(crate) fn org_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_page(self.org.back.0, cx);
        if self.page == Page::Chat {
            self.composer.update(cx, |c, cx| c.focus(window, cx));
        }
    }

    /// Org sidebar's back row: the normal sidebar and the page from before the org,
    /// on the last non-org chat for the chat page.
    pub(crate) fn org_exit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.org.back.0 != Page::Chat {
            return self.open_page(self.org.back.0, cx);
        }
        let last = self.org.back_chat.and_then(|id| self.chat_ix(id));
        match last.filter(|&ix| !self.chats[ix].archived && self.chat_org(ix).is_none()) {
            Some(ix) => self.select(ix, window, cx),
            None => self.new_chat_in_current_project(window, cx),
        }
    }

    /// The sidebar follows the page and chat shown: the Org page or an org chat shows
    /// that org's view (an org chat enters it only when `enter`), anything else leaves it.
    pub(crate) fn sync_org_view(&mut self, enter: bool) {
        if self.page == Page::Org {
            return;
        }
        let org = (self.page == Page::Chat).then(|| self.chat_org(self.active)).flatten();
        if let Some(project) = org.clone()
            && (enter || self.sidebar_view == SidebarView::Org)
        {
            if self.sidebar_view != SidebarView::Org {
                self.org.back = (Page::Chat, self.sidebar_view);
                self.sidebar_view = SidebarView::Org;
            }
            return self.org_show(Some(project));
        }
        if self.page == Page::Chat && org.is_none() {
            self.org.back_chat = self.chats.get(self.active).map(|c| c.id);
        }
        self.sidebar_view = self.sidebar_view.off_org(self.page, self.org.back.1);
        if self.org.project.take().is_some() {
            self.org.overlay = OrgOverlay::None;
            self.org.selected = None;
        }
    }

    /// Makes the pinned lead chat for `head` in `project`; its onboarding is sent separately.
    fn org_head_chat(
        &mut self,
        project: &str,
        head: &Head,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<i64, String> {
        let o = &self.org;
        let keep = (o.back, o.back_chat, o.project.clone(), o.overlay.clone(), o.selected.clone());
        self.new_chat_in(PathBuf::from(project), window, cx);
        self.open_page(Page::Org, cx);
        (
            self.org.back,
            self.org.back_chat,
            self.org.project,
            self.org.overlay,
            self.org.selected,
        ) = keep;
        let ix = self.active;
        let access = self.settings.default_access(head.provider);
        let chat = &mut self.chats[ix];
        if !chat.messages.is_empty() || chat.lead {
            return Err(format!("couldn't create the {} chat", head.title));
        }
        chat.provider = head.provider;
        chat.provider_model = head.model.clone();
        chat.effort = head.effort.clone();
        chat.access = access;
        chat.title = head.chat_title().into();
        chat.pinned = true;
        chat.lead = true;
        let id = chat.id;
        self.persist(ix);
        Ok(id)
    }

    /// Sends `text` to `chat`, queued behind a running turn.
    fn org_send(&mut self, chat: i64, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.chat_ix(chat) else { return };
        if self.chats[ix].busy() {
            self.chats[ix].queue.push(Queued {
                text,
                attachments: vec![],
                delivering: false,
                from_children: false,
                deliveries: vec![],
            });
        } else {
            self.submit(ix, text, vec![], window, cx);
        }
    }

    fn org_onboard(
        &mut self,
        project: &str,
        role: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let dir = org_dir(project).ok_or("no config folder")?;
        let org = self.org_for_project(project).ok_or("no organization in this project")?;
        let head = org.head(role).ok_or_else(|| format!("no {role} head"))?;
        let chat = head.chat.ok_or_else(|| format!("{} has no chat", head.title))?;
        let prompt = onboarding_prompt(org, head, &role_text(&dir, head)?, &dir);
        self.org_send(chat, prompt, window, cx);
        Ok(())
    }

    /// Creates the org in the current project: a pinned lead chat per head, CEO first, each
    /// onboarded with its role; the goal, if any, then goes to the CEO.
    pub(crate) fn org_create(
        &mut self,
        spec: OrgSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let project = self.org_project().ok_or("open a project chat first")?;
        if self.org_for_project(&project).is_some() {
            return Err("this project already has an organization".into());
        }
        let mut heads = spec.heads;
        heads.sort_by_key(|h| h.role != "CEO");
        for i in 0..heads.len() {
            match self.org_head_chat(&project, &heads[i], window, cx) {
                Ok(chat) => heads[i].chat = Some(chat),
                Err(e) => {
                    for chat in heads.iter().filter_map(|h| h.chat) {
                        if let Some(ix) = self.chat_ix(chat) {
                            self.archive_chat(ix, window, cx);
                        }
                    }
                    return Err(e);
                }
            }
        }
        let roles: Vec<String> = heads.iter().map(|h| h.role.clone()).collect();
        self.org.orgs.push(Org {
            project: project.clone(),
            heads,
            requests: vec![],
            goal: String::new(),
        });
        self.org_save(&project)?;
        for role in roles {
            self.org_onboard(&project, &role, window, cx)?;
        }
        if !spec.goal.trim().is_empty() {
            self.org_send_goal(spec.goal, None, window, cx)?;
        }
        self.open_page(Page::Org, cx);
        Ok(())
    }

    pub(crate) fn org_add_head(
        &mut self,
        mut head: Head,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<i64, String> {
        let project = self.org_project().ok_or("open a project chat first")?;
        let org = self.org_for_project(&project).ok_or("no organization in this project")?;
        if org.head(&head.role).is_some() {
            return Err(format!("there is already a {} head", head.role));
        }
        let ceo = org.ceo_chat();
        let chat = self.org_head_chat(&project, &head, window, cx)?;
        head.chat = Some(chat);
        let note = format!("[Aether] New head: {} ({}), chat {chat}.", head.title, head.role);
        let role = head.role.clone();
        self.org_current_mut().ok_or("no organization in this project")?.heads.push(head);
        self.org_save(&project)?;
        self.org_onboard(&project, &role, window, cx)?;
        if let Some(ceo) = ceo {
            self.org_send(ceo, note, window, cx);
        }
        self.open_page(Page::Org, cx);
        Ok(chat)
    }

    /// Saves `head` over the head with its role; `role_text` rewrites its role file.
    pub(crate) fn org_update_head(
        &mut self,
        head: Head,
        role_text: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let project = self.org_project().ok_or("open a project chat first")?;
        let org = self.org_current_mut().ok_or("no organization in this project")?;
        let slot = org.head_mut(&head.role).ok_or_else(|| format!("no {} head", head.role))?;
        let chat = slot.chat;
        let switched = slot.provider != head.provider;
        *slot = Head { chat, ..head.clone() };
        if let Some(ix) = chat.and_then(|id| self.chat_ix(id)) {
            if switched {
                self.handoff_to(ix, head.provider, window, cx);
            }
            let chat = &mut self.chats[ix];
            chat.provider_model = head.model.clone();
            chat.effort = head.effort.clone();
            chat.title = head.chat_title().into();
            self.persist(ix);
        }
        if let Some(text) = role_text {
            let dir = org_dir(&project).ok_or("no config folder")?;
            write_new(&role_path(&dir, &head.role), "")?;
            std::fs::write(role_path(&dir, &head.role), text).map_err(|e| e.to_string())?;
        }
        self.org_save(&project)?;
        cx.notify();
        Ok(())
    }

    /// Archives the head's chat and drops it from the roster; its files stay.
    pub(crate) fn org_remove_head(
        &mut self,
        role: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if role == "CEO" {
            return Err("the CEO can't be removed".into());
        }
        let project = self.org_project().ok_or("open a project chat first")?;
        let org = self.org_current_mut().ok_or("no organization in this project")?;
        let at = org
            .heads
            .iter()
            .position(|h| h.role == role)
            .ok_or_else(|| format!("no {role} head"))?;
        let head = org.heads.remove(at);
        self.org_save(&project)?;
        if let Some(ix) = head.chat.and_then(|id| self.chat_ix(id)) {
            self.archive_chat(ix, window, cx);
        }
        if self.org.overlay == OrgOverlay::HeadEditor(role.into()) {
            self.org.overlay = OrgOverlay::None;
        }
        cx.notify();
        Ok(())
    }

    /// Sends a goal to the CEO, or straight to the head with role `target`.
    pub(crate) fn org_send_goal(
        &mut self,
        text: String,
        target: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err("the goal is empty".into());
        }
        let project = self.org_project().ok_or("open a project chat first")?;
        let org = self.org_current_mut().ok_or("no organization in this project")?;
        let role = target.unwrap_or_else(|| "CEO".into());
        let head = org.head(&role).ok_or_else(|| format!("no {role} head"))?;
        let chat = head.chat.ok_or_else(|| format!("{} has no chat", head.title))?;
        if role == "CEO" {
            org.goal = text.clone();
        }
        self.org_save(&project)?;
        self.org_send(chat, text, window, cx);
        cx.notify();
        Ok(())
    }

    /// Clears the head's CLI session and onboards it again; its title is kept.
    pub(crate) fn org_rotate(
        &mut self,
        role: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let project = self.org_project().ok_or("open a project chat first")?;
        let org = self.org_current().ok_or("no organization in this project")?;
        let head = org.head(role).ok_or_else(|| format!("no {role} head"))?;
        let title = head.chat_title();
        let ix = head
            .chat
            .and_then(|id| self.chat_ix(id))
            .ok_or_else(|| format!("{} has no chat", head.title))?;
        if self.chats[ix].cancel.is_some() {
            return Err(format!("{} is running; pause it first", head.title));
        }
        self.clear_chat_at(ix, cx);
        self.chats[ix].title = title.into();
        self.persist(ix);
        self.org_onboard(&project, role, window, cx)
    }

    /// Spawns a worker as a child of the head's chat; returns the worker chat id.
    pub(crate) fn org_hire(
        &mut self,
        spec: HireSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Option<i64>, String> {
        let org = self.org_current().ok_or("no organization in this project")?;
        let head = org.head(&spec.head).ok_or_else(|| format!("no {} head", spec.head))?;
        let chat = head.chat.ok_or_else(|| format!("{} has no chat", head.title))?;
        let reply = self.org_spawn(chat, hire_args(&spec, None), true, window, cx)?;
        cx.notify();
        Ok(reply["id"].as_i64())
    }

    fn org_spawn(
        &mut self,
        head_chat: i64,
        args: Value,
        capped: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let ix = self.chat_ix(head_chat).ok_or("the head's chat is gone")?;
        self.spawn_capped(ix, &args, capped, window, cx)
    }

    /// Stops running turns of every head and worker.
    pub(crate) fn org_pause(&mut self, cx: &mut Context<Self>) {
        let Some(org) = self.org_current() else { return };
        let ids = self.org_chats(org);
        for id in ids {
            if let Some(ix) = self.chat_ix(id).filter(|&ix| self.chats[ix].busy()) {
                self.stop_chat(ix, cx);
            }
        }
    }

    pub(crate) fn org_decide(
        &mut self,
        request: u64,
        decision: Decision,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let project = self.org_project().ok_or("open a project chat first")?;
        let org = self.org_current().ok_or("no organization in this project")?;
        let req = org
            .requests
            .iter()
            .find(|r| r.id == request)
            .cloned()
            .ok_or_else(|| format!("no request #{request}"))?;
        let head = org.head(&req.head).ok_or_else(|| format!("no {} head", req.head))?;
        let chat = head.chat.ok_or_else(|| format!("{} has no chat", head.title))?;
        let spec = HireSpec {
            head: req.head.clone(),
            card: req.card,
            title: req.title.clone(),
            task: req.task.clone(),
            why: req.why.clone(),
            provider: req.provider,
            model: req.model.clone(),
            effort: String::new(),
            worktree: true,
        };
        let name = format!("#{} \"{}\"", req.id, req.title);
        let org = self.org_current_mut().ok_or("no organization in this project")?;
        if decision == Decision::Queue {
            if let Some(r) = org.requests.iter_mut().find(|r| r.id == request) {
                r.queued = true;
            }
        } else {
            org.requests.retain(|r| r.id != request);
        }
        self.org_save(&project)?;
        let text = match decision {
            Decision::Approve => {
                match self.org_spawn(chat, hire_args(&spec, None), false, window, cx) {
                    Ok(reply) => {
                        format!("[Aether] The user approved worker request {name}: {reply}")
                    }
                    Err(e) => format!(
                        "[Aether] The user approved worker request {name}, but it failed to start: {e}"
                    ),
                }
            }
            Decision::Queue => {
                let nonce = format!("{REQUEST_NONCE}{request}");
                self.spawn_queue.push((chat, hire_args(&spec, Some(nonce))));
                format!(
                    "[Aether] The user queued worker request {name}; it starts when a worker slot frees and its id arrives as a turn. Don't poll."
                )
            }
            Decision::Deny => format!("[Aether] The user denied worker request {name}."),
        };
        self.tell_lead(chat, text, window, cx);
        if decision == Decision::Queue {
            self.drain_spawn_queue(window, cx);
        }
        cx.notify();
        Ok(())
    }

    /// Drops a queued request once the spawn queue starts it.
    pub(crate) fn org_request_started(&mut self, args: &Value) {
        let Some(id) = args["nonce"]
            .as_str()
            .and_then(|n| n.strip_prefix(REQUEST_NONCE))
            .and_then(|n| n.parse::<u64>().ok())
        else {
            return;
        };
        let Some(org) =
            self.org.orgs.iter_mut().find(|o| o.requests.iter().any(|r| r.id == id && r.queued))
        else {
            return;
        };
        org.requests.retain(|r| r.id != id);
        let project = org.project.clone();
        let _ = self.org_save(&project);
    }

    pub(crate) fn org_needs_you(&self) -> Vec<NeedsYou> {
        let Some(org) = self.org_current() else { return vec![] };
        let in_queue = |id: u64| {
            let nonce = format!("{REQUEST_NONCE}{id}");
            self.spawn_queue.iter().any(|(_, a)| a["nonce"] == nonce.as_str())
        };
        let mut out: Vec<NeedsYou> = org
            .requests
            .iter()
            .filter(|r| !r.queued || !in_queue(r.id))
            .map(|r| NeedsYou::Hire(r.id))
            .collect();
        let ids = self.org_chats(org);
        out.extend(
            self.chats
                .iter()
                .filter(|c| !c.archived && ids.contains(&c.id))
                .filter(|c| c.messages.last().and_then(Message::pending_request).is_some())
                .map(|c| NeedsYou::Waiting { chat: c.id }),
        );
        out.extend(
            self.cards
                .iter()
                .filter(|c| c.project == org.project && c.col == Col::Review)
                .map(|c| NeedsYou::Review { card: c.id }),
        );
        out
    }

    /// `request_worker` lead tool: `title`, `task`, `card?`, `provider?`, `model?`, `why?`.
    pub(crate) fn org_request_worker(
        &mut self,
        lead_ix: usize,
        args: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let lead_id = self.chats[lead_ix].id;
        let org = self
            .org
            .orgs
            .iter()
            .find(|o| o.head_of_chat(lead_id).is_some())
            .ok_or("request_worker is only for organization heads; use spawn_agent")?;
        let head = org.head_of_chat(lead_id).cloned().ok_or("not an organization head")?;
        let project = org.project.clone();
        let next_id = org.next_request_id();
        let text = |k: &str| args[k].as_str().unwrap_or_default().trim().to_string();
        let need = |k: &str| {
            Some(text(k)).filter(|s| !s.is_empty()).ok_or_else(|| format!(r#"missing "{k}""#))
        };
        let provider = match text("provider") {
            k if k.is_empty() => head.provider,
            k => CLIS.into_iter().find(|p| p.key() == k).ok_or_else(|| {
                format!(
                    r#"unknown provider "{k}"; use one of {}"#,
                    CLIS.map(|p| p.key()).join(", ")
                )
            })?,
        };
        let model = match text("model") {
            m if !m.is_empty() => m,
            _ if provider == head.provider => head.model.clone(),
            _ => models(provider).first().map_or(String::new(), |m| m.to_string()),
        };
        let spec = HireSpec {
            head: head.role.clone(),
            card: args["card"].as_i64(),
            title: need("title")?,
            task: need("task")?,
            why: text("why"),
            provider,
            model,
            effort: text("effort"),
            worktree: true,
        };
        let running = self.running_children(lead_ix);
        let checks =
            hire_checks(&head, spec.card, running, self.org_pool_busy(), self.org_pool_cap());
        match hire_outcome(self.settings.org_hire, &checks) {
            HireOutcome::Hire => {
                let reply =
                    self.spawn_capped(lead_ix, &hire_args(&spec, None), true, window, cx)?;
                cx.notify();
                Ok(match reply["id"].as_i64() {
                    Some(id) => json!({"status": "hired", "id": id, "card": reply["card"]}),
                    None => reply,
                })
            }
            HireOutcome::Deny => {
                let reasons: Vec<&str> =
                    checks.iter().filter(|c| !c.1).map(|c| c.0.as_str()).collect();
                Ok(json!({"status": "denied", "reasons": reasons}))
            }
            HireOutcome::Pending => {
                let org = self
                    .org
                    .orgs
                    .iter_mut()
                    .find(|o| o.project == project)
                    .ok_or("no organization in this project")?;
                org.requests.push(Request {
                    id: next_id,
                    head: spec.head,
                    title: spec.title,
                    why: spec.why,
                    card: spec.card,
                    provider: spec.provider,
                    model: spec.model,
                    task: spec.task,
                    checks,
                    created_ms: now_ms(),
                    queued: false,
                });
                self.org_save(&project)?;
                cx.notify();
                Ok(json!({
                    "status": "pending",
                    "request": next_id,
                    "note": "the user decides; the outcome arrives as your next turn. Don't poll.",
                }))
            }
        }
    }

    /// Org-wide worker cap for the current machine.
    pub(crate) fn org_pool_cap(&self) -> usize {
        let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
        let mem_gb = self.sys_sample.map_or(16, |s| (s.mem_total >> 30) as usize);
        pool_cap(&self.settings, cores, mem_gb)
    }

    /// Org workers running now, across every head.
    pub(crate) fn org_pool_busy(&self) -> usize {
        self.org
            .orgs
            .iter()
            .flat_map(|o| &o.heads)
            .filter_map(|h| h.chat.and_then(|id| self.chat_ix(id)))
            .map(|ix| self.running_children(ix))
            .sum()
    }

    /// `ceo` is the CEO chat of an org that has `head` as one of its heads.
    pub(crate) fn org_ceo_of(&self, ceo: i64, head: i64) -> bool {
        self.org.orgs.iter().any(|o| {
            o.heads.iter().any(|h| h.role == "CEO" && h.chat == Some(ceo))
                && o.heads.iter().any(|h| h.chat == Some(head))
        })
    }

    pub(crate) fn org_spend(&self, chat: i64) -> f64 {
        self.chats.iter().find(|c| c.id == chat).map_or(0., |c| self.spend_today(c))
    }

    /// `message_agent` traffic into the org's head chats, newest first.
    pub(crate) fn org_messages(&self, org: &Org) -> Vec<OrgMessage> {
        let mut out: Vec<OrgMessage> = org
            .heads
            .iter()
            .filter_map(|h| self.chats.iter().find(|c| Some(c.id) == h.chat))
            .flat_map(|chat| {
                chat.messages.iter().filter(|m| m.role == Role::User).filter_map(|m| {
                    let (from, text) = parse_from(&m.text)?;
                    Some(OrgMessage {
                        from,
                        to: chat.id,
                        text: text.to_string(),
                        at_ms: m.sent_at_ms.unwrap_or(0),
                    })
                })
            })
            .collect();
        out.sort_by_key(|m| std::cmp::Reverse(m.at_ms));
        out
    }
}

pub(crate) fn next_layout(layout: aether_core::OrgLayout) -> aether_core::OrgLayout {
    let all = aether_core::OrgLayout::ALL;
    all[(all.iter().position(|l| *l == layout).unwrap_or(0) + 1) % all.len()]
}
