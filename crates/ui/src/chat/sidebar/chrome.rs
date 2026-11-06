use std::sync::Arc;

use gpui_kit::component::{ActiveTheme, StyledExt, tooltip::Tooltip};
use gpui_kit::{
    AnyElement, App, Div, ElementId, FontFeatures, Hsla, SharedString, Stateful, Window, black,
    div, prelude::*, px, rems, transparent_black,
};

use super::NEST;

#[derive(Clone, Copy)]
struct Fills {
    hover: Hsla,
    pressed: Hsla,
    selected: Hsla,
}

impl Fills {
    /// Hover is subtle translucent blend; pressed is darker than hover in both themes.
    fn new(cx: &App) -> Self {
        let theme = cx.theme();
        let hover = theme.sidebar.blend(theme.sidebar_accent.opacity(0.4));
        Self { hover, pressed: hover.blend(black().opacity(0.08)), selected: theme.sidebar_accent }
    }
}

/// The fill `row_style` paints under a hovered row.
pub fn row_hover(cx: &App) -> Hsla {
    Fills::new(cx).hover
}

/// Shared row chrome: a transparent border that becomes the focus ring on
/// keyboard focus (so focus never shifts layout), radius and fills.
pub fn row_style<E: StatefulInteractiveElement + Styled>(el: E, selected: bool, cx: &App) -> E {
    let ring = cx.theme().ring;
    let fills = Fills::new(cx);
    let border_color =
        if selected { cx.theme().sidebar_border.opacity(0.6) } else { transparent_black() };
    let el = el
        .border_1()
        .border_color(border_color)
        .focus_visible(move |s| s.border_color(ring))
        .rounded(cx.theme().radius)
        .cursor_pointer();
    let el = if selected { el.bg(fills.selected) } else { el.hover(move |s| s.bg(fills.hover)) };
    el.active(move |s| s.bg(fills.pressed))
}

/// Wraps a row; nested rows hang off a subtle 1 px guide under their folder icon.
pub(super) fn nest(row: impl IntoElement, nested: bool, cx: &App) -> AnyElement {
    if !nested {
        return div().w_full().pb_0p5().child(row).into_any_element();
    }
    div()
        .w_full()
        .pl(rems(NEST))
        .child(
            div()
                .w_full()
                .pl_1()
                .pb_0p5()
                .border_l_1()
                .border_color(cx.theme().sidebar_border.opacity(0.6))
                .child(row),
        )
        .into_any_element()
}

pub(super) fn tooltip(
    text: SharedString,
) -> impl Fn(&mut Window, &mut App) -> gpui_kit::AnyView + 'static {
    move |window, cx| Tooltip::new(text.clone()).build(window, cx)
}

/// Mango count pill pinned over a nav icon's top-right corner.
pub fn count_badge(id: impl Into<ElementId>, count: usize, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .top_0()
        .right_0()
        .min_w(px(16.))
        .h(px(16.))
        .px(px(3.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(crate::theme::mango_text(cx).opacity(0.18))
        .text_color(crate::theme::mango_text(cx))
        .text_size(px(10.))
        .line_height(px(16.))
        .cursor_pointer()
        .font_medium()
        .font_features(tabular())
        .child(if count > 99 { "99+".to_string() } else { count.to_string() })
}

pub(super) fn tabular() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

/// Visible while hovered (via `group`), keyboard-focused or its menu is open.
pub(super) fn reveal_on_hover<E: InteractiveElement + Styled>(
    el: E,
    group: SharedString,
    show: bool,
) -> E {
    if show { el } else { el.opacity(0.).group_hover(group, |s| s.opacity(1.)) }
}

/// Keeps a click on an inner control from also activating its row.
pub(super) fn swallow_click(id: impl Into<ElementId>, child: impl IntoElement) -> impl IntoElement {
    div().id(id).flex_none().on_click(|_, _, cx| cx.stop_propagation()).child(child)
}

#[cfg(test)]
#[path = "../../../tests/unit/chat/sidebar/chrome.rs"]
mod tests;
