use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::actions::{NewChat, ToggleSidebar};
use crate::agents_view::{external_state_text, open_blocker};
use crate::project_page::model::{Card, card_status, last_text};
use crate::projects;
use crate::settings;
use aether_agent::{ExternalAgent, ExternalState};
use aether_core::{
    Ask, Message, Provider, SidebarGrouping, SidebarOrdering, SidebarStyle, Watch, home_dir,
};
use aether_driver::CliModel;
use aether_git::BranchInfo;
use aether_ui::{
    AgentState, CLI_DEFAULT, ChatLine, ChatSidebar, ChatStatus, OutsideLine, OutsideState,
    ProjectHeader, RowPlace, SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH, edit_totals,
};
use gpui_kit::{Context, Entity, PathPromptOptions, SharedString, Window, prelude::*};

use super::format::now_ms;
use super::providers::initial_model;
use super::{Bulk, CLIS, Chat, ChatApp, Page, SidebarView, chat_cwd, project_dir};

pub(super) fn disambiguated_names(folders: &[PathBuf]) -> HashMap<PathBuf, String> {
    let mut names: Vec<String> =
        folders.iter().map(|f| folder_label(&f.display().to_string())).collect();
    projects::disambiguate(&mut names, folders);
    folders.iter().cloned().zip(names).collect()
}

pub(super) fn split_hidden(
    groups: Vec<projects::Project>,
    hidden: &[String],
    active: usize,
) -> (Vec<projects::Project>, Vec<projects::Project>) {
    groups
        .into_iter()
        .partition(|g| !projects::listed(hidden, &g.folder) || g.chats.contains(&active))
}

pub(super) fn folder_label(path: &str) -> String {
    if std::path::Path::new(path) == home_dir() {
        return "Home".into();
    }
    std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

/// Where a sidebar row says its chat runs: its worktree branch, else the branch
/// checked out in its folder; outside its project group, the folder when no
/// branch is known. A detached checkout keeps its short commit, flagged.
fn row_place(
    in_project: bool,
    worktree_branch: &str,
    folder: &Path,
    project: &Path,
    branches: &HashMap<PathBuf, Option<BranchInfo>>,
) -> Option<RowPlace> {
    if !worktree_branch.is_empty() {
        return Some(RowPlace::Branch {
            name: worktree_branch.into(),
            worktree: true,
            detached: false,
        });
    }
    let known = branches.get(folder);
    let branch = known.cloned().flatten().map(|b| RowPlace::Branch {
        name: b.branch.into(),
        worktree: b.worktree,
        detached: b.detached,
    });
    match (branch, in_project) {
        (None, false) => {
            Some(RowPlace::Folder(folder_label(&project.display().to_string()).into()))
        }
        // `None` until the folder is checked, so a loading row doesn't flash it
        (None, true) if matches!(known, Some(None)) => Some(RowPlace::NoGit),
        (branch, _) => branch,
    }
}

/// Catalog name of `model`; an empty model is the CLI's flagged default.
pub(super) fn model_label(models: &[CliModel], model: &str) -> String {
    match models.iter().find(|m| if model.is_empty() { m.default } else { m.id == model }) {
        Some(m) => m.name.clone(),
        None if model.is_empty() => CLI_DEFAULT.into(),
        None => model.into(),
    }
}

/// Outside aether row: title, else folder; folder · origin · elapsed; the
/// estimate flag, path and why it can't open stay in the hover text.
fn outside_line(a: &ExternalAgent) -> OutsideLine {
    let folder = folder_label(&a.cwd.display().to_string());
    let title = [&a.title, &folder].into_iter().find(|t| !t.is_empty()).map_or("Untitled", |t| t);
    let detail: Vec<&str> =
        [folder.as_str(), &a.origin, &a.elapsed].into_iter().filter(|s| !s.is_empty()).collect();
    let path = a.cwd.display().to_string();
    let state_text = external_state_text(&a.state, a.exact);
    let tip: Vec<&str> = [state_text.as_str(), &path, open_blocker(a).unwrap_or_default()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    OutsideLine {
        id: a.pid.map_or_else(|| a.session.clone(), |pid| pid.to_string()).into(),
        title: title.to_string().into(),
        provider: a.provider,
        detail: detail.join(" · ").into(),
        state: match a.state {
            ExternalState::Working => OutsideState::Working,
            ExternalState::Waiting(_) => OutsideState::Waiting,
            ExternalState::Idle => OutsideState::Idle,
        },
        label: external_state_text(&a.state, true).into(),
        tip: tip.join(" · ").into(),
    }
}

fn agent_state(chat: &Chat, live: bool) -> Option<AgentState> {
    if chat.cancel.is_some() {
        return Some(
            match chat.messages.last().and_then(Message::pending_request).map(|r| &r.ask) {
                Some(Ask::Approval(_)) => AgentState::Waiting("Needs approval"),
                Some(Ask::Question { .. } | Ask::Questions(_)) => {
                    AgentState::Waiting("Needs answer")
                }
                Some(Ask::Form(_)) => AgentState::Waiting("Needs input"),
                Some(Ask::Plan(_)) => AgentState::Waiting("Needs review"),
                Some(Ask::Unknown) | None => AgentState::Working(chat.queue.len()),
            },
        );
    }
    if !chat.queue.is_empty() {
        return Some(AgentState::Queued(chat.queue.len()));
    }
    match chat.unseen {
        ChatStatus::Failed => Some(AgentState::Failed),
        ChatStatus::Done => Some(AgentState::Done),
        _ => live.then_some(AgentState::Open),
    }
}

/// Group order with each lead's live children (same group, `parent` is a lead) right after it, by id.
pub(super) fn nest_children(chats: &[Chat], group: &[usize]) -> Vec<(usize, bool)> {
    let leads: std::collections::HashSet<i64> =
        group.iter().filter(|&&ix| chats[ix].lead).map(|&ix| chats[ix].id).collect();
    let live = |ix: usize| !chats[ix].lead && chats[ix].parent.is_some_and(|p| leads.contains(&p));
    let mut kids: std::collections::HashMap<i64, Vec<usize>> = std::collections::HashMap::new();
    for &ix in group {
        if live(ix) {
            kids.entry(chats[ix].parent.unwrap()).or_default().push(ix);
        }
    }
    for list in kids.values_mut() {
        list.sort_by_key(|&k| chats[k].id);
    }
    let mut out = Vec::with_capacity(group.len());
    for &ix in group.iter().filter(|&&ix| !live(ix)) {
        out.push((ix, false));
        if chats[ix].lead
            && let Some(list) = kids.get(&chats[ix].id)
        {
            out.extend(list.iter().map(|&k| (k, true)));
        }
    }
    out
}

/// Keep a crew in its lead's section, even across date or project boundaries.
/// An explicitly pinned child stays independent; an absent lead never hides a chat.
pub(super) fn nest_section_children(
    chats: &[Chat],
    sections: Vec<(String, Vec<usize>)>,
) -> Vec<(String, Vec<(usize, bool)>)> {
    let group: Vec<usize> = sections
        .iter()
        .flat_map(|(_, ixs)| ixs.iter().copied())
        .filter(|&ix| !chats[ix].pinned || chats[ix].lead)
        .collect();
    let mut families: HashMap<usize, Vec<(usize, bool)>> = HashMap::new();
    let mut lead = None;
    for (ix, child) in nest_children(chats, &group) {
        if !child {
            lead = Some(ix);
        }
        if let Some(lead) = lead {
            families.entry(lead).or_default().push((ix, child));
        }
    }
    sections
        .into_iter()
        .filter_map(|(label, ixs)| {
            let rows: Vec<_> = ixs
                .into_iter()
                .flat_map(|ix| {
                    if chats[ix].pinned && !chats[ix].lead {
                        vec![(ix, false)]
                    } else {
                        families.remove(&ix).unwrap_or_default()
                    }
                })
                .collect();
            (!rows.is_empty()).then_some((label, rows))
        })
        .collect()
}

impl ChatApp {
    pub(super) fn project_groups(&self) -> (Vec<projects::Project>, Vec<projects::Project>) {
        let root = &self.settings.workspace_root;
        let groups = projects::group_projects(
            self.chats
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.archived && !c.pinned && c.provider != Provider::OpenRouter)
                .map(|(ix, c)| (ix, project_dir(c, root), c.activity()))
                .collect(),
        );
        let mut groups = projects::with_saved(groups, &self.settings.projects);
        groups.sort_by_cached_key(|g| {
            let name = g.folder.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
            (name, g.folder.clone())
        });
        for group in &mut groups {
            group.chats.sort_by_cached_key(|&ix| (self.chats[ix].title.to_lowercase(), ix));
        }
        split_hidden(groups, &self.settings.hidden_projects, self.active)
    }

    pub(crate) fn project_names(&self) -> HashMap<PathBuf, String> {
        let folders: Vec<PathBuf> = self.project_groups().0.into_iter().map(|g| g.folder).collect();
        disambiguated_names(&folders)
    }

    pub(crate) fn switch_sidebar_to_projects(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_view = SidebarView::Projects;
        if !self.chats[self.active].is_agent()
            && let Some((agent_ix, _)) = self
                .chats
                .iter()
                .enumerate()
                .filter(|(_, c)| c.is_agent() && !c.archived)
                .max_by_key(|(_, c)| (c.activity(), c.id))
        {
            self.select(agent_ix, window, cx);
            return;
        }
        cx.notify();
    }

    pub(crate) fn switch_sidebar_to_direct_chats(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_view = SidebarView::DirectChats;
        if self.chats[self.active].is_agent() {
            if let Some((direct_ix, _)) = self
                .chats
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.is_agent() && !c.archived)
                .max_by_key(|(_, c)| (c.activity(), c.id))
            {
                self.select(direct_ix, window, cx);
                return;
            } else {
                self.new_direct_chat(window, cx);
                return;
            }
        }
        cx.notify();
    }

    pub(crate) fn switch_sidebar_to_archive(&mut self, cx: &mut Context<Self>) {
        self.sidebar_view = SidebarView::Archived;
        self.archive_picks.clear();
        cx.notify();
    }

    pub(crate) fn toggle_direct_chats(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_view == SidebarView::DirectChats {
            self.switch_sidebar_to_projects(window, cx);
        } else {
            self.switch_sidebar_to_direct_chats(window, cx);
        }
    }

    pub(crate) fn toggle_archive(&mut self, cx: &mut Context<Self>) {
        if self.sidebar_view != SidebarView::Archived {
            return self.switch_sidebar_to_archive(cx);
        }
        self.leave_sidebar_view(cx);
    }

    fn leave_sidebar_view(&mut self, cx: &mut Context<Self>) {
        self.sidebar_view = if self.chats[self.active].is_agent() {
            SidebarView::Projects
        } else {
            SidebarView::DirectChats
        };
        cx.notify();
    }

    pub(crate) fn switch_sidebar_to_outside(&mut self, cx: &mut Context<Self>) {
        self.sidebar_view = SidebarView::Outside;
        cx.notify();
    }

    pub(crate) fn toggle_outside(&mut self, cx: &mut Context<Self>) {
        if self.sidebar_view != SidebarView::Outside {
            return self.switch_sidebar_to_outside(cx);
        }
        self.leave_sidebar_view(cx);
    }

    /// Chats with an agent state: (chat index, title, provider, detail, state), newest first.
    pub(crate) fn in_aether_agents(
        &self,
    ) -> Vec<(usize, SharedString, Provider, String, AgentState)> {
        let live = self.agent.live_sessions();
        self.chats
            .iter()
            .enumerate()
            .rev()
            .filter_map(|(ix, chat)| {
                let state = agent_state(chat, live.contains(&chat.id.to_string()))?;
                let mut detail = chat.provider.label().to_string();
                if chat.provider != Provider::OpenRouter && !chat.cwd.is_empty() {
                    let project = project_dir(chat, &self.settings.workspace_root);
                    detail = format!("{detail} · {}", folder_label(&project.display().to_string()));
                }
                Some((ix, chat.title.clone(), chat.provider, detail, state))
            })
            .collect()
    }

    pub(crate) fn toggle_sidebar(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.settings.sidebar_collapsed = !self.settings.sidebar_collapsed;
        crate::settings::save(&self.settings);
        cx.notify();
    }

    pub(super) fn render_sidebar(&self, cx: &mut Context<Self>) -> ChatSidebar {
        if self.sidebar_view == SidebarView::DirectChats {
            return self.render_direct_chats_sidebar(cx);
        }
        if self.sidebar_view == SidebarView::Outside {
            return self.render_outside_sidebar(cx);
        }
        if self.sidebar_view == SidebarView::Archived {
            return self.render_archive_sidebar(cx);
        }
        if self.settings.sidebar_style != SidebarStyle::Default {
            return self.render_flat_sidebar(self.settings.sidebar_style, cx);
        }
        let mut sidebar = self
            .sidebar_chrome("Aether", cx)
            .on_new_chat(
                cx.listener(|this, _, window, cx| this.new_chat_in_current_project(window, cx)),
            )
            .new_chat_action(NewChat)
            .pinned_collapsible(
                self.settings.pinned_section_collapsed,
                cx.listener(|this, collapsed: &bool, _, cx| {
                    this.update_settings(cx, |s| s.pinned_section_collapsed = *collapsed)
                }),
            )
            .agents_collapsible(
                self.settings.agents_section_collapsed,
                cx.listener(|this, collapsed: &bool, _, cx| {
                    this.update_settings(cx, |s| s.agents_section_collapsed = *collapsed)
                }),
            )
            .projects_collapsible(
                self.settings.projects_section_collapsed,
                cx.listener(|this, collapsed: &bool, _, cx| {
                    this.update_settings(cx, |s| s.projects_section_collapsed = *collapsed)
                }),
            )
            .on_add_project(
                cx.listener(|this, _, window, cx| this.add_project_from_picker(window, cx)),
            );
        if let Some(ix) = self.renaming.filter(|_| !self.rename_in_tab) {
            sidebar = sidebar.renaming(ix, &self.rename_input);
        }
        let agents =
            if self.settings.show_running_agents { self.in_aether_agents() } else { vec![] };
        for (ix, title, provider, detail, state) in agents {
            // Agents section is running-only; finished work stays under its project.
            if !matches!(state, AgentState::Working(_) | AgentState::Waiting(_)) {
                continue;
            }
            sidebar = sidebar.agent(
                ix,
                title,
                provider,
                detail.into(),
                self.chat_project(ix).map(|dir| self.project_name(&dir)),
                state,
                cx.listener(move |this, _, window, cx| this.select(ix, window, cx)),
                Some(cx.listener(move |this, _, _, cx| this.stop_chat(ix, cx))),
            );
        }
        let now = now_ms();
        let (groups, hidden) = self.project_groups();
        // Keep each lead's children together for the sidebar's crew preview.
        let nested: Vec<Vec<(usize, bool)>> =
            groups.iter().map(|g| nest_children(&self.chats, &g.chats)).collect();
        sidebar = sidebar.child_agents(
            nested.iter().flatten().filter(|(_, c)| *c).map(|&(ix, _)| self.chats[ix].id).collect(),
        );
        for (ix, chat) in self.chats.iter().enumerate().rev() {
            if chat.pinned && !chat.archived && chat.is_agent() {
                sidebar =
                    self.chat_row(sidebar, ix, None, None, self.chat_line(ix, false, now), cx);
            }
        }
        let folders: Vec<PathBuf> = groups.iter().map(|g| g.folder.clone()).collect();
        let mut names = disambiguated_names(&folders);
        for (k, group) in groups.iter().enumerate() {
            let name = names.remove(&group.folder).unwrap_or_default();
            let path = group.folder.display().to_string();
            let header = ProjectHeader {
                name: name.into(),
                count: group.chats.len(),
                collapsed: projects::listed(&self.settings.collapsed_projects, &group.folder),
                status: projects::rollup(group.chats.iter().map(|&ix| self.chats[ix].status(now))),
                missing: self.missing_folders.contains(&group.folder),
                path: path.into(),
            };
            let (fold, new, files, hide, page) = (
                group.folder.clone(),
                group.folder.clone(),
                group.folder.clone(),
                group.folder.clone(),
                group.folder.clone(),
            );
            sidebar = sidebar.project(
                header,
                cx.listener(move |this, collapsed: &bool, _, cx| {
                    this.update_settings(cx, |s| {
                        projects::set_listed(&mut s.collapsed_projects, &fold, *collapsed)
                    })
                }),
                cx.listener(move |this, _, window, cx| this.new_chat_in(new.clone(), window, cx)),
                cx.listener(move |this, _, window, cx| {
                    this.open_project_explorer(files.clone(), window, cx)
                }),
                cx.listener(move |this, _, _, cx| {
                    this.update_settings(cx, |s| {
                        projects::set_listed(&mut s.hidden_projects, &hide, true)
                    })
                }),
                cx.listener(move |this, _, _, cx| this.open_project_page(page.clone(), cx)),
            );
            for &(ix, _) in &nested[k] {
                sidebar =
                    self.chat_row(sidebar, ix, Some(k), None, self.chat_line(ix, true, now), cx);
            }
        }
        if !hidden.is_empty() {
            let label = match hidden.len() {
                1 => "Show 1 hidden project".to_string(),
                n => format!("Show {n} hidden projects"),
            };
            sidebar = sidebar.hidden_projects(
                label,
                cx.listener(|this, _, _, cx| {
                    this.update_settings(cx, |s| s.hidden_projects.clear())
                }),
            );
        }
        sidebar
    }

    /// Minimal and Focus: agent chats in flat sections instead of project folders.
    fn render_flat_sidebar(&self, style: SidebarStyle, cx: &mut Context<Self>) -> ChatSidebar {
        let mut sidebar = self
            .sidebar_chrome("Aether", cx)
            .style(style)
            .on_new_chat(
                cx.listener(|this, _, window, cx| this.new_chat_in_current_project(window, cx)),
            )
            .new_chat_action(NewChat);
        if style == SidebarStyle::Minimal {
            sidebar = sidebar
                .top_action(
                    gpui_kit::assets::IconName::SquarePen,
                    "New chat",
                    cx.listener(|this, _, window, cx| this.new_chat_in_current_project(window, cx)),
                )
                .top_action(
                    gpui_kit::assets::IconName::Search,
                    "Search",
                    cx.listener(|this, _, window, cx| this.toggle_palette(window, cx)),
                )
                .section_tools(self.sidebar_tools(cx));
        }
        if let Some(ix) = self.renaming.filter(|_| !self.rename_in_tab) {
            sidebar = sidebar.renaming(ix, &self.rename_input);
        }
        let now = now_ms();
        let row = |sidebar: ChatSidebar, section: &str, ix: usize, cx: &mut Context<Self>| {
            self.section_chat_row(sidebar, section.to_string(), ix, now, cx)
        };
        let pinned: Vec<usize> = (0..self.chats.len())
            .rev()
            .filter(|&ix| {
                let chat = &self.chats[ix];
                chat.pinned && !chat.archived && chat.is_agent()
            })
            .collect();
        let (groups, _) = self.project_groups();
        let names = self.project_names();
        let listed: Vec<(usize, String)> = groups
            .iter()
            .flat_map(|g| {
                let name = names.get(&g.folder).cloned().unwrap_or_default();
                g.chats.iter().map(move |&ix| (ix, name.clone()))
            })
            .collect();
        let sections: Vec<(String, Vec<usize>)> = match style {
            SidebarStyle::Focus => {
                let chats: Vec<_> = listed
                    .iter()
                    .map(|(ix, _)| (*ix, self.chats[*ix].status(now), self.chats[*ix].activity()))
                    .collect();
                projects::focus_sections(&chats)
                    .into_iter()
                    .map(|(label, ixs)| (label.to_string(), ixs))
                    .collect()
            }
            _ => {
                let chats: Vec<_> = listed
                    .iter()
                    .map(|(ix, project)| projects::Listed {
                        ix: *ix,
                        activity: self.chats[*ix].activity(),
                        title: &self.chats[*ix].title,
                        project,
                    })
                    .collect();
                projects::minimal_sections(
                    &chats,
                    self.settings.sidebar_grouping,
                    self.settings.sidebar_ordering,
                    now,
                    &chrono::Local,
                )
            }
        };
        let sections = std::iter::once(("Pinned".to_string(), pinned)).chain(sections).collect();
        let sections = if style == SidebarStyle::Minimal {
            nest_section_children(&self.chats, sections)
        } else {
            sections
                .into_iter()
                .map(|(label, ixs)| (label, ixs.into_iter().map(|ix| (ix, false)).collect()))
                .collect()
        };
        sidebar = sidebar.child_agents(
            sections
                .iter()
                .flat_map(|(_, rows)| rows)
                .filter(|(_, child)| *child)
                .map(|&(ix, _)| self.chats[ix].id)
                .collect(),
        );
        for (label, rows) in sections {
            for (ix, _) in rows {
                sidebar = row(sidebar, &label, ix, cx);
            }
        }
        sidebar
    }

    /// Archived chats of every kind, newest first; opening one restores it.
    fn render_archive_sidebar(&self, cx: &mut Context<Self>) -> ChatSidebar {
        let mut archived: Vec<usize> = (0..self.chats.len())
            .filter(|&ix| self.chats[ix].archived && !self.chats[ix].messages.is_empty())
            .collect();
        archived.sort_by_key(|&ix| std::cmp::Reverse((self.chats[ix].activity(), ix)));
        let mut sidebar = self
            .sidebar_chrome("Archive", cx)
            .on_new_chat(
                cx.listener(|this, _, window, cx| this.new_chat_in_current_project(window, cx)),
            )
            .new_chat_action(NewChat)
            .count(archived.len())
            .empty("No archived chats")
            .style(self.settings.sidebar_style)
            .archived();
        let ids: Vec<i64> = archived.iter().map(|&ix| self.chats[ix].id).collect();
        let picked: Vec<i64> =
            ids.iter().copied().filter(|id| self.archive_picks.contains(id)).collect();
        let view = cx.entity().downgrade();
        sidebar = sidebar.pickable(picked.clone(), move |id, _, cx| {
            view.update(cx, |this, cx| {
                if !this.archive_picks.remove(&id) {
                    this.archive_picks.insert(id);
                }
                cx.notify();
            })
            .ok();
        });
        if let Some(bar) =
            self.bulk_bar("archive-bulk", picked, ids, &[Bulk::Unarchive, Bulk::Delete], false, cx)
        {
            sidebar = sidebar.bulk_bar(bar);
        }
        if let Some(ix) = self.renaming.filter(|_| !self.rename_in_tab) {
            sidebar = sidebar.renaming(ix, &self.rename_input);
        }
        let now = now_ms();
        for ix in archived {
            let chat = &self.chats[ix];
            let section = projects::categorize_time(chat.activity(), now, &chrono::Local).label();
            sidebar = sidebar.chat_in_section(
                section,
                ix,
                chat.id,
                chat.title.clone(),
                ix == self.active && self.page == Page::Chat,
                chat.status(now),
                chat.pinned,
                chat.is_agent().then(|| self.chat_line(ix, false, now)).flatten(),
                cx.listener(move |this, _, window, cx| this.select(ix, window, cx)),
                cx.listener(move |this, _, window, cx| this.begin_rename(ix, window, cx)),
                cx.listener(move |this, _, window, cx| this.duplicate_chat(ix, window, cx)),
                cx.listener(move |this, _, _, cx| this.toggle_pin(ix, cx)),
                cx.listener(move |this, _, _, cx| this.unarchive_chat(ix, cx)),
                cx.listener(move |this, _, window, cx| this.confirm_delete_chat(ix, window, cx)),
                None,
                None,
            );
        }
        sidebar
    }

    /// Minimal's filter menu and add-project button, on its first section header.
    fn sidebar_tools(
        &self,
        cx: &mut Context<Self>,
    ) -> impl Fn(&mut Window, &mut gpui_kit::App) -> gpui_kit::AnyElement + 'static {
        use gpui_kit::component::Sizable;
        use gpui_kit::component::button::ButtonVariants;
        use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
        let app = cx.entity().downgrade();
        let (grouping, ordering) = (self.settings.sidebar_grouping, self.settings.sidebar_ordering);
        move |_, _| {
            let (menu_app, add_app) = (app.clone(), app.clone());
            let filter = aether_ui::button("sidebar-filter")
                .ghost()
                .xsmall()
                .icon(gpui_kit::assets::IconName::ListFilter)
                .accessibility_label("Group and order chats")
                .tooltip("Group and order")
                .dropdown_menu(move |mut menu: PopupMenu, _, _| {
                    let set = |change: fn(&mut aether_core::Settings)| {
                        let app = menu_app.clone();
                        move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut gpui_kit::App| {
                            app.update(cx, |this, cx| this.update_settings(cx, change)).ok();
                        }
                    };
                    menu = menu
                        .label("Group by")
                        .item(
                            PopupMenuItem::new("Updated")
                                .checked(grouping == SidebarGrouping::Updated)
                                .on_click(set(|s| s.sidebar_grouping = SidebarGrouping::Updated)),
                        )
                        .item(
                            PopupMenuItem::new("Project")
                                .checked(grouping == SidebarGrouping::Project)
                                .on_click(set(|s| s.sidebar_grouping = SidebarGrouping::Project)),
                        )
                        .separator()
                        .label("Order")
                        .item(
                            PopupMenuItem::new("Newest first")
                                .checked(ordering == SidebarOrdering::Newest)
                                .on_click(set(|s| s.sidebar_ordering = SidebarOrdering::Newest)),
                        )
                        .item(
                            PopupMenuItem::new("Oldest first")
                                .checked(ordering == SidebarOrdering::Oldest)
                                .on_click(set(|s| s.sidebar_ordering = SidebarOrdering::Oldest)),
                        )
                        .item(
                            PopupMenuItem::new("Name")
                                .checked(ordering == SidebarOrdering::Name)
                                .on_click(set(|s| s.sidebar_ordering = SidebarOrdering::Name)),
                        );
                    menu
                });
            let add = aether_ui::button("sidebar-add-project")
                .ghost()
                .xsmall()
                .icon(gpui_kit::assets::IconName::FolderPlus)
                .accessibility_label("Add project")
                .tooltip("Add project")
                .on_click(move |_, window, cx| {
                    add_app.update(cx, |this, cx| this.add_project_from_picker(window, cx)).ok();
                });
            gpui_kit::component::h_flex().gap_0p5().child(filter).child(add).into_any_element()
        }
    }

    fn section_chat_row(
        &self,
        sidebar: ChatSidebar,
        section: String,
        ix: usize,
        now: i64,
        cx: &mut Context<Self>,
    ) -> ChatSidebar {
        let chat = &self.chats[ix];
        sidebar.chat_in_section(
            section,
            ix,
            chat.id,
            chat.title.clone(),
            ix == self.active && self.page == Page::Chat,
            chat.status(now),
            chat.pinned,
            self.chat_line(ix, false, now),
            cx.listener(move |this, _, window, cx| this.select(ix, window, cx)),
            cx.listener(move |this, _, window, cx| this.begin_rename(ix, window, cx)),
            cx.listener(move |this, _, window, cx| this.duplicate_chat(ix, window, cx)),
            cx.listener(move |this, _, _, cx| this.toggle_pin(ix, cx)),
            cx.listener(move |this, _, window, cx| this.archive_chat(ix, window, cx)),
            cx.listener(move |this, _, window, cx| this.confirm_delete_chat(ix, window, cx)),
            None,
            None,
        )
    }

    /// Header and resizable edge shared by every sidebar view.
    fn sidebar_chrome(&self, title: &'static str, cx: &mut Context<Self>) -> ChatSidebar {
        let (resize, resize_end, watch, handoff) =
            (cx.entity(), cx.entity(), cx.entity(), cx.entity());
        ChatSidebar::new(title)
            .now(now_ms())
            .handoff(
                self.handoff_targets(None),
                self.chats
                    .iter()
                    .filter(|c| !c.messages.is_empty())
                    .map(|c| (c.id, c.provider))
                    .collect(),
                move |id, target, window, cx| {
                    handoff.update(cx, |this, cx| {
                        if let Some(ix) = this.chats.iter().position(|c| c.id == id) {
                            this.continue_in_new_chat(ix, target, window, cx);
                        }
                    })
                },
            )
            .lead_agents(self.chats.iter().filter(|c| c.lead).map(|c| c.id).collect())
            .when(self.settings.watch_enabled, |sidebar| {
                sidebar.watching(
                    self.settings
                        .watchlist
                        .iter()
                        .filter(|&w| matches!(w, Watch::Chat(_)))
                        .map(|w| w.chat())
                        .collect(),
                    move |id, _, cx| {
                        watch.update(cx, |this, cx| this.toggle_watch(Watch::Chat(id), cx))
                    },
                )
            })
            .mark("岚")
            .when(self.sidebar_view != SidebarView::Projects, |sidebar| {
                sidebar.on_back(
                    cx.listener(|this, _, window, cx| this.switch_sidebar_to_projects(window, cx)),
                )
            })
            .on_collapse(cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx)))
            .collapse_action(ToggleSidebar)
            .resizable(
                self.settings.sidebar_width.clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH),
                move |width, _, cx| {
                    resize.update(cx, |this, cx| {
                        this.sidebar_resizing = true;
                        this.settings.sidebar_width =
                            width.clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
                        cx.notify();
                    })
                },
                move |_, cx| {
                    resize_end.update(cx, |this, _| {
                        if std::mem::take(&mut this.sidebar_resizing) {
                            crate::settings::save(&this.settings);
                        }
                    })
                },
            )
    }

    fn render_outside_sidebar(&self, cx: &mut Context<Self>) -> ChatSidebar {
        let sidebar = self
            .sidebar_chrome("Outside aether", cx)
            .on_new_chat(
                cx.listener(|this, _, window, cx| this.new_chat_in_current_project(window, cx)),
            )
            .new_chat_action(NewChat)
            .count(self.external.len())
            .empty("No agents running outside aether");
        self.outside_rows(sidebar, cx)
    }

    fn render_direct_chats_sidebar(&self, cx: &mut Context<Self>) -> ChatSidebar {
        let mut sidebar = self
            .sidebar_chrome("Direct Chats", cx)
            .on_new_chat(cx.listener(|this, _, window, cx| this.new_direct_chat(window, cx)));

        if let Some(ix) = self.renaming.filter(|_| !self.rename_in_tab) {
            sidebar = sidebar.renaming(ix, &self.rename_input);
        }

        let now = now_ms();
        let mut pinned_direct: Vec<usize> = Vec::new();
        let mut unpinned_direct: Vec<usize> = Vec::new();

        for (ix, chat) in self.chats.iter().enumerate() {
            if !chat.archived && !chat.is_agent() {
                if chat.pinned {
                    pinned_direct.push(ix);
                } else {
                    unpinned_direct.push(ix);
                }
            }
        }

        pinned_direct.sort_by_key(|&ix| std::cmp::Reverse((self.chats[ix].activity(), ix)));
        unpinned_direct.sort_by_key(|&ix| std::cmp::Reverse((self.chats[ix].activity(), ix)));

        for ix in pinned_direct {
            sidebar = self.direct_chat_row(sidebar, "Pinned", ix, cx);
        }

        let mut groups: HashMap<projects::ChatSection, Vec<usize>> = HashMap::new();
        for ix in unpinned_direct {
            let sec = projects::categorize_time(self.chats[ix].activity(), now, &chrono::Local);
            groups.entry(sec).or_default().push(ix);
        }

        for sec in [
            projects::ChatSection::Today,
            projects::ChatSection::Yesterday,
            projects::ChatSection::Previous7Days,
            projects::ChatSection::Previous30Days,
            projects::ChatSection::Older,
        ] {
            if let Some(chats) = groups.get(&sec) {
                for &ix in chats {
                    sidebar = self.direct_chat_row(sidebar, sec.label(), ix, cx);
                }
            }
        }

        sidebar
    }

    fn outside_rows(&self, mut sidebar: ChatSidebar, cx: &mut Context<Self>) -> ChatSidebar {
        for a in &self.external {
            let (open, stop) = (a.clone(), a.clone());
            sidebar = sidebar.outside_agent(
                outside_line(a),
                cx.listener(move |this, _, window, cx| {
                    this.open_external(open.clone(), window, cx)
                }),
                cx.listener(move |this, _, window, cx| {
                    this.stop_external(stop.clone(), window, cx)
                }),
            );
        }
        sidebar
    }

    fn direct_chat_row(
        &self,
        sidebar: ChatSidebar,
        section: impl Into<SharedString>,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> ChatSidebar {
        let chat = &self.chats[ix];
        let id = chat.id;
        let title = chat.title.clone();
        let active = ix == self.active && self.page == Page::Chat;
        let status = chat.status(now_ms());
        let pinned = chat.pinned;
        sidebar.chat_in_section(
            section,
            ix,
            id,
            title,
            active,
            status,
            pinned,
            None,
            cx.listener(move |this, _, window, cx| this.select(ix, window, cx)),
            cx.listener(move |this, _, window, cx| this.begin_rename(ix, window, cx)),
            cx.listener(move |this, _, window, cx| this.duplicate_chat(ix, window, cx)),
            cx.listener(move |this, _, _, cx| this.toggle_pin(ix, cx)),
            cx.listener(move |this, _, window, cx| this.archive_chat(ix, window, cx)),
            cx.listener(move |this, _, window, cx| this.confirm_delete_chat(ix, window, cx)),
            None,
            None,
        )
    }

    fn chat_row(
        &self,
        sidebar: ChatSidebar,
        ix: usize,
        project: Option<usize>,
        worktree: Option<usize>,
        line: Option<ChatLine>,
        cx: &mut Context<Self>,
    ) -> ChatSidebar {
        let chat = &self.chats[ix];
        let remove =
            chat.has_worktree(&self.missing_folders, &self.settings.workspace_root).then(|| {
                Box::new(
                    cx.listener(move |this, _, window, cx| this.remove_worktree(ix, window, cx)),
                ) as aether_ui::ClickHandler
            });
        sidebar.chat(
            ix,
            chat.id,
            chat.title.clone(),
            ix == self.active,
            chat.status(now_ms()),
            chat.pinned,
            project,
            worktree,
            line,
            cx.listener(move |this, _, window, cx| this.select(ix, window, cx)),
            cx.listener(move |this, _, window, cx| this.begin_rename(ix, window, cx)),
            cx.listener(move |this, _, window, cx| this.duplicate_chat(ix, window, cx)),
            cx.listener(move |this, _, _, cx| this.toggle_pin(ix, cx)),
            cx.listener(move |this, _, window, cx| this.archive_chat(ix, window, cx)),
            cx.listener(move |this, _, window, cx| this.confirm_delete_chat(ix, window, cx)),
            remove,
            None,
        )
    }

    /// Details of a CLI chat's row: project, branch inside its project group
    /// (folder elsewhere), age or how long the turn has run, and model.
    fn chat_line(&self, ix: usize, in_project: bool, now: i64) -> Option<ChatLine> {
        let chat = &self.chats[ix];
        if chat.provider == Provider::OpenRouter {
            return None;
        }
        let folder = chat_cwd(chat, &self.settings.workspace_root);
        let path = folder.display().to_string();
        let project = project_dir(chat, &self.settings.workspace_root);
        let place = row_place(in_project, &chat.worktree_branch, &folder, &project, &self.branches);
        let status = chat.status(now);
        let time = match status {
            ChatStatus::Idle if chat.messages.is_empty() => String::new(),
            ChatStatus::Idle | ChatStatus::Background(_) => {
                projects::relative_time(chat.activity(), now, &chrono::Local)
            }
            _ => status.text(now),
        };
        let removed = chat.worktree_removed() || self.worktree_missing(ix);
        let (changed, added, deleted, ahead_behind) =
            self.sidebar_git_summary(&folder).unwrap_or((0, 0, 0, None));
        let models = self.probes.get(&chat.provider).map_or(&[][..], |probe| &probe.models);
        Some(ChatLine {
            provider: chat.provider,
            project: (project != home_dir())
                .then(|| folder_label(&project.display().to_string()).into()),
            model: model_label(models, &chat.model(&self.settings)).into(),
            place,
            time: time.into(),
            path: path.into(),
            removed,
            changed,
            added,
            deleted,
            ahead_behind,
            background: chat.background_count(),
        })
    }

    pub(crate) fn project_cards(&self, folder: &Path) -> Vec<Card> {
        let root = &self.settings.workspace_root;
        self.chats
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.is_agent()
                    && !c.archived
                    && !c.messages.is_empty()
                    && project_dir(c, root) == folder
            })
            .map(|(ix, chat)| {
                let cwd = chat_cwd(chat, root);
                let last = chat.messages.last();
                let status = card_status(
                    chat.status(now_ms()),
                    last.is_some_and(|m| m.failed),
                    chat.wait_label(),
                );
                let place = if chat.worktree_branch.is_empty() {
                    self.branches
                        .get(&cwd)
                        .cloned()
                        .flatten()
                        .map_or_else(|| folder_label(&cwd.display().to_string()), |b| b.branch)
                } else {
                    chat.worktree_branch.clone()
                };
                let models = self.probes.get(&chat.provider).map_or(&[][..], |p| &p.models);
                let (added, removed) = edit_totals(chat.messages.iter().flat_map(|m| &m.steps));
                Card {
                    ix,
                    id: chat.id,
                    title: chat.title.to_string(),
                    provider: chat.provider,
                    model: model_label(models, &chat.model(&self.settings)),
                    status,
                    since_ms: chat.turn_at,
                    place,
                    added,
                    removed,
                    activity_ms: chat.activity(),
                    text: last_text(last, status),
                    cwd,
                }
            })
            .collect()
    }

    /// Resolves the active project or worktree folder for the current chat,
    /// falling back to workspace root or user home.
    pub(crate) fn active_project_folder(&self) -> PathBuf {
        let root = &self.settings.workspace_root;
        if let Some(chat) = self.chats.get(self.active) {
            chat_cwd(chat, root)
        } else {
            aether_terminal::resolve_global_folder(root)
        }
    }

    /// Returns the cached ExplorerState entity for the currently active project/worktree folder,
    /// creating one if it doesn't exist yet.
    pub(crate) fn active_project_explorer(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Entity<aether_explorer::ExplorerState> {
        if self.page == Page::Notes {
            return self.notes_explorer(cx);
        }
        let folder = self.active_project_folder();
        if let Some(exp) = self.project_explorers.get(&folder) {
            return exp.clone();
        }
        let exp = cx.new(|_| aether_explorer::ExplorerState::new(folder.clone()));
        self.project_explorers.insert(folder, exp.clone());
        self.load_explorer_index(&exp, cx);
        exp
    }

    /// Opens the project explorer for `folder`, selecting its most active chat or creating one if needed.
    pub(crate) fn open_project_explorer(
        &mut self,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = &self.settings.workspace_root;
        let matching_ix = self
            .chats
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.archived && project_dir(c, root) == folder)
            .max_by_key(|(_, c)| c.activity())
            .map(|(ix, _)| ix);

        if let Some(ix) = matching_ix {
            self.select(ix, window, cx);
        } else {
            self.new_chat_in(folder.clone(), window, cx);
        }

        if self.page != Page::Chat {
            self.open_page(Page::Chat, cx);
        }

        self.open_explorer_panel(window, cx);

        cx.notify();
    }

    /// Adds a reference to `path` and the active tab's
    /// selected lines, pre-populated in the composer.
    pub(crate) fn ask_agent_about_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mention = self.explorer_file_mention(&path, cx);
        if self.page != Page::Chat {
            self.open_page(Page::Chat, cx);
        }
        self.composer.update(cx, |s, cx| {
            let current = s.value().to_string();
            let new_text =
                if current.trim().is_empty() { mention } else { format!("{current} {mention}") };
            s.set_value(new_text, window, cx);
        });
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(crate) fn add_project_from_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.pop() else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.update_settings(cx, |s| projects::add_project(s, &path));
                this.new_chat_in(path, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Starts an agent chat fixed to `folder`, on the CLI last used there,
    /// else the last CLI picked, else Claude Code.
    pub(crate) fn new_chat_in(
        &mut self,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = self.settings.workspace_root.clone();
        let provider = self
            .chats
            .iter()
            .filter(|c| {
                !c.messages.is_empty()
                    && c.folder(&root).is_some()
                    && project_dir(c, &root) == folder
            })
            .max_by_key(|c| c.activity())
            .map(|c| c.provider)
            .or(CLIS.contains(&self.default_provider()).then_some(self.default_provider()))
            .unwrap_or(Provider::Claude);
        self.new_chat(window, cx);
        let ix = self.active;
        let chat = &mut self.chats[ix];
        chat.provider = provider;
        chat.provider_model = initial_model(&self.settings, provider);
        chat.access = self.settings.default_access(provider);
        chat.effort.clear();
        chat.cwd = folder.display().to_string();
        self.unhide_project(ix);
        self.sync_selector(cx);
        self.sync_placeholder(window, cx);
        self.load_bases(cx);
        self.refresh_git(false, cx);
        cx.notify();
    }

    /// A removed project comes back once one of its chats is used again.
    pub(super) fn unhide_project(&mut self, ix: usize) {
        let chat = &self.chats[ix];
        if chat.provider == Provider::OpenRouter {
            return;
        }
        let folder = project_dir(chat, &self.settings.workspace_root);
        if projects::listed(&self.settings.hidden_projects, &folder) {
            projects::set_listed(&mut self.settings.hidden_projects, &folder, false);
            settings::save(&self.settings);
        }
    }

    /// Re-reads branches off the UI thread: just `only`, or every shown agent
    /// folder. Asked again mid-refresh, it runs one full pass afterwards.
    pub(super) fn refresh_branches(&mut self, only: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.refreshing_branches {
            self.branches_stale = true;
            return;
        }
        let root = &self.settings.workspace_root;
        let active = self.chats.get(self.active).and_then(|c| c.folder(root));
        let mut folders: Vec<PathBuf> = match only {
            Some(folder) => vec![folder],
            None => self
                .chats
                .iter()
                .filter(|c| !c.archived && c.provider != Provider::OpenRouter)
                .flat_map(|c| [chat_cwd(c, root), project_dir(c, root)])
                .filter(|f| {
                    !projects::listed(&self.settings.hidden_projects, f)
                        || active.as_ref() == Some(f)
                })
                .collect(),
        };
        folders.sort();
        folders.dedup();
        if folders.is_empty() {
            return;
        }
        self.refreshing_branches = true;
        let found = self.agent.branch_infos(folders);
        cx.spawn(async move |this, cx| {
            let found = found.await;
            this.update(cx, |this, cx| {
                this.refreshing_branches = false;
                let mut git_folders = Vec::new();
                for (folder, exists, info) in found {
                    if exists {
                        this.missing_folders.remove(&folder);
                    } else {
                        this.missing_folders.insert(folder.clone());
                    }
                    git_folders.push(folder.clone());
                    this.branches.insert(folder, info);
                }
                // Branch reads just mapped every shown folder, so piggyback
                // the sidebar counts on the same pass; its own dedup keeps
                // one worktree at 1 git call.
                this.refresh_sidebar_git(git_folders, cx);
                if std::mem::take(&mut this.branches_stale) {
                    this.refresh_branches(None, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Sidebar row counts for `folder` from the shared per-folder git cache.
    pub(crate) fn sidebar_git_summary(
        &self,
        folder: &Path,
    ) -> Option<(usize, u32, u32, Option<(u32, u32)>)> {
        self.git.sidebar_git_summary(folder)
    }

    /// Refreshes sidebar folders with no good read yet, off the UI thread.
    /// Callers pass visible plus active folders; dedup plus the in-flight
    /// set keeps one worktree at 1 git call and the panel paints each
    /// result as it lands.
    pub(crate) fn refresh_sidebar_git(&mut self, folders: Vec<PathBuf>, cx: &mut Context<Self>) {
        // gpui test scheduler aborts on wakeups from tokio git tasks
        if cfg!(test) {
            return;
        }
        // Filter first: sidebar_missing claims its result in flight, and a
        // skipped folder would hold its claim forever with no read landing.
        let folders: Vec<PathBuf> =
            folders.into_iter().filter(|f| !self.missing_folders.contains(f)).collect();
        for folder in self.git.sidebar_missing(&folders) {
            let task = self.agent.git_status(folder.clone());
            cx.spawn(async move |this, cx| {
                let status = task.await;
                this.update(cx, |this, cx| {
                    this.git.insert_sidebar_status(folder, status);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/sidebar.rs"]
mod tests;
