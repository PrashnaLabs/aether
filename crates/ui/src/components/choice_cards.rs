use crate::theme::Corners;
use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, IconName, StyledExt, button::ButtonVariants, h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, Role, ScrollHandle, SharedString, Window, canvas, div,
    linear_color_stop, linear_gradient, point, prelude::*, px, transparent_black,
};

use super::ClickHandler;

pub const CAROUSEL_CARD_WIDTH: f32 = 158.;
const ARROW_TOP: f32 = 39.;
const ARROW_INSET: f32 = 16.;
const FADE_WIDTH: f32 = 48.;

/// A preview box above a centered label. The ring sits in the box's padding,
/// so a selected card takes up the same space as any other.
#[derive(IntoElement)]
pub struct ChoiceCard {
    id: SharedString,
    label: SharedString,
    selected: bool,
    preview: Option<AnyElement>,
    label_element: Option<AnyElement>,
    on_click: Option<ClickHandler>,
}

impl ChoiceCard {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            selected: false,
            preview: None,
            label_element: None,
            on_click: None,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn preview(mut self, preview: impl IntoElement) -> Self {
        self.preview = Some(preview.into_any_element());
        self
    }

    /// Replaces the plain label, such as with a menu button. It sits outside
    /// the card's click target.
    pub fn label_element(mut self, label: AnyElement) -> Self {
        self.label_element = Some(label);
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for ChoiceCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let ring = theme.ring;
        let id = self.id;
        let label_id = format!("{id}-label");
        let on_click = self.on_click.map(Rc::new);
        let on_label = on_click.clone().filter(|_| self.label_element.is_none());
        v_flex()
            .w_full()
            .gap_1p5()
            .child(
                div()
                    .id(id.clone())
                    .debug_selector(|| id.to_string())
                    .role(Role::Button)
                    .aria_label(self.label.clone())
                    .aria_selected(self.selected)
                    .tab_index(0)
                    .cursor_pointer()
                    .p(px(2.))
                    .corner(px(16.))
                    .border_2()
                    .border_color(if self.selected {
                        theme.foreground
                    } else {
                        transparent_black()
                    })
                    .focus_visible(move |s| s.border_color(ring))
                    .when_some(on_click, |card, handler| {
                        card.on_click(move |event, window, cx| handler(event, window, cx))
                    })
                    .child(
                        div()
                            .corner(px(12.))
                            .border_1()
                            .border_color(theme.border)
                            .overflow_hidden()
                            .children(self.preview),
                    ),
            )
            .child(
                h_flex()
                    .id(SharedString::from(label_id.clone()))
                    .debug_selector(|| label_id)
                    .w_full()
                    .h_6()
                    .justify_center()
                    .text_sm()
                    .when_some(on_label, |row, handler| {
                        row.cursor_pointer()
                            .on_click(move |event, window, cx| handler(event, window, cx))
                    })
                    .text_color(if self.selected {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .when(self.selected, |row| row.font_semibold())
                    .child(
                        self.label_element.unwrap_or_else(|| {
                            div().truncate().child(self.label).into_any_element()
                        }),
                    ),
            )
    }
}

/// Equal-width cards that always fit, without scrolling.
#[derive(IntoElement)]
pub struct CardRow {
    cards: Vec<AnyElement>,
}

impl CardRow {
    pub fn new(cards: impl IntoIterator<Item = impl IntoElement>) -> Self {
        Self { cards: cards.into_iter().map(IntoElement::into_any_element).collect() }
    }
}

impl RenderOnce for CardRow {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        h_flex()
            .w_full()
            .items_start()
            .gap_1()
            .children(self.cards.into_iter().map(|card| div().flex_1().min_w_0().child(card)))
    }
}

/// Fixed-width cards in a row that scrolls, with arrows and faded edges where
/// more cards are clipped.
#[derive(IntoElement)]
pub struct CardCarousel {
    id: SharedString,
    selected_ix: usize,
    card_width: f32,
    card_gap: f32,
    cards: Vec<AnyElement>,
}

impl CardCarousel {
    pub fn compact(mut self) -> Self {
        self.card_width = 120.;
        self.card_gap = 12.;
        self
    }
    pub fn new(id: impl Into<SharedString>, selected_ix: usize) -> Self {
        Self {
            id: id.into(),
            selected_ix,
            card_width: CAROUSEL_CARD_WIDTH,
            card_gap: 4.,
            cards: Vec::new(),
        }
    }
}

impl ParentElement for CardCarousel {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.cards.extend(elements);
    }
}

fn can_scroll(scroll: &ScrollHandle) -> (bool, bool) {
    let (at, max) = (-scroll.offset().x, scroll.max_offset().x);
    (at > px(1.), at < max - px(1.))
}

impl RenderOnce for CardCarousel {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window
            .use_keyed_state(self.id.clone(), cx, |_, _| (ScrollHandle::new(), None::<usize>));
        let (scroll, shown) = state.read(cx).clone();
        // first prepaint runs item scrolls before it knows the axis and drops them
        if shown != Some(self.selected_ix) && scroll.children_count() > 0 {
            scroll.scroll_to_item(self.selected_ix);
            state.update(cx, |state, _| state.1 = Some(self.selected_ix));
        }
        let (back, forward) = can_scroll(&scroll);
        let theme = cx.theme();
        let (background, border) =
            (theme.background.blend(crate::settings::section_surface(cx)), theme.border);
        let fade = |from, to| {
            div().absolute().top_0().bottom_0().w(px(FADE_WIDTH)).bg(linear_gradient(
                90.,
                linear_color_stop(from, 0.),
                linear_color_stop(to, 1.),
            ))
        };
        let arrow = |suffix: &str, icon: IconName, label: &'static str, direction: f32| {
            let scroll = scroll.clone();
            let id = format!("{}-{suffix}", self.id);
            crate::components::button(SharedString::from(id.clone()))
                .debug_selector(|| id)
                .secondary()
                .icon(icon)
                .accessibility_label(label)
                .occlude()
                .absolute()
                .top(px(ARROW_TOP))
                .size_7()
                .rounded_full()
                .border_1()
                .border_color(border)
                .on_click(move |_, window, _| {
                    let step = scroll.bounds().size.width * 0.8 * direction;
                    scroll.set_offset(point(scroll.offset().x + step, px(0.)));
                    window.refresh();
                })
        };
        let measured = scroll.clone();
        div()
            .relative()
            .w_full()
            .child(
                h_flex()
                    .id(self.id.clone())
                    .w_full()
                    .items_start()
                    .gap(px(self.card_gap))
                    .overflow_x_scroll()
                    // a vertical wheel over the row keeps scrolling the page
                    .restrict_scroll_to_axis()
                    .track_scroll(&scroll)
                    .children(
                        self.cards
                            .into_iter()
                            .map(|card| div().flex_none().w(px(self.card_width)).child(card)),
                    ),
            )
            .when(back, |row| {
                row.child(fade(background, background.opacity(0.)).left_0()).child(
                    arrow("prev", IconName::ChevronLeft, "Scroll back", 1.).left(px(ARROW_INSET)),
                )
            })
            .when(forward, |row| {
                row.child(fade(background.opacity(0.), background).right_0()).child(
                    arrow("next", IconName::ChevronRight, "Scroll forward", -1.)
                        .right(px(ARROW_INSET)),
                )
            })
            // arrows above were chosen from last frame's layout
            .child(canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    if can_scroll(&measured) != (back, forward) {
                        window.request_animation_frame();
                    }
                },
            ))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/components/choice_cards.rs"]
mod tests;
