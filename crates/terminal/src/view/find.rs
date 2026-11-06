use aether_ui::icon_chip;
use alacritty_terminal::grid::{Dimensions, Scroll};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Icon, h_flex};
use gpui_kit::{Context, MouseButton, SharedString, Window, div, prelude::*, px};

use crate::grid::{Palette, display_offset_for_line, search_term};

use super::{TerminalPaneEvent, TerminalView, hsla};

impl TerminalView {
    pub(crate) fn toggle_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = !self.find_open;
        if self.find_open {
            self.refresh_find_matches(true);
        }
        window.focus(&self.focus, cx);
        cx.emit(TerminalPaneEvent::Focused(self.pane_id));
        cx.notify();
    }

    pub(crate) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find_open {
            self.find_open = false;
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    pub(crate) fn find_next(&mut self, cx: &mut Context<Self>) {
        if self.find_matches.is_empty() {
            self.refresh_find_matches(false);
        }
        if !self.find_matches.is_empty() {
            self.find_index = (self.find_index + 1) % self.find_matches.len();
            self.scroll_to_active_match();
            cx.notify();
        }
    }

    pub(crate) fn find_prev(&mut self, cx: &mut Context<Self>) {
        if self.find_matches.is_empty() {
            self.refresh_find_matches(false);
        }
        if !self.find_matches.is_empty() {
            self.find_index =
                (self.find_index + self.find_matches.len() - 1) % self.find_matches.len();
            self.scroll_to_active_match();
            cx.notify();
        }
    }

    pub(super) fn refresh_find_matches(&mut self, scroll_to_match: bool) {
        if self.find_query.trim().is_empty() {
            self.find_matches.clear();
            self.find_index = 0;
            return;
        }
        let Some(session) = &self.session else {
            self.find_matches.clear();
            self.find_index = 0;
            return;
        };
        let matches = {
            let term = session.term.lock();
            search_term(&term, &self.find_query)
        };
        self.find_matches = matches;
        if self.find_matches.is_empty() {
            self.find_index = 0;
        } else {
            self.find_index = self.find_index.min(self.find_matches.len() - 1);
            if scroll_to_match {
                self.scroll_to_active_match();
            }
        }
    }

    fn scroll_to_active_match(&mut self) {
        let Some(matched) = self.find_matches.get(self.find_index).copied() else {
            return;
        };
        let Some(session) = &self.session else {
            return;
        };
        let mut term = session.term.lock();
        let screen_lines = term.screen_lines();
        let history_size = term.history_size();
        let current_offset = term.grid().display_offset();
        let target_offset =
            display_offset_for_line(matched.line, screen_lines, history_size, current_offset);
        let delta = target_offset as i32 - current_offset as i32;
        if delta != 0 {
            term.scroll_display(Scroll::Delta(delta));
        }
    }

    pub(super) fn render_find_overlay(
        &self,
        palette: Palette,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let pane_idx = self.pane_id.0 as usize;
        let has_query = !self.find_query.is_empty();
        let query_text: SharedString =
            if has_query { self.find_query.clone().into() } else { "Find in terminal…".into() };
        let count_label: Option<SharedString> = if !self.find_query.trim().is_empty() {
            if self.find_matches.is_empty() {
                Some("0/0".into())
            } else {
                Some(
                    format!(
                        "{}/{}",
                        (self.find_index + 1).min(self.find_matches.len()),
                        self.find_matches.len()
                    )
                    .into(),
                )
            }
        } else {
            None
        };
        h_flex()
            .id(("terminal-find-bar", pane_idx))
            .absolute()
            .top(px(8.))
            .right(px(12.))
            .h(px(28.))
            .px_2()
            .gap_1p5()
            .items_center()
            .text_xs()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .bg(hsla(palette.bg))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    cx.emit(TerminalPaneEvent::Focused(this.pane_id));
                    window.focus(&this.focus, cx);
                }),
            )
            .child(Icon::new(IconName::Search).size(px(12.)).text_color(theme.muted_foreground))
            .child(
                div()
                    .min_w(px(120.))
                    .max_w(px(200.))
                    .truncate()
                    .text_color(if has_query { theme.foreground } else { theme.muted_foreground })
                    .child(query_text),
            )
            .children(
                count_label
                    .map(|count| div().flex_none().text_color(theme.muted_foreground).child(count)),
            )
            .child(
                icon_chip(("terminal-find-prev", pane_idx), false, cx)
                    .icon(IconName::ChevronUp)
                    .accessibility_label("Previous match")
                    .tooltip("Previous match (Shift+Enter)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.find_prev(cx);
                        window.focus(&this.focus, cx);
                    })),
            )
            .child(
                icon_chip(("terminal-find-next", pane_idx), false, cx)
                    .icon(IconName::ChevronDown)
                    .accessibility_label("Next match")
                    .tooltip("Next match (Enter)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.find_next(cx);
                        window.focus(&this.focus, cx);
                    })),
            )
            .child(
                icon_chip(("terminal-find-close", pane_idx), false, cx)
                    .icon(IconName::Close)
                    .accessibility_label("Close find")
                    .tooltip("Close find (Esc)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_find(window, cx);
                    })),
            )
    }
}
