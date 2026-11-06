use crate::theme::Corners;
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, FontWeight, Role, SharedString, Window, div, prelude::*, px, rems,
};

use crate::{overlay, overlay_strong, theme};

/// Centered heading with content (usually the composer) below it.
#[derive(IntoElement)]
pub struct Hero {
    heading: AnyElement,
    children: Vec<AnyElement>,
}

impl Hero {
    pub fn new(heading: impl IntoElement) -> Self {
        Self { heading: heading.into_any_element(), children: Vec::new() }
    }
}

impl ParentElement for Hero {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Hero {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        v_flex().id("hero").flex_1().min_h_0().w_full().overflow_y_scroll().child(
            v_flex()
                .w_full()
                .my_auto()
                .gap_8()
                .pb(px(96.))
                .child(self.heading)
                .children(self.children),
        )
    }
}

/// One-click starter prompts on an empty chat, as (label, prompt) pairs.
#[derive(IntoElement)]
pub struct Starters {
    prompts: &'static [(&'static str, &'static str)],
    on_pick: Rc<dyn Fn(&'static str, &mut Window, &mut App)>,
}

impl Starters {
    pub fn new(
        prompts: &'static [(&'static str, &'static str)],
        on_pick: impl Fn(&'static str, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self { prompts, on_pick: Rc::new(on_pick) }
    }
}

impl RenderOnce for Starters {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let on_pick = self.on_pick;
        h_flex()
            .w_full()
            .max_w(theme::CONTENT_WIDTH)
            .mx_auto()
            .px(px(24.))
            .justify_center()
            .flex_wrap()
            .gap(px(8.))
            .children(self.prompts.iter().enumerate().map(|(ix, &(label, prompt))| {
                let on_pick = on_pick.clone();
                crate::components::button(("starter", ix))
                    .outline()
                    .small()
                    .rounded(px(999.))
                    .label(label)
                    .tooltip(prompt)
                    .on_click(move |_, window, cx| on_pick(prompt, window, cx))
            }))
    }
}

pub enum CardHealth {
    Checking,
    Ready(SharedString),
    Degraded(SharedString),
    Missing,
}

/// One provider on the new-chat screen: version, health line and capability chips.
#[derive(IntoElement)]
pub struct HealthCard {
    id: SharedString,
    icon: Icon,
    name: SharedString,
    version: SharedString,
    health: CardHealth,
    chips: Vec<SharedString>,
    selected: bool,
    on_click: Rc<dyn Fn(&mut Window, &mut App)>,
}

impl HealthCard {
    pub fn new(
        id: impl Into<SharedString>,
        icon: Icon,
        name: impl Into<SharedString>,
        version: impl Into<SharedString>,
        health: CardHealth,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            icon,
            name: name.into(),
            version: version.into(),
            health,
            chips: Vec::new(),
            selected: false,
            on_click: Rc::new(on_click),
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn chip(mut self, label: impl Into<SharedString>) -> Self {
        self.chips.push(label.into());
        self
    }
}

impl RenderOnce for HealthCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (muted, fg, ring) = {
            let t = cx.theme();
            (t.muted_foreground, t.foreground, t.ring)
        };
        let (border, wash, mono, chip_fill) = {
            let t = cx.theme();
            (
                if self.selected { t.foreground } else { t.border },
                t.secondary,
                t.mono_font_family.clone(),
                t.muted,
            )
        };
        let (selected, hover, pressed) = (self.selected, overlay(cx), overlay_strong(cx));
        let hover_border = if selected { border } else { muted };
        let (icon, color, text): (AnyElement, _, SharedString) = match self.health {
            CardHealth::Checking => (
                Spinner::new().icon(Lucide::LoaderCircle).xsmall().color(muted).into_any_element(),
                muted,
                "Checking…".into(),
            ),
            CardHealth::Ready(t) => (
                Icon::new(Lucide::CircleCheck).size(px(13.)).into_any_element(),
                theme::success_text(cx),
                t,
            ),
            CardHealth::Degraded(t) => (
                Icon::new(Lucide::TriangleAlert).size(px(13.)).into_any_element(),
                theme::mango_text(cx),
                t,
            ),
            CardHealth::Missing => (
                Icon::new(Lucide::CircleX).size(px(13.)).into_any_element(),
                theme::danger_text(cx),
                "Not installed".into(),
            ),
        };
        let on_click = self.on_click;
        let label = format!(
            "{}, {text}{}",
            [self.name.as_ref(), self.version.as_ref()]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" "),
            if selected { ", selected" } else { "" }
        );
        v_flex()
            .id(self.id)
            .role(Role::Button)
            .tab_index(0)
            .aria_label(label)
            .flex_1()
            .flex_basis(px(220.))
            .min_w(px(220.))
            .gap(px(8.))
            .p(px(12.))
            .corner(px(12.))
            .border_1()
            .border_color(border)
            .bg(wash)
            .cursor_pointer()
            .hover(move |s| s.bg(hover).border_color(hover_border))
            .active(move |s| s.bg(pressed))
            .focus_visible(move |s| s.border_color(ring))
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(self.icon.size(px(16.)).flex_none())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(rems(0.8125))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(fg)
                            .child(self.name),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(rems(0.75))
                            .font_family(mono)
                            .text_color(muted)
                            .child(self.version),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_start()
                    .text_size(rems(0.75))
                    .text_color(color)
                    .child(div().flex_none().mt(px(4.)).child(icon))
                    .child(div().min_w_0().child(text)),
            )
            .child(h_flex().flex_wrap().gap(px(4.)).children(self.chips.into_iter().map(|chip| {
                div()
                    .px(px(8.))
                    .py(px(4.))
                    .corner(px(6.))
                    .bg(chip_fill)
                    .text_size(rems(0.75))
                    .text_color(muted)
                    .child(chip)
            })))
            .on_click(move |_, window, cx| on_click(window, cx))
    }
}
