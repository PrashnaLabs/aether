use crate::theme::Corners;
use gpui_kit::rems;
use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::{ActiveTheme, StyledExt, v_flex};
use gpui_kit::{
    App, Bounds, Decorations, Pixels, Role, ScrollHandle, SharedString, Window, div, prelude::*, px,
};

type PickHandler = Rc<dyn Fn(&usize, &mut Window, &mut App)>;

/// Suggestions for what's being typed (`@skills`, `/commands`), shown above
/// the composer.
#[derive(IntoElement)]
pub struct SuggestionMenu {
    title: SharedString,
    items: Vec<(SharedString, SharedString)>,
    selected: usize,
    on_pick: Option<PickHandler>,
    max_height: Option<Pixels>,
}

impl SuggestionMenu {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            items: Vec::new(),
            selected: 0,
            on_pick: None,
            max_height: None,
        }
    }

    pub fn item(
        mut self,
        label: impl Into<SharedString>,
        description: impl Into<SharedString>,
    ) -> Self {
        self.items.push((label.into(), description.into()));
        self
    }

    pub fn selected(mut self, index: usize) -> Self {
        self.selected = index;
        self
    }

    pub fn max_height(mut self, max: impl Into<Pixels>) -> Self {
        self.max_height = Some(max.into());
        self
    }

    pub fn on_pick(mut self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_pick = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for SuggestionMenu {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let scroll =
            window.use_keyed_state("suggestion-menu-scroll", cx, |_, _| ScrollHandle::new());
        let handle = scroll.read(cx).clone();
        handle.scroll_to_item(self.selected);

        let composer_bounds = window
            .use_keyed_state("composer-bounds", cx, |_, _| Cell::new(Bounds::<Pixels>::default()));
        let bounds: Bounds<Pixels> = composer_bounds.read(cx).get();
        let viewport_h = f32::from(window.viewport_size().height);
        let available_room = if bounds.top() > px(0.) {
            let inset = match window.window_decorations() {
                Decorations::Client { .. } => window.client_inset().unwrap_or_default(),
                Decorations::Server => px(0.),
            };
            let top_room = f32::from(bounds.top() - inset);
            (top_room - 24.0).clamp(120.0, 360.0)
        } else {
            (viewport_h * 0.4).clamp(160.0, 320.0)
        };
        let max_h = self.max_height.map(f32::from).unwrap_or(available_room);

        let theme = cx.theme();
        let (muted, accent) = (theme.muted_foreground, theme.accent);
        let on_pick = self.on_pick;
        v_flex()
            .id("suggestion-menu")
            .role(Role::ListBox)
            .aria_label(self.title.clone())
            .w_full()
            .max_h(px(max_h))
            .p_1()
            .corner(rems(0.5))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_lg()
            .child(div().flex_none().px_2().py_1().text_xs().text_color(muted).child(self.title))
            .child(
                v_flex()
                    .id("suggestion-menu-items")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&handle)
                    .children(self.items.into_iter().enumerate().map(
                        |(ix, (label, description))| {
                            v_flex()
                                .id(("suggestion", ix))
                                .role(Role::ListBoxOption)
                                .aria_label(label.clone())
                                .px_2()
                                .py_1p5()
                                .corner(rems(0.375))
                                .cursor_pointer()
                                .when(ix == self.selected, |row| row.bg(accent))
                                .hover(|row| row.bg(accent))
                                .child(div().text_sm().font_medium().truncate().child(label))
                                .child(
                                    div().text_xs().text_color(muted).truncate().child(description),
                                )
                                .when_some(on_pick.clone(), |row, pick| {
                                    row.on_click(move |_, window, cx| pick(&ix, window, cx))
                                })
                        },
                    )),
            )
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/suggestion_menu.rs"]
mod tests;
