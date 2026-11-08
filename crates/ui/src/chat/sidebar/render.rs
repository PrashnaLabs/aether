use std::{rc::Rc, time::Duration};

use gpui_kit::base::motion::{Transition, transition};
use gpui_kit::base::{HandleEdge, resize_handle};
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, StyledExt, button::ButtonVariants, h_flex,
    resizable::resize_handle_appearance, v_flex,
};
use gpui_kit::{
    App, Axis, DragMoveEvent, ElementId, MouseButton, SharedString, Window, div, linear_color_stop,
    linear_gradient, list, prelude::*, px, rems,
};

const FADE_HEIGHT: f32 = 2.;
const FADE_TRANSITION: Duration = Duration::from_millis(200);

fn fade(color: gpui_kit::Hsla, angle: f32, opacity: f32) -> gpui_kit::Div {
    div().h(rems(FADE_HEIGHT)).opacity(opacity).bg(linear_gradient(
        angle,
        linear_color_stop(color.opacity(0.), 0.),
        linear_color_stop(color, 1.),
    ))
}

use super::ChatSidebar;
use super::rows::{ChatRow, chat_key, count_chip};
use super::state::SidebarState;
use crate::components::BrandMark;

#[derive(Clone)]
struct SidebarResize;

impl Render for SidebarResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::Empty
    }
}

impl RenderOnce for ChatSidebar {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state =
            window.use_keyed_state("chat-sidebar", cx, |window, _| SidebarState::new(window));
        self.collapsed_crews = state.read(cx).collapsed_crews.clone();
        let rows = self.rows();
        let keys: Vec<SharedString> = rows.iter().map(ChatRow::key).collect();
        let focusable: Vec<bool> = rows.iter().map(ChatRow::focusable).collect();
        let shown = rows.iter().find_map(|row| match row {
            ChatRow::Chat { chat, rename_editor, .. }
                if rename_editor.is_some() || (chat.active && self.rename_ix.is_none()) =>
            {
                Some(chat_key(chat.id))
            }
            _ => None,
        });
        state.update(cx, |s, cx| {
            s.sync(keys.clone(), &focusable, cx);
            s.reveal(shown);
        });
        let list_state = state.read(cx).list.clone();
        let offset = list_state.scroll_px_offset_for_scrollbar().y;
        let max = list_state.max_offset_for_scrollbar().y;
        let top_fade = transition(
            ("chat-sidebar", "top-fade"),
            if offset < px(0.) { 1. } else { 0. },
            Transition::new(FADE_TRANSITION),
            window,
            cx,
        );
        let bottom_fade = transition(
            ("chat-sidebar", "bottom-fade"),
            if offset + max > px(0.) { 1. } else { 0. },
            Transition::new(FADE_TRANSITION),
            window,
            cx,
        );
        let count = rows.len();
        let rows = Rc::new(rows);
        let body = list(list_state.clone(), {
            let state = state.clone();
            move |ix, window, cx| {
                let Some(row) = rows.get(ix).cloned() else {
                    return div().into_any_element();
                };
                let key = keys[ix].clone();
                div()
                    .id(ElementId::Name(key.clone()))
                    .when(ix == 0, |el| el.pt_3())
                    .when(ix + 1 == count, |el| el.pb_3())
                    .child(row.render(key, &state, window, cx))
                    .into_any_element()
            }
        })
        .size_full();
        let count = self.count.map(|n| count_chip(n, cx));
        let theme = cx.theme();
        let (sidebar_bg, sidebar_fg, border) =
            (theme.tokens.sidebar, theme.sidebar_foreground, theme.sidebar_border);
        v_flex()
            .id("sidebar")
            .flex_shrink_0()
            .h_full()
            .relative()
            .w(px(self.width))
            .overflow_hidden()
            .when_some(self.on_resize.clone(), |this, on_resize| {
                this.on_drag_move(move |e: &DragMoveEvent<Rc<SidebarResize>>, window, cx| {
                    on_resize(f32::from(e.event.position.x - e.bounds.left()), window, cx)
                })
            })
            .when_some(self.on_resize_end.clone(), |this, end| {
                let up = end.clone();
                this.capture_any_mouse_up(move |_, window, cx| up(window, cx))
                    .on_mouse_up_out(MouseButton::Left, move |_, window, cx| end(window, cx))
            })
            .when(self.on_resize.is_some(), |this| {
                this.child(
                    resize_handle::<SidebarResize, SidebarResize>(
                        "sidebar-resize",
                        Axis::Horizontal,
                    )
                    .inside(HandleEdge::Trailing)
                    .with_appearance(resize_handle_appearance())
                    .on_drag(SidebarResize, |_: Rc<SidebarResize>, _, _, cx| {
                        cx.new(|_| SidebarResize)
                    }),
                )
            })
            .bg(sidebar_bg)
            .text_color(sidebar_fg)
            .border_r_1()
            .border_color(border)
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .when(
                        !cfg!(any(target_os = "macos", target_os = "linux", target_os = "windows")),
                        |header| header.pt_3().px_3(),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .p_1()
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .gap_2()
                                    .items_center()
                                    .font_semibold()
                                    .children(self.mark.map(BrandMark::new))
                                    .child(div().truncate().child(self.title))
                                    .children(count),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(
                                        crate::components::button("new-chat")
                                            .ghost()
                                            .small()
                                            .icon(IconName::Plus)
                                            .accessibility_label("New chat")
                                            .map(|b| match &self.new_chat_action {
                                                Some(action) => b.tooltip_with_action(
                                                    "New chat",
                                                    action.as_ref(),
                                                    None,
                                                ),
                                                None => b.tooltip("New chat"),
                                            })
                                            .when_some(self.on_new_chat, |b, handler| {
                                                b.on_click(handler)
                                            }),
                                    )
                                    .when_some(self.on_collapse, |flex, handler| {
                                        flex.child(
                                            crate::components::button("collapse-sidebar")
                                                .ghost()
                                                .small()
                                                .icon(IconName::PanelLeftClose)
                                                .accessibility_label("Collapse sidebar")
                                                .map(|b| match &self.collapse_action {
                                                    Some(action) => b.tooltip_with_action(
                                                        "Collapse sidebar",
                                                        action.as_ref(),
                                                        None,
                                                    ),
                                                    None => b.tooltip("Collapse sidebar"),
                                                })
                                                .on_click(handler),
                                        )
                                    }),
                            ),
                    )
                    .map(|header| {
                        if cfg!(any(
                            target_os = "macos",
                            target_os = "linux",
                            target_os = "windows"
                        )) {
                            gpui_kit::component::TitleBar::new()
                                .h(px(36.))
                                .when(window.is_fullscreen(), |bar| bar.pl_0())
                                .bg(sidebar_bg)
                                .border_0()
                                .pr_2()
                                .child(header)
                                .into_any_element()
                        } else {
                            header.into_any_element()
                        }
                    }),
            )
            .child(
                v_flex()
                    .id("sidebar-body")
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .px_3()
                    .child(body)
                    .when(sidebar_bg.a >= 1. && top_fade > 0., |this| {
                        this.child(
                            fade(*sidebar_bg, 0., top_fade).absolute().left_0().right_0().top_0(),
                        )
                    })
                    .when(sidebar_bg.a >= 1. && bottom_fade > 0., |this| {
                        this.child(
                            fade(*sidebar_bg, 180., bottom_fade)
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0(),
                        )
                    }),
            )
            .when_some(self.bulk_bar, |sidebar, bar| {
                sidebar.child(div().flex_none().w_full().px_3().pt_2().child(bar))
            })
            .when_some(self.on_back, |sidebar, handler| {
                sidebar.child(
                    h_flex().flex_none().w_full().px_3().pt_2().pb_3().child(
                        div().w_full().debug_selector(|| "sidebar-back".into()).child(
                            crate::components::button("sidebar-back")
                                .ghost()
                                .w_full()
                                .h_9()
                                .px_3()
                                .justify_start()
                                .child(
                                    h_flex()
                                        .w_full()
                                        .justify_start()
                                        .gap_2()
                                        .child(Icon::new(IconName::ArrowLeft).size_4().flex_none())
                                        .child(
                                            div().min_w_0().text_ellipsis().child("Agent Projects"),
                                        ),
                                )
                                .accessibility_label("Back to Agent Projects")
                                .tooltip("Back to Agent Projects")
                                .on_click(handler),
                        ),
                    ),
                )
            })
    }
}
