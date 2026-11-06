use aether_ui::theme::Corners;
use aether_ui::{field, file_icon, file_icon_color};
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, button::ButtonVariants, h_flex,
    scroll::ScrollableElement, v_flex,
};
use gpui_kit::rems;
use gpui_kit::{
    AnyElement, Context, InteractiveElement, SharedString, Window, actions, div, prelude::*, px,
};

use crate::app::ChatApp;

actions!(quick_open, [QuickOpenPrev, QuickOpenNext, QuickOpenConfirm, QuickOpenDismiss]);

pub(crate) const QUICK_OPEN_CONTEXT: &str = "QuickOpen";

impl ChatApp {
    /// Opens or closes the Quick Open (⌘P) fuzzy file search modal.
    pub(crate) fn open_quick_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != crate::app::Page::Notes && !self.is_agent_chat(&self.chats[self.active]) {
            return;
        }
        let explorer = self.active_project_explorer(cx);
        let is_open = explorer.read(cx).quick_open_open;
        if is_open {
            self.close_quick_open(window, cx);
        } else {
            self.quick_open_restore = window.focused(cx);
            self.quick_open_scroll.scroll_to_item(0);
            explorer.update(cx, |exp, _| {
                exp.quick_open_open = true;
                exp.set_quick_open_query(String::new());
            });
            self.load_explorer_index(&explorer, cx);
            self.explorer_quick_open_input.update(cx, |input, cx| {
                input.set_value("", window, cx);
                input.focus(window, cx);
            });
            cx.notify();
        }
    }

    /// Closes the Quick Open modal and refocuses what had focus before it opened.
    pub(crate) fn close_quick_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let explorer = if self.page == crate::app::Page::Notes {
            self.notes.explorer.clone()
        } else {
            self.project_explorers.get(&self.active_project_folder()).cloned()
        };
        if let Some(explorer) = explorer {
            explorer.update(cx, |exp, _| {
                exp.quick_open_open = false;
            });
            if let Some(previous) = self.quick_open_restore.take() {
                previous.focus(window, cx);
            }
            cx.notify();
        }
    }

    fn move_quick_open(&mut self, delta: isize, cx: &mut Context<Self>) {
        let explorer = self.active_project_explorer(cx);
        let len = explorer.read(cx).quick_open_items().len() as isize;
        if len == 0 {
            return;
        }
        let ix = explorer.update(cx, |exp, _| {
            exp.quick_open_selected =
                (exp.quick_open_selected as isize + delta).rem_euclid(len) as usize;
            exp.quick_open_selected
        });
        self.quick_open_scroll.scroll_to_item(ix);
        cx.notify();
    }

    fn confirm_quick_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let explorer = self.active_project_explorer(cx);
        let explorer = explorer.read(cx);
        if let Some(item) = explorer.quick_open_items().get(explorer.quick_open_selected) {
            let path = item.path.clone();
            self.close_quick_open(window, cx);
            self.open_quick_open_file(path, window, cx);
        }
    }

    fn open_quick_open_file(
        &mut self,
        path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page == crate::app::Page::Notes {
            self.open_note(path, None, window, cx);
        } else {
            self.open_explorer_file(path, window, cx);
        }
    }

    /// Renders the Quick Open (⌘P) modal overlay.
    pub(crate) fn render_quick_open_overlay(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.page != crate::app::Page::Notes && !self.is_agent_chat(&self.chats[self.active]) {
            return None;
        }
        let explorer_entity = if self.page == crate::app::Page::Notes {
            self.notes.explorer.clone()?
        } else {
            self.project_explorers.get(&self.active_project_folder())?.clone()
        };
        if !explorer_entity.read(cx).quick_open_open {
            return None;
        }

        let theme = cx.theme();
        let border = theme.border;
        let card_bg = theme.popover;
        let muted = theme.muted_foreground;

        let items = explorer_entity.read(cx).quick_open_items().to_vec();
        let selected = explorer_entity.read(cx).quick_open_selected;

        Some(
            div()
                .id("quick-open-backdrop")
                .absolute()
                .inset_0()
                .bg(theme.background.opacity(0.6))
                .flex()
                .items_start()
                .justify_center()
                .pt(px(80.))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_quick_open(window, cx);
                }))
                .child(
                    v_flex()
                        .id("quick-open-modal-box")
                        .key_context(QUICK_OPEN_CONTEXT)
                        .on_action(cx.listener(|this, _: &QuickOpenPrev, _, cx| {
                            this.move_quick_open(-1, cx)
                        }))
                        .on_action(
                            cx.listener(|this, _: &QuickOpenNext, _, cx| {
                                this.move_quick_open(1, cx)
                            }),
                        )
                        .on_action(cx.listener(|this, _: &QuickOpenConfirm, window, cx| {
                            this.confirm_quick_open(window, cx)
                        }))
                        .on_action(cx.listener(|this, _: &QuickOpenDismiss, window, cx| {
                            this.close_quick_open(window, cx)
                        }))
                        .w(px(560.))
                        .max_h(px(460.))
                        .bg(card_bg)
                        .corner(rems(0.5))
                        .border_1()
                        .border_color(border)
                        .shadow_lg()
                        .overflow_hidden()
                        .on_click(|_, _, cx| {
                            cx.stop_propagation();
                        })
                        .child(
                            // Input field
                            h_flex().p_3().border_b_1().border_color(border).child(
                                div().flex_1().child(
                                    field(&self.explorer_quick_open_input).icon(IconName::Search),
                                ),
                            ),
                        )
                        .child(
                            v_flex()
                                .id("quick-open-list")
                                .flex_1()
                                .min_h_0()
                                .vertical_scrollbar(&self.quick_open_scroll)
                                .child(
                                    // Matching files list
                                    v_flex()
                                        .id("quick-open-list-scroll")
                                        .flex_1()
                                        .min_h_0()
                                        .overflow_y_scroll()
                                        .track_scroll(&self.quick_open_scroll)
                                        .p_1()
                                        .gap_0p5()
                                        .when(items.is_empty(), |l| {
                                            l.p_4().items_center().child(
                                                div()
                                                    .text_xs()
                                                    .text_color(muted)
                                                    .child("No matching files found"),
                                            )
                                        })
                                        .children(items.into_iter().enumerate().map(
                                            |(ix, item)| {
                                                let path = item.path.clone();
                                                let rel = item.rel_path.clone();
                                                let folder = path.parent().map(|p| p.to_path_buf());
                                                let icon = file_icon(&path);
                                                let icon_color = file_icon_color(&path, cx);

                                                let p_open = path.clone();
                                                let f_agent = folder.clone();

                                                h_flex()
                                                    .id(SharedString::from(format!(
                                                        "qo-item-{}",
                                                        path.display()
                                                    )))
                                                    .w_full()
                                                    .items_center()
                                                    .px_3()
                                                    .py_2()
                                                    .corner(rems(0.375))
                                                    .cursor_pointer()
                                                    .hover(|r| r.bg(theme.foreground.opacity(0.06)))
                                                    .when(ix == selected, |r| r.bg(theme.accent))
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            this.close_quick_open(window, cx);
                                                            this.open_quick_open_file(
                                                                p_open.clone(),
                                                                window,
                                                                cx,
                                                            );
                                                        },
                                                    ))
                                                    .child(
                                                        Icon::new(icon)
                                                            .size(px(14.))
                                                            .text_color(icon_color),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .pl_2()
                                                            .text_xs()
                                                            .truncate()
                                                            .child(rel),
                                                    )
                                                    .when_some(f_agent, |row, folder| {
                                                        row.child(
                                                            aether_ui::button(SharedString::from(
                                                                format!(
                                                                    "qo-agent-{}",
                                                                    path.display()
                                                                ),
                                                            ))
                                                            .ghost()
                                                            .xsmall()
                                                            .icon(IconName::Bot)
                                                            .label("Agent")
                                                            .tooltip("Open Agent in this folder")
                                                            .on_click(cx.listener(
                                                                move |this, _, window, cx| {
                                                                    this.close_quick_open(
                                                                        window, cx,
                                                                    );
                                                                    this.new_chat_in(
                                                                        folder.clone(),
                                                                        window,
                                                                        cx,
                                                                    );
                                                                },
                                                            )),
                                                        )
                                                    })
                                                    .into_any_element()
                                            },
                                        )),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
#[path = "../../tests/unit/explorer/quick_open.rs"]
mod tests;
