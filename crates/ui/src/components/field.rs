use crate::theme::Corners;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, Size,
    input::{Input, InputState, Textarea, TextareaState},
};
use gpui_kit::{App, Entity, Hsla, IntoElement, prelude::*, px};

/// Fill of form controls: `foreground` at 6% reads as lighter gray in dark
/// mode and a soft darker wash in light mode.
pub fn field_fill(cx: &App) -> Hsla {
    cx.theme().foreground.opacity(0.06)
}

/// Filled, borderless text field.
fn shell(cx: &App) -> gpui_kit::Div {
    gpui_kit::div().w_full().bg(field_fill(cx)).corner(px(10.))
}

#[derive(IntoElement)]
pub struct Field {
    state: Entity<InputState>,
    icon: Option<IconName>,
    small: bool,
    aria: Option<&'static str>,
}

pub fn field(state: &Entity<InputState>) -> Field {
    Field { state: state.clone(), icon: None, small: false, aria: None }
}

impl Field {
    pub fn aria_label(mut self, label: &'static str) -> Self {
        self.aria = Some(label);
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }
}

impl Sizable for Field {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.small = !matches!(size.into(), Size::Medium | Size::Large);
        self
    }
}

impl RenderOnce for Field {
    fn render(self, _: &mut gpui_kit::Window, cx: &mut App) -> impl IntoElement {
        // Input pads itself 10px (medium) / 8px (small); outer px tops up to 12.
        let (h, pad, size) =
            if self.small { (32., 4., Size::Small) } else { (40., 2., Size::Medium) };
        let input = Input::new(&self.state)
            .appearance(false)
            .with_size(size)
            .when_some(self.aria, |i, l| i.aria_label(l));
        shell(cx)
            .h(px(h))
            .px(px(pad))
            .flex()
            .items_center()
            .when_some(self.icon, |d, icon| {
                d.child(
                    Icon::new(icon)
                        .small()
                        .text_color(cx.theme().muted_foreground)
                        .ml(px(10. - pad)),
                )
            })
            .child(gpui_kit::div().flex_1().min_w_0().child(input))
    }
}

#[derive(IntoElement)]
pub struct FieldArea {
    state: Entity<TextareaState>,
    aria: Option<&'static str>,
}

pub fn field_area(state: &Entity<TextareaState>) -> FieldArea {
    FieldArea { state: state.clone(), aria: None }
}

impl FieldArea {
    pub fn aria_label(mut self, label: &'static str) -> Self {
        self.aria = Some(label);
        self
    }
}

impl RenderOnce for FieldArea {
    fn render(self, _: &mut gpui_kit::Window, cx: &mut App) -> impl IntoElement {
        shell(cx).px(px(12.)).py(px(12.)).child(
            Textarea::new(&self.state)
                .appearance(false)
                .when_some(self.aria, |input, label| input.aria_label(label)),
        )
    }
}
