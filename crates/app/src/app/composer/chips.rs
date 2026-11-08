use aether_core::{AccessMode, FooterChips};
use aether_ui::theme::Corners;
use aether_ui::{
    ChoiceMenu, ChoiceOption, WorkspaceSelect, WorkspaceSelection, WorktreeRow, anchored_popup,
    composer_chip, context_label, control_chip, provider_icon,
};
use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, IconName, h_flex, v_flex};
use gpui_kit::{AnyElement, Context, FontWeight, PathPromptOptions, Window, div, prelude::*, px};

use std::path::PathBuf;

use aether_core::home_dir;

use crate::app::sidebar::folder_label;
use crate::app::worktree::{workspace_label, worktree_preview_branch};
use crate::app::{ChatApp, chat_cwd, now_ms};
use crate::projects::{ProjectEntry, add_project, project_list, project_switcher_options};

fn access_icon(mode: AccessMode) -> gpui_kit::assets::IconName {
    use gpui_kit::assets::IconName;
    match mode {
        AccessMode::Ask => IconName::Lock,
        AccessMode::AutoEdit => IconName::Pencil,
        AccessMode::Auto => IconName::Asterisk,
        AccessMode::ReadOnly => IconName::LockKeyhole,
        AccessMode::Full => IconName::LockOpen,
    }
}

const BUSY_TIP: &str = "Can't change while the agent is running";
pub(in crate::app) const JOB_TIP: &str = "Can't change while a Git operation is running";

pub(super) fn resting_chip(chip: Button, resting: bool) -> Button {
    if resting { chip.h(px(22.)).text_size(px(11.5)) } else { chip }
}

fn tray_chip(id: impl Into<gpui_kit::ElementId>, active: bool, cx: &gpui_kit::App) -> Button {
    control_chip(id, active, cx)
        .h(px(24.))
        .px(px(7.))
        .text_size(px(12.))
        .font_weight(FontWeight::NORMAL)
}

impl ChatApp {
    pub(crate) fn set_draft_folder(&mut self, ix: usize, folder: PathBuf, cx: &mut Context<Self>) {
        self.apply_draft_folder(ix, folder, true, cx);
    }

    pub(crate) fn apply_draft_folder(
        &mut self,
        ix: usize,
        folder: PathBuf,
        persist: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_can_change(ix) {
            return;
        }
        self.chats[ix].cwd = folder.display().to_string();
        self.chats[ix].new_worktree = None;
        self.chats[ix].new_worktree_existing = None;
        self.chats[ix].worktree_error = None;
        if persist {
            self.persist(ix);
        }
        self.load_bases(cx);
        self.refresh_git(false, cx);
        cx.notify();
    }

    pub(in crate::app) fn switcher_list(&self) -> Vec<ProjectEntry> {
        let active = self
            .project_groups()
            .0
            .into_iter()
            .filter(|g| !g.chats.is_empty())
            .map(|g| {
                let at = g.chats.iter().map(|&i| self.chats[i].activity()).max().unwrap_or(0);
                (g.folder, at)
            })
            .collect();
        project_list(active, &self.settings.projects, &self.settings.hidden_projects)
    }

    pub(crate) fn open_project_switcher_sync(&self, cx: &mut Context<Self>) {
        let chat = &self.chats[self.active];
        let current = chat_cwd(chat, &self.settings.workspace_root);
        let list = self.switcher_list();
        let options = project_switcher_options(&list, &self.project_names(), &current, &home_dir());
        *self.switcher_folders.borrow_mut() = list.into_iter().map(|p| p.folder).collect();
        for menu in [&self.project_menu, &self.headline_project_menu] {
            menu.update(cx, |m, cx| m.sync(options.clone(), Some("Projects".into()), 320., cx));
        }
    }

    pub(in crate::app) fn pick_project(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folder = self.switcher_folders.borrow().get(ix).cloned();
        match folder {
            Some(folder) => self.set_draft_folder(self.active, folder, cx),
            None => self.add_project_and_move(window, cx),
        }
    }

    fn add_project_and_move(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.chats[self.active].id;
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
            this.update(cx, |this, cx| {
                this.update_settings(cx, |s| add_project(s, &path));
                if let Some(ix) = this.chats.iter().position(|c| c.id == id) {
                    this.set_draft_folder(ix, path, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn set_access(&mut self, mode: AccessMode, cx: &mut Context<Self>) {
        let ix = self.active;
        if self.switching_folder(&chat_cwd(&self.chats[ix], &self.settings.workspace_root)) {
            return;
        }
        if self.chats[ix].cancel.is_some() {
            return;
        }
        self.chats[ix].access = mode;
        if !self.chats[ix].messages.is_empty() {
            self.persist(ix);
        }
        cx.notify();
    }

    pub(in crate::app) fn set_effort(&mut self, effort: String, cx: &mut Context<Self>) {
        let ix = self.active;
        if self.switching_folder(&chat_cwd(&self.chats[ix], &self.settings.workspace_root)) {
            return;
        }
        if self.chats[ix].cancel.is_some() {
            return;
        }
        self.chats[ix].effort = effort;
        if !self.chats[ix].messages.is_empty() {
            self.persist(ix);
        }
        self.sync_selector(cx);
        cx.notify();
    }

    /// Access menu for the control row; `None` when the provider has no access modes.
    pub(super) fn access_control(
        &self,
        resting: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let chat = &self.chats[self.active];
        let (provider, busy, access) = (chat.provider, chat.cancel.is_some(), chat.access);
        if !FooterChips::for_provider(provider).access {
            return None;
        }
        let job = chat.worktree_job.is_some()
            || self.switching_folder(&chat_cwd(chat, &self.settings.workspace_root));
        let options = AccessMode::for_provider(provider)
            .iter()
            .map(|&mode| ChoiceOption {
                mark: None,
                icon: Some(access_icon(mode)),
                image: None,
                title: mode.label().into(),
                description: Some(mode.description().into()),
                checked: mode == access,
                disabled: false,
            })
            .collect();
        self.access_menu.update(cx, |m, cx| m.sync(options, None, 310., cx));
        let trigger = resting_chip(control_chip("access", false, cx), resting)
            .icon(access_icon(access))
            .label(access.label())
            .tooltip(if busy { BUSY_TIP } else { "Access" })
            .disabled(busy || job);
        Some(ChoiceMenu::new(&self.access_menu, trigger).into_any_element())
    }

    /// Strip under the card: folder and workspace on the left, context on the right.
    pub(super) fn tray(&self, cx: &mut Context<Self>) -> AnyElement {
        let chat = &self.chats[self.active];
        let (provider, busy) = (chat.provider, chat.cancel.is_some());
        let muted = cx.theme().muted_foreground;
        let context = self.context_ring().and_then(|(ring, used, total)| {
            context_label(used, total).map(|label| {
                h_flex()
                    .gap_1p5()
                    .pr_1()
                    .items_center()
                    .flex_none()
                    .text_size(px(12.))
                    .text_color(muted)
                    .child(ring.diameter(14.))
                    .child(label)
            })
        });
        let mut left = h_flex().gap(px(2.)).items_center().min_w_0();
        if !FooterChips::for_provider(provider).folder {
            left = left
                .pl(px(7.))
                .gap_1p5()
                .text_size(px(12.))
                .text_color(muted)
                .child(Icon::new(IconName::Globe).size_3())
                .child("Direct chat, no workspace");
        } else {
            let locked = !chat.messages.is_empty() || chat.worktree_bound();
            let path = chat_cwd(chat, &self.settings.workspace_root).display().to_string();
            let job = chat.worktree_job.is_some()
                || self.switching_folder(&chat_cwd(chat, &self.settings.workspace_root));
            self.open_project_switcher_sync(cx);
            let folder = tray_chip("folder", false, cx)
                .icon(IconName::Folder)
                .label(folder_label(&path))
                .max_w(px(160.))
                .tooltip(match (locked, job) {
                    (true, _) => format!("Folder is fixed for this chat: {path}"),
                    (_, true) => JOB_TIP.into(),
                    _ => path.clone(),
                })
                .disabled(locked || job);
            left = left.child(ChoiceMenu::new(&self.project_menu, folder));
            left = left.children(self.workspace_select(busy, cx));
        }
        let running = chat.live_background().count();
        let background = (running > 0).then(|| {
            let id = chat.id;
            let shown = !self.background_hidden.contains(&id);
            tray_chip("background", shown, cx)
                .icon(gpui_kit::assets::IconName::Layers)
                .label(format!("{running} background"))
                .tooltip(if shown { "Hide background tasks" } else { "Show background tasks" })
                .on_click(self.session_listener(cx, move |this, _, _, cx| {
                    if !this.background_hidden.remove(&id) {
                        this.background_hidden.insert(id);
                    }
                    cx.notify();
                }))
        });
        let crew =
            chat.lead.then(|| self.crew(self.active)).filter(|kids| !kids.is_empty()).map(|kids| {
                let now = now_ms();
                let count = kids.len();
                let open = self.crew_pop;
                let trigger = control_chip("crew-strip", open, cx)
                    .icon(gpui_kit::assets::IconName::Users)
                    .label(count.to_string())
                    .tooltip("Running agents")
                    .debug_selector(|| "crew-strip".into())
                    .when(!open, |t| {
                        t.on_click(self.session_listener(cx, |this, _, _, cx| {
                            this.crew_pop = true;
                            cx.notify();
                        }))
                    });
                let pop = open.then(|| {
                    let theme = cx.theme();
                    let list = v_flex()
                        .id("crew-pop")
                        .debug_selector(|| "crew-pop".into())
                        .w(px(320.))
                        .max_h(px(320.))
                        .overflow_y_scroll()
                        .p(px(6.))
                        .gap(px(2.))
                        .corner(px(12.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.popover)
                        .shadow_lg()
                        .children(kids.into_iter().map(|ix| {
                            let child = &self.chats[ix];
                            let id = child.id;
                            let title = match child.title.char_indices().nth(24) {
                                Some((i, _)) => format!("{}…", &child.title[..i]),
                                None => child.title.to_string(),
                            };
                            composer_chip(("crew-chip", id as u64), false, cx)
                                .w_full()
                                .justify_start()
                                .tooltip(format!(
                                    "{} — {}. Click to open: message or stop it there.",
                                    child.title,
                                    crate::app::orchestrate::child_status_label(child)
                                ))
                                .child(
                                    h_flex()
                                        .w_full()
                                        .justify_start()
                                        .gap_1()
                                        .child(provider_icon(child.provider).size_3p5().flex_none())
                                        .child(div().min_w_0().text_ellipsis().child(title))
                                        .child(child.status(now).badge(
                                            ("crew-status", ix),
                                            Some(now),
                                            cx,
                                        )),
                                )
                                .debug_selector(move || format!("crew-chip-{id}"))
                                .on_click(self.session_listener(cx, move |this, _, window, cx| {
                                    this.crew_pop = false;
                                    this.open_chat(id, window, cx)
                                }))
                        }));
                    h_flex()
                        .on_mouse_down_out(self.session_listener(cx, |this, _, _, cx| {
                            this.crew_pop = false;
                            cx.notify();
                        }))
                        .child(list)
                        .into_any_element()
                });
                let hug = h_flex().child(trigger);
                anchored_popup("crew-pop-anchor", hug, &self.crew_pop_trigger, false)
                    .when_some(pop, |p, pop| p.content(pop))
            });
        h_flex()
            .w_full()
            .px(px(8.))
            .pt(px(5.))
            .pb(px(6.))
            .gap_2()
            .items_center()
            .justify_between()
            .child(left)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .children(background)
                    .children(crew)
                    .children(context),
            )
            .into_any_element()
    }

    fn workspace_select(&self, busy: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let chat = &self.chats[self.active];
        let locked = !chat.messages.is_empty() || chat.worktree_bound();
        let path = chat_cwd(chat, &self.settings.workspace_root).display().to_string();
        let shared = self.shareable_worktrees(self.active);
        let bases = self.active_bases();
        if bases.is_some() || !shared.is_empty() || locked {
            use gpui_kit::assets::IconName as Lucide;
            let chat = &self.chats[self.active];
            let current = self
                .branches
                .get(&chat_cwd(chat, &self.settings.workspace_root))
                .and_then(|b| b.as_ref())
                .map(|b| {
                    if b.detached {
                        format!("Detached HEAD · {}", b.branch)
                    } else {
                        b.branch.clone()
                    }
                });
            let selection = match (&chat.new_worktree, &chat.new_worktree_existing) {
                (Some(base), _) => WorkspaceSelection::NewWorktree(base.clone()),
                (_, Some(path)) => WorkspaceSelection::Worktree(path.clone()),
                _ if !chat.worktree_path.is_empty() => {
                    WorkspaceSelection::Worktree(chat.worktree_path.clone().into())
                }
                _ => current
                    .as_ref()
                    .map(|b| WorkspaceSelection::Branch(b.clone()))
                    .unwrap_or_default(),
            };
            let selection_kind = selection.clone();
            let label = workspace_label(&selection, current.as_deref());
            let tip = match &selection {
                WorkspaceSelection::NewWorktree(base) => format!(
                    "Your first message creates {} from {base}",
                    worktree_preview_branch(
                        &self.worktree_name_input.read(cx).value(),
                        &self.composer.read(cx).value()
                    )
                ),
                WorkspaceSelection::Worktree(path) => path.display().to_string(),
                _ => path.clone(),
            };
            let disabled = !self.workspace_can_change(self.active);
            self.worktree_menu.update(cx, |m, cx| {
                m.sync(
                    bases.map(|b| b.branches.clone()).unwrap_or_default(),
                    shared
                        .into_iter()
                        .map(|(path, branch)| WorktreeRow {
                            running: self.worktree_running(&path),
                            path,
                            branch,
                        })
                        .collect(),
                    selection,
                    disabled,
                    cx,
                )
            });
            let trigger = tray_chip(
                "worktree",
                chat.new_worktree.is_some() || chat.new_worktree_existing.is_some(),
                cx,
            )
            .icon(if matches!(selection_kind, WorkspaceSelection::Branch(_)) {
                Lucide::GitBranch
            } else {
                Lucide::GitFork
            })
            .label(label)
            .max_w(px(220.))
            .tooltip(if locked {
                "Workspace is fixed for this chat".into()
            } else if busy {
                BUSY_TIP.into()
            } else if disabled {
                JOB_TIP.to_string()
            } else {
                tip
            })
            .disabled(disabled);
            return Some(WorkspaceSelect::new(&self.worktree_menu, trigger).into_any_element());
        }
        None
    }
}
