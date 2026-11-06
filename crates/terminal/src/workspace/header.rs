use aether_ui::{Tab, TabBar, icon_chip};
use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::{
    AnyElement, App, Axis, Context, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::*, px,
};

use crate::view::{TerminalEvent, file_name};
use crate::{
    CloseTerminalPane, NewTerminalGroup, SplitTerminalDown, SplitTerminalRight, ToggleTerminal,
    ToggleTerminalFind, ToggleTerminalMaximize, ToggleTerminalSidebar,
};

use super::{RenameTarget, TerminalWorkspace, WORKSPACE_HEADER_HEIGHT};

impl TerminalWorkspace {
    pub(super) fn render_header(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let workspace_view = cx.entity();
        let active_group_id = self.model.active_group_id();
        let find_open = self.active_pane().is_some_and(|p| p.read(cx).find_open);
        let folder =
            self.active_pane().map_or_else(|| self.folder.clone(), |p| p.read(cx).folder.clone());
        let folder_str = folder.display().to_string();
        let folder_tip = SharedString::from(folder_str.clone());
        let folder_basename = file_name(&folder_str);
        let tooltip = |tip: SharedString| {
            move |window: &mut Window, cx: &mut App| Tooltip::new(tip.clone()).build(window, cx)
        };

        let tabs: Vec<AnyElement> = self
            .model
            .groups
            .iter()
            .map(|group| {
                let group_id = group.id;
                let group_idx = group_id.0 as usize;
                let group_active_pane = group.active_pane;
                let is_active = group_id == active_group_id;
                let pane_count = group.pane_count();
                let label = self.group_display_label(group_id, cx);
                let is_renaming = self.renaming == Some(RenameTarget::Group(group_id));
                if is_renaming {
                    return self.render_tab_rename_row(&label, cx);
                }
                let menu_workspace = workspace_view.clone();
                let watched = self.watched;

                let mut tab = Tab::new(("terminal-group-tab", group_idx), label)
                    .icon(IconName::SquareTerminal)
                    .selected(is_active)
                    .max_label_width(px(140.))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_group(group_id, window, cx);
                    }))
                    .on_double_click(cx.listener(move |this, _, window, cx| {
                        this.begin_rename_group(group_id, window, cx);
                    }))
                    .on_close(
                        ("terminal-group-close", group_idx),
                        "Close group",
                        cx.listener(move |this, _, window, cx| {
                            this.request_close_group(group_id, window, cx);
                        }),
                    )
                    .context_menu(move |menu, _, _| {
                        let rename_ws = menu_workspace.clone();
                        let split_r_ws = menu_workspace.clone();
                        let split_d_ws = menu_workspace.clone();
                        let close_ws = menu_workspace.clone();
                        let close_label =
                            if pane_count > 1 { "Close group" } else { "Delete terminal" };
                        menu.item(PopupMenuItem::new("Rename").icon(IconName::Pencil).on_click(
                            move |_, window, cx| {
                                rename_ws.update(cx, |this, cx| {
                                    this.begin_rename_group(group_id, window, cx);
                                });
                            },
                        ))
                        .item(
                            PopupMenuItem::new("Split right").icon(IconName::PanelRight).on_click(
                                move |_, window, cx| {
                                    split_r_ws.update(cx, |this, cx| {
                                        this.split_pane(
                                            group_active_pane,
                                            Axis::Horizontal,
                                            window,
                                            cx,
                                        );
                                    });
                                },
                            ),
                        )
                        .item(
                            PopupMenuItem::new("Split down").icon(IconName::PanelBottom).on_click(
                                move |_, window, cx| {
                                    split_d_ws.update(cx, |this, cx| {
                                        this.split_pane(
                                            group_active_pane,
                                            Axis::Vertical,
                                            window,
                                            cx,
                                        );
                                    });
                                },
                            ),
                        )
                        .when_some(watched, |menu, watched| {
                            let watch_ws = menu_workspace.clone();
                            menu.item(
                                PopupMenuItem::new(if watched { "Unwatch" } else { "Watch" })
                                    .icon(if watched { IconName::EyeOff } else { IconName::Eye })
                                    .on_click(move |_, _, cx| {
                                        watch_ws.update(cx, |_, cx| {
                                            cx.emit(TerminalEvent::ToggleWatch)
                                        });
                                    }),
                            )
                        })
                        .separator()
                        .item(
                            PopupMenuItem::new(close_label).icon(IconName::Trash).on_click(
                                move |_, window, cx| {
                                    close_ws.update(cx, |this, cx| {
                                        this.request_close_group(group_id, window, cx);
                                    });
                                },
                            ),
                        )
                    });

                if pane_count > 1 {
                    tab = tab.badge_with_id(
                        ("terminal-group-count", group_idx),
                        pane_count.to_string(),
                        Some(SharedString::from(format!("{pane_count} split panes"))),
                    );
                }

                tab.into_any_element()
            })
            .collect();

        let selected = self.model.groups.iter().position(|g| g.id == active_group_id);
        let mut bar = TabBar::new("terminal-workspace-header")
            .tabs_id("terminal-group-tabs")
            .when_some(selected, |bar, ix| bar.selected_index(ix))
            .height(px(WORKSPACE_HEADER_HEIGHT))
            .border_top(!self.full_page && !self.model.maximized)
            .tabs(tabs)
            .trailing(
                icon_chip("terminal-new-group", false, cx)
                    .icon(IconName::Plus)
                    .accessibility_label("New terminal")
                    .tooltip_with_action("New terminal", &NewTerminalGroup, None)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.new_group(window, cx);
                    })),
            )
            .info(
                div()
                    .id("terminal-workspace-folder")
                    .ml_1()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.muted_foreground)
                    .tooltip(tooltip(folder_tip))
                    .child(folder_basename),
            )
            .action(
                icon_chip("terminal-split-right", false, cx)
                    .icon(IconName::PanelRight)
                    .accessibility_label("Split terminal right")
                    .tooltip_with_action("Split terminal right", &SplitTerminalRight, None)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let target = this.model.active_pane_id();
                        this.split_pane(target, Axis::Horizontal, window, cx);
                    })),
            )
            .action(
                icon_chip("terminal-split-down", false, cx)
                    .icon(IconName::PanelBottom)
                    .accessibility_label("Split terminal down")
                    .tooltip_with_action("Split terminal down", &SplitTerminalDown, None)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let target = this.model.active_pane_id();
                        this.split_pane(target, Axis::Vertical, window, cx);
                    })),
            )
            .action(
                icon_chip("terminal-find", find_open, cx)
                    .icon(IconName::Search)
                    .accessibility_label("Find in terminal")
                    .tooltip_with_action("Find in terminal", &ToggleTerminalFind, None)
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(pane) = this.active_pane() {
                            pane.update(cx, |p, cx| p.toggle_find(window, cx));
                            cx.notify();
                        }
                    })),
            )
            .action(
                icon_chip("terminal-kill-pane", false, cx)
                    .icon(IconName::Trash)
                    .accessibility_label("Kill active terminal pane")
                    .tooltip_with_action("Kill active terminal pane", &CloseTerminalPane, None)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let target = this.model.active_pane_id();
                        this.request_close_pane(target, window, cx);
                    })),
            )
            .action(div().flex_none().w(px(1.)).h(px(14.)).mx_0p5().bg(theme.border));

        if self.full_page {
            let sidebar_open = self.model.sidebar_open;
            bar = bar.action(
                icon_chip("terminal-toggle-sidebar", sidebar_open, cx)
                    .icon(if sidebar_open {
                        IconName::PanelRightClose
                    } else {
                        IconName::PanelRightOpen
                    })
                    .accessibility_label(if sidebar_open {
                        "Collapse terminal sidebar"
                    } else {
                        "Expand terminal sidebar"
                    })
                    .tooltip(if sidebar_open {
                        "Collapse terminal sidebar"
                    } else {
                        "Expand terminal sidebar"
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.on_toggle_sidebar(&ToggleTerminalSidebar, window, cx);
                    })),
            );
        }

        let maximized = self.model.maximized;
        let maximize_label = match (self.full_page, maximized) {
            (true, false) => "Fullscreen terminal",
            (true, true) => "Exit fullscreen",
            (false, false) => "Maximize terminal size",
            (false, true) => "Restore terminal size",
        };
        bar = bar.action(
            icon_chip("terminal-toggle-maximize", maximized, cx)
                .debug_selector(|| "terminal-toggle-maximize".into())
                .icon(if maximized { IconName::Minimize } else { IconName::Maximize })
                .accessibility_label(maximize_label)
                .tooltip(maximize_label)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.on_toggle_maximize(&ToggleTerminalMaximize, window, cx);
                })),
        );

        bar.action(
            icon_chip("terminal-hide", false, cx)
                .icon(IconName::Close)
                .accessibility_label("Hide terminal")
                .tooltip_with_action("Hide terminal", &ToggleTerminal, None)
                .on_click(cx.listener(|_, _, _, cx| cx.emit(TerminalEvent::Hide))),
        )
    }
}
