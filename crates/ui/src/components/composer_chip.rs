use crate::theme::Corners;
use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonCustomVariant, ButtonVariants},
};
use gpui_kit::{App, ElementId, FontWeight, Hsla, prelude::*, px};

use crate::theme;

pub fn overlay(cx: &App) -> Hsla {
    cx.theme().foreground.opacity(0.05)
}

pub fn overlay_strong(cx: &App) -> Hsla {
    cx.theme().foreground.opacity(0.09)
}

/// A `composer_chip` that shows only an icon. The pill's side padding would leave
/// less room than the icon needs and clip it, so it is dropped for a square.
pub fn icon_chip(id: impl Into<ElementId>, active: bool, cx: &App) -> Button {
    composer_chip(id, active, cx).px(px(0.)).w(px(26.))
}

/// Control-row chip of the composer card: 28px, 12.5px medium text.
pub fn control_chip(id: impl Into<ElementId>, active: bool, cx: &App) -> Button {
    let fg = if active { theme::mango_text(cx) } else { cx.theme().muted_foreground };
    crate::components::button(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .foreground(fg)
                .hover(overlay_strong(cx))
                .active(overlay_strong(cx)),
        )
        .small()
        .h(px(28.))
        .px(px(9.))
        .corner(px(8.))
        .text_size(px(12.5))
        .font_weight(FontWeight::MEDIUM)
}

/// Borderless footer pill; open (`selected`) and hover share the soft overlay fill.
/// `active` swaps the label to the accent for on/off toggles.
pub fn composer_chip(id: impl Into<ElementId>, active: bool, cx: &App) -> Button {
    let fg = if active { theme::mango_text(cx) } else { cx.theme().muted_foreground };
    crate::components::button(id)
        .custom(ButtonCustomVariant::new(cx).foreground(fg).hover(overlay(cx)).active(overlay(cx)))
        .small()
        .h(px(26.))
        .px(px(7.))
        .corner(px(6.))
}
