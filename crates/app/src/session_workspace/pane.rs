use super::*;
use crate::actions::*;
use crate::app::{ChatApp, watch_item};
use aether_core::{Step, ToolCall, ToolDetail};
use aether_ui::theme::Corners;
use aether_ui::{
    AttachmentView, BackgroundItem, BackgroundPanel, ChatScroller, SubagentCard, ToolRow,
    attachment_strip,
};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, AppContext, Context, ElementId, KeyDownEvent, MouseButton, ScrollWheelEvent,
    Window, div, prelude::*, px,
};
use std::rc::Rc;

use super::render::PaneDrag;

impl ChatApp {
    pub(super) fn render_session_pane(
        &mut self,
        pane: PaneId,
        host: WindowId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let body = self.session_body(("session-messages", pane as usize).into(), window, cx);
        let view = self.session_handle(cx);
        let chat = &self.chats[self.active];
        let title = chat.title.clone();
        let provider = aether_ui::provider_icon(chat.provider).size(px(12.));
        let busy = chat.busy();
        let pinned = chat.pinned;
        let has_messages = !chat.messages.is_empty();
        let project = self.chat_project(self.active);
        let has_project = project.is_some();
        let can_resume = self.resume_command_of(self.active).is_some();
        let active_title = project.map_or(cx.theme().foreground, |dir| {
            aether_ui::project_color(&self.project_name(&dir), cx)
        });
        let waiting = chat.messages.last().and_then(|m| m.pending_request()).is_some();
        let focused =
            self.workspace.model.windows.get(&host).is_some_and(|w| w.focused == Some(pane));
        let floating = self
            .workspace
            .model
            .windows
            .get(&host)
            .and_then(|w| w.floats.iter().find(|f| f.pane == pane))
            .map(|f| f.bounds);
        let app = cx.entity();
        let (background_count, background) = self.background_panel(cx);
        let body = match background {
            Some(panel) => h_flex()
                .items_stretch()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .child(v_flex().flex_1().min_w_0().min_h_0().child(body))
                .child(panel)
                .into_any_element(),
            None => body,
        };
        let body = if host != 0 {
            if let Some(drawer) = self.terminal_drawer(window, cx) {
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .child(v_flex().flex_1().min_h_0().overflow_hidden().child(body))
                    .child(
                        div()
                            .id("session-terminal")
                            .flex_none()
                            .on_mouse_down(MouseButton::Right, block_session_menu)
                            .child(drawer),
                    )
                    .into_any_element()
            } else {
                body
            }
        } else {
            body
        };
        let body = if host != 0 && self.side_open() {
            let full = self.side_full;
            let (agent, body) = if full { (Some(body), None) } else { (None, Some(body)) };
            h_flex()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .children(body.map(|body| v_flex().flex_1().min_w_0().min_h_0().child(body)))
                .children(self.side_panel(agent, window, cx))
                .into_any_element()
        } else {
            body
        };
        let header_group = gpui_kit::SharedString::from(format!("pane-header-{pane}"));
        let header = h_flex()
            .id(("pane-header", pane as usize))
            .group(header_group.clone())
            .h(px(32.))
            .flex_none()
            .gap_2()
            .px_3()
            .bg(cx.theme().foreground.opacity(if focused { 0.035 } else { 0.015 }))
            .border_b_1()
            .border_color(cx.theme().foreground.opacity(0.055))
            .child(provider)
            .child(
                div()
                    .text_xs()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(if focused { active_title } else { cx.theme().muted_foreground })
                    .child(title),
            )
            .when(busy, |d| {
                d.child(
                    div()
                        .text_xs()
                        .text_color(if waiting {
                            aether_ui::theme::warning_text(cx)
                        } else {
                            cx.theme().muted_foreground
                        })
                        .child(if waiting { "Needs input" } else { "Running" }),
                )
            })
            .when(background_count > 0, |d| {
                let chat = self.chats[self.active].id;
                let shown = !self.background_hidden.contains(&chat);
                d.child(
                    aether_ui::button(("pane-background", pane as usize))
                        .ghost()
                        .xsmall()
                        .selected(shown)
                        .icon(Lucide::PanelRight)
                        .label(format!("{background_count} in background"))
                        .tooltip(if shown {
                            "Hide background tasks"
                        } else {
                            "Show background tasks"
                        })
                        .on_click(self.session_listener(cx, move |this, _, _, cx| {
                            if !this.background_hidden.remove(&chat) {
                                this.background_hidden.insert(chat);
                            }
                            cx.notify();
                        })),
                )
            });
        let surface = self.workspace.surface_sizes.get(&host).copied();
        let gap = self.settings.tile_gap.clamp(0., 24.);
        let split_sides = [DockSide::Right, DockSide::Left, DockSide::Bottom, DockSide::Top]
            .map(|side| (side, self.workspace.model.can_split_in(pane, side, surface, gap).err()));
        let context_app = app.clone();
        let chat_id = self.chats[self.active].id;
        let menu_session = view.clone();
        let floats = floating.is_some();
        let session_menu = move |menu: PopupMenu,
                                 window: &mut Window,
                                 cx: &mut Context<PopupMenu>| {
            let popup = cx.entity();
            context_app.update(cx, |app, cx| {
                if host == 0 {
                    app.browser.update(cx, |browser, cx| browser.hide_for_menu(&popup, cx));
                }
                app.workspace.model.focus(pane);
                cx.notify();
            });
            let pin = menu_session.clone();
            let stop = menu_session.clone();
            let jump = menu_session.clone();
            let split = context_app.clone();
            let float = context_app.clone();
            let terminal = menu_session.clone();
            let resume = menu_session.clone();
            let explorer = menu_session.clone();
            menu.item(
                PopupMenuItem::new(if pinned { "Unpin session" } else { "Pin session" })
                    .icon(if pinned { Lucide::PinOff } else { Lucide::Pin })
                    .on_click(move |_, _, cx| {
                        pin.update(cx, |app, cx| app.toggle_pin(app.active, cx));
                    }),
            )
            .when(context_app.read(cx).settings.watch_enabled, |menu| {
                menu.item(watch_item(&context_app, chat_id, " session", cx))
            })
            .item(
                PopupMenuItem::new("Stop generating")
                    .icon(Lucide::Square)
                    .disabled(!busy)
                    .on_click(move |_, window, cx| stop.update(cx, |app, cx| app.stop(window, cx))),
            )
            .separator()
            .submenu_with_icon(
                Some(Icon::new(Lucide::ArrowDownUp)),
                "Jump to",
                window,
                cx,
                move |menu, _, _| {
                    let (first, latest) = (jump.clone(), jump.clone());
                    menu.item(
                        PopupMenuItem::new("First message")
                            .icon(Lucide::ArrowUpFromLine)
                            .disabled(!has_messages)
                            .on_click(move |_, _, cx| {
                                first.update(cx, |app, cx| {
                                    app.scroller.update(cx, |scroll, cx| {
                                        scroll.scroll_to_item(0, cx);
                                    });
                                });
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Latest message")
                            .icon(Lucide::ArrowDownToLine)
                            .disabled(!has_messages)
                            .on_click(move |_, _, cx| {
                                latest.update(cx, |app, cx| {
                                    app.scroller.update(cx, |scroll, cx| scroll.scroll_to_end(cx));
                                });
                            }),
                    )
                },
            )
            .submenu_with_icon(
                Some(Icon::new(Lucide::PanelRight)),
                "Split session",
                window,
                cx,
                move |mut menu, _, _| {
                    for (label, icon, (side, reason)) in [
                        ("Right", Lucide::PanelRight, split_sides[0]),
                        ("Left", Lucide::PanelLeft, split_sides[1]),
                        ("Down", Lucide::PanelBottom, split_sides[2]),
                        ("Up", Lucide::PanelTop, split_sides[3]),
                    ] {
                        let app = split.clone();
                        menu = menu.item(split_menu_item(label, reason).icon(icon).on_click(
                            move |_, window, cx| {
                                app.update(cx, |app, cx| app.split_session(pane, side, window, cx));
                            },
                        ));
                    }
                    menu
                },
            )
            .item(
                PopupMenuItem::new(if floats { "Dock session" } else { "Float session" })
                    .icon(if floats { Lucide::Minimize2 } else { Lucide::PictureInPicture2 })
                    .on_click(move |_, window, cx| {
                        let action = if floats { PaneAction::Dock } else { PaneAction::Float };
                        float.update(cx, |app, cx| {
                            app.run_session_pane_action(pane, action, window, cx)
                        });
                    }),
            )
            .separator()
            .item(
                PopupMenuItem::new("Terminal")
                    .icon(Lucide::SquareTerminal)
                    .disabled(!has_project)
                    .on_click(move |_, window, cx| {
                        terminal.update(cx, |app, cx| app.open_terminal(window, cx));
                    }),
            )
            .item(
                PopupMenuItem::new("Resume in terminal")
                    .icon(Lucide::SquareTerminal)
                    .disabled(!can_resume)
                    .on_click(move |_, window, cx| {
                        resume.update(cx, |app, cx| app.resume_in_terminal(app.active, window, cx));
                    }),
            )
            .item(
                PopupMenuItem::new("File Explorer")
                    .icon(Lucide::FolderOpen)
                    .disabled(!has_project)
                    .on_click(move |_, window, cx| {
                        explorer.update(cx, |app, cx| {
                            app.open_explorer_panel(window, cx);
                        });
                    }),
            )
        };
        let controls = self.render_pane_controls(pane, host, focused, cx);
        let is_tabbed = host == 0 && self.side_full && self.side_open();
        let show_pane_header = floating.is_some()
            || !is_tabbed
            || self.workspace.model.windows.get(&host).is_some_and(|w| w.panes().len() > 1);
        let pane_header = show_pane_header.then(|| {
            let mut header = if host == 0 && !is_tabbed {
                let layout = &self.workspace.model.windows[&host];
                let leading = super::render::leading_pane(layout);
                let inset = leading == Some(pane) && self.settings.sidebar_collapsed;
                let beside_rail = inset
                    && cfg!(target_os = "macos")
                    && !window.is_fullscreen()
                    && crate::side_panel::chat_body_in_sidebar(
                        self.settings.chat_layout,
                        self.side_open(),
                    );
                let lights = aether_ui::traffic_lights_gap(inset, window, cx)
                    .when(beside_rail, |gap| gap.w(px(40.)));
                let pop = focused.then(|| self.render_workspace_pop(window, cx)).flatten();
                let merged = self.render_header_with_pane(
                    pop,
                    controls,
                    leading == Some(pane),
                    focused,
                    host,
                    cx,
                );
                h_flex()
                    .id(("pane-header", pane as usize))
                    .group(gpui_kit::SharedString::from(format!("pane-header-{pane}")))
                    .h(px(32.))
                    .flex_none()
                    .bg(cx.theme().foreground.opacity(if focused { 0.035 } else { 0.015 }))
                    .border_b_1()
                    .border_color(cx.theme().foreground.opacity(0.055))
                    .child(lights)
                    .child(merged)
                    .when(!self.side_open(), |header| {
                        header.child(aether_ui::window_controls_gap(window))
                    })
            } else {
                header.child(controls)
            };
            if let Some(start) = floating {
                header = header.on_drag(
                    PaneDrag { pane, resize: false, start, origin: Default::default() },
                    move |d: &PaneDrag, offset, _, cx| {
                        d.origin
                            .set([f32::from(offset.x) + start.x, f32::from(offset.y) + start.y]);
                        cx.new(|_| d.clone())
                    },
                );
            }
            header
        });
        let mut content = v_flex()
            .id(("agent-pane", pane as usize))
            .relative()
            .size_full()
            .min_w_0()
            .min_h_0()
            .key_context("AetherSession")
            .when(floating.is_some(), |d| {
                d.corner(px(12.))
                    .overflow_hidden()
                    .border_1()
                    .border_color(cx.theme().foreground.opacity(if focused { 0.16 } else { 0.1 }))
                    .bg(cx.theme().background.alpha(1.))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |app, _, _, cx| {
                    app.workspace.model.focus(pane);
                    cx.notify();
                }),
            )
            .on_action(
                self.session_listener(cx, |app, _: &StopGenerating, window, cx| {
                    app.stop(window, cx)
                }),
            )
            .on_action(self.session_listener(cx, |app, _: &FocusComposer, window, cx| {
                app.focus_composer(window, cx)
            }))
            .on_action(self.session_listener(cx, |app, _: &PreviousSideTab, window, cx| {
                app.cycle_side_tab(true, window, cx)
            }))
            .on_action(self.session_listener(cx, |app, _: &NextSideTab, window, cx| {
                app.cycle_side_tab(false, window, cx)
            }))
            .on_action(self.session_listener(cx, move |app, _: &NextSessionPane, window, cx| {
                app.cycle_session(host, false, window, cx)
            }))
            .on_action(self.session_listener(
                cx,
                move |app, _: &PreviousSessionPane, window, cx| {
                    app.cycle_session(host, true, window, cx)
                },
            ))
            .on_action(self.session_listener(cx, move |app, _: &FocusSessionLeft, window, cx| {
                app.focus_session_direction(host, Direction::Left, window, cx)
            }))
            .on_action(self.session_listener(cx, move |app, _: &FocusSessionRight, window, cx| {
                app.focus_session_direction(host, Direction::Right, window, cx)
            }))
            .on_action(self.session_listener(cx, move |app, _: &FocusSessionUp, window, cx| {
                app.focus_session_direction(host, Direction::Up, window, cx)
            }))
            .on_action(self.session_listener(cx, move |app, _: &FocusSessionDown, window, cx| {
                app.focus_session_direction(host, Direction::Down, window, cx)
            }))
            .on_action(self.session_listener(cx, |app, _: &ToggleCommandMenu, window, cx| {
                app.toggle_palette(window, cx)
            }))
            .child(
                div().id("session-background-menu").absolute().inset_0().context_menu(session_menu),
            )
            .children(pane_header)
            .child(body);
        if let Some(editor) = self.render_draft_editor(cx) {
            content = content.child(
                div()
                    .id("session-draft-editor")
                    .absolute()
                    .inset_0()
                    .on_mouse_down(MouseButton::Right, block_session_menu)
                    .child(editor),
            );
        }
        if host != 0 && self.palette_open {
            content = content.child(
                div()
                    .id("session-palette")
                    .absolute()
                    .inset_0()
                    .on_mouse_down(MouseButton::Right, block_session_menu)
                    .child(self.render_palette(host, cx)),
            );
        }
        if let Some(start) = floating {
            content = content.child(
                div()
                    .id(("pane-resize", pane as usize))
                    .on_mouse_down(MouseButton::Right, block_session_menu)
                    .absolute()
                    .right_0()
                    .bottom_0()
                    .w(px(14.))
                    .h(px(14.))
                    .cursor_crosshair()
                    .on_drag(
                        PaneDrag { pane, resize: true, start, origin: Default::default() },
                        move |d: &PaneDrag, offset, _, cx| {
                            d.origin.set([
                                start.x + start.w - 14. + f32::from(offset.x),
                                start.y + start.h - 14. + f32::from(offset.y),
                            ]);
                            cx.new(|_| d.clone())
                        },
                    ),
            );
        }
        let measure = cx.entity();
        content = content.child(
            gpui_kit::canvas(
                move |bounds, _, cx| {
                    measure.update(cx, |app, cx| {
                        let size = [f32::from(bounds.size.width), f32::from(bounds.size.height)];
                        app.workspace.pane_rects.insert(
                            pane,
                            Rect::new(
                                f32::from(bounds.origin.x),
                                f32::from(bounds.origin.y),
                                size[0],
                                size[1],
                            ),
                        );
                        if app.workspace.bounds.insert(pane, size) != Some(size) {
                            cx.notify();
                        }
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        );
        let _ = window;
        content.into_any_element()
    }

    pub(crate) fn session_body(
        &mut self,
        scroller: ElementId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_pending_worktree_dialog(window, cx);
        self.sync_rows(None, cx);
        let row_view = self.session_handle(cx);
        let transcript = ChatScroller::new(scroller, self.scroller.clone(), move |row, _, cx| {
            row_view
                .update(cx, |app, cx| Some(app.render_row(row, &row_view, cx)))
                .map(|row_view| {
                    div().id(("session-row", row)).w_full().child(row_view).into_any_element()
                })
                .unwrap_or_else(|| div().into_any_element())
        })
        .scrollbar(false)
        .jump_button(false)
        .with_bottom_fade(cx.theme().background)
        .flex_1();
        let composer = div()
            .id("session-composer")
            .w_full()
            .flex_none()
            .on_mouse_down(MouseButton::Right, block_session_menu)
            .child(self.render_composer(cx))
            .into_any_element();
        if self.chats[self.active].messages.is_empty() {
            self.render_start(composer, cx)
        } else {
            let on_wheel = self.session_listener(cx, |this, _: &ScrollWheelEvent, window, cx| {
                this.hide_selection_bar(cx);
                this.unfocus_composer(window, cx)
            });
            let on_key = self.session_listener(cx, |this, ev: &KeyDownEvent, _, cx| {
                if ev.keystroke.key == "escape" {
                    this.hide_selection_bar(cx)
                }
            });
            v_flex()
                .flex_1()
                .min_h_0()
                .child(
                    v_flex()
                        .flex_1()
                        .min_h_0()
                        .debug_selector(|| "transcript".into())
                        .on_scroll_wheel(on_wheel)
                        .on_key_down(on_key)
                        .child(transcript)
                        .children(self.render_selection_bar(cx)),
                )
                .child(composer)
                .into_any_element()
        }
    }

    pub(crate) fn render_pane_controls(
        &self,
        pane: PaneId,
        host: WindowId,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let app = cx.entity();
        let floating = self
            .workspace
            .model
            .windows
            .get(&host)
            .and_then(|w| w.floats.iter().find(|f| f.pane == pane))
            .map(|f| f.bounds);
        let header_group = gpui_kit::SharedString::from(format!("pane-header-{pane}"));
        let mut controls = h_flex()
            .id(("pane-controls", pane as usize))
            .on_mouse_down(MouseButton::Right, block_session_menu)
            .flex_none()
            .gap_0p5()
            .opacity(if focused { 0.8 } else { 0.4 })
            .group_hover(header_group, |s| s.opacity(1.));
        controls = controls.when(floating.is_some(), |controls| {
            controls.child(
                aether_ui::button(("pane-dock", pane as usize))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Minimize)
                    .accessibility_label("Dock session")
                    .tooltip("Dock session")
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.run_session_pane_action(pane, PaneAction::Dock, window, cx)
                    })),
            )
        });
        let dock_targets: Vec<_> = self
            .workspace
            .model
            .windows
            .get(&host)
            .and_then(|w| w.root.as_ref())
            .map(|r| r.leaves())
            .unwrap_or_default()
            .into_iter()
            .filter(|target| *target != pane)
            .filter_map(|target| {
                self.workspace
                    .model
                    .panes
                    .get(&target)
                    .and_then(|id| self.chats.iter().find(|c| c.id == *id))
                    .map(|chat| (target, chat.title.clone()))
            })
            .collect();
        let available_sessions: Vec<_> = self
            .chats
            .iter()
            .filter(|chat| !chat.archived && self.workspace.model.pane_for(chat.id).is_none())
            .map(|chat| (chat.id, chat.title.clone()))
            .collect();
        let menu_app = app.clone();
        let surface = self.workspace.surface_sizes.get(&host).copied();
        let gap = self.settings.tile_gap.clamp(0., 24.);
        let split_sides = [DockSide::Right, DockSide::Left, DockSide::Bottom, DockSide::Top]
            .map(|side| (side, self.workspace.model.can_split_in(pane, side, surface, gap).err()));
        let can_close =
            self.workspace.model.windows.get(&host).is_some_and(|layout| layout.panes().len() > 1);
        let maximized = self
            .workspace
            .model
            .windows
            .get(&host)
            .is_some_and(|layout| layout.maximized == Some(pane));
        let layout_menu = move |mut menu: PopupMenu,
                                window: &mut Window,
                                cx: &mut Context<PopupMenu>| {
            if host == 0 {
                let popup = cx.entity();
                menu_app.update(cx, |app, cx| {
                    app.browser.update(cx, |browser, cx| browser.hide_for_menu(&popup, cx));
                });
            }
            let max = menu_app.clone();
            for (label, (side, reason)) in [
                ("Split session right", split_sides[0]),
                ("Split session left", split_sides[1]),
                ("Split session down", split_sides[2]),
                ("Split session up", split_sides[3]),
            ] {
                let app = menu_app.clone();
                menu = menu.item(split_menu_item(label, reason).on_click(move |_, window, cx| {
                    app.update(cx, |app, cx| app.split_session(pane, side, window, cx));
                }));
            }
            for (label, action, enabled) in [
                (
                    if floating.is_some() { "Dock session" } else { "Float session" },
                    if floating.is_some() { PaneAction::Dock } else { PaneAction::Float },
                    true,
                ),
                (
                    if host == 0 { "Move to desktop window" } else { "Return to main window" },
                    if host == 0 { PaneAction::Detach } else { PaneAction::Return },
                    true,
                ),
            ] {
                let app = menu_app.clone();
                menu = menu.item(PopupMenuItem::new(label).disabled(!enabled).on_click(
                    move |_, window, cx| {
                        app.update(cx, |app, cx| {
                            app.run_session_pane_action(pane, action, window, cx)
                        });
                    },
                ));
            }
            let mut menu = menu.separator().item(
                PopupMenuItem::new(if maximized { "Restore pane" } else { "Maximize pane" })
                    .on_click(move |_, _, cx| {
                        max.update(cx, |app, cx| {
                            if let Some(w) = app.workspace.model.windows.get_mut(&host) {
                                w.maximized =
                                    if w.maximized == Some(pane) { None } else { Some(pane) };
                            }
                            cx.notify();
                        });
                    }),
            );
            let docking = menu_app.clone();
            menu = menu.submenu("Dock position", window, cx, move |mut menu, _, _| {
                for (label, side) in [
                    ("Dock left", DockSide::Left),
                    ("Dock right", DockSide::Right),
                    ("Dock above", DockSide::Top),
                    ("Dock below", DockSide::Bottom),
                ] {
                    let app = docking.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                        app.update(cx, |app, cx| {
                            let _ = app.workspace.model.dock(pane, None, side);
                            cx.notify();
                        });
                    }));
                }
                menu
            });
            if !available_sessions.is_empty() {
                let sessions = available_sessions.clone();
                let app = menu_app.clone();
                menu = menu.submenu("Open session beside…", window, cx, move |mut menu, _, _| {
                    for (id, title) in &sessions {
                        let id = *id;
                        let app = app.clone();
                        menu = menu.item(PopupMenuItem::new(title.clone()).on_click(
                            move |_, window, cx| {
                                app.update(cx, |app, cx| {
                                    app.split_existing_session(
                                        pane,
                                        id,
                                        SplitAxis::Horizontal,
                                        window,
                                        cx,
                                    )
                                });
                            },
                        ));
                    }
                    menu
                });
            }
            for (target, title) in &dock_targets {
                let target = *target;
                let app = menu_app.clone();
                menu = menu.submenu(
                    format!("Dock beside {title}"),
                    window,
                    cx,
                    move |mut menu, _, _| {
                        for (label, side) in [
                            ("Left", DockSide::Left),
                            ("Right", DockSide::Right),
                            ("Above", DockSide::Top),
                            ("Below", DockSide::Bottom),
                        ] {
                            let app = app.clone();
                            menu =
                                menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                                    app.update(cx, |app, cx| {
                                        if let Err(e) =
                                            app.workspace.model.dock(pane, Some(target), side)
                                        {
                                            app.workspace.error = Some(e.into());
                                        }
                                        cx.notify();
                                    });
                                }));
                        }
                        menu
                    },
                );
            }
            let close = menu_app.clone();
            menu.separator().item(PopupMenuItem::new("Close pane").disabled(!can_close).on_click(
                move |_, window, cx| {
                    close.update(cx, |app, cx| {
                        let multiple = app
                            .workspace
                            .model
                            .windows
                            .get(&host)
                            .is_some_and(|w| w.panes().len() > 1);
                        if multiple {
                            app.run_session_pane_action(pane, PaneAction::Close, window, cx);
                        }
                    });
                },
            ))
        };
        controls = controls.child(
            aether_ui::button(("pane-more", pane as usize))
                .ghost()
                .xsmall()
                .accessibility_label("Session pane actions")
                .icon(IconName::Ellipsis)
                .tooltip("Session pane actions")
                .dropdown_menu(layout_menu),
        );
        controls = controls.when(can_close, |controls| {
            controls.child(
                aether_ui::button(("pane-close", pane as usize))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .accessibility_label("Close session pane")
                    .tooltip("Close pane; keep agent running")
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.run_session_pane_action(pane, PaneAction::Close, window, cx)
                    })),
            )
        });
        controls.into_any_element()
    }
}

impl ChatApp {
    /// Running background count, and the panel beside the chat unless hidden.
    pub(crate) fn background_panel(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (usize, Option<AnyElement>) {
        let cwd = self.transcript_cwd();
        let chat = &self.chats[self.active];
        let (id, key, session) = (chat.id, chat.id.to_string(), chat.session.clone());
        let opened = aether_ui::opened_tools(cx, &session);
        let steps: Vec<&Step> = chat.messages.iter().flat_map(|m| &m.steps).collect();
        let stopper = self.session_handle(cx).downgrade();
        let row = |call: &ToolCall| -> AnyElement {
            let tool = aether_ui::tool_key(&session, &call.id);
            match call.detail {
                ToolDetail::Task { .. } => {
                    let (stopper, chat_key, id) = (stopper.clone(), key.clone(), call.id.clone());
                    let stop: aether_ui::OnOpen = Rc::new(move |window, cx| {
                        stopper
                            .update(cx, |this, cx| {
                                this.agent.stop_tool(&chat_key, &id);
                                this.focus_composer(window, cx);
                            })
                            .ok();
                    });
                    SubagentCard::new(tool, call.clone(), descendants(&steps, &call.id))
                        .session(session.clone())
                        .cwd(cwd.clone())
                        .on_stop(Some(stop))
                        .into_any_element()
                }
                _ => ToolRow::new(tool, call.clone()).cwd(cwd.clone()).into_any_element(),
            }
        };
        let items: Vec<_> = chat
            .live_background()
            .map(|call| {
                let (agent, tools, name) = match &call.detail {
                    ToolDetail::Task { agent, tools, description, .. } => (
                        true,
                        *tools,
                        if description.is_empty() { agent.clone() } else { description.clone() },
                    ),
                    detail => (false, 0, detail.summary()),
                };
                BackgroundItem {
                    id: call.id.clone(),
                    agent,
                    name,
                    tools,
                    started_ms: call.started_ms,
                    body: opened.contains(&call.id).then(|| row(call)),
                }
            })
            .collect();
        let running =
            self.inspect.as_ref().filter(|(chat, _)| *chat == id).and_then(|(_, tool)| {
                steps.iter().find_map(|s| match s {
                    Step::Tool(call) if &call.id == tool => Some(row(call)),
                    _ => None,
                })
            });
        let queued = self
            .inspect_queued
            .as_ref()
            .filter(|(chat, _)| *chat == id)
            .and_then(|(_, text)| chat.queue.iter().find(|q| &q.text == text))
            .map(|q| {
                let views = q
                    .attachments
                    .iter()
                    .map(|a| AttachmentView {
                        name: a.name.clone().into(),
                        path: a.path.clone(),
                        image: aether_core::is_image(&a.mime),
                        size: a.size,
                        missing: false,
                    })
                    .collect::<Vec<_>>();
                let app = cx.entity();
                let open: Rc<dyn Fn(&std::path::Path, &mut Window, &mut gpui_kit::App)> =
                    Rc::new(move |path, window, cx| {
                        if !app.update(cx, |app, cx| app.preview_image(path, window, cx)) {
                            cx.open_with_system(path);
                        }
                    });
                v_flex()
                    .px(px(8.))
                    .gap_2()
                    .text_sm()
                    .child(div().whitespace_normal().child(q.text.clone()))
                    .when(!views.is_empty(), |v| {
                        v.child(attachment_strip(views, px(56.), Some(open), None, false, cx))
                    })
                    .into_any_element()
            });
        let steered =
            self.inspect_steer.as_ref().filter(|(chat, _)| *chat == id).map(|(_, text)| {
                v_flex()
                    .px(px(8.))
                    .text_sm()
                    .child(div().whitespace_normal().child(text.clone()))
                    .into_any_element()
            });
        let count = items.len();
        if count == 0 && running.is_none() && queued.is_none() && steered.is_none() {
            self.background_hidden.remove(&id);
            return (0, None);
        }
        if self.background_hidden.contains(&id) {
            return (count, None);
        }
        let app = self.session_handle(cx).downgrade();
        let (open, stop, close) = (app.clone(), app.clone(), app);
        let (resize, resize_end) = (cx.entity(), cx.entity());
        let width = self.settings.background_panel_width.clamp(BG_MIN_WIDTH, BG_MAX_WIDTH);
        let panel = BackgroundPanel::new(
            items,
            crate::app::now_ms(),
            move |tool, _, cx| {
                aether_ui::open_tool(cx, aether_ui::tool_key(&session, tool));
                open.update(cx, |_, cx| cx.notify()).ok();
            },
            move |tool, window, cx| {
                stop.update(cx, |this, cx| {
                    this.agent.stop_tool(&key, tool);
                    this.focus_composer(window, cx);
                })
                .ok();
            },
            move |_, cx| {
                close
                    .update(cx, |this, cx| {
                        this.background_hidden.insert(id);
                        this.inspect = None;
                        this.inspect_queued = None;
                        this.inspect_steer = None;
                        cx.notify();
                    })
                    .ok();
            },
        )
        .section("Running now", running)
        .section("Queued message", queued)
        .section("Your message", steered)
        .resizable(
            width,
            move |width, _, cx| {
                resize.update(cx, |this, cx| {
                    this.background_resizing = true;
                    this.settings.background_panel_width = width.clamp(BG_MIN_WIDTH, BG_MAX_WIDTH);
                    cx.notify();
                })
            },
            move |_, cx| {
                resize_end.update(cx, |this, _| {
                    if std::mem::take(&mut this.background_resizing) {
                        crate::settings::save(&this.settings);
                    }
                })
            },
        );
        (count, Some(panel.into_any_element()))
    }
}

const BG_MIN_WIDTH: f32 = 260.;
const BG_MAX_WIDTH: f32 = 720.;

/// A task's rows, nested ones included; parents arrive before their children.
fn descendants(steps: &[&Step], root: &str) -> Vec<ToolCall> {
    let mut ids = vec![root];
    let mut rows = Vec::new();
    for step in steps {
        if let Step::Tool(call) = step
            && call.parent.as_deref().is_some_and(|p| ids.contains(&p))
        {
            ids.push(&call.id);
            rows.push(call.clone());
        }
    }
    rows
}

impl ChatApp {
    /// Top-bar chip that shows or hides the active chat's Background panel while tasks run.
    pub(crate) fn background_toggle(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let chat = &self.chats[self.active];
        let count = chat.live_background().count();
        if count == 0 {
            return None;
        }
        let id = chat.id;
        let shown = !self.background_hidden.contains(&id);
        let tip = format!("Background · {count} running");
        Some(
            aether_ui::icon_chip("background-toggle", shown, cx)
                .selected(shown)
                .toggled(shown)
                .icon(Lucide::PanelRight)
                .child(
                    div()
                        .absolute()
                        .top(px(4.))
                        .right(px(4.))
                        .size(px(6.))
                        .rounded_full()
                        .bg(aether_ui::theme::info_text(cx)),
                )
                .accessibility_label(tip.clone())
                .tooltip(tip)
                .on_click(self.session_listener(cx, move |this, _, _, cx| {
                    if !this.background_hidden.remove(&id) {
                        this.background_hidden.insert(id);
                    }
                    cx.notify();
                }))
                .into_any_element(),
        )
    }
}

fn split_menu_item(label: &'static str, blocked: Option<&'static str>) -> PopupMenuItem {
    let Some(reason) = blocked else { return PopupMenuItem::new(label) };
    PopupMenuItem::element(move |_, _| {
        div().id(label).child(label).tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(reason).build(window, cx)
        })
    })
    .disabled(true)
}
