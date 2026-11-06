use crate::session_workspace::WindowId;
use std::path::PathBuf;

use aether_core::{Appearance, Message, Reasoning, Role};
use aether_ui::CommandMenu;
use gpui_kit::component::{
    IconName, IndexPath,
    command::{CommandGroup, CommandItem},
};
use gpui_kit::{ClipboardItem, Context, SharedString, Window};

use crate::actions::{
    NewChat, OpenSettings, OpenSkills, OpenUsage, StopGenerating, ToggleHackerNews, ToggleWebSearch,
};
use crate::app::orchestrate::shows_lead_chip;
use crate::app::{ChatApp, chat_markdown, picker_value};

/// What a palette row does when it has no Action of its own.
#[derive(Clone)]
pub(crate) enum PaletteCommand {
    SessionLayout(crate::session_workspace::PaneAction),
    TileLayout(aether_core::TileLayout),
    ChatLayout(aether_core::ChatLayout),
    OpenChat(usize),
    SearchMatch {
        chat: usize,
        message: usize,
    },
    CopyChat,
    ExportChat,
    Retry,
    Theme(Appearance),
    Reasoning(Reasoning),
    Model(String),
    UseSkill(String),
    RenameChat,
    TogglePin,
    ToggleLead,
    ArchiveChat,
    DeleteChat,
    RevealFolder,
    ResumeInTerminal,
    OpenYazi,
    RemoveWorktree,
    CleanUpWorktrees,
    ToggleChanges,
    /// `Some(base)`: work in a new worktree from it; `None`: work locally.
    Worktree(Option<String>),
    /// Share this live worktree on the first send.
    ShareWorktree(PathBuf),
    NewDirectChat,
    SwitchSidebar(crate::app::SidebarView),
}

/// A palette item and the text it's found by: its label, then its keywords.
struct Item {
    item: CommandItem,
    terms: Vec<SharedString>,
}

fn item(label: impl Into<SharedString>) -> Item {
    let label = label.into();
    Item { item: CommandItem::new().label(label.clone()), terms: vec![label] }
}

impl Item {
    fn icon(mut self, icon: impl Into<gpui_kit::component::Icon>) -> Self {
        self.item = self.item.icon(icon);
        self
    }

    fn checked(mut self, checked: bool) -> Self {
        self.item = self.item.checked(checked);
        self
    }

    fn action(mut self, action: Box<dyn gpui_kit::Action>) -> Self {
        self.item = self.item.action(action);
        self
    }

    fn keywords<S: Into<SharedString>>(mut self, keywords: impl IntoIterator<Item = S>) -> Self {
        let keywords: Vec<SharedString> = keywords.into_iter().map(Into::into).collect();
        self.item = self.item.keywords(keywords.clone());
        self.terms.extend(keywords);
        self
    }
}

/// gpui-kit's palette rule: the trimmed query, ignoring case, is in the label or a keyword.
fn matches(query: &str, terms: &[SharedString]) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || terms.iter().any(|t| t.to_lowercase().contains(&query))
}

type Row = (Item, Option<PaletteCommand>);

impl ChatApp {
    pub(crate) fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette_open {
            return self.close_palette(window, cx);
        }
        self.palette_open = true;
        self.palette_restore = window.focused(cx);
        self.palette_query.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        self.palette.update(cx, |state, cx| state.set_query("", window, cx));
        cx.notify();
    }

    /// Puts `@name` at the end of the composer so the next message uses it.
    fn mention_skill(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let mention = format!("@{name}");
        self.page = crate::app::Page::Chat;
        self.composer.update(cx, |input, cx| {
            let text = input.value();
            if !text.split_whitespace().any(|word| word == mention) {
                let sep =
                    if text.is_empty() || text.ends_with(char::is_whitespace) { "" } else { " " };
                input.set_value(format!("{text}{sep}{mention} "), window, cx);
            }
            input.focus(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = false;
        match self.palette_restore.take() {
            Some(previous) => previous.focus(window, cx),
            None => self.composer.update(cx, |s, cx| s.focus(window, cx)),
        }
        cx.notify();
    }

    fn run_palette(&mut self, ix: IndexPath, window: &mut Window, cx: &mut Context<Self>) {
        let command =
            self.palette_rows.get(ix.section).and_then(|rows| rows.get(ix.row)).cloned().flatten();
        self.close_palette(window, cx);
        match command {
            Some(PaletteCommand::SessionLayout(action)) => {
                if let Some(pane) = self.workspace.model.pane_for(self.chats[self.active].id) {
                    self.run_session_pane_action(pane, action, window, cx);
                }
            }
            Some(PaletteCommand::TileLayout(layout)) => {
                self.update_settings(cx, |s| s.tile_layout = layout)
            }
            Some(PaletteCommand::ChatLayout(layout)) => {
                self.update_settings(cx, |s| s.chat_layout = layout)
            }
            Some(PaletteCommand::OpenChat(chat)) => self.select(chat, window, cx),
            Some(PaletteCommand::SearchMatch { chat, message }) => {
                self.jump_to_message(chat, message, window, cx)
            }
            Some(PaletteCommand::CopyChat) => {
                let md = chat_markdown(&self.chats[self.active]);
                cx.write_to_clipboard(ClipboardItem::new_string(md));
            }
            Some(PaletteCommand::ExportChat) => self.export_chat(window, cx),
            Some(PaletteCommand::Retry) => self.retry(window, cx),
            Some(PaletteCommand::Theme(appearance)) => self.set_appearance(appearance, window, cx),
            Some(PaletteCommand::Reasoning(reasoning)) => {
                self.update_settings(cx, |s| s.reasoning = reasoning)
            }
            Some(PaletteCommand::Model(id)) => self.pick_model(id, window, cx),
            Some(PaletteCommand::UseSkill(name)) => self.mention_skill(&name, window, cx),
            Some(PaletteCommand::RenameChat) => self.begin_rename(self.active, window, cx),
            Some(PaletteCommand::TogglePin) => self.toggle_pin(self.active, cx),
            Some(PaletteCommand::ToggleLead) => self.toggle_lead(self.active, window, cx),
            Some(PaletteCommand::ArchiveChat) => self.archive_chat(self.active, window, cx),
            Some(PaletteCommand::DeleteChat) => self.confirm_delete_chat(self.active, window, cx),
            Some(PaletteCommand::RemoveWorktree) => self.remove_worktree(self.active, window, cx),
            Some(PaletteCommand::CleanUpWorktrees) => self.open_worktrees(cx),
            Some(PaletteCommand::ToggleChanges) => self.toggle_changes(false, window, cx),
            Some(PaletteCommand::Worktree(base)) => self.choose_worktree(base, cx),
            Some(PaletteCommand::ShareWorktree(path)) => self.choose_existing_worktree(path, cx),
            Some(PaletteCommand::RevealFolder) => {
                if let Some(folder) = self.chats[self.active].folder(&self.settings.workspace_root)
                {
                    cx.reveal_path(&folder);
                }
            }
            Some(PaletteCommand::ResumeInTerminal) => {
                self.resume_in_terminal(self.active, window, cx)
            }
            Some(PaletteCommand::OpenYazi) => self.open_tool_tab("yazi", window, cx),
            Some(PaletteCommand::NewDirectChat) => self.new_direct_chat(window, cx),
            Some(PaletteCommand::SwitchSidebar(view)) => match view {
                crate::app::SidebarView::Projects => self.switch_sidebar_to_projects(window, cx),
                crate::app::SidebarView::DirectChats => {
                    self.switch_sidebar_to_direct_chats(window, cx)
                }
                crate::app::SidebarView::Outside => self.switch_sidebar_to_outside(cx),
                crate::app::SidebarView::Archived => self.switch_sidebar_to_archive(cx),
            },
            None => {}
        }
    }

    pub(crate) fn render_palette(&mut self, host: WindowId, cx: &mut Context<Self>) -> CommandMenu {
        let mut groups: Vec<(&str, Vec<Row>)> = Vec::new();

        groups.push((
            "Session panes",
            [
                ("Split session right", crate::session_workspace::PaneAction::SplitRight),
                ("Split session down", crate::session_workspace::PaneAction::SplitDown),
                ("Float session pane", crate::session_workspace::PaneAction::Float),
                ("Dock session pane", crate::session_workspace::PaneAction::Dock),
                ("Move session to desktop window", crate::session_workspace::PaneAction::Detach),
                ("Return session to main window", crate::session_workspace::PaneAction::Return),
                ("Maximize / restore session pane", crate::session_workspace::PaneAction::Maximize),
                ("Make session the main pane", crate::session_workspace::PaneAction::Promote),
                ("Hide session pane", crate::session_workspace::PaneAction::Close),
            ]
            .into_iter()
            .map(|(label, action)| {
                (
                    item(label).icon(IconName::PanelRight),
                    Some(PaletteCommand::SessionLayout(action)),
                )
            })
            .collect(),
        ));
        groups.push((
            "Layout",
            aether_core::TileLayout::ALL
                .into_iter()
                .map(|layout| {
                    (
                        item(format!("Layout: {}", layout.label()))
                            .keywords(["tiling", "tile", "arrange", "panes"])
                            .checked(self.settings.tile_layout == layout),
                        Some(PaletteCommand::TileLayout(layout)),
                    )
                })
                .collect(),
        ));
        groups.push((
            "Chat layout",
            [
                (aether_core::ChatLayout::Tabbed, "Tabbed"),
                (aether_core::ChatLayout::Sidebar, "Sidebar"),
                (aether_core::ChatLayout::Split, "Split"),
            ]
            .into_iter()
            .map(|(layout, name)| {
                (
                    item(format!("Chat layout: {name}"))
                        .keywords(["chat layout", "tabbed", "sidebar", "split", "panel"])
                        .checked(self.settings.chat_layout == layout),
                    Some(PaletteCommand::ChatLayout(layout)),
                )
            })
            .collect(),
        ));
        let mut chat = vec![
            action_row("New chat", IconName::Plus, Box::new(NewChat)),
            (
                item("New direct chat (non-agentic)")
                    .icon(gpui_kit::assets::IconName::MessageCircle)
                    .keywords(["openrouter", "normal", "direct"]),
                Some(PaletteCommand::NewDirectChat),
            ),
            (
                item(if self.sidebar_view == crate::app::SidebarView::DirectChats {
                    "Switch sidebar to agent projects"
                } else {
                    "Switch sidebar to direct chats"
                })
                .icon(IconName::PanelLeft)
                .keywords(["sidebar", "view", "projects", "direct"]),
                Some(PaletteCommand::SwitchSidebar(
                    if self.sidebar_view == crate::app::SidebarView::DirectChats {
                        crate::app::SidebarView::Projects
                    } else {
                        crate::app::SidebarView::DirectChats
                    },
                )),
            ),
            (
                item("Web search")
                    .icon(IconName::Globe)
                    .checked(self.settings.web_search)
                    .action(Box::new(ToggleWebSearch)),
                None,
            ),
        ];
        if self.chats[self.active].cancel.is_some() {
            chat.push(action_row("Stop generating", IconName::Square, Box::new(StopGenerating)));
        }
        let can_retry = self.chats[self.active].cancel.is_none()
            && self.chats[self.active].messages.iter().any(|m| m.role == Role::User);
        if can_retry {
            chat.push((
                item("Retry last reply")
                    .icon(IconName::RefreshCw)
                    .keywords(["regenerate", "again"]),
                Some(PaletteCommand::Retry),
            ));
        }
        chat.push((
            item("Copy chat as Markdown").icon(IconName::Copy).keywords(["export", "clipboard"]),
            Some(PaletteCommand::CopyChat),
        ));
        chat.push((
            item("Save chat as Markdown...")
                .icon(IconName::FileText)
                .keywords(["export", "file", "download"]),
            Some(PaletteCommand::ExportChat),
        ));
        let active = &self.chats[self.active];
        if !active.messages.is_empty() {
            let pin = if active.pinned { "Unpin chat" } else { "Pin chat" };
            chat.extend([
                (
                    item("Rename chat").icon(IconName::CaseSensitive).keywords(["title"]),
                    Some(PaletteCommand::RenameChat),
                ),
                (item(pin).icon(IconName::Star), Some(PaletteCommand::TogglePin)),
                (item("Archive chat").icon(IconName::Inbox), Some(PaletteCommand::ArchiveChat)),
                (
                    item("Delete chat...").icon(gpui_kit::assets::IconName::Trash),
                    Some(PaletteCommand::DeleteChat),
                ),
            ]);
        }
        if shows_lead_chip(active) {
            let lead = if active.lead {
                "Stop leading".to_string()
            } else if active.messages.is_empty() {
                "Make lead".to_string()
            } else {
                "Make lead (restarts driver with orchestration tools)".to_string()
            };
            chat.push((
                item(lead).keywords(["orchestration", "agents", "spawn"]),
                Some(PaletteCommand::ToggleLead),
            ));
        }
        if active.folder(&self.settings.workspace_root).is_some() {
            let reveal =
                if cfg!(target_os = "macos") { "Reveal folder in Finder" } else { "Show folder" };
            chat.push((
                item(reveal).icon(IconName::FolderOpen).keywords(["project", "open"]),
                Some(PaletteCommand::RevealFolder),
            ));
        }
        if self.resume_command_of(self.active).is_some() {
            chat.push((
                item("Resume in terminal")
                    .icon(IconName::SquareTerminal)
                    .keywords(["tmux", "neovim", "cli", "tui"]),
                Some(PaletteCommand::ResumeInTerminal),
            ));
        }
        if self.terminal_folder().is_some() {
            chat.push((
                item("Open yazi").icon(IconName::SquareTerminal).keywords([
                    "files",
                    "file manager",
                    "browse",
                ]),
                Some(PaletteCommand::OpenYazi),
            ));
        }
        if self.git_repo().is_some() {
            let label = if self.changes_view_shown(host) { "Hide changes" } else { "Show changes" };
            chat.push((
                item(label)
                    .icon(gpui_kit::assets::IconName::GitCompareArrows)
                    .keywords(["git", "diff", "stage", "commit", "push"]),
                Some(PaletteCommand::ToggleChanges),
            ));
        }
        if active.has_worktree(&self.missing_folders, &self.settings.workspace_root) {
            chat.push((
                item("Remove worktree...")
                    .icon(gpui_kit::assets::IconName::FolderX)
                    .keywords(["branch", "delete", "clean up"]),
                Some(PaletteCommand::RemoveWorktree),
            ));
        }
        let draft =
            active.messages.is_empty() && active.provider != aether_core::Provider::OpenRouter;
        if let Some(bases) = self.active_bases().filter(|b| draft && !b.branches.is_empty()) {
            chat.push(match (&active.new_worktree, &active.new_worktree_existing) {
                (None, None) => {
                    let base = bases.default.clone().or_else(|| bases.branches.first().cloned());
                    (
                        item("Work in a new worktree")
                            .icon(gpui_kit::assets::IconName::GitFork)
                            .keywords(["branch", "isolated", "checkout"]),
                        Some(PaletteCommand::Worktree(base)),
                    )
                }
                _ => (
                    item("Work locally")
                        .icon(gpui_kit::assets::IconName::Laptop)
                        .keywords(["worktree", "folder"]),
                    Some(PaletteCommand::Worktree(None)),
                ),
            });
        }
        if draft {
            for (path, branch) in self.shareable_worktrees(self.active) {
                let label = if branch.is_empty() {
                    format!("Share worktree {}", path.display())
                } else {
                    format!("Share worktree {branch}")
                };
                chat.push((
                    item(label)
                        .icon(gpui_kit::assets::IconName::GitFork)
                        .keywords(["branch", "isolated", "share", "existing"]),
                    Some(PaletteCommand::ShareWorktree(path)),
                ));
            }
        }
        chat.push((
            item("Clean up worktrees...")
                .icon(gpui_kit::assets::IconName::GitFork)
                .keywords(["remove", "delete", "branch"]),
            Some(PaletteCommand::CleanUpWorktrees),
        ));
        groups.push(("Chat", chat));

        groups.push((
            "Go to chat",
            self.chats
                .iter()
                .enumerate()
                .rev()
                .filter(|(_, chat)| {
                    (!chat.messages.is_empty()
                        || !chat.draft.trim().is_empty()
                        || !chat.pending.is_empty())
                        && !chat.archived
                })
                .map(|(ix, chat)| {
                    (
                        item(chat.title.clone())
                            .keywords(chat.folder_name(&self.settings.workspace_root))
                            .checked(ix == self.active && self.page == crate::app::Page::Chat),
                        Some(PaletteCommand::OpenChat(ix)),
                    )
                })
                .collect(),
        ));

        let archived: Vec<Row> = self
            .chats
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, chat)| !chat.messages.is_empty() && chat.archived)
            .map(|(ix, chat)| {
                (
                    item(chat.title.clone()).keywords(["archived"]),
                    Some(PaletteCommand::OpenChat(ix)),
                )
            })
            .collect();
        if !archived.is_empty() {
            groups.push(("Archived", archived));
        }

        let query = self.palette_query.read(cx).value();
        let needle = query.trim().to_lowercase();
        if !needle.is_empty() {
            let matches: Vec<Row> = self
                .chats
                .iter()
                .enumerate()
                .rev()
                .filter_map(|(ix, chat)| {
                    let (message, text) = first_match(&chat.messages, &needle)?;
                    Some((
                        item(format!("{} — {}", chat.title, text)).keywords([chat.title.clone()]),
                        Some(PaletteCommand::SearchMatch { chat: ix, message }),
                    ))
                })
                .take(8)
                .collect();
            if !matches.is_empty() {
                groups.push(("Matches", matches));
            }
        }

        groups.push(("Theme", theme_rows(self.settings.appearance)));

        groups.push((
            "Reasoning",
            Reasoning::ALL
                .into_iter()
                .zip(["Off", "Low", "Medium", "High"])
                .map(|(reasoning, label)| {
                    (
                        item(format!("Reasoning: {label}"))
                            .keywords(["thinking", "effort"])
                            .checked(self.settings.reasoning == reasoning),
                        Some(PaletteCommand::Reasoning(reasoning)),
                    )
                })
                .collect(),
        ));

        if self.settings.notes_enabled {
            groups.push((
                "Notes",
                vec![
                    action_row(
                        "Open notes",
                        IconName::BookOpen,
                        Box::new(crate::actions::OpenNotes),
                    ),
                    action_row(
                        "Search notes",
                        IconName::Search,
                        Box::new(crate::actions::SearchNotes),
                    ),
                ],
            ));
        }
        let mut skills = vec![action_row("Open skills", IconName::BookOpen, Box::new(OpenSkills))];
        skills.extend(self.enabled_skills().into_iter().map(|skill| {
            (
                item(format!("Use @{}", skill.name)).keywords([skill.description.clone()]),
                Some(PaletteCommand::UseSkill(skill.name)),
            )
        }));
        groups.push(("Skills", skills));

        groups.push((
            "Settings",
            self.settings
                .hackernews_enabled
                .then(|| {
                    (
                        item("Open Hacker News")
                            .icon(gpui_kit::assets::IconName::Flame)
                            .keywords(["news", "hn", "ycombinator", "tech", "stories", "articles"])
                            .action(Box::new(ToggleHackerNews)),
                        None,
                    )
                })
                .into_iter()
                .chain([
                    (
                        item("Open usage")
                            .icon(gpui_kit::assets::IconName::ChartColumn)
                            .keywords(["cost", "tokens", "analytics", "spend", "history"])
                            .action(Box::new(OpenUsage)),
                        None,
                    ),
                    action_row("Open settings", IconName::Settings, Box::new(OpenSettings)),
                ])
                .collect(),
        ));

        let active = &self.chats[self.active];
        if !query.is_empty() && active.cancel.is_none() {
            let provider = active.provider;
            let items = self.model_items(provider);
            let current = picker_value(provider, &active.model(&self.settings), &items);
            groups.push((
                "Switch model",
                items
                    .into_iter()
                    .map(|model| {
                        let id = model.id.to_string();
                        (
                            item(format!("{} · {}", model.name, model.detail))
                                .keywords([model.id])
                                .checked(id == current),
                            Some(PaletteCommand::Model(id)),
                        )
                    })
                    .collect(),
            ));
        }

        for (_, rows) in &mut groups {
            rows.retain(|(item, _)| matches(&query, &item.terms));
        }
        groups.retain(|(_, rows)| !rows.is_empty());
        self.palette_rows = groups
            .iter()
            .map(|(_, rows)| rows.iter().map(|(_, command)| command.clone()).collect())
            .collect();

        let view = self.session_handle(cx);
        let on_dismiss = view.clone();
        groups
            .into_iter()
            .fold(CommandMenu::new(&self.palette, &self.palette_query), |menu, (label, rows)| {
                menu.group(
                    CommandGroup::new()
                        .label(label)
                        .items(rows.into_iter().map(|(row, _)| row.item)),
                )
            })
            .on_confirm(move |ix, window, cx| {
                view.update(cx, |this, cx| this.run_palette(ix, window, cx));
            })
            .on_dismiss(move |window, cx| {
                on_dismiss.update(cx, |this, cx| this.close_palette(window, cx));
            })
    }
}

fn theme_rows(current: Appearance) -> Vec<Row> {
    Appearance::ALL
        .into_iter()
        .map(|appearance| {
            (
                item(format!("Theme: {}", appearance.label()))
                    .keywords(["appearance", "dark mode", "light mode"])
                    .keywords([appearance.label().replace('é', "e")])
                    .checked(current == appearance),
                Some(PaletteCommand::Theme(appearance)),
            )
        })
        .collect()
}

fn action_row(label: &str, icon: IconName, action: Box<dyn gpui_kit::Action>) -> Row {
    (item(label.to_string()).icon(icon).action(action), None)
}

/// Index and trimmed line of the first message holding `needle` (already lowercase).
fn first_match(messages: &[Message], needle: &str) -> Option<(usize, String)> {
    messages.iter().enumerate().find_map(|(ix, m)| {
        let line = m.text.lines().find(|line| line.to_lowercase().contains(needle))?;
        Some((ix, snippet(line)))
    })
}

/// One palette row's worth of a matching line.
fn snippet(line: &str) -> String {
    let line = line.trim();
    const MAX: usize = 90;
    if line.chars().count() <= MAX {
        line.to_string()
    } else {
        format!("{}…", line.chars().take(MAX - 1).collect::<String>())
    }
}

#[cfg(test)]
#[path = "../tests/unit/palette.rs"]
mod tests;
