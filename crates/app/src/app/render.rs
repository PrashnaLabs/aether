use crate::actions::{
    BackToChat, FocusComposer, NewChat, NextSideTab, OpenSettings, OpenSkills, OpenUsage,
    PreviousSideTab, QuickOpen, SaveBuffer, SearchWorkspace, StopGenerating, ToggleBrowser,
    ToggleChanges, ToggleCommandMenu, ToggleExplorer, ToggleGlobalTerminal, ToggleHackerNews,
    ToggleSidePanel, ToggleSidebar, ToggleTerminal, ToggleWatchList, ToggleWebSearch,
};
use aether_ui::{BranchSelect, BranchSelectState, ChatScroller, field};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::{Context, ScrollWheelEvent, Window, div, prelude::*};

use super::{ChatApp, Page, apply_font_size};

impl Render for ChatApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        apply_font_size(self.settings.font_size, window);
        let news_shown = self.page == Page::HackerNews;
        let reader_shown =
            news_shown && self.hn_reader_open && !self.palette_open && !self.notes.search_ui.open;
        self.hn_browser.update(cx, |browser, _| browser.set_shown(reader_shown));
        if self.page != Page::Chat {
            self.browser.update(cx, |browser, _| browser.set_shown(false));
        }
        #[cfg(target_os = "macos")]
        self.sync_touch_bar(0, window, cx);
        if self.page == Page::Chat
            && let Some(id) = self
                .workspace
                .model
                .windows
                .get(&0)
                .and_then(|w| w.focused)
                .and_then(|p| self.workspace.model.panes.get(&p).copied())
            && let Some(ix) = self.chats.iter().position(|c| c.id == id)
        {
            let changed = self.chats.get(self.active).is_none_or(|chat| chat.id != id);
            self.activate_session_ui(id, window, cx);
            self.active = ix;
            if changed {
                if self.folder_of(&self.chats[ix]).is_none() {
                    self.active_terminal_tab = aether_terminal::TerminalTab::Global;
                } else if matches!(
                    self.active_terminal_tab,
                    aether_terminal::TerminalTab::Project(_)
                ) {
                    self.active_terminal_tab = aether_terminal::TerminalTab::Project(id);
                }
                self.refresh_git(false, cx);
                self.refresh_missing_attachments(false, cx);
                self.load_bases(cx);
            }
        }
        let native_bounds = window.window_bounds().get_bounds();
        if let Some(layout) = self.workspace.model.windows.get_mut(&0) {
            layout.bounds = Some(crate::session_workspace::Rect::new(
                f32::from(native_bounds.origin.x),
                f32::from(native_bounds.origin.y),
                f32::from(native_bounds.size.width),
                f32::from(native_bounds.size.height),
            ));
        }
        self.schedule_workspace_save(cx);
        self.render_pending_worktree_dialog(window, cx);
        self.sync_placeholder(window, cx);
        let composer = self.render_composer(cx);
        let full = self.side_full && self.side_open();
        let show_header = !full && self.page == Page::Chat;
        // Tabbed chat renders Workspace in its unified tab bar, even when the
        // standalone header is hidden. Dismiss only when that trigger disappears.
        if self.page != Page::Chat
            || !self.chats.get(self.active).is_some_and(|c| self.is_agent_chat(c))
        {
            self.workspace_pop.dismiss();
        }
        let pane_header = self.page == Page::Chat
            && self.workspace.model.windows.get(&0).is_some_and(|w| !w.panes().is_empty());
        let header = if show_header && !pane_header {
            let pop = self.render_workspace_pop(window, cx);
            Some(self.render_header(pop, cx).into_any_element())
        } else if self.page == Page::Project {
            Some(self.render_project_top_bar(cx).into_any_element())
        } else {
            None
        };
        let body = if self.page == Page::Settings {
            self.render_settings(cx).into_any_element()
        } else if self.page == Page::Notes {
            self.render_notes(window, cx)
        } else if self.page == Page::Skills {
            self.render_skills(cx).into_any_element()
        } else if self.page == Page::Agents {
            self.render_agents(cx).into_any_element()
        } else if self.page == Page::Usage {
            self.render_usage(cx)
        } else if self.page == Page::Project {
            self.render_project_page(cx)
        } else if self.page == Page::Terminal {
            self.render_terminal_page(window, cx)
        } else if self.page == Page::Board {
            self.render_board(window, cx)
        } else if self.page == Page::HackerNews {
            self.render_hackernews_page(window, cx).into_any_element()
        } else if self.page == Page::Chat || self.page == Page::Explorer {
            self.render_session_workspace(0, window, cx)
        } else if self.chats[self.active].messages.is_empty() {
            self.render_start(composer, cx)
        } else {
            let view = self.session_handle(cx);
            let on_wheel = self.session_listener(cx, |this, _: &ScrollWheelEvent, window, cx| {
                this.unfocus_composer(window, cx)
            });
            v_flex()
                .flex_1()
                .w_full()
                .min_h_0()
                .child(
                    v_flex().flex_1().min_h_0().on_scroll_wheel(on_wheel).child(
                        ChatScroller::new("messages", self.scroller.clone(), move |ix, _, cx| {
                            view.update(cx, |app, cx| Some(app.render_row(ix, &view, cx)))
                                .unwrap_or_else(|| div().into_any_element())
                        })
                        .scrollbar(false)
                        .jump_button(false)
                        .with_bottom_fade(cx.theme().background)
                        .flex_1(),
                    ),
                )
                .child(composer)
                .into_any_element()
        };
        let is_agent = self.is_agent_chat(&self.chats[self.active]);
        let drawer = is_agent.then(|| self.terminal_drawer(window, cx)).flatten();
        let terminal_maximized = is_agent && self.is_terminal_drawer_maximized(cx);
        let fullscreen = self.fullscreen_terminal(window, cx);
        let chromeless = fullscreen.is_some();
        let sidebar_layout =
            crate::side_panel::chat_body_in_sidebar(self.settings.chat_layout, self.side_open());
        let (agent, body) =
            if full || sidebar_layout { (Some(body), None) } else { (None, Some(body)) };
        let side =
            (is_agent && !terminal_maximized).then(|| self.side_panel(agent, window, cx)).flatten();
        let side = match (side, full.then(|| self.hoisted_session_tabs(cx)).flatten()) {
            (Some(side), Some(tabs)) => Some(
                v_flex().flex_1().min_w_0().h_full().child(tabs).child(side).into_any_element(),
            ),
            (side, _) => side,
        };
        let palette = self.palette_open.then(|| self.render_palette(0, cx));
        let quick_open = (is_agent || self.page == Page::Notes)
            .then(|| self.render_quick_open_overlay(window, cx))
            .flatten();
        let notes_search = self.render_notes_search(window, cx);
        let image_viewer = self.image_viewer_overlay(window, cx);
        let view_handle = cx.entity().clone();
        v_flex()
            .relative()
            .size_full()
            .track_focus(&self.focus)
            .key_context(if self.page == Page::Chat { "Aether Chat" } else { "Aether" })
            .on_action(cx.listener(|this, _: &FocusComposer, window, cx| {
                if this.page == Page::Chat {
                    this.focus_composer(window, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &PreviousSideTab, window, cx| {
                this.cycle_side_tab(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &NextSideTab, window, cx| {
                this.cycle_side_tab(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleCommandMenu, window, cx| {
                this.toggle_palette(window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::actions::SearchNotes, window, cx| {
                this.toggle_notes_search(window, cx)
            }))
            .on_action(cx.listener(|this, _: &NewChat, window, cx| {
                this.new_chat_in_current_project(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &OpenSettings, _, cx| this.open_page(Page::Settings, cx)),
            )
            .on_action(cx.listener(|this, _: &crate::actions::OpenNotes, window, cx| {
                this.open_notes(window, cx)
            }))
            .on_action(cx.listener(|this, _: &OpenSkills, _, cx| this.open_page(Page::Skills, cx)))
            .on_action(cx.listener(|this, _: &OpenUsage, _, cx| this.toggle_usage_page(cx)))
            .on_action(cx.listener(|this, _: &crate::actions::OpenBoard, window, cx| {
                this.open_board(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleHackerNews, window, cx| {
                this.toggle_hackernews_page(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleWatchList, _, cx| this.toggle_watch_list(cx)))
            .on_action(cx.listener(|this, _: &BackToChat, window, cx| {
                let editing_skill = this.page == Page::Skills && this.skill_editor.is_some();
                if matches!(
                    this.page,
                    Page::Settings
                        | Page::Skills
                        | Page::Usage
                        | Page::HackerNews
                        | Page::Notes
                        | Page::Board
                ) && !editing_skill
                {
                    this.open_page(Page::Chat, cx);
                    this.composer.update(cx, |s, cx| s.focus(window, cx));
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleWebSearch, _, cx| {
                this.update_settings(cx, |s| s.web_search = !s.web_search)
            }))
            .on_action(cx.listener(|this, _: &StopGenerating, window, cx| this.stop(window, cx)))
            .on_action(cx.listener(|this, _: &ToggleGlobalTerminal, window, cx| {
                this.toggle_terminal_page(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ToggleTerminal, window, cx| {
                    this.toggle_terminal(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &aether_terminal::FocusTerminal, window, cx| {
                this.focus_terminal(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleExplorer, window, cx| {
                this.toggle_explorer_page(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &QuickOpen, window, cx| this.open_quick_open(window, cx)),
            )
            .on_action(cx.listener(|this, _: &SearchWorkspace, window, cx| {
                if this.page == Page::Notes {
                    this.toggle_notes_search(window, cx);
                    return;
                }
                this.search_workspace(window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::actions::SaveSelectionToNote, window, cx| {
                this.save_selection_to_note(window, cx)
            }))
            .on_action(cx.listener(|this, _: &SaveBuffer, window, cx| {
                if this.page == Page::Notes
                    || (this.side_open()
                        && this.side_tab_in(window) == crate::side_panel::SideTab::Explorer)
                {
                    this.save_active_explorer_file(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleChanges, window, cx| {
                this.toggle_changes(true, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ToggleBrowser, window, cx| this.toggle_browser(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ToggleSidePanel, window, cx| this.toggle_side(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ToggleSidebar, window, cx| this.toggle_sidebar(window, cx)),
            )
            .text_color(cx.theme().foreground)
            .child(
                h_flex()
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .when(!self.settings.sidebar_collapsed && !chromeless, |root| {
                        root.child(self.render_sidebar(cx))
                    })
                    .child(
                        v_flex()
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .min_h_0()
                            .when(
                                cfg!(any(target_os = "macos", target_os = "linux", target_os = "windows"))
                                    && (self.settings.sidebar_collapsed || chromeless)
                                    && !window.is_fullscreen(),
                                |main| {
                                    main.child(
                                        gpui_kit::component::TitleBar::new()
                                            .h(gpui_kit::px(36.))
                                            .bg(cx.theme().background)
                                            .border_0(),
                                    )
                                },
                            )
                            .map(|main| {
                                if let Some(view) = fullscreen {
                                    return main.child(aether_terminal::dark(
                                        view.cached(
                                            gpui_kit::StyleRefinement::default().size_full(),
                                        ),
                                        cx,
                                    ));
                                }
                                if terminal_maximized && let Some(drawer) = drawer {
                                    return main.child(drawer);
                                }
                                let Some(body) = body else { return main.children(side) };
                                main.child(
                                    // Non-chat pages retain their normal header and content.
                                    h_flex()
                                        .relative()
                                        .flex_1()
                                        .min_h_0()
                                        .w_full()
                                        .child(
                                            v_flex()
                                                .relative()
                                                .flex_1()
                                                .h_full()
                                                .min_w_0()
                                                .children(header)
                                                .child(v_flex().flex_1().min_h_0().min_w_0().map(
                                                    |column| {
                                                        match drawer {
                                                            // the body shrinks and clips so a tall drawer never pushes off-screen
                                                            Some(drawer) => column
                                                                .child(
                                                                    v_flex()
                                                                        .flex_1()
                                                                        .min_h_0()
                                                                        .overflow_hidden()
                                                                        .child(body),
                                                                )
                                                                .child(drawer),
                                                            None => column.child(body),
                                                        }
                                                    },
                                                )),
                                        )
                                        .children(side),
                                )
                            }),
                    ),
            )
            .child(self.render_global_status_bar(&view_handle, window, cx))
            .children(self.render_watch(window, cx))
            .children(palette)
            .children(quick_open)
            .children(image_viewer)
            .children(notes_search)
    }
}

impl ChatApp {
    pub(crate) fn render_pending_worktree_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_worktree_dialog {
            self.pending_worktree_dialog = false;
            let bases = self.active_bases().map(|b| b.branches.clone()).unwrap_or_default();
            let base = self
                .pending_worktree_dialog_base
                .take()
                .or_else(|| self.active_bases().and_then(|b| b.default.clone()))
                .unwrap_or_default();
            let state = cx.new(|cx| BranchSelectState::new(window, cx));
            state.update(cx, |s, cx| s.sync(bases, Some(&base), cx));
            self.worktree_base_select = Some(state.clone());
            let view = self.session_handle(cx);
            let name_input = self.worktree_name_input.clone();
            let chat_id = self.chats[self.active].id;
            aether_ui::theme::open_alert(window, cx, move |alert, _, _cx| {
                let (view, state, name_input) = (view.clone(), state.clone(), name_input.clone());
                alert
                    .confirm()
                    .title("New worktree")
                    .description(
                        v_flex()
                            .gap_2()
                            .child(div().text_xs().child("Base branch"))
                            .child(BranchSelect::new(&state))
                            .child(div().text_xs().child("Custom name (empty = auto)"))
                            .child(field(&name_input)),
                    )
                    .ok_text("Use worktree")
                    .on_ok(move |_, window, cx| {
                        let base = state.read(cx).selected().unwrap_or_default().to_string();
                        let name = name_input.read(cx).value().trim().to_string();
                        view.update(cx, |this, cx| {
                            if this.chats[this.active].id != chat_id
                                || !this.workspace_can_change(this.active)
                            {
                                return;
                            }
                            if !base.is_empty() {
                                this.choose_worktree(Some(base), cx);
                            }
                            if !name.is_empty() {
                                this.worktree_name_input
                                    .update(cx, |s, cx| s.set_value(name, window, cx));
                            }
                        });
                        true
                    })
            });
        }
    }
}
