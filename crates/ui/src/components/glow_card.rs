use crate::theme::Corners;
use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::{
    AnyElement, App, BoxShadow, Window, black, div, linear_color_stop, linear_gradient, point,
    prelude::*, px,
};

use crate::theme;

/// A raised surface whose border turns into a mango gradient with a soft
/// glow while `focused`.
#[derive(IntoElement)]
pub struct GlowCard {
    focused: bool,
    children: Vec<AnyElement>,
}

impl GlowCard {
    pub fn new() -> Self {
        Self { focused: false, children: Vec::new() }
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
}

impl Default for GlowCard {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for GlowCard {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for GlowCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let dark = theme.is_dark();
        let mango = theme::mango();
        let ring = if self.focused {
            linear_gradient(
                135.,
                linear_color_stop(mango.opacity(0.85), 0.),
                linear_color_stop(theme::papaya().opacity(0.85), 1.),
            )
        } else {
            theme.border.into()
        };
        let lift = BoxShadow {
            color: black().opacity(if dark { 0. } else { 0.08 }),
            offset: point(px(0.), px(8.)),
            blur_radius: px(24.),
            spread_radius: px(-4.),
            inset: false,
        };
        let glow = BoxShadow {
            color: mango.opacity(if self.focused { 0.16 } else { 0. }),
            offset: point(px(0.), px(6.)),
            blur_radius: px(32.),
            spread_radius: px(0.),
            inset: false,
        };
        let surface = if dark { theme.secondary } else { theme.background };
        div().p(px(1.5)).corner(px(24.)).bg(ring).shadow(vec![lift, glow]).child(
            v_flex().gap_3().px_4().py_3().corner(px(22.5)).bg(surface).children(self.children),
        )
    }
}
