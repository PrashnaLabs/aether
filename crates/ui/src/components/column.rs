use gpui_kit::{AnyElement, App, Div, Window, div, prelude::*};

use crate::theme::CONTENT_WIDTH;

/// Centered column at the shared reading width.
#[derive(IntoElement)]
pub struct Column {
    base: Div,
}

impl Column {
    pub fn new() -> Self {
        Self { base: div().w_full().max_w(CONTENT_WIDTH).mx_auto().px_6() }
    }
}

impl Default for Column {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Column {
    fn style(&mut self) -> &mut gpui_kit::StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Column {
    fn interactivity(&mut self) -> &mut gpui_kit::Interactivity {
        self.base.interactivity()
    }
}

impl ParentElement for Column {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(elements);
    }
}

impl RenderOnce for Column {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
    }
}
