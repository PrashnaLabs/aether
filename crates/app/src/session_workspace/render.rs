use super::*;
use crate::actions::*;
use crate::app::ChatApp;
use aether_core::{ChatLayout, TileLayout};
use aether_ui::theme::Corners;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::component::{ActiveTheme, IconName, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, AppContext, Context, DragMoveEvent, Entity, MouseButton, Render, Window, div,
    prelude::*, px, relative,
};

pub(crate) fn next_chat_layout(layout: ChatLayout) -> ChatLayout {
    match layout {
        ChatLayout::Tabbed => ChatLayout::Sidebar,
        ChatLayout::Sidebar => ChatLayout::Split,
        ChatLayout::Split => ChatLayout::Tabbed,
    }
}

pub(crate) fn chat_layout_allows_monocle(layout: ChatLayout) -> bool {
    layout != ChatLayout::Split
}

#[derive(Clone)]
pub(super) struct PaneDrag {
    pub(super) pane: PaneId,
    pub(super) resize: bool,
    pub(super) start: Rect,
    pub(super) origin: std::rc::Rc<std::cell::Cell<[f32; 2]>>,
}
impl Render for PaneDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::Empty
    }
}
#[derive(Clone, Copy)]
struct DividerDrag(u64, WindowId, SplitAxis);
impl Render for DividerDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::Empty
    }
}

pub(crate) struct SessionPane {
    app: gpui_kit::WeakEntity<ChatApp>,
    pane: PaneId,
    chat: i64,
    window: WindowId,
    _subscription: gpui_kit::Subscription,
}
impl SessionPane {
    fn new(
        app: Entity<ChatApp>,
        pane: PaneId,
        chat: i64,
        window: WindowId,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe(&app, |_, _, cx| cx.notify());
        Self { app: app.downgrade(), pane, chat, window, _subscription: subscription }
    }
}
impl Render for SessionPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (pane, chat, host) = (self.pane, self.chat, self.window);
        self.app
            .update(cx, |app, cx| {
                app.ensure_session_ui(chat, window, cx);
                if window.is_window_active() {
                    app.acknowledge_session(chat, cx);
                }
                app.in_session(chat, cx, |app, cx| app.render_session_pane(pane, host, window, cx))
                    .unwrap_or_else(|| div().into_any_element())
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}
impl ChatApp {
    pub(crate) fn render_session_workspace(
        &mut self,
        host: WindowId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let available = self.workspace.surface_sizes.get(&host).copied().unwrap_or([
            f32::from(window.viewport_size().width),
            f32::from(window.viewport_size().height),
        ]);
        let tiling = self.settings.tile_layout;
        self.workspace.model.retile(host, tiling, self.settings.tile_ratio.clamp(0.3, 0.8));
        let mut tabs = Vec::new();
        if let Some(layout) = self.workspace.model.windows.get_mut(&host) {
            for float in &mut layout.floats {
                float.bounds = float.bounds.sanitize(available);
            }
            if tiling == TileLayout::Monocle
                && chat_layout_allows_monocle(self.settings.chat_layout)
                && layout.maximized.is_none()
            {
                tabs = layout.root.as_ref().map(DockNode::leaves).unwrap_or_default();
            }
            layout.shown = [layout.focused, layout.shown]
                .into_iter()
                .flatten()
                .find(|p| tabs.contains(p))
                .or(tabs.first().copied());
        }
        let layout = self.workspace.model.windows.get(&host).cloned().unwrap_or_default();
        let solo = layout.maximized.or(layout.shown);
        let canvas = if let Some(pane) = solo {
            let pane = self.session_pane_element(pane, host, cx);
            let hoisted = host == 0
                && self.side_full
                && self.side_open()
                && chat_layout_allows_monocle(self.settings.chat_layout);
            if tabs.len() > 1 && !hoisted {
                v_flex()
                    .size_full()
                    .child(self.session_tabs_element(&tabs, solo, cx))
                    .child(div().flex_1().min_h_0().child(pane))
                    .into_any_element()
            } else {
                pane
            }
        } else if let Some(root) = &layout.root {
            self.session_split_element(root, host, cx)
        } else {
            div()
                .flex_1()
                .p_4()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Open a session from the sidebar or create a new session.")
                .into_any_element()
        };
        let minimum = if solo.is_some() {
            [320., 240.]
        } else {
            layout.root.as_ref().map(|r| r.minimum()).unwrap_or([320., 240.])
        };
        let canvas =
            div().id(("session-dock-scroll", host as usize)).size_full().overflow_scroll().child(
                div().w_full().h_full().min_w(px(minimum[0])).min_h(px(minimum[1])).child(canvas),
            );
        let app = cx.entity();
        let mut body = div()
            .id(("session-workspace", host as usize))
            .key_context("SessionWorkspace")
            .relative()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .on_action(cx.listener(|app, _: &CycleTileLayout, _, cx| app.cycle_tile_layout(cx)))
            .on_action(cx.listener(|app, _: &CycleChatLayout, _, cx| {
                app.update_settings(cx, |s| s.chat_layout = next_chat_layout(s.chat_layout))
            }))
            .on_action(cx.listener(move |app, _: &SwapSessionLeft, _, cx| {
                app.swap_session(host, Direction::Left, cx)
            }))
            .on_action(cx.listener(move |app, _: &SwapSessionRight, _, cx| {
                app.swap_session(host, Direction::Right, cx)
            }))
            .on_action(cx.listener(move |app, _: &SwapSessionUp, _, cx| {
                app.swap_session(host, Direction::Up, cx)
            }))
            .on_action(cx.listener(move |app, _: &SwapSessionDown, _, cx| {
                app.swap_session(host, Direction::Down, cx)
            }))
            .on_action(cx.listener(move |app, _: &PromoteSession, window, cx| {
                app.focused_pane_action(host, PaneAction::Promote, window, cx)
            }))
            .on_action(cx.listener(move |app, _: &ToggleSessionFloat, window, cx| {
                app.focused_pane_action(host, PaneAction::Float, window, cx)
            }))
            .on_action(
                cx.listener(|app, _: &NarrowMainPane, _, cx| app.nudge_tile_ratio(-0.05, cx)),
            )
            .on_action(cx.listener(|app, _: &WidenMainPane, _, cx| app.nudge_tile_ratio(0.05, cx)))
            .on_drag_move(cx.listener(move |app, event: &DragMoveEvent<PaneDrag>, _, cx| {
                let drag = event.drag(cx).clone();
                let dx = f32::from(event.event.position.x - event.bounds.origin.x)
                    - drag.origin.get()[0];
                let dy = f32::from(event.event.position.y - event.bounds.origin.y)
                    - drag.origin.get()[1];
                let size =
                    [f32::from(event.bounds.size.width), f32::from(event.bounds.size.height)];
                if let Some(layout) = app.workspace.model.windows.get_mut(&host)
                    && let Some(float) = layout.floats.iter_mut().find(|p| p.pane == drag.pane)
                {
                    let mut rect = drag.start;
                    if drag.resize {
                        rect.w += dx;
                        rect.h += dy;
                    } else {
                        rect.x += dx;
                        rect.y += dy;
                    }
                    float.bounds = rect.sanitize(size);
                    cx.notify();
                }
            }))
            .child(canvas);
        if layout.maximized.is_none() {
            let mut floats = layout.floats.clone();
            floats.sort_by_key(|f| layout.stack.iter().position(|p| *p == f.pane).unwrap_or(0));
            for float in floats {
                let bounds = float.bounds.sanitize(available);
                let pane = float.pane;
                let content = self.session_pane_element(pane, host, cx);
                body = body.child(
                    div()
                        .absolute()
                        .left(px(bounds.x))
                        .top(px(bounds.y))
                        .w(px(bounds.w))
                        .h(px(bounds.h))
                        .corner(px(12.))
                        .shadow_lg()
                        .child(content),
                );
            }
        }
        if let Some(error) = self.workspace.error.as_ref().or(self.workspace.save_error.as_ref()) {
            body = body.child(
                h_flex()
                    .absolute()
                    .bottom_2()
                    .left_2()
                    .right_2()
                    .p_2()
                    .gap_2()
                    .bg(cx.theme().background.alpha(1.))
                    .border_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .child(div().flex_1().child(error.clone()))
                    .child(
                        aether_ui::button("dismiss-pane-error")
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .on_click(move |_, _, cx| {
                                app.update(cx, |app, cx| {
                                    app.workspace.error = None;
                                    app.workspace.save_error = None;
                                    cx.notify();
                                });
                            }),
                    ),
            );
        }
        let measure = cx.entity();
        body = body.child(
            gpui_kit::canvas(
                move |bounds, _, cx| {
                    let size = [f32::from(bounds.size.width), f32::from(bounds.size.height)];
                    measure.update(cx, |app, cx| {
                        if app.workspace.surface_sizes.insert(host, size) != Some(size) {
                            cx.notify();
                        }
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        );
        body.into_any_element()
    }
    fn session_pane_element(
        &mut self,
        pane: PaneId,
        host: WindowId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(chat) = self.workspace.model.panes.get(&pane).copied() else {
            return div().into_any_element();
        };
        let app = cx.entity();
        let view = self
            .workspace
            .views
            .entry(pane)
            .or_insert_with(|| cx.new(|cx| SessionPane::new(app, pane, chat, host, cx)))
            .clone();
        div().size_full().min_w_0().min_h_0().child(view).into_any_element()
    }
    /// Monocle tabs drawn above the main window's tab bar instead of inside the workspace.
    pub(crate) fn hoisted_session_tabs(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let layout = self.workspace.model.windows.get(&0)?;
        if self.settings.tile_layout != TileLayout::Monocle
            || !chat_layout_allows_monocle(self.settings.chat_layout)
            || layout.maximized.is_some()
        {
            return None;
        }
        let tabs = layout.root.as_ref()?.leaves();
        (tabs.len() > 1).then(|| self.session_tabs_element(&tabs, layout.shown, cx))
    }
    fn session_tabs_element(
        &self,
        tabs: &[PaneId],
        shown: Option<PaneId>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let app = cx.entity();
        h_flex()
            .id("session-tabs")
            .flex_none()
            .h(px(32.))
            .gap_1()
            .px_2()
            .overflow_x_scroll()
            .border_b_1()
            .border_color(cx.theme().foreground.opacity(0.055))
            .children(tabs.iter().map(|&pane| {
                let chat = self.workspace.model.panes.get(&pane).copied();
                let title = chat
                    .and_then(|id| self.chats.iter().find(|c| c.id == id))
                    .map(|c| c.title.clone())
                    .unwrap_or_default();
                let button = aether_ui::button(("session-tab", pane as usize))
                    .ghost()
                    .xsmall()
                    .selected(shown == Some(pane))
                    .max_w(px(224.))
                    .label(title)
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.workspace.model.focus(pane);
                        if let Some(id) = chat {
                            app.in_session(id, cx, |app, cx| app.focus_composer(window, cx));
                        }
                        cx.notify();
                    }));
                let app = app.clone();
                div().id(("session-tab-wrap", pane as usize)).child(button).context_menu(
                    move |menu, _, cx| match chat {
                        Some(id) if app.read(cx).settings.watch_enabled => {
                            let popup = cx.entity();
                            app.update(cx, |app, cx| {
                                app.browser.update(cx, |b, cx| b.hide_for_menu(&popup, cx))
                            });
                            menu.item(crate::app::watch_item(&app, id, " session", cx))
                        }
                        _ => menu,
                    },
                )
            }))
            .into_any_element()
    }
    fn session_split_element(
        &mut self,
        node: &DockNode,
        host: WindowId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match node {
            DockNode::Leaf(pane) => self.session_pane_element(*pane, host, cx),
            DockNode::Split { id, axis, ratio, first, second } => {
                let (id, axis, ratio) = (*id, *axis, *ratio);
                let gap = self.settings.tile_gap.clamp(0., 24.);
                let a = self.session_split_element(first, host, cx);
                let b = self.session_split_element(second, host, cx);
                let min_a = first.minimum();
                let min_b = second.minimum();
                let layout = if axis == SplitAxis::Horizontal { h_flex() } else { v_flex() };
                let divider = div()
                    .id(("session-divider", id as usize))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|s| s.bg(cx.theme().foreground.opacity(0.08)))
                    .child(div().bg(cx.theme().foreground.opacity(0.09)).map(|d| {
                        if axis == SplitAxis::Horizontal {
                            d.w(px(1.)).h_full()
                        } else {
                            d.h(px(1.)).w_full()
                        }
                    }))
                    .map(|d| {
                        if axis == SplitAxis::Horizontal {
                            d.w(px(DIVIDER + gap)).h_full().cursor_col_resize()
                        } else {
                            d.h(px(DIVIDER + gap)).w_full().cursor_row_resize()
                        }
                    })
                    .on_drag(DividerDrag(id, host, axis), |d: &DividerDrag, _, _, cx| {
                        cx.new(|_| *d)
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, e: &gpui_kit::MouseDownEvent, _, cx| {
                            if e.click_count == 2 {
                                if let Some(root) = app
                                    .workspace
                                    .model
                                    .windows
                                    .get_mut(&host)
                                    .and_then(|w| w.root.as_mut())
                                {
                                    root.set_ratio(id, 0.5);
                                }
                                cx.notify();
                            }
                        }),
                    );
                layout
                    .id(("session-split", id as usize))
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .on_drag_move(cx.listener(move |app, e: &DragMoveEvent<DividerDrag>, _, cx| {
                        let drag = e.drag(cx);
                        if drag.0 != id || drag.1 != host || drag.2 != axis {
                            return;
                        }
                        let (pos, total, a, b) = if axis == SplitAxis::Horizontal {
                            (
                                f32::from(e.event.position.x - e.bounds.origin.x),
                                f32::from(e.bounds.size.width),
                                min_a[0],
                                min_b[0],
                            )
                        } else {
                            (
                                f32::from(e.event.position.y - e.bounds.origin.y),
                                f32::from(e.bounds.size.height),
                                min_a[1],
                                min_b[1],
                            )
                        };
                        if let Some((first, _)) = split_sizes(total, a, b, pos / total) {
                            if let Some(root) = app
                                .workspace
                                .model
                                .windows
                                .get_mut(&host)
                                .and_then(|w| w.root.as_mut())
                            {
                                root.set_ratio(id, first / (total - DIVIDER));
                            }
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .flex_basis(relative(ratio))
                            .flex_shrink(1.)
                            .min_w(px(min_a[0]))
                            .min_h(px(min_a[1]))
                            .map(|d| {
                                if axis == SplitAxis::Horizontal { d.h_full() } else { d.w_full() }
                            })
                            .child(a),
                    )
                    .child(divider)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(min_b[0]))
                            .min_h(px(min_b[1]))
                            .map(|d| {
                                if axis == SplitAxis::Horizontal { d.h_full() } else { d.w_full() }
                            })
                            .child(b),
                    )
                    .into_any_element()
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/session_workspace/render.rs"]
mod tests;
