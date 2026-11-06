//! Terminal drawer: one shell per chat, docked under the chat (or in the side
//! panel's Terminal tab) and started on first open.

use std::path::{Path, PathBuf};

use aether_core::{Provider, Settings, Watch, home_dir};
use aether_terminal::view::TerminalEvent;
use aether_terminal::workspace::TerminalWorkspace;
use aether_terminal::{
    DEFAULT_HEIGHT, TerminalTab, dark, drawer_height, effective_drawer_height, min_height,
    resolve_global_folder,
};
use gpui_kit::component::v_flex;
use gpui_kit::{
    AnyElement, App, Context, Entity, SharedString, StyleRefinement, Subscription, Window, div,
    prelude::*, px,
};

use crate::app::{Chat, ChatApp, Page, now_ms};
use crate::side_panel::SideTab;

pub(crate) struct Drawer {
    pub(crate) view: Entity<TerminalWorkspace>,
    pub(crate) _events: Subscription,
}

/// Attaches tmux session `aether-<id>`, first creating it with Neovim in pane 1 and
/// the CLI resuming `session` in pane 2. Without tmux, runs the CLI alone.
pub(crate) fn resume_command(
    s: &Settings,
    provider: Provider,
    session: &str,
    id: i64,
    folder: &Path,
) -> Option<String> {
    let (name, flag, binary) = match provider {
        Provider::Claude => ("claude", "--resume", &s.claude_binary),
        Provider::Codex => ("codex", "resume", &s.codex_binary),
        Provider::Pi => ("pi", "--session", &s.pi_binary),
        Provider::OpenCode => ("opencode", "--session", &s.opencode_binary),
        Provider::Antigravity => ("agy", "--conversation", &s.antigravity_binary),
        Provider::OpenRouter => return None,
    };
    if session.is_empty() {
        return None;
    }
    let q = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
    let bin = match binary.strip_prefix("~/") {
        _ if binary.is_empty() => name.to_string(),
        Some(rest) => home_dir().join(rest).display().to_string(),
        None => binary.clone(),
    };
    let cli = format!("{} {flag} {}", q(&bin), q(session));
    let (tmux, dir) = (format!("aether-{id}"), q(&folder.display().to_string()));
    // zsh expands an unquoted leading `=word` to a command path
    let (exact, window) = (q(&format!("={tmux}")), q(&format!("={tmux}:")));
    Some(format!(
        "if command -v tmux >/dev/null; then \
         tmux has-session -t {exact} 2>/dev/null || \
         {{ tmux new-session -d -s {tmux} -c {dir} nvim && \
         tmux split-window -h -t {window} -c {dir} {pane}; }}; \
         env -u TMUX -u TMUX_PANE tmux attach-session -t {exact}; \
         else {cli}; fi",
        pane = q(&cli)
    ))
}

impl ChatApp {
    /// A chat's shell folder: its own (the worktree when bound) or, for plain
    /// chats, the workspace.
    pub(crate) fn folder_of(&self, chat: &Chat) -> Option<PathBuf> {
        let root = &self.settings.workspace_root;
        chat.folder(root).or_else(|| crate::open_in::chat_folder("", root))
    }

    /// The active chat's shell folder; `None` hides the terminal.
    pub(crate) fn terminal_folder(&self) -> Option<PathBuf> {
        if !cfg!(unix) || self.page != Page::Chat || !self.is_agent_chat(&self.chats[self.active]) {
            return None;
        }
        self.folder_of(&self.chats[self.active]).filter(|f| !self.missing_folders.contains(f))
    }

    pub(crate) fn global_folder(&self) -> PathBuf {
        resolve_global_folder(&self.settings.workspace_root)
    }

    pub(crate) fn ensure_global_terminal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TerminalWorkspace> {
        if let Some(drawer) = self.global_terminal.as_ref() {
            return drawer.view.clone();
        }
        let folder = self.global_folder();
        let terminal_theme = self.settings.terminal_theme;
        let font_size = self.settings.terminal_font_size;
        let font_family = self.settings.terminal_font_family.clone();
        let option_as_meta = self.settings.terminal_option_as_meta;
        let view = cx.new(|cx| {
            let mut v = TerminalWorkspace::new(folder.clone(), window, cx);
            v.sync_settings(
                folder,
                option_as_meta,
                font_family.into(),
                font_size,
                terminal_theme,
                self.settings.terminal_cursor,
                TerminalTab::Global,
                true,
                cx,
            );
            v.model.sidebar_open = false;
            v
        });
        let events =
            cx.subscribe_in(&view, window, move |this, _, event, window, cx| match event {
                TerminalEvent::Resize(h) => {
                    let (viewport, min) = (
                        f32::from(window.viewport_size().height),
                        min_height(px(this.settings.terminal_font_size)),
                    );
                    this.settings.terminal_height = drawer_height(*h, viewport, min);
                    cx.notify();
                }
                TerminalEvent::ResizeEnd => crate::settings::save(&this.settings),
                TerminalEvent::ResetHeight => {
                    this.settings.terminal_height = DEFAULT_HEIGHT;
                    crate::settings::save(&this.settings);
                    cx.notify();
                }
                TerminalEvent::Hide => this.hide_global_terminal(window, cx),
                TerminalEvent::SelectTab(tab) => this.select_terminal_tab(*tab, window, cx),
                TerminalEvent::ToggleWatch | TerminalEvent::CommandEnded { .. } => {}
            });
        self.global_terminal = Some(Drawer { view: view.clone(), _events: events });
        view
    }

    pub(crate) fn hide_global_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(drawer) = self.global_terminal.as_ref() {
            drawer.view.update(cx, |t, _| {
                t.open = false;
                t.leave_fullscreen();
            });
        }
        if self.page == Page::Terminal {
            self.open_page(Page::Chat, cx);
        }
        if self.page == Page::Chat {
            self.composer.update(cx, |s, cx| s.focus(window, cx));
        }
        cx.notify();
    }

    /// "Open in Aether Terminal": the terminal page, on a new tab whose shell starts in `folder`.
    pub(crate) fn open_in_terminal(
        &mut self,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != Page::Terminal {
            self.toggle_terminal_page(window, cx);
        }
        let view = self.ensure_global_terminal(window, cx);
        view.update(cx, |t, cx| t.open_folder(folder, window, cx));
    }

    /// The composer's `!command`: a new tab in the active chat's terminal.
    pub(crate) fn run_in_new_tab(
        &mut self,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.terminal_view(window, cx) else { return };
        let key = SharedString::from(format!("shell-{}", now_ms()));
        let title = SharedString::from(command.lines().next().unwrap_or(command).to_string());
        let command = command.to_string();
        view.update(cx, |t, cx| t.run_command(key, title, command, window, cx));
        self.show_terminal(&view, window, cx);
    }

    /// The active chat's tab for `tool`: back to it while it runs, else (re)started there.
    pub(crate) fn open_tool_tab(
        &mut self,
        tool: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.terminal_view(window, cx) else { return };
        view.update(cx, |t, cx| {
            if t.command_running(tool, cx) {
                t.focus_command(tool, cx);
            } else {
                t.run_command(tool.into(), tool.into(), tool.to_string(), window, cx);
            }
        });
        self.show_terminal(&view, window, cx);
    }

    /// Chat `ix`'s "Resume in terminal" command; `None` while it runs or can't resume.
    pub(crate) fn resume_command_of(&self, ix: usize) -> Option<String> {
        let chat = &self.chats[ix];
        if !cfg!(unix) || chat.busy() {
            return None;
        }
        let folder = self.folder_of(chat).filter(|f| !self.missing_folders.contains(f))?;
        resume_command(&self.settings, chat.provider, &chat.cli_session, chat.id, &folder)
    }

    /// Hands chat `ix` to its CLI on the terminal page; Aether lets go of the session first.
    pub(crate) fn resume_in_terminal(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(command) = self.resume_command_of(ix) else { return };
        let (id, title) = (self.chats[ix].id, self.chats[ix].title.clone());
        self.agent.close_session(&id.to_string());
        let key = SharedString::from(format!("resume-{id}"));
        let view = self.ensure_global_terminal(window, cx);
        view.update(cx, |t, cx| {
            if t.command_running(&key, cx) {
                t.focus_command(&key, cx);
            } else {
                t.run_command(key, title, command, window, cx);
            }
            t.open = true;
            t.full_page = true;
            t.set_shown(true, cx);
        });
        self.active_terminal_tab = TerminalTab::Global;
        self.open_page(Page::Terminal, cx);
        let focus = view.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(crate) fn toggle_terminal_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == Page::Terminal {
            self.open_page(Page::Chat, cx);
            if self.page == Page::Chat {
                self.composer.update(cx, |s, cx| s.focus(window, cx));
            }
        } else {
            self.open_page(Page::Terminal, cx);
            self.active_terminal_tab = TerminalTab::Global;
            let view = self.ensure_global_terminal(window, cx);
            let focus = view.update(cx, |t, cx| {
                t.open = true;
                t.full_page = true;
                t.focus_handle(cx)
            });
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    fn terminal_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TerminalWorkspace>> {
        self.terminal_folder()?;
        self.drawer_for(self.active, window, cx)
    }

    pub(crate) fn drawer_for(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TerminalWorkspace>> {
        let chat = &self.chats[ix];
        let folder = self.folder_of(chat).filter(|f| !self.missing_folders.contains(f))?;
        let id = chat.id;
        if let Some(view) = self.terminals.get(&id).map(|d| d.view.clone()) {
            let s = &self.settings;
            let (meta, family) = (s.terminal_option_as_meta, s.terminal_font_family.clone());
            let (size, theme, cursor) = (s.terminal_font_size, s.terminal_theme, s.terminal_cursor);
            let in_tab = self.terminal_in_tab(id);
            view.update(cx, |t, cx| {
                let tab = TerminalTab::Project(id);
                t.sync_settings(folder, meta, family.into(), size, theme, cursor, tab, in_tab, cx)
            });
            return Some(view);
        }
        let watched = self.terminal_watch(id);
        let terminal_theme = self.settings.terminal_theme;
        let font_size = self.settings.terminal_font_size;
        let font_family = self.settings.terminal_font_family.clone();
        let option_as_meta = self.settings.terminal_option_as_meta;
        let view = cx.new(|cx| {
            let mut v = TerminalWorkspace::new(folder.clone(), window, cx);
            v.sync_settings(
                folder,
                option_as_meta,
                font_family.into(),
                font_size,
                terminal_theme,
                self.settings.terminal_cursor,
                TerminalTab::Project(id),
                false,
                cx,
            );
            v.watched = watched;
            v
        });
        let events =
            cx.subscribe_in(&view, window, move |this, _, event, window, cx| match event {
                TerminalEvent::Resize(h) => {
                    let (viewport, min) = (
                        f32::from(window.viewport_size().height),
                        min_height(px(this.settings.terminal_font_size)),
                    );
                    this.settings.terminal_height = drawer_height(*h, viewport, min);
                    cx.notify();
                }
                TerminalEvent::ResizeEnd => crate::settings::save(&this.settings),
                TerminalEvent::ResetHeight => {
                    this.settings.terminal_height = DEFAULT_HEIGHT;
                    crate::settings::save(&this.settings);
                    cx.notify();
                }
                TerminalEvent::Hide if this.peeked_terminal() == Some(id) => this.close_peek(cx),
                TerminalEvent::Hide => this.hide_terminal(id, window, cx),
                TerminalEvent::SelectTab(tab) => this.select_terminal_tab(*tab, window, cx),
                TerminalEvent::ToggleWatch => this.toggle_watch(Watch::Terminal(id), cx),
                TerminalEvent::CommandEnded { key, .. } => {
                    this.script_runs.end_owned(id, key);
                    this.setup_ended(id, event, window, cx);
                    cx.notify();
                }
            });
        self.terminals.insert(id, Drawer { view: view.clone(), _events: events });
        Some(view)
    }

    pub(crate) fn select_terminal_tab(
        &mut self,
        tab: TerminalTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_terminal_tab = tab;
        let view = match tab {
            TerminalTab::Global => {
                for drawer in self.terminals.values() {
                    drawer.view.update(cx, |t, _| t.open = false);
                }
                self.ensure_global_terminal(window, cx)
            }
            TerminalTab::Project(_) => {
                if let Some(g) = &self.global_terminal {
                    g.view.update(cx, |t, _| t.open = false);
                }
                if let Some(view) = self.terminal_view(window, cx) {
                    view
                } else {
                    self.active_terminal_tab = TerminalTab::Global;
                    self.ensure_global_terminal(window, cx)
                }
            }
        };
        self.show_terminal(&view, window, cx);
    }

    fn show_terminal(
        &mut self,
        view: &Entity<TerminalWorkspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.chats[self.active].id;
        if self.terminals.get(&id).is_some_and(|d| d.view == *view) && self.terminal_in_tab(id) {
            self.git.open.insert(id);
            let host = self.host_of(window);
            self.set_side_tab(host, SideTab::Terminal);
        }
        let focus = view.update(cx, |t, cx| {
            t.open = true;
            t.focus_handle(cx)
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Ctrl+` and the header button: open and focus, or hide and return to the
    /// composer. In a side panel tab, hides only once that tab shows and has focus.
    pub(crate) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == Page::Terminal {
            self.open_page(Page::Chat, cx);
            if self.page == Page::Chat {
                self.composer.update(cx, |s, cx| s.focus(window, cx));
            }
            cx.notify();
            return;
        }
        if !self.is_agent_chat(&self.chats[self.active]) {
            return;
        }
        if let Some(view) = self.terminal_view(window, cx) {
            let id = self.chats[self.active].id;
            let in_tab = self.terminal_in_tab(id);
            let tab_shown = self.side_open() && self.side_tab_in(window) == SideTab::Terminal;
            let t = view.read(cx);
            if t.open && (!in_tab || (tab_shown && t.contains_focused(window, cx))) {
                return self.hide_terminal(id, window, cx);
            }
            self.show_terminal(&view, window, cx)
        }
    }

    /// Opens the project terminal drawer and focuses the shell, as "Open terminal" buttons do.
    pub(crate) fn open_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_agent_chat(&self.chats[self.active]) {
            return;
        }
        if let Some(view) = self.terminal_view(window, cx) {
            self.show_terminal(&view, window, cx);
        }
    }

    pub(crate) fn show_command(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.terminal_view(window, cx) {
            view.update(cx, |t, cx| t.focus_command(key, cx));
            self.show_terminal(&view, window, cx);
        }
    }

    /// Cmd+J: open and focus; on an open drawer, move focus between it and the composer.
    pub(crate) fn focus_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == Page::Terminal {
            let view = self.ensure_global_terminal(window, cx);
            let (open, focused) = {
                let t = view.read(cx);
                (t.open, t.contains_focused(window, cx))
            };
            if open && focused {
                self.open_page(Page::Chat, cx);
                self.composer.update(cx, |s, cx| s.focus(window, cx));
                return;
            }
            self.show_terminal(&view, window, cx);
            return;
        }
        if !self.is_agent_chat(&self.chats[self.active]) {
            return;
        }
        if let Some(view) = self.terminal_view(window, cx) {
            let (open, focused) = {
                let t = view.read(cx);
                (t.open, t.contains_focused(window, cx))
            };
            if open && focused {
                if self.page == Page::Chat {
                    self.focus_composer(window, cx);
                }
                return;
            }
            self.show_terminal(&view, window, cx);
        }
    }

    pub(crate) fn hide_terminal(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(drawer) = self.terminals.get(&id) {
            drawer.view.update(cx, |t, _| {
                t.open = false;
                t.leave_fullscreen();
            });
        }
        if self.page == Page::Terminal {
            self.open_page(Page::Chat, cx);
        }
        if self.page == Page::Chat {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    /// The page or tab terminal showing in `window`, while its fullscreen toggle is on.
    pub(crate) fn fullscreen_terminal(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<Entity<TerminalWorkspace>> {
        let id = self.chats[self.active].id;
        let drawer = match self.page {
            Page::Terminal => self.global_terminal.as_ref(),
            Page::Chat
                if self.side_open()
                    && self.side_tab_in(window) == SideTab::Terminal
                    && self.terminal_in_tab(id)
                    && self.peeked_terminal() != Some(id) =>
            {
                self.terminals.get(&id)
            }
            _ => None,
        };
        drawer.map(|d| d.view.clone()).filter(|view| view.read(cx).is_maximized())
    }

    pub(crate) fn is_terminal_drawer_maximized(&self, cx: &App) -> bool {
        if self.page != Page::Chat || !self.is_agent_chat(&self.chats[self.active]) {
            return false;
        }
        let id = self.chats[self.active].id;
        self.terminal_folder().is_some()
            && !self.terminal_in_tab(id)
            && self
                .terminals
                .get(&id)
                .is_some_and(|d| d.view.read(cx).open && d.view.read(cx).is_maximized())
    }

    /// The active chat's open drawer. Keeps the drawer's folder and settings
    /// current, stops polling in drawers not drawn, and ends shells whose folder
    /// is gone. `None` while the side panel's Terminal tab hosts it.
    pub(crate) fn terminal_drawer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.page != Page::Chat || !self.is_agent_chat(&self.chats[self.active]) {
            return None;
        }
        let gone: Vec<i64> = self
            .terminals
            .keys()
            .copied()
            .filter(|id| {
                let chat = self.chats.iter().find(|c| c.id == *id);
                chat.and_then(|c| self.folder_of(c))
                    .is_some_and(|f| self.missing_folders.contains(&f))
            })
            .collect();
        for id in gone {
            self.close_terminal(id, cx);
        }
        let id = self.chats[self.active].id;
        let in_tab = self.terminal_in_tab(id);
        let tab_shown = self.side_open() && self.side_tab_in(window) == SideTab::Terminal;
        let chat_folder = self.terminal_folder();
        let peeked = self.peeked_terminal();
        let shown = chat_folder
            .as_ref()
            .and_then(|folder| self.terminals.get(&id).map(|d| (d.view.clone(), folder.clone())))
            .filter(|(v, _)| v.read(cx).open && (!in_tab || tab_shown) && peeked != Some(id));

        for (other, drawer) in &self.terminals {
            if (shown.is_none() || *other != id)
                && peeked != Some(*other)
                && drawer.view.read(cx).shown
            {
                drawer.view.update(cx, |t, cx| t.set_shown(false, cx));
            }
        }

        let (view, folder) = shown?;
        let option_as_meta = self.settings.terminal_option_as_meta;
        let font_family = self.settings.terminal_font_family.clone();
        let font_size = self.settings.terminal_font_size;
        let terminal_theme = self.settings.terminal_theme;
        view.update(cx, |t, cx| {
            t.sync_settings(
                folder,
                option_as_meta,
                font_family.into(),
                font_size,
                terminal_theme,
                self.settings.terminal_cursor,
                TerminalTab::Project(id),
                in_tab,
                cx,
            );
        });
        if in_tab {
            return None;
        }
        let maximized = view.read(cx).is_maximized();
        if maximized {
            return Some(
                div()
                    .flex_1()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .child(dark(view.cached(StyleRefinement::default().size_full()), cx))
                    .into_any_element(),
            );
        }
        let (viewport, min) = (f32::from(window.viewport_size().height), min_height(px(font_size)));
        let height = effective_drawer_height(self.settings.terminal_height, viewport, min, false);
        Some(
            div()
                .flex_none()
                .h(px(height))
                .child(dark(view.cached(StyleRefinement::default().size_full()), cx))
                .into_any_element(),
        )
    }

    /// Renders the global terminal as a full main-section page filling 100% width and height.
    pub(crate) fn render_terminal_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = self.ensure_global_terminal(window, cx);
        let folder = self.global_folder();

        let peeked = self.peeked_terminal();
        for (id, drawer) in &self.terminals {
            if peeked != Some(*id) && drawer.view.read(cx).shown {
                drawer.view.update(cx, |t, cx| t.set_shown(false, cx));
            }
        }
        if let Some(global) = &self.global_terminal
            && !global.view.read(cx).shown
        {
            global.view.update(cx, |t, cx| t.set_shown(true, cx));
        }

        let option_as_meta = self.settings.terminal_option_as_meta;
        let font_family = self.settings.terminal_font_family.clone();
        let font_size = self.settings.terminal_font_size;
        let terminal_theme = self.settings.terminal_theme;
        view.update(cx, |t, cx| {
            t.open = true;
            t.sync_settings(
                folder,
                option_as_meta,
                font_family.into(),
                font_size,
                terminal_theme,
                self.settings.terminal_cursor,
                TerminalTab::Global,
                true,
                cx,
            );
        });
        v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(dark(view.cached(StyleRefinement::default().size_full()), cx))
            .into_any_element()
    }

    /// Ends the chat's shell; for chat delete and worktree removal.
    pub(crate) fn close_terminal(&mut self, chat_id: i64, cx: &mut Context<Self>) {
        if let Some(drawer) = self.terminals.remove(&chat_id) {
            drawer.view.update(cx, |t, cx| t.shutdown(cx));
        }
        self.script_runs.drop_chat(chat_id);
        self.unwatch_terminal(chat_id, cx);
        if self.setup_drawer_gone(chat_id) {
            // render calls this too, and gpui drops a notify made mid-draw
            let view = cx.entity();
            cx.defer(move |cx| view.update(cx, |_, cx| cx.notify()));
        }
        if self.active_terminal_tab == TerminalTab::Project(chat_id) {
            self.active_terminal_tab = TerminalTab::Global;
        }
    }

    /// Ends every shell; for app quit.
    pub(crate) fn shutdown_terminals(&mut self, cx: &mut Context<Self>) {
        for (id, drawer) in std::mem::take(&mut self.terminals) {
            drawer.view.update(cx, |t, cx| t.shutdown(cx));
            self.script_runs.drop_chat(id);
        }
        if let Some(drawer) = self.global_terminal.take() {
            drawer.view.update(cx, |t, cx| t.shutdown(cx));
        }
    }

    /// The chat has a live shell.
    pub(crate) fn terminal_running(&self, chat_id: i64, cx: &App) -> bool {
        self.terminals.get(&chat_id).is_some_and(|d| d.view.read(cx).is_running(cx))
    }

    /// What the chat's shell is running in front of the prompt, for confirms:
    /// the program's name, or "a program" when it can't be read.
    pub(crate) fn terminal_job(&self, chat_id: i64, cx: &App) -> Option<String> {
        let job = self.terminals.get(&chat_id)?.view.read(cx).running_job(cx)?;
        Some(if job.is_empty() { "a program".into() } else { job })
    }
}

#[cfg(test)]
#[path = "../tests/unit/terminal.rs"]
mod tests;
