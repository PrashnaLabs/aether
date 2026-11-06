use aether_ui::icon_chip;
use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon, StyledExt, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Axis, Context, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::*, px,
};

use crate::ToggleTerminalSidebar;

use super::{
    GroupId, PaneId, RenameTarget, SIDEBAR_COLLAPSED_WIDTH, SIDEBAR_WIDTH, TerminalWorkspace,
};

impl TerminalWorkspace {
    pub(super) fn render_sidebar(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let workspace_view = cx.entity();
        let active_group_id = self.model.active_group_id();
        let active_pane_id = self.model.active_pane_id();
        let tooltip = |tip: SharedString| {
            move |window: &mut Window, cx: &mut App| Tooltip::new(tip.clone()).build(window, cx)
        };

        let make_pane_context_menu =
            |pane_id: PaneId, group_id: GroupId, group_pane_count: usize| {
                let ws = workspace_view.clone();
                move |menu: gpui_kit::component::menu::PopupMenu,
                      _: &mut Window,
                      _: &mut Context<gpui_kit::component::menu::PopupMenu>| {
                    let rename_ws = ws.clone();
                    let split_r_ws = ws.clone();
                    let split_d_ws = ws.clone();
                    let close_pane_ws = ws.clone();
                    let close_group_ws = ws.clone();
                    let menu = menu
                        .item(PopupMenuItem::new("Rename").icon(IconName::Pencil).on_click(
                            move |_, window, cx| {
                                rename_ws.update(cx, |this, cx| {
                                    this.model.sidebar_open = true;
                                    this.begin_rename_pane(pane_id, window, cx);
                                });
                            },
                        ))
                        .item(
                            PopupMenuItem::new("Split right").icon(IconName::PanelRight).on_click(
                                move |_, window, cx| {
                                    split_r_ws.update(cx, |this, cx| {
                                        this.split_pane(pane_id, Axis::Horizontal, window, cx);
                                    });
                                },
                            ),
                        )
                        .item(
                            PopupMenuItem::new("Split down").icon(IconName::PanelBottom).on_click(
                                move |_, window, cx| {
                                    split_d_ws.update(cx, |this, cx| {
                                        this.split_pane(pane_id, Axis::Vertical, window, cx);
                                    });
                                },
                            ),
                        )
                        .separator()
                        .item(
                            PopupMenuItem::new("Delete terminal").icon(IconName::Trash).on_click(
                                move |_, window, cx| {
                                    close_pane_ws.update(cx, |this, cx| {
                                        this.request_close_pane(pane_id, window, cx);
                                    });
                                },
                            ),
                        );
                    if group_pane_count > 1 {
                        menu.item(PopupMenuItem::new("Close group").icon(IconName::Close).on_click(
                            move |_, window, cx| {
                                close_group_ws.update(cx, |this, cx| {
                                    this.request_close_group(group_id, window, cx);
                                });
                            },
                        ))
                    } else {
                        menu
                    }
                }
            };

        if !self.model.sidebar_open {
            let mut items: Vec<AnyElement> = Vec::new();
            for (g_idx, group) in self.model.groups.iter().enumerate() {
                if g_idx > 0 {
                    items.push(
                        div()
                            .flex_none()
                            .w(px(18.))
                            .h(px(1.))
                            .my_0p5()
                            .bg(theme.border)
                            .into_any_element(),
                    );
                }
                let group_id = group.id;
                let group_pane_count = group.pane_count();
                let is_active_group = group_id == active_group_id;
                for (pane_id, connector) in group.root.tree_rows() {
                    let pane_idx = pane_id.0 as usize;
                    let is_active_pane = is_active_group && pane_id == active_pane_id;
                    let label = self.pane_display_label(pane_id, cx);
                    let tip_text = label;
                    let icon_fg =
                        if is_active_pane { theme.foreground } else { theme.muted_foreground };
                    let item_bg = if is_active_pane {
                        theme.foreground.opacity(0.12)
                    } else if is_active_group {
                        theme.foreground.opacity(0.05)
                    } else {
                        gpui_kit::transparent_black()
                    };

                    items.push(
                        div()
                            .id(("terminal-tree-icon", pane_idx))
                            .flex_none()
                            .w(px(26.))
                            .h(px(26.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(theme.radius)
                            .bg(item_bg)
                            .when(is_active_pane, |el| {
                                el.border_1().border_color(theme.ring.opacity(0.65))
                            })
                            .hover(|s| s.bg(theme.foreground.opacity(0.14)))
                            .cursor_pointer()
                            .tooltip(tooltip(tip_text))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    this.focus_pane_by_id(pane_id, window, cx);
                                }),
                            )
                            .context_menu(make_pane_context_menu(
                                pane_id,
                                group_id,
                                group_pane_count,
                            ))
                            .child(
                                Icon::new(if connector.is_split_child() {
                                    IconName::CornerDownRight
                                } else {
                                    IconName::SquareTerminal
                                })
                                .size(px(13.))
                                .text_color(icon_fg),
                            )
                            .into_any_element(),
                    );
                }
            }

            return v_flex()
                .id("terminal-session-sidebar")
                .flex_none()
                .w(px(SIDEBAR_COLLAPSED_WIDTH))
                .h_full()
                .py_1()
                .px_1()
                .gap_1()
                .items_center()
                .border_l_1()
                .border_color(theme.border)
                .overflow_y_scroll()
                .child(
                    icon_chip("terminal-sidebar-expand", false, cx)
                        .icon(IconName::PanelRightOpen)
                        .accessibility_label("Expand terminal sidebar")
                        .tooltip("Expand terminal sidebar")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.on_toggle_sidebar(&ToggleTerminalSidebar, window, cx);
                        })),
                )
                .child(div().flex_none().w(px(20.)).h(px(1.)).bg(theme.border))
                .children(items)
                .child(
                    icon_chip("terminal-sidebar-new-collapsed", false, cx)
                        .icon(IconName::Plus)
                        .accessibility_label("New terminal")
                        .tooltip("New terminal")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.new_group(window, cx);
                        })),
                )
                .into_any_element();
        }

        let mut rows: Vec<AnyElement> = Vec::new();
        for group in &self.model.groups {
            let group_id = group.id;
            let group_pane_count = group.pane_count();
            let is_active_group = group_id == active_group_id;
            for (pane_id, connector) in group.root.tree_rows() {
                let pane_idx = pane_id.0 as usize;
                let is_active_pane = is_active_group && pane_id == active_pane_id;
                let is_renaming = self.renaming == Some(RenameTarget::Pane(pane_id));
                let label = self.pane_display_label(pane_id, cx);
                if is_renaming {
                    rows.push(self.render_sidebar_rename_row(&label, cx));
                    continue;
                }
                let label_tip = label.clone();
                let is_split_child = connector.is_split_child();
                let row_fg = if is_active_pane { theme.foreground } else { theme.muted_foreground };
                let row_bg = if is_active_pane {
                    theme.foreground.opacity(0.10)
                } else if is_active_group {
                    theme.foreground.opacity(0.04)
                } else {
                    gpui_kit::transparent_black()
                };

                rows.push(
                    h_flex()
                        .id(("terminal-tree-row", pane_idx))
                        .w_full()
                        .h(px(26.))
                        .px_1p5()
                        .when(is_split_child, |el| el.pl_3())
                        .gap_1p5()
                        .items_center()
                        .rounded(theme.radius)
                        .bg(row_bg)
                        .hover(|s| s.bg(theme.foreground.opacity(0.12)))
                        .cursor_pointer()
                        .tooltip(tooltip(label_tip))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                if e.click_count == 2 {
                                    this.begin_rename_pane(pane_id, window, cx);
                                } else {
                                    this.focus_pane_by_id(pane_id, window, cx);
                                }
                            }),
                        )
                        .context_menu(make_pane_context_menu(pane_id, group_id, group_pane_count))
                        .when(is_split_child, |el| {
                            el.child(
                                Icon::new(IconName::CornerDownRight)
                                    .size(px(11.))
                                    .text_color(theme.muted_foreground)
                                    .flex_none(),
                            )
                        })
                        .child(
                            Icon::new(IconName::SquareTerminal)
                                .size(px(12.))
                                .flex_none()
                                .text_color(if is_active_pane {
                                    theme.ring
                                } else {
                                    theme.muted_foreground
                                }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(row_fg)
                                .when(is_active_pane, |el| el.font_medium())
                                .child(label),
                        )
                        .into_any_element(),
                );
            }
        }

        v_flex()
            .id("terminal-session-sidebar")
            .flex_none()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .p_1()
            .gap_0p5()
            .text_xs()
            .border_l_1()
            .border_color(theme.border)
            .overflow_y_scroll()
            .child(
                h_flex()
                    .flex_none()
                    .w_full()
                    .h(px(24.))
                    .pl_1p5()
                    .pr_0p5()
                    .items_center()
                    .justify_between()
                    .child(
                        div().text_color(theme.muted_foreground).font_medium().child("Terminals"),
                    )
                    .child(
                        h_flex()
                            .gap_0p5()
                            .items_center()
                            .child(
                                icon_chip("terminal-sidebar-new", false, cx)
                                    .icon(IconName::Plus)
                                    .accessibility_label("New terminal")
                                    .tooltip("New terminal")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.new_group(window, cx);
                                    })),
                            )
                            .child(
                                icon_chip("terminal-sidebar-collapse", false, cx)
                                    .icon(IconName::PanelRightClose)
                                    .accessibility_label("Collapse terminal sidebar")
                                    .tooltip("Collapse terminal sidebar")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.on_toggle_sidebar(&ToggleTerminalSidebar, window, cx);
                                    })),
                            ),
                    ),
            )
            .children(rows)
            .into_any_element()
    }
}
