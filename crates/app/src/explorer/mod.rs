use crate::session_workspace::WindowId;
use aether_explorer::{ExplorerSidebarTab, ExplorerState};
use aether_ui::icon_chip;
use aether_ui::theme::Corners;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, IconName, StyledExt, h_flex, v_flex};
use gpui_kit::rems;
use gpui_kit::{
    AnyElement, AppContext, Context, DragMoveEvent, Empty, Entity, MouseButton, Render, Window,
    actions, div, prelude::*, px,
};

use crate::actions::{SearchWorkspace, key_hint};
use crate::app::{ChatApp, ChatViewMode, Page, format_time};
use crate::git_panel::changed_files;
use crate::side_panel::SideTab;

mod buffers;
mod diff_tree;
mod editor;
mod grep;
pub(crate) mod quick_open;
mod tree;

actions!(explorer, [CloseEditorTab, NextEditorTab, PrevEditorTab]);

/// Key context around the explorer's tab bar and editor.
pub(crate) const EXPLORER_EDITOR_CONTEXT: &str = "ExplorerEditor";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ExplorerView {
    #[default]
    Files,
    Changes,
    Diff,
}

impl ChatApp {
    pub(crate) fn changes_view_shown(&self, host: WindowId) -> bool {
        self.side_open()
            && self.side_tab_for(host) == SideTab::Explorer
            && self.explorer_view == ExplorerView::Changes
    }

    pub(crate) fn open_explorer_view(
        &mut self,
        view: ExplorerView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_explorer_panel(window, cx);
        self.explorer_view = view;
        match view {
            ExplorerView::Files => {}
            ExplorerView::Changes => {
                self.git.refocus = true;
                self.refresh_git(false, cx);
            }
            ExplorerView::Diff => {
                self.diff_open = true;
                window.focus(&self.diff_focus, cx);
            }
        }
        cx.notify();
    }

    fn collapse_explorer_folders(&mut self, cx: &mut Context<Self>) {
        let explorer = self.active_project_explorer(cx);
        explorer.update(cx, |state, cx| {
            state.tree_state.collapse_all();
            cx.notify();
        });
        cx.notify();
    }

    /// Switches between the fixed Explorer and Agent tabs.
    pub(crate) fn toggle_explorer_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chat = &self.chats[self.active];
        if !self.is_agent_chat(chat) {
            return;
        }
        if self.side_open() && self.side_tab_in(window) == SideTab::Explorer {
            self.focus_composer(window, cx);
            cx.notify();
        } else {
            self.open_explorer_panel(window, cx);
        }
    }

    pub(crate) fn open_explorer_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_agent_chat(&self.chats[self.active]) {
            return;
        }
        if self.page != Page::Chat {
            self.explorer_prev_page = self.page;
            self.open_page(Page::Chat, cx);
        }
        let chat = &mut self.chats[self.active];
        let refresh = chat.view != ChatViewMode::Explorer;
        chat.view = ChatViewMode::Explorer;
        self.git.open.insert(chat.id);
        let host = self.host_of(window);
        self.set_side_tab(host, SideTab::Explorer);
        if self.explorer_view == ExplorerView::Diff {
            self.explorer_view = ExplorerView::Files;
        }
        if refresh {
            let explorer = self.active_project_explorer(cx);
            self.refresh_explorer(&explorer, cx);
        }
        self.check_explorer_disk(&self.active_project_folder(), window, cx);
        cx.notify();
    }

    #[allow(dead_code)]
    pub(crate) fn close_explorer_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.chats[self.active].view = ChatViewMode::Chat;
        if self.side_tab == SideTab::Explorer {
            self.side_tab = SideTab::Agent;
        }
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Opens `path` in this chat's explorer, switching to it first.
    pub(crate) fn open_in_explorer(
        &mut self,
        path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_in_explorer_at(path, None, None, window, cx);
    }

    /// Opens `path` at 1-indexed `line` and `col` in this chat's explorer, switching
    /// to it first; a folder is revealed in the tree instead.
    pub(crate) fn open_in_explorer_at(
        &mut self,
        path: std::path::PathBuf,
        line: Option<usize>,
        col: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if path.is_dir() {
            self.open_explorer_panel(window, cx);
            self.active_project_explorer(cx).update(cx, |exp, _| exp.reveal(&path));
            cx.notify();
        } else {
            self.open_explorer_file_at(path, line, col, window, cx);
        }
    }

    /// Re-reads the tree's open folders and reloads the file index.
    pub(crate) fn refresh_explorer(
        &mut self,
        explorer: &Entity<ExplorerState>,
        cx: &mut Context<Self>,
    ) {
        explorer.update(cx, |exp, _| exp.refresh_tree());
        self.load_explorer_index(explorer, cx);
    }

    /// Lists the explorer's files off the UI thread for Quick Open, grep and
    /// dimming; a grep with a query re-runs on the new list. A newer load drops this one.
    pub(crate) fn load_explorer_index(
        &mut self,
        explorer: &Entity<ExplorerState>,
        cx: &mut Context<Self>,
    ) {
        let (root, generation) = explorer.update(cx, |exp, _| {
            exp.index_generation += 1;
            (exp.root.clone(), exp.index_generation)
        });
        let task =
            cx.background_executor().spawn(async move { aether_explorer::load_index(&root) });
        let explorer = explorer.clone();
        cx.spawn(async move |this, cx| {
            let index = task.await;
            this.update(cx, |this, cx| {
                let regrep = explorer.update(cx, |exp, _| {
                    exp.set_index(generation, index) && !exp.grep_options.query.trim().is_empty()
                });
                if regrep {
                    this.spawn_grep(&explorer, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Opens this chat's explorer on the grep tab with its query focused.
    pub(crate) fn search_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_agent_chat(&self.chats[self.active]) {
            return;
        }
        self.open_explorer_panel(window, cx);
        self.set_explorer_sidebar_tab(ExplorerSidebarTab::Grep, cx);
        self.explorer_grep_input.update(cx, |input, cx| input.focus(window, cx));
    }

    /// Switches the explorer sidebar between file tree and grep search modes.
    pub(crate) fn set_explorer_sidebar_tab(
        &mut self,
        tab: ExplorerSidebarTab,
        cx: &mut Context<Self>,
    ) {
        if !self.is_agent_chat(&self.chats[self.active]) {
            return;
        }
        let explorer = self.active_project_explorer(cx);
        explorer.update(cx, |exp, _| {
            exp.sidebar_tab = tab;
        });
        if self.explorer_view != ExplorerView::Files {
            self.explorer_view = ExplorerView::Files;
        }
        cx.notify();
    }

    /// File tree and editor hosted by the Explorer tab.
    pub(crate) fn render_explorer_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let content = match self.explorer_view {
            ExplorerView::Files | ExplorerView::Changes | ExplorerView::Diff => {
                self.render_explorer_files(window, cx)
            }
        };
        v_flex().size_full().min_w_0().min_h_0().child(content).into_any_element()
    }

    /// Which sidebar view is showing: 0 files, 1 search, 2 changes, 3 diff.
    fn explorer_mode(&mut self, cx: &mut Context<Self>) -> usize {
        match self.explorer_view {
            ExplorerView::Diff => 3,
            ExplorerView::Changes => 2,
            ExplorerView::Files => match self.active_project_explorer(cx).read(cx).sidebar_tab {
                ExplorerSidebarTab::Files => 0,
                ExplorerSidebarTab::Grep => 1,
            },
        }
    }

    fn set_explorer_mode(&mut self, mode: usize, window: &mut Window, cx: &mut Context<Self>) {
        match mode {
            0 => self.set_explorer_sidebar_tab(ExplorerSidebarTab::Files, cx),
            1 => self.set_explorer_sidebar_tab(ExplorerSidebarTab::Grep, cx),
            2 => self.open_explorer_view(ExplorerView::Changes, window, cx),
            _ => self.open_explorer_view(ExplorerView::Diff, window, cx),
        }
    }

    /// The vertical tab strip beside the sidebar: Explorer, Search, Changes and
    /// Diff. Picking the active one folds the sidebar away, and back.
    fn render_explorer_rail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (border, sidebar_bg, muted, foreground, primary) =
            (theme.border, theme.sidebar, theme.muted_foreground, theme.foreground, theme.primary);
        let active = self.explorer_mode(cx);
        let changes_count = self.git_status().map(|s| changed_files(&s.changes)).unwrap_or(0);
        let grep_tip =
            key_hint(&SearchWorkspace, cx).map_or("Search".into(), |k| format!("Search ({k})"));
        let changes_tip = match changes_count {
            0 => "Changes".to_string(),
            n => format!("Changes ({n})"),
        };
        let items: [(Lucide, String); 4] = [
            (Lucide::Files, "Explorer".into()),
            (Lucide::Search, grep_tip),
            (Lucide::GitBranch, changes_tip),
            (Lucide::FileDiff, "Reply Diff".into()),
        ];
        let mut rail = v_flex()
            .w(px(44.))
            .h_full()
            .flex_none()
            .items_center()
            .py_1()
            .gap_0p5()
            .bg(sidebar_bg)
            .border_r_1()
            .border_color(border);
        for (mode, (icon, tip)) in items.into_iter().enumerate() {
            let selected = active == mode;
            let shown = selected && !(self.side_full && self.explorer_tree_collapsed);
            let dot = mode == 2 && changes_count > 0;
            rail = rail.child(
                div()
                    .id(("explorer-rail", mode))
                    .relative()
                    .size(px(40.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .corner(rems(0.375))
                    .text_color(if shown { foreground } else { muted })
                    .hover(|d| d.text_color(foreground).bg(foreground.opacity(0.06)))
                    .when(shown, |d| {
                        d.child(
                            div()
                                .absolute()
                                .left_0()
                                .top(px(8.))
                                .bottom(px(8.))
                                .w(px(2.))
                                .rounded_full()
                                .bg(primary),
                        )
                    })
                    .child(Icon::new(icon).size(px(20.)))
                    .when(dot, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top(px(8.))
                                .right(px(8.))
                                .size(px(7.))
                                .rounded_full()
                                .bg(aether_ui::theme::success_text(cx)),
                        )
                    })
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if selected && this.side_full {
                            this.explorer_tree_collapsed = !this.explorer_tree_collapsed;
                        } else {
                            this.explorer_tree_collapsed = false;
                            this.set_explorer_mode(mode, window, cx);
                        }
                        cx.notify();
                    })),
            );
        }
        rail.into_any_element()
    }

    fn render_explorer_files(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let has_files =
            self.explorer_view != ExplorerView::Files || self.active_explorer_has_tabs(cx);
        let size = self.explorer_tree_size;
        let rail = self.render_explorer_rail(cx);
        // Right panel is narrow: keep left/right, but fit tree to at most 45% of it.
        let narrow = self.settings.chat_layout != aether_core::ChatLayout::Tabbed;
        let size = if narrow { size.clamp(TREE_MIN, 240.) } else { size };
        if self.side_full {
            if self.explorer_tree_collapsed {
                return h_flex()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .child(rail)
                    .child(self.render_explorer_main(window, cx))
                    .into_any_element();
            }
            let tree = self.render_explorer_sidebar(window, cx);
            h_flex()
                .id("explorer-split")
                .size_full()
                .min_w_0()
                .min_h_0()
                .on_drag_move(cx.listener(|this, e: &DragMoveEvent<TreeDrag>, _, cx| {
                    // the rail sits left of the sidebar, so its width counts
                    let width = f32::from(e.event.position.x - e.bounds.origin.x) - 44.;
                    this.explorer_tree_size = width.clamp(TREE_MIN, TREE_MAX);
                    cx.notify();
                }))
                .child(rail)
                .child(div().w(px(size)).h_full().flex_none().child(tree))
                .child(tree_handle(true, cx))
                .child(self.render_explorer_main(window, cx))
                .into_any_element()
        } else {
            let tree = self.render_explorer_sidebar(window, cx);
            let height = size.clamp(TREE_MIN_HEIGHT, TREE_MAX_HEIGHT);
            v_flex()
                .id("explorer-split")
                .size_full()
                .min_w_0()
                .min_h_0()
                .on_drag_move(cx.listener(|this, e: &DragMoveEvent<TreeDrag>, _, cx| {
                    let height = f32::from(e.event.position.y - e.bounds.origin.y);
                    this.explorer_tree_size = height.clamp(TREE_MIN_HEIGHT, TREE_MAX_HEIGHT);
                    cx.notify();
                }))
                .child(
                    h_flex()
                        .w_full()
                        .min_h_0()
                        .when(has_files, |d| d.h(px(height)).flex_none())
                        .when(!has_files, |d| d.flex_1())
                        .child(rail)
                        .child(div().flex_1().min_w_0().h_full().child(tree)),
                )
                .when(has_files, |d| {
                    d.child(tree_handle(false, cx)).child(self.render_explorer_main(window, cx))
                })
                .into_any_element()
        }
    }

    fn active_explorer_has_tabs(&mut self, cx: &mut Context<Self>) -> bool {
        !self.active_project_explorer(cx).read(cx).tabs.is_empty()
    }

    /// The section right of the sidebar: open files, the selected change's diff, or the reply's diffs.
    fn render_explorer_main(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.explorer_view {
            ExplorerView::Diff => div()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(self.reply_diff_content(cx))
                .into_any_element(),
            ExplorerView::Changes => {
                div().flex_1().min_w_0().h_full().child(self.git_diff_pane(cx)).into_any_element()
            }
            ExplorerView::Files => self.render_explorer_editor_canvas(window, cx),
        }
    }

    /// Renders the sidebar panel: the directory tree, grep search, changes or reply files.
    fn render_explorer_sidebar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let explorer_entity = self.active_project_explorer(cx);
        let theme = cx.theme();
        let border = theme.border;
        let sidebar_bg = theme.sidebar;
        let muted = theme.muted_foreground;
        let sidebar_tab = explorer_entity.read(cx).sidebar_tab;
        let changes = self.explorer_view == ExplorerView::Changes;
        let diff = self.explorer_view == ExplorerView::Diff;
        let can_collapse = explorer_entity.read(cx).tree_state.has_expanded_folders();
        let title = match (changes, diff, sidebar_tab) {
            (_, true, _) => "DIFF",
            (true, _, _) => "Changes",
            (_, _, ExplorerSidebarTab::Files) => "EXPLORER",
            (_, _, ExplorerSidebarTab::Grep) => "SEARCH",
        };

        let mut header = h_flex()
            .h(px(40.))
            .px_3()
            .gap_1()
            .items_center()
            .border_b_1()
            .border_color(border)
            .child(div().text_xs().font_semibold().text_color(muted).child(title))
            .child(div().flex_1());

        if diff {
        } else if changes {
            if self.git_repo().is_some() {
                let updated = self.git_loaded().and_then(|loaded| format_time(loaded.at));
                let refresh_tip = match updated {
                    Some(at) => format!("Refresh · updated {at}"),
                    None => "Refresh".into(),
                };
                let (icon, label) = self.git_view_toggle();
                header = header
                    .child(
                        icon_chip("git-view-mode", false, cx)
                            .icon(icon)
                            .accessibility_label(label)
                            .tooltip(label)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_git_tree_view(window, cx)
                            })),
                    )
                    .child(
                        icon_chip("git-refresh", false, cx)
                            .icon(Lucide::RotateCw)
                            .loading(self.git.is_refreshing())
                            .accessibility_label("Refresh changes")
                            .tooltip(refresh_tip)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh_git(true, cx))),
                    )
                    .children(self.git_more_button(cx));
            }
        } else {
            header = header
                .child(
                    icon_chip("refresh-tree", false, cx)
                        .icon(IconName::RefreshCw)
                        .tooltip("Refresh Directory Tree")
                        .on_click(cx.listener(|this, _, _, cx| {
                            let explorer = this.active_project_explorer(cx);
                            this.refresh_explorer(&explorer, cx);
                            cx.notify();
                        })),
                )
                .child(
                    icon_chip("collapse-explorer-folders", false, cx)
                        .icon(Lucide::ChevronsDownUp)
                        .accessibility_label("Collapse all folders")
                        .tooltip("Collapse all folders")
                        .disabled(!can_collapse || sidebar_tab != ExplorerSidebarTab::Files)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.collapse_explorer_folders(cx);
                        })),
                );
        }

        let body = if diff {
            self.render_explorer_diff_pane(cx)
        } else if changes {
            self.git_panel(window, cx).unwrap_or_else(|| {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p_4()
                    .text_sm()
                    .text_color(muted)
                    .child("This folder is not a git repository.")
                    .into_any_element()
            })
        } else {
            match sidebar_tab {
                ExplorerSidebarTab::Files => self.render_explorer_tree_pane(window, cx),
                ExplorerSidebarTab::Grep => self.render_explorer_grep_pane(window, cx),
            }
        };

        v_flex()
            .w_full()
            .h_full()
            .min_w_0()
            .min_h_0()
            .bg(sidebar_bg)
            .child(header)
            .child(body)
            .into_any_element()
    }
}

/// Drag payload of the divider between the file tree and the editor.
#[derive(Clone, Copy)]
struct TreeDrag;

impl Render for TreeDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

const TREE_MIN: f32 = 180.;
const TREE_MAX: f32 = 640.;
const TREE_MIN_HEIGHT: f32 = 120.;
const TREE_MAX_HEIGHT: f32 = 600.;

/// Draggable divider after the tree; double-click restores the default size.
fn tree_handle(vertical: bool, cx: &mut Context<ChatApp>) -> impl IntoElement {
    let border = cx.theme().border;
    div()
        .id("explorer-tree-handle")
        .flex_none()
        .bg(border)
        .hover(|d| d.bg(border.opacity(0.4)))
        .map(|d| {
            if vertical {
                d.w(px(2.)).h_full().cursor_col_resize()
            } else {
                d.h(px(2.)).w_full().cursor_row_resize()
            }
        })
        .on_drag(TreeDrag, |d: &TreeDrag, _, _, cx| cx.new(|_| *d))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, e: &gpui_kit::MouseDownEvent, _, cx| {
                if e.click_count == 2 {
                    this.explorer_tree_size = 240.;
                    cx.notify();
                }
            }),
        )
}
