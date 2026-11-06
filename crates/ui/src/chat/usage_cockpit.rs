use crate::theme::Corners;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable, StyledExt, button::ButtonVariants, h_flex, v_flex,
};
use gpui_kit::rems;
use gpui_kit::{App, ClickEvent, RenderOnce, SharedString, Window, div, prelude::*, px, relative};
use std::rc::Rc;

pub type CockpitClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(Clone, Debug, PartialEq)]
pub struct UsageLimitRow {
    pub label: SharedString,
    pub percent_left: f32,
    pub threshold: Option<f32>,
    pub trend_down: bool,
    pub reset_desc: SharedString,
}

impl UsageLimitRow {
    pub fn new(
        label: impl Into<SharedString>,
        percent_left: f32,
        reset_desc: impl Into<SharedString>,
    ) -> Self {
        Self {
            label: label.into(),
            percent_left: percent_left.clamp(0.0, 100.0),
            threshold: Some(75.0),
            trend_down: true,
            reset_desc: reset_desc.into(),
        }
    }

    pub fn threshold(mut self, t: Option<f32>) -> Self {
        self.threshold = t;
        self
    }

    pub fn trend_down(mut self, down: bool) -> Self {
        self.trend_down = down;
        self
    }
}

/// A native, high-fidelity Usage Limits cockpit card matching the official agent limit UI.
#[derive(IntoElement)]
pub struct UsageCockpit {
    provider_title: SharedString,
    subscription_subtitle: SharedString,
    rows: Vec<UsageLimitRow>,
    footer_text: Option<SharedString>,
    footer_action: Option<SharedString>,
    on_close: Option<CockpitClickHandler>,
    on_action: Option<CockpitClickHandler>,
}

impl UsageCockpit {
    pub fn new(provider: impl Into<SharedString>, subtitle: impl Into<SharedString>) -> Self {
        Self {
            provider_title: provider.into(),
            subscription_subtitle: subtitle.into(),
            rows: Vec::new(),
            footer_text: None,
            footer_action: None,
            on_close: None,
            on_action: None,
        }
    }

    pub fn row(mut self, row: UsageLimitRow) -> Self {
        self.rows.push(row);
        self
    }

    pub fn rows(mut self, rows: Vec<UsageLimitRow>) -> Self {
        self.rows = rows;
        self
    }

    pub fn footer_text(mut self, text: Option<impl Into<SharedString>>) -> Self {
        self.footer_text = text.map(Into::into);
        self
    }

    pub fn footer_action(mut self, action: Option<impl Into<SharedString>>) -> Self {
        self.footer_action = action.map(Into::into);
        self
    }

    pub fn on_close(mut self, handler: Option<CockpitClickHandler>) -> Self {
        self.on_close = handler;
        self
    }

    pub fn on_action(mut self, handler: Option<CockpitClickHandler>) -> Self {
        self.on_action = handler;
        self
    }
}

impl RenderOnce for UsageCockpit {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let fg = cx.theme().foreground;
        let surface = cx.theme().popover;
        let border = cx.theme().border;
        let track = cx.theme().secondary;

        let on_close = self.on_close;
        let on_action = self.on_action;
        let footer_text = self.footer_text;
        let footer_action = self.footer_action;

        v_flex()
            .w_full()
            .corner(rems(0.75))
            .bg(surface)
            .border_1()
            .border_color(border)
            .text_color(fg)
            .px_4()
            .py_3()
            .gap_2p5()
            .child(
                // Header: [Gauge] Usage limits Provider · Subscription    [X]
                h_flex()
                    .justify_between()
                    .items_center()
                    .w_full()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .size(px(14.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(muted)
                                    .child(Icon::new(Lucide::Gauge).xsmall()),
                            )
                            .child(div().text_sm().font_bold().text_color(fg).child("Usage limits"))
                            .child(div().text_xs().text_color(muted).child(
                                if self.provider_title.is_empty() {
                                    self.subscription_subtitle
                                } else if self.subscription_subtitle.is_empty() {
                                    self.provider_title
                                } else {
                                    SharedString::from(format!(
                                        "{} · {}",
                                        self.provider_title, self.subscription_subtitle
                                    ))
                                },
                            )),
                    )
                    .child(
                        crate::components::button("close-usage-cockpit")
                            .ghost()
                            .small()
                            .icon(Lucide::Close)
                            .tooltip("Close usage limits")
                            .when_some(on_close, |btn, handler| {
                                btn.on_click(move |ev, window, cx| handler(ev, window, cx))
                            }),
                    ),
            )
            .children(self.rows.into_iter().map(|row| {
                let bar_width = row.percent_left.clamp(0.0, 100.0);
                let threshold = row.threshold;

                h_flex()
                    .items_center()
                    .gap_3()
                    .w_full()
                    .text_xs()
                    .child(div().w(px(64.)).flex_none().text_color(muted).child(row.label))
                    .child(
                        div()
                            .w(px(68.))
                            .flex_none()
                            .font_bold()
                            .text_color(fg)
                            .child(format!("{:.0}% left", row.percent_left)),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .h(px(8.))
                            .rounded_full()
                            .bg(track)
                            .overflow_hidden()
                            .child(
                                div().h_full().rounded_full().bg(fg).w(relative(bar_width / 100.0)),
                            )
                            .when_some(threshold, |track, t| {
                                track.child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .bottom_0()
                                        .w(px(1.5))
                                        .bg(muted)
                                        .left(relative(t / 100.0)),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .w(px(120.))
                            .flex_none()
                            .justify_end()
                            .items_center()
                            .gap_1()
                            .text_color(muted)
                            .child(
                                Icon::new(if row.trend_down {
                                    Lucide::TrendingDown
                                } else {
                                    Lucide::TrendingUp
                                })
                                .size(px(12.))
                                .text_color(muted)
                                .flex_none(),
                            )
                            .child(div().child(row.reset_desc)),
                    )
            }))
            .when(footer_text.is_some() || footer_action.is_some(), |card| {
                card.child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .pt_1()
                        .text_xs()
                        .child(div().text_color(muted).child(footer_text.unwrap_or_default()))
                        .when_some(footer_action, |row, act| {
                            row.child(
                                crate::components::button("cockpit-action")
                                    .secondary()
                                    .compact()
                                    .corner(px(6.))
                                    .px_2p5()
                                    .py_0p5()
                                    .child(div().text_xs().font_bold().child(act))
                                    .when_some(on_action, |btn, handler| {
                                        btn.on_click(move |ev, window, cx| handler(ev, window, cx))
                                    }),
                            )
                        }),
                )
            })
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/usage_cockpit.rs"]
mod tests;
