use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Axis, Context, DragMoveEvent, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, StatefulInteractiveElement, StyleRefinement, Styled, Window,
    div, prelude::*, px, relative,
};

use super::model::{SplitId, SplitNode};
use super::{PaneId, SPLIT_DIVIDER_SIZE, TerminalWorkspace};

#[derive(Clone, Copy)]
struct SplitDrag(SplitId);

impl Render for SplitDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::Empty
    }
}

impl TerminalWorkspace {
    pub(super) fn render_split_node(
        &self,
        node: &SplitNode,
        active_pane: PaneId,
        multi_pane: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        match node {
            SplitNode::Leaf(pane_id) => {
                let Some(pane) = self.panes.get(pane_id).cloned() else {
                    return div().size_full().into_any_element();
                };
                let is_active = *pane_id == active_pane;
                div()
                    .id(("terminal-split-leaf", pane_id.0 as usize))
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .when(multi_pane, |el| {
                        el.border_1().border_color(if is_active {
                            theme.ring.opacity(0.65)
                        } else {
                            gpui_kit::transparent_black()
                        })
                    })
                    .child(pane.cached(StyleRefinement::default().size_full()))
                    .into_any_element()
            }
            SplitNode::Split { id, axis, ratio, first, second } => {
                let split_id = *id;
                let split_idx = split_id.0 as usize;
                let split_axis = *axis;
                let clamped = SplitNode::clamp_ratio(*ratio);
                let first_el = self.render_split_node(first, active_pane, multi_pane, cx);
                let second_el = self.render_split_node(second, active_pane, multi_pane, cx);
                let divider_tooltip = |window: &mut Window, cx: &mut App| {
                    Tooltip::new("Drag to resize · Double-click to reset").build(window, cx)
                };

                match split_axis {
                    Axis::Horizontal => h_flex()
                        .id(("terminal-split", split_idx))
                        .size_full()
                        .min_w_0()
                        .min_h_0()
                        .on_drag_move(cx.listener(
                            move |this, e: &DragMoveEvent<SplitDrag>, _, cx| {
                                if e.drag(cx).0 != split_id {
                                    return;
                                }
                                let w = f32::from(e.bounds.size.width);
                                if w > 0. {
                                    let next =
                                        f32::from(e.event.position.x - e.bounds.origin.x) / w;
                                    if this.model.set_split_ratio(split_id, next) {
                                        cx.notify();
                                    }
                                }
                            },
                        ))
                        .child(
                            div()
                                .h_full()
                                .flex_basis(relative(clamped))
                                .flex_shrink(1.0)
                                .min_w_0()
                                .min_h_0()
                                .child(first_el),
                        )
                        .child(
                            div()
                                .id(("terminal-split-divider", split_idx))
                                .flex_none()
                                .w(px(SPLIT_DIVIDER_SIZE))
                                .h_full()
                                .cursor_col_resize()
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|s| s.bg(theme.ring.opacity(0.35)))
                                .tooltip(divider_tooltip)
                                .on_drag(SplitDrag(split_id), |drag: &SplitDrag, _, _, cx| {
                                    cx.new(|_| *drag)
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                        if e.click_count == 2
                                            && this.model.reset_split_ratio(split_id)
                                        {
                                            cx.notify();
                                        }
                                    }),
                                )
                                .child(div().w(px(1.)).h_full().bg(theme.border)),
                        )
                        .child(div().flex_1().h_full().min_w_0().min_h_0().child(second_el))
                        .into_any_element(),
                    Axis::Vertical => v_flex()
                        .id(("terminal-split", split_idx))
                        .size_full()
                        .min_w_0()
                        .min_h_0()
                        .on_drag_move(cx.listener(
                            move |this, e: &DragMoveEvent<SplitDrag>, _, cx| {
                                if e.drag(cx).0 != split_id {
                                    return;
                                }
                                let h = f32::from(e.bounds.size.height);
                                if h > 0. {
                                    let next =
                                        f32::from(e.event.position.y - e.bounds.origin.y) / h;
                                    if this.model.set_split_ratio(split_id, next) {
                                        cx.notify();
                                    }
                                }
                            },
                        ))
                        .child(
                            div()
                                .w_full()
                                .flex_basis(relative(clamped))
                                .flex_shrink(1.0)
                                .min_w_0()
                                .min_h_0()
                                .child(first_el),
                        )
                        .child(
                            div()
                                .id(("terminal-split-divider", split_idx))
                                .flex_none()
                                .h(px(SPLIT_DIVIDER_SIZE))
                                .w_full()
                                .cursor_row_resize()
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|s| s.bg(theme.ring.opacity(0.35)))
                                .tooltip(divider_tooltip)
                                .on_drag(SplitDrag(split_id), |drag: &SplitDrag, _, _, cx| {
                                    cx.new(|_| *drag)
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                        if e.click_count == 2
                                            && this.model.reset_split_ratio(split_id)
                                        {
                                            cx.notify();
                                        }
                                    }),
                                )
                                .child(div().h(px(1.)).w_full().bg(theme.border)),
                        )
                        .child(div().flex_1().w_full().min_w_0().min_h_0().child(second_el))
                        .into_any_element(),
                }
            }
        }
    }
}
