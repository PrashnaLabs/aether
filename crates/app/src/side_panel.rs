//! Full-width chat tabs: Agent, Terminal and Explorer followed by an optional Browser tab.

use aether_core::{ChatLayout, Message, Role, Watch};
use aether_ui::{
    FileEdits, ReplyDiff, Tab, TabBar, anchored_popup, icon_chip, reply_edits, reveal_file,
};
use aether_ui::{SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::{HandleEdge, resize_handle};
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::resizable::resize_handle_appearance;
use gpui_kit::component::{ActiveTheme, IconName, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::{
    Anchor, AnyElement, App, Axis, Context, DragMoveEvent, KeyDownEvent, ScrollHandle,
    SharedString, StyleRefinement, Window, point, prelude::*, px, relative,
};

use crate::actions::ToggleSidebar;
use crate::app::{ChatApp, Page, watch_item};
use crate::explorer::ExplorerView;
use crate::git_panel::note;
use crate::session_workspace::WindowId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SideTab {
    Agent,
    Terminal,
    Explorer,
    Browser,
    Board,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Surface {
    Browser,
    Board,
    Btw,
    Terminal,
    Files,
    Review,
}

impl Surface {
    /// Tab order in the right panel: Terminal and Files fixed, BTW and Review then Browser closable.
    pub(crate) fn tabs(btw: bool, review: bool, browser: bool, board: bool) -> Vec<Self> {
        let mut tabs = vec![Self::Terminal, Self::Files];
        if btw {
            tabs.push(Self::Btw);
        }
        if review {
            tabs.push(Self::Review);
        }
        if browser {
            tabs.push(Self::Browser);
        }
        if board {
            tabs.push(Self::Board);
        }
        tabs
    }

    pub(crate) fn closable(self) -> bool {
        matches!(self, Self::Btw | Self::Review | Self::Browser | Self::Board)
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Btw => "By the way",
            Self::Browser => "Browser",
            Self::Board => "Board",
            Self::Terminal => "Terminal",
            Self::Files => "Files",
            Self::Review => "Review",
        }
    }

    pub(crate) fn icon(self) -> Lucide {
        match self {
            Self::Btw => Lucide::MessageCircle,
            Self::Browser => Lucide::Globe,
            Self::Board => Lucide::LayoutDashboard,
            Self::Terminal => Lucide::Terminal,
            Self::Files => Lucide::Folder,
            Self::Review => Lucide::FileDiff,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn tab(self) -> SideTab {
        match self {
            Self::Browser => SideTab::Browser,
            Self::Board => SideTab::Board,
            Self::Terminal => SideTab::Terminal,
            Self::Files | Self::Review => SideTab::Explorer,
            Self::Btw => SideTab::Agent,
        }
    }

    pub(crate) fn explorer_view(self, diff_open: bool) -> Option<ExplorerView> {
        match self {
            Self::Files => Some(ExplorerView::Files),
            Self::Review => {
                Some(if diff_open { ExplorerView::Diff } else { ExplorerView::Changes })
            }
            Self::Browser | Self::Board | Self::Btw | Self::Terminal => None,
        }
    }
}

pub(crate) fn surface_for_tab(
    tab: SideTab,
    explorer_view: ExplorerView,
    _diff_open: bool,
) -> Surface {
    match tab {
        SideTab::Browser => Surface::Browser,
        SideTab::Board => Surface::Board,
        SideTab::Terminal => Surface::Terminal,
        SideTab::Explorer => match explorer_view {
            ExplorerView::Files => Surface::Files,
            ExplorerView::Changes | ExplorerView::Diff => Surface::Review,
        },
        SideTab::Agent => Surface::Files,
    }
}

/// True when the chat body must render inside the sidebar panel instead of beside it.
pub(crate) fn chat_body_in_sidebar(layout: ChatLayout, side_open: bool) -> bool {
    side_open && matches!(layout, ChatLayout::Sidebar | ChatLayout::Split)
}

/// Panel share bounds: chat 40% / panel 60% by default, capped 25–80%.
pub(crate) fn right_panel_share(share: f32) -> f32 {
    share.clamp(0.25, 0.8)
}

/// BTW sidebar width bounds in px.
pub(crate) fn btw_width(width: f32) -> f32 {
    width.clamp(240., 640.)
}

fn right_panel_collapsed(layout: ChatLayout, collapsed: bool) -> bool {
    layout == ChatLayout::Sidebar && collapsed
}

#[derive(Clone)]
struct RightPanelResize;

impl Render for RightPanelResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::div()
    }
}

/// The reply a chat's Diff tab shows, as of the chat's epoch, and its scroll.
pub(crate) struct DiffTarget {
    reply: usize,
    epoch: u64,
    scroll: ScrollHandle,
}

/// Fixed tabs always lead; optional tabs appear only after being opened.
#[cfg(test)]
fn tab_order(browser: bool) -> Vec<SideTab> {
    side_panel_tab_order(browser, false)
}

fn side_panel_tab_order(browser: bool, board: bool) -> Vec<SideTab> {
    use SideTab::*;
    let mut tabs = vec![Agent, Terminal, Explorer];
    if browser {
        tabs.push(Browser);
    }
    if board {
        tabs.push(Board);
    }
    tabs
}

/// Closed optional tabs return to the conversation.
fn shown_tab(tab: SideTab, _back: SideTab, tabs: &[SideTab]) -> SideTab {
    if tabs.contains(&tab) { tab } else { SideTab::Agent }
}

fn adjacent_tab(current: SideTab, tabs: &[SideTab], previous: bool) -> Option<SideTab> {
    let index = tabs.iter().position(|tab| *tab == current)?;
    let next =
        if previous { (index + tabs.len() - 1) % tabs.len() } else { (index + 1) % tabs.len() };
    tabs.get(next).copied()
}

/// The chat's terminal sits in the Terminal tab, not the drawer, once moved
/// there or while the chat's open panel fills the window.
fn tab_hosts_terminal(moved: bool, full: bool, panel_open: bool) -> bool {
    moved || (full && panel_open)
}

/// The reply `target` points at, unless the chat was cleared or rewound since.
fn shown_reply<'a>(
    messages: &'a [Message],
    epoch: u64,
    target: &DiffTarget,
) -> Option<&'a Message> {
    (target.epoch == epoch)
        .then(|| messages.get(target.reply))
        .flatten()
        .filter(|m| m.role == Role::Assistant)
}

impl ChatApp {
    pub(crate) fn cycle_side_tab(
        &mut self,
        previous: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.side_open() {
            cx.propagate();
            return;
        }
        let tabs = self.side_tabs(cx);
        let current = shown_tab(self.side_tab_in(window), self.side_back, &tabs);
        if let Some(next) = adjacent_tab(current, &tabs, previous) {
            self.select_tab(next, window, cx);
        }
    }

    /// The legacy panel shortcut switches between Changes and Agent.
    pub(crate) fn toggle_side(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != Page::Chat || !self.is_agent_chat(&self.chats[self.active]) {
            return;
        }
        if self.changes_view_shown(self.host_of(window)) {
            self.focus_composer(window, cx);
        } else {
            self.open_explorer_view(ExplorerView::Changes, window, cx);
        }
        cx.notify();
    }

    /// Shows reply `reply`'s edits in the Diff tab, at `file` when given.
    pub(crate) fn open_reply_diff(
        &mut self,
        reply: usize,
        file: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chat = &self.chats[self.active];
        if self.page != Page::Chat || !self.is_agent_chat(chat) {
            return;
        }
        let (id, epoch) = (chat.id, chat.epoch);
        let files = chat.messages.get(reply).map(|m| self.reply_files(m)).unwrap_or_default();
        let target = self.diff_targets.entry(id).or_insert_with(|| DiffTarget {
            reply,
            epoch,
            scroll: ScrollHandle::new(),
        });
        if (target.reply, target.epoch) != (reply, epoch) {
            *target = DiffTarget { reply, epoch, scroll: ScrollHandle::new() };
        }
        reveal_file(&target.scroll, &files, file.as_deref());
        self.diff_open = true;
        self.open_explorer_view(ExplorerView::Diff, window, cx);
        self.git.open.insert(id);
        window.focus(&self.diff_focus, cx);
        cx.notify();
    }

    /// A reply's edits in the active chat, paths from its folder.
    fn reply_files(&self, message: &Message) -> Vec<FileEdits> {
        let cwd = self.chats[self.active].folder(&self.settings.workspace_root);
        reply_edits(&message.steps, cwd.as_ref().map(|c| c.to_string_lossy()).as_deref())
    }

    fn open_reply_file(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(folder) = self.chats[self.active].folder(&self.settings.workspace_root) {
            self.open_in_explorer(folder.join(path), window, cx);
        }
    }

    /// Esc back to the composer, and arrows, Page Up/Down and Home/End to scroll.
    fn reply_diff_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if ev.keystroke.modifiers.modified() {
            return;
        }
        let key = ev.keystroke.key.as_str();
        if key == "escape" {
            cx.stop_propagation();
            if self.side_full {
                return self.focus_composer(window, cx);
            }
            return self.close_changes(window, cx);
        }
        let Some(target) = self.diff_targets.get(&self.chats[self.active].id) else {
            return;
        };
        let scroll = &target.scroll;
        let (offset, max, line) = (scroll.offset(), scroll.max_offset(), px(20.));
        let page = (scroll.bounds().size.height - line).max(line);
        let y = match key {
            "up" => -offset.y - line,
            "down" => -offset.y + line,
            "pageup" => -offset.y - page,
            "pagedown" => -offset.y + page,
            "home" => px(0.),
            "end" => max.y,
            _ => return,
        };
        scroll.set_offset(point(offset.x, -y.clamp(px(0.), max.y)));
        cx.stop_propagation();
        cx.notify();
    }

    /// The files the Diff view shows and the scroll of their diffs, if any.
    pub(crate) fn reply_diff_files(&self) -> Option<(Vec<FileEdits>, ScrollHandle)> {
        let chat = &self.chats[self.active];
        let target = self.diff_targets.get(&chat.id)?;
        let reply = shown_reply(&chat.messages, chat.epoch, target)?;
        Some((self.reply_files(reply), target.scroll.clone()))
    }

    /// The Diff tab: the chosen reply's edits, or a note when there's none.
    pub(crate) fn reply_diff_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let chat = &self.chats[self.active];
        let shown = self.diff_targets.get(&chat.id).and_then(|target| {
            let reply = shown_reply(&chat.messages, chat.epoch, target)?;
            Some((target, self.reply_files(reply)))
        });
        let body = match shown {
            Some((target, files)) if !files.is_empty() => {
                let view = cx.entity();
                let cwd = chat.folder(&self.settings.workspace_root);
                ReplyDiff::new(
                    format!("reply-diff-{}-{}", chat.id, target.reply),
                    files,
                    &target.scroll,
                )
                .cwd(cwd.map(|c| c.to_string_lossy().into_owned().into()))
                .on_open_file(move |path, window, cx| {
                    view.update(cx, |this, cx| this.open_reply_file(path, window, cx))
                })
                .into_any_element()
            }
            _ => note(
                Lucide::FileDiff,
                cx.theme().muted_foreground,
                "No edits to show",
                "Open a reply's changed files to see their diffs here.",
                cx,
            )
            .size_full()
            .into_any_element(),
        };
        v_flex()
            .id("side-diff")
            .track_focus(&self.diff_focus)
            .flex_1()
            .min_w_0()
            .min_h_0()
            .size_full()
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                this.reply_diff_key(ev, window, cx)
            }))
            .child(body)
            .into_any_element()
    }

    pub(crate) fn host_of(&self, window: &Window) -> WindowId {
        let id = window.window_handle().window_id();
        let found = self.workspace.windows.iter().find(|(_, handle)| handle.window_id() == id);
        found.map_or(0, |(host, _)| *host)
    }

    pub(crate) fn side_tab_for(&self, host: WindowId) -> SideTab {
        if host == 0 {
            return self.side_tab;
        }
        self.side_tabs.get(&host).copied().unwrap_or(SideTab::Agent)
    }

    /// The one native webview serves every window, so any window on Browser keeps it shown.
    fn browser_tab_shown(&self) -> bool {
        let live = |host: &WindowId| {
            self.workspace.model.windows.get(host).is_some_and(|w| !w.panes().is_empty())
        };
        self.side_tab == SideTab::Browser
            || self.side_tabs.iter().any(|(host, tab)| *tab == SideTab::Browser && live(host))
    }

    pub(crate) fn side_tab_in(&self, window: &Window) -> SideTab {
        self.side_tab_for(self.host_of(window))
    }

    pub(crate) fn set_side_tab(&mut self, host: WindowId, tab: SideTab) -> SideTab {
        if host == 0 {
            return std::mem::replace(&mut self.side_tab, tab);
        }
        self.side_tabs.insert(host, tab).unwrap_or(SideTab::Agent)
    }

    pub(crate) fn select_tab(&mut self, tab: SideTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.side_tab_in(window) == tab {
            return;
        }
        let host = self.host_of(window);
        let back = self.set_side_tab(host, tab);
        match tab {
            SideTab::Agent => {
                self.side_back = back;
                self.composer.update(cx, |composer, cx| composer.focus(window, cx));
            }
            SideTab::Explorer => self.open_explorer_view(self.explorer_view, window, cx),
            SideTab::Terminal => self.open_terminal(window, cx),
            SideTab::Browser => {
                self.browser.update(cx, |browser, cx| browser.focus_address(window, cx))
            }
            SideTab::Board => {}
        }
        cx.notify();
    }

    /// Removes the Diff tab, falling back to changes if it was showing.
    #[allow(dead_code)]
    pub(crate) fn close_diff_tab(&mut self, cx: &mut Context<Self>) {
        self.diff_open = false;
        if self.explorer_view == ExplorerView::Diff {
            self.explorer_view = ExplorerView::Files;
        }
        cx.notify();
    }

    /// Agent chats always show their tabbed workspace.
    pub(crate) fn side_open(&self) -> bool {
        let chat = &self.chats[self.active];
        self.page == Page::Chat && self.is_agent_chat(chat)
    }

    /// The active chat's panel tabs, in order.
    fn side_tabs(&self, _cx: &App) -> Vec<SideTab> {
        side_panel_tab_order(self.browser_open, self.board_tab)
    }

    /// Chat `id`'s terminal shows in the Terminal tab rather than the drawer.
    pub(crate) fn terminal_in_tab(&self, id: i64) -> bool {
        tab_hosts_terminal(
            self.terminal_tab,
            self.side_full,
            self.is_agent_chat(&self.chats[self.active]) && self.chats[self.active].id == id,
        )
    }

    /// Focuses the composer, bringing the Agent tab forward while the panel
    /// fills the window.
    pub(crate) fn focus_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.expand_composer(cx);
        let peeked = self.watch_peek == Some(Watch::Chat(self.chats[self.active].id));
        if self.side_full && self.side_open() && !peeked {
            self.select_tab(SideTab::Agent, window, cx);
        }
        self.composer.update(cx, |composer, cx| composer.focus(window, cx));
    }

    fn side_tab_bar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let border = theme.sidebar_border;
        let tabs = self.side_tabs(cx);
        let (browser_open, board_open) = (self.browser_open, self.board_tab);
        let view = cx.entity();
        let add = {
            let view = view.clone();
            move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
                let (v1, v2) = (view.clone(), view.clone());
                menu.item(
                    PopupMenuItem::new("Browser")
                        .icon(Lucide::Globe)
                        .disabled(browser_open)
                        .on_click(move |_, window, cx| {
                            v1.update(cx, |this, cx| this.open_browser_tab(window, cx))
                        }),
                )
                .item(
                    PopupMenuItem::new("Board")
                        .icon(Lucide::LayoutDashboard)
                        .disabled(board_open)
                        .on_click(move |_, window, cx| {
                            v2.update(cx, |this, cx| this.open_board_tab(window, cx))
                        }),
                )
            }
        };
        let active = self.side_tab_in(window);
        let mut bar = TabBar::new("side-panel-tab-bar")
            .tabs_id("side-panel-tabs")
            .height(gpui_kit::px(38.))
            .border_color(border);

        let chat = &self.chats[self.active];
        let is_agent = self.is_agent_chat(chat);
        let expand_btn = self.settings.sidebar_collapsed.then(|| {
            aether_ui::button("expand-sidebar")
                .ghost()
                .small()
                .icon(IconName::PanelLeftOpen)
                .accessibility_label("Expand sidebar")
                .tooltip_with_action("Expand sidebar", &ToggleSidebar, None)
                .on_click(cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx)))
        });
        let project: Option<SharedString> =
            self.chat_project(self.active).filter(|_| is_agent).map(|dir| self.project_name(&dir));
        let crumb = project
            .as_ref()
            .zip(self.chat_project(self.active))
            .map(|(name, dir)| self.project_crumb(name, dir, cx));

        if expand_btn.is_some() || crumb.is_some() {
            bar = bar.leading(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .children(expand_btn)
                    .children(crumb)
                    .child(gpui_kit::div().h(px(14.)).w(px(1.)).bg(border).mx_1()),
            );
        }

        for tab in tabs {
            if tab == SideTab::Agent && self.rename_in_tab && self.renaming == Some(self.active) {
                bar = bar.tab(
                    gpui_kit::div().w(px(180.)).child(aether_ui::field(&self.rename_input).small()),
                );
                continue;
            }
            let el = match tab {
                SideTab::Agent => {
                    let (app, id) = (cx.entity(), self.chats[self.active].id);
                    let title = &self.chats[self.active].title;
                    let label = if title.is_empty() { "Agent".into() } else { title.clone() };
                    Tab::new("side-tab-agent", label)
                        .icon(aether_ui::provider_icon(self.chats[self.active].provider))
                        .max_label_width(px(140.))
                        .on_double_click(cx.listener(|this, _, window, cx| {
                            this.begin_rename(this.active, window, cx);
                            this.rename_in_tab = true;
                        }))
                        .context_menu(move |menu, _, cx| {
                            if !app.read(cx).settings.watch_enabled {
                                return menu;
                            }
                            let popup = cx.entity();
                            app.update(cx, |app, cx| {
                                app.browser.update(cx, |b, cx| b.hide_for_menu(&popup, cx))
                            });
                            menu.item(watch_item(&app, id, "", cx))
                        })
                }
                SideTab::Explorer => {
                    Tab::new("side-tab-explorer", "Explorer").icon(Lucide::FolderOpen)
                }
                SideTab::Terminal => {
                    Tab::new("side-tab-terminal", "Terminal").icon(Lucide::Terminal)
                }
                SideTab::Browser => {
                    Tab::new("side-tab-browser", "Browser").icon(Lucide::Globe).on_close(
                        "side-tab-browser-close",
                        "Close browser tab",
                        cx.listener(|this, _, _, cx| this.close_browser_tab(cx)),
                    )
                }
                SideTab::Board => {
                    Tab::new("side-tab-board", "Board").icon(Lucide::LayoutDashboard).on_close(
                        "side-tab-board-close",
                        "Close board tab",
                        cx.listener(|this, _, _, cx| this.close_board_tab(cx)),
                    )
                }
            };
            bar = bar.tab(el.selected(active == tab).on_click(
                cx.listener(move |this, _, window, cx| this.select_tab(tab, window, cx)),
            ));
        }

        bar = bar.trailing(
            icon_chip("side-tab-add", false, cx)
                .icon(Lucide::Plus)
                .accessibility_label("Add tab")
                .tooltip("Add tab")
                .dropdown_menu_with_anchor(Anchor::TopLeft, add),
        );

        if is_agent {
            let pop = self.render_workspace_pop(window, cx);
            let open = pop.is_some();
            let toggle = icon_chip("workspace-toggle", open, cx)
                .selected(open)
                .toggled(open)
                .icon(Lucide::LayoutList)
                .accessibility_label("Workspace")
                .tooltip("Workspace")
                .when(!open, |d| {
                    d.on_click(self.session_listener(cx, |this, _, window, cx| {
                        this.toggle_workspace_pop(window, cx)
                    }))
                });
            let workspace_btn =
                anchored_popup("workspace-pop-anchor", toggle, &self.workspace_pop.trigger, true)
                    .anchor(Anchor::TopRight)
                    .when_some(pop, |p, pop| p.content(pop));

            let btw = icon_chip("btw-toggle", self.btw_open, cx)
                .selected(self.btw_open)
                .icon(Lucide::MessageCircle)
                .accessibility_label("By the way")
                .tooltip("By the way")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.btw_open = !this.btw_open;
                    cx.notify();
                }));
            bar = bar.action(btw);
            if let Some(toggle) = self.background_toggle(cx) {
                bar = bar.action(toggle);
            }
            bar = bar.action(workspace_btn);
            bar = bar.action(self.side_button(self.host_of(window), cx));
        }

        let host = 0;
        let pane = self.workspace.model.windows.get(&host).and_then(|w| w.focused).or_else(|| {
            self.workspace.model.windows.get(&host).and_then(|w| w.panes().first().copied())
        });
        if let Some(pane) = pane {
            let controls = self.render_pane_controls(pane, host, true, cx);
            bar = bar.action(controls);
        }

        bar
    }

    fn side_terminal_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let id = self.chats[self.active].id;
        let view = self
            .terminals
            .get(&id)
            .filter(|_| self.peeked_terminal() != Some(id))
            .map(|d| d.view.clone());
        gpui_kit::div()
            .flex_1()
            .min_h_0()
            .w_full()
            .children(view.map(|v| {
                aether_terminal::dark(v.cached(StyleRefinement::default().size_full()), cx)
            }))
            .into_any_element()
    }

    pub(crate) fn select_surface(
        &mut self,
        surface: Surface,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.right_surface = Some(surface);
        match surface {
            Surface::Btw => {}
            Surface::Browser => self.open_browser_tab(window, cx),
            Surface::Board => {}
            Surface::Terminal => self.open_terminal(window, cx),
            Surface::Files | Surface::Review => {
                let view = surface.explorer_view(self.diff_open).unwrap_or(ExplorerView::Files);
                self.open_explorer_view(view, window, cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn close_surface(&mut self, surface: Surface, cx: &mut Context<Self>) {
        match surface {
            Surface::Btw => self.btw_open = false,
            Surface::Review => self.right_review_open = false,
            Surface::Browser => {
                self.right_browser_open = false;
                self.close_browser_tab(cx);
            }
            Surface::Board => self.close_board_tab(cx),
            Surface::Terminal | Surface::Files => return,
        }
        let active = self.right_surface.unwrap_or(Surface::Terminal);
        if active == surface {
            let tabs =
                Surface::tabs(self.btw_open, self.right_review_open, self.right_browser_open, self.board_tab);
            self.right_surface = tabs.first().copied();
        }
        cx.notify();
    }

    pub(crate) fn open_btw_surface(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.btw_open = true;
        self.select_surface(Surface::Btw, window, cx);
    }

    fn toggle_right_panel(&mut self, cx: &mut Context<Self>) {
        self.update_settings(cx, |s| s.right_panel_collapsed = !s.right_panel_collapsed);
        cx.notify();
    }

    fn right_panel_surface_content(
        &mut self,
        surface: Surface,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match surface {
            Surface::Btw => self.render_btw_panel(cx),
            Surface::Browser => {
                self.browser_content(!self.palette_open && !self.notes.search_ui.open, cx)
            }
            Surface::Board => self.render_board_tab(cx),
            Surface::Files | Surface::Review => self.render_explorer_page(window, cx),
            Surface::Terminal => self.side_terminal_content(cx),
        }
    }

    fn right_panel_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let tabs = Surface::tabs(self.btw_open, self.right_review_open, self.right_browser_open, self.board_tab);
        let active = self.right_surface.filter(|s| tabs.contains(s)).unwrap_or(Surface::Terminal);
        let mut bar = TabBar::new("right-panel-tab-bar")
            .tabs_id("right-panel-tabs")
            .height(px(38.))
            .border_color(cx.theme().sidebar_border);
        for (index, surface) in tabs.iter().enumerate() {
            let surface = *surface;
            let mut tab =
                Tab::new(("right-panel-tab", index), surface.label())
                    .icon(surface.icon())
                    .selected(active == surface)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_surface(surface, window, cx)
                    }));
            if surface.closable() {
                tab = tab.on_close(
                    ("right-panel-tab-close", index),
                    format!("Close {} tab", surface.label()),
                    cx.listener(move |this, _, _, cx| this.close_surface(surface, cx)),
                );
            }
            bar = bar.tab(tab);
        }
        let btw_open = self.btw_open;
        let review_open = self.right_review_open;
        let browser_open = self.right_browser_open;
        let board_open = self.board_tab;
        let add = {
            let view = cx.entity();
            move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
                let v0 = view.clone();
                let v1 = view.clone();
                let v2 = view.clone();
                let v3 = view.clone();
                menu.item(
                    PopupMenuItem::new("By the way")
                        .icon(Lucide::MessageCircle)
                        .disabled(btw_open)
                        .on_click(move |_, window, cx| {
                            v0.update(cx, |this, cx| this.open_btw_surface(window, cx))
                        }),
                )
                .item(
                    PopupMenuItem::new("Review")
                        .icon(Lucide::FileDiff)
                        .disabled(review_open)
                        .on_click(move |_, window, cx| {
                            v1.update(cx, |this, cx| {
                                this.right_review_open = true;
                                this.select_surface(Surface::Review, window, cx)
                            })
                        }),
                )
                .item(
                    PopupMenuItem::new("Browser")
                        .icon(Lucide::Globe)
                        .disabled(browser_open)
                        .on_click(move |_, window, cx| {
                            v2.update(cx, |this, cx| {
                                this.right_browser_open = true;
                                this.select_surface(Surface::Browser, window, cx)
                            })
                        }),
                )
                .item(
                    PopupMenuItem::new("Board")
                        .icon(Lucide::LayoutDashboard)
                        .disabled(board_open)
                        .on_click(move |_, window, cx| {
                            v3.update(cx, |this, cx| this.open_board_tab(window, cx))
                        }),
                )
            }
        };
        bar = bar.trailing(
            icon_chip("right-panel-add", false, cx)
                .icon(Lucide::Plus)
                .accessibility_label("Add tab")
                .tooltip("Add tab")
                .dropdown_menu_with_anchor(Anchor::TopRight, add),
        );
        let pop = self.render_workspace_pop(window, cx);
        let open = pop.is_some();
        let toggle = icon_chip("workspace-toggle", open, cx)
            .selected(open)
            .toggled(open)
            .icon(Lucide::LayoutList)
            .accessibility_label("Workspace")
            .tooltip("Workspace")
            .when(!open, |d| {
                d.on_click(self.session_listener(cx, |this, _, window, cx| {
                    this.toggle_workspace_pop(window, cx)
                }))
            });
        let workspace_btn =
            anchored_popup("workspace-pop-anchor", toggle, &self.workspace_pop.trigger, true)
                .anchor(Anchor::TopRight)
                .when_some(pop, |p, pop| p.content(pop));
        bar = bar.action(workspace_btn);
        if self.settings.chat_layout != ChatLayout::Split {
            bar = bar.action(
                icon_chip("right-panel-collapse", false, cx)
                    .icon(Lucide::PanelRightClose)
                    .accessibility_label("Collapse right panel")
                    .tooltip("Collapse panel")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_right_panel(cx))),
            );
        }
        bar.into_any_element()
    }

    fn right_panel_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut rail = v_flex()
            .id("right-panel-rail")
            .w(px(32.))
            .h_full()
            .flex_none()
            .items_center()
            .py_1()
            .gap_1()
            .border_l_1()
            .border_color(cx.theme().sidebar_border);
        rail = rail.child(
            icon_chip("right-panel-expand", false, cx)
                .icon(Lucide::PanelRightOpen)
                .accessibility_label("Expand right panel")
                .tooltip("Expand panel")
                .on_click(cx.listener(|this, _, _, cx| this.toggle_right_panel(cx))),
        );
        let tabs = Surface::tabs(self.btw_open, self.right_review_open, self.right_browser_open, self.board_tab);
        for (index, surface) in tabs.into_iter().enumerate() {
            let active = self.right_surface == Some(surface);
            rail = rail.child(
                icon_chip(("right-panel-rail", index), active, cx)
                    .icon(surface.icon())
                    .accessibility_label(surface.label())
                    .tooltip(surface.label())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.toggle_right_panel(cx);
                        this.select_surface(surface, window, cx)
                    })),
            );
        }
        rail.into_any_element()
    }

    pub(crate) fn side_panel_sidebar(
        &mut self,
        agent: Option<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let layout = self.settings.chat_layout;
        if layout == ChatLayout::Tabbed {
            return None;
        }
        let collapsed = right_panel_collapsed(layout, self.settings.right_panel_collapsed);
        let surface = self.right_surface.or_else(|| {
            let tab = self.side_tab_in(window);
            (tab != SideTab::Agent)
                .then(|| surface_for_tab(tab, self.explorer_view, self.diff_open))
        });
        if !collapsed && surface != Some(Surface::Browser) {
            self.browser.update(cx, |browser, _| browser.set_shown(false));
        }
        let center = if self.settings.sidebar_collapsed {
            let rail = v_flex()
                .id("sidebar-expand-rail")
                .w(px(32.))
                .h_full()
                .flex_none()
                .items_center()
                .py_1()
                .child(
                    aether_ui::button("expand-sidebar")
                        .ghost()
                        .small()
                        .icon(IconName::PanelLeftOpen)
                        .accessibility_label("Expand sidebar")
                        .tooltip_with_action("Expand sidebar", &ToggleSidebar, None)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx)),
                        ),
                );
            h_flex()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .h_full()
                .child(rail)
                .child(v_flex().flex_1().min_w_0().min_h_0().h_full().children(agent))
        } else {
            v_flex().flex_1().min_w_0().min_h_0().h_full().children(agent)
        };
        let center = center.into_any_element();
        let panel = if collapsed {
            self.browser.update(cx, |browser, _| browser.set_shown(false));
            self.right_panel_rail(cx)
        } else {
            let active = surface.unwrap_or(Surface::Terminal);
            let content = self.right_panel_surface_content(active, window, cx);
            let (resize, resize_end) = (cx.entity(), cx.entity());
            v_flex()
                .id("right-panel")
                .flex_none()
                .relative()
                .flex_basis(relative(right_panel_share(self.settings.right_panel_share)))
                .flex_shrink(1.)
                .h_full()
                .min_h_0()
                .border_l_1()
                .border_color(cx.theme().sidebar_border)
                .on_drag_move(
                    move |e: &DragMoveEvent<std::rc::Rc<RightPanelResize>>, window, cx| {
                        let panel_px = f32::from(e.bounds.right()) - f32::from(e.event.position.x);
                        let root_px = f32::from(window.bounds().size.width);
                        resize.update(cx, |this, cx| {
                            this.right_panel_resizing = true;
                            let sidebar_px = if this.settings.sidebar_collapsed {
                                0.
                            } else {
                                this.settings
                                    .sidebar_width
                                    .clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH)
                            };
                            let available = (root_px - sidebar_px).max(panel_px);
                            this.settings.right_panel_share =
                                right_panel_share(panel_px / available);
                            cx.notify();
                        })
                    },
                )
                .capture_any_mouse_up(move |_, _, cx| {
                    resize_end.update(cx, |this, _| {
                        if std::mem::take(&mut this.right_panel_resizing) {
                            crate::settings::save(&this.settings);
                        }
                    })
                })
                .child(
                    resize_handle::<RightPanelResize, RightPanelResize>(
                        "right-panel-resize",
                        Axis::Horizontal,
                    )
                    .inside(HandleEdge::Leading)
                    .with_appearance(resize_handle_appearance())
                    .on_drag(RightPanelResize, |_: std::rc::Rc<RightPanelResize>, _, _, cx| {
                        cx.new(|_| RightPanelResize)
                    }),
                )
                .child(self.right_panel_tabs(window, cx))
                .child(content)
                .into_any_element()
        };
        Some(
            h_flex()
                .id("chat-sidebar-layout")
                .flex_1()
                .min_w_0()
                .h_full()
                .child(center)
                .child(panel)
                .into_any_element(),
        )
    }

    /// The full-width tabbed workspace, with `agent` supplying the conversation.
    /// Also tells the native webview whether its tab is visible.
    pub(crate) fn side_panel(
        &mut self,
        agent: Option<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.side_open() {
            self.browser.update(cx, |browser, _| browser.set_shown(false));
            return None;
        }
        if self.settings.chat_layout != ChatLayout::Tabbed {
            return self.side_panel_sidebar(agent, window, cx);
        }
        let host = self.host_of(window);
        let tab = shown_tab(self.side_tab_for(host), self.side_back, &self.side_tabs(cx));
        self.set_side_tab(host, tab);
        if !self.browser_tab_shown() {
            self.browser.update(cx, |browser, _| browser.set_shown(false));
        }
        let content = match tab {
            SideTab::Agent => {
                v_flex().flex_1().min_h_0().w_full().children(agent).into_any_element()
            }
            SideTab::Terminal => {
                let id = self.chats[self.active].id;
                let view = self
                    .terminals
                    .get(&id)
                    .filter(|_| self.peeked_terminal() != Some(id))
                    .map(|d| d.view.clone());
                gpui_kit::div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .children(view.map(|v| {
                        aether_terminal::dark(v.cached(StyleRefinement::default().size_full()), cx)
                    }))
                    .into_any_element()
            }
            SideTab::Explorer => self.render_explorer_page(window, cx),
            SideTab::Browser => {
                self.browser_content(!self.palette_open && !self.notes.search_ui.open, cx)
            }
            SideTab::Board => self.render_board_tab(cx),
        };
        let bar = self.side_tab_bar(window, cx);
        let workspace = v_flex()
            .id("chat-tabs")
            .flex_1()
            .min_w_0()
            .h_full()
            .child(bar)
            .child(content)
            .into_any_element();
        if self.btw_open {
            let (resize, resize_end) = (cx.entity(), cx.entity());
            let panel = v_flex()
                .id("btw-sidebar")
                .flex_none()
                .relative()
                .w(px(btw_width(self.settings.btw_width)))
                .h_full()
                .min_h_0()
                .border_l_1()
                .border_color(cx.theme().sidebar_border)
                .on_drag_move(
                    move |e: &DragMoveEvent<std::rc::Rc<RightPanelResize>>, _, cx| {
                        let width = f32::from(e.bounds.right()) - f32::from(e.event.position.x);
                        resize.update(cx, |this, cx| {
                            this.right_panel_resizing = true;
                            this.settings.btw_width = btw_width(width);
                            cx.notify();
                        })
                    },
                )
                .capture_any_mouse_up(move |_, _, cx| {
                    resize_end.update(cx, |this, _| {
                        if std::mem::take(&mut this.right_panel_resizing) {
                            crate::settings::save(&this.settings);
                        }
                    })
                })
                .child(
                    resize_handle::<RightPanelResize, RightPanelResize>(
                        "btw-resize",
                        Axis::Horizontal,
                    )
                    .inside(HandleEdge::Leading)
                    .with_appearance(resize_handle_appearance())
                    .on_drag(RightPanelResize, |_: std::rc::Rc<RightPanelResize>, _, _, cx| {
                        cx.new(|_| RightPanelResize)
                    }),
                )
                .child(self.render_btw_panel(cx))
                .into_any_element();
            Some(
                h_flex()
                    .id("chat-tabbed-btw")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(workspace)
                    .child(panel)
                    .into_any_element(),
            )
        } else {
            Some(workspace)
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/side_panel.rs"]
mod tests;
