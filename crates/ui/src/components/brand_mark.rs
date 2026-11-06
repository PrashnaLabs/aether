use gpui_kit::component::StyledExt;
use gpui_kit::{
    App, Pixels, SharedString, Window, div, linear_color_stop, linear_gradient, prelude::*, px,
    relative, rgb,
};

use crate::theme;

/// Square app mark: a glyph on the brand gradient.
#[derive(IntoElement)]
pub struct BrandMark {
    glyph: SharedString,
    size: Pixels,
}

impl BrandMark {
    pub fn new(glyph: impl Into<SharedString>) -> Self {
        Self { glyph: glyph.into(), size: px(24.) }
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for BrandMark {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .flex_none()
            .size(self.size)
            .flex()
            .items_center()
            .justify_center()
            .rounded(self.size * 0.28)
            .bg(linear_gradient(
                135.,
                linear_color_stop(theme::mango(), 0.),
                linear_color_stop(theme::papaya(), 1.),
            ))
            .text_color(rgb(0x3B1D0B))
            .text_size(self.size * 0.62)
            .line_height(relative(1.))
            .font_semibold()
            .child(self.glyph)
    }
}
