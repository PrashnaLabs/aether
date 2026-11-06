use aether_ui::theme::Corners;
use aether_ui::{overlay, overlay_strong};
use gpui_kit::component::ActiveTheme;
use gpui_kit::rems;
use gpui_kit::{AnyElement, App, div, prelude::*, px};

use super::{CHART_HEIGHT, UsageViewMode, card_surface};

pub(super) fn usage_skeleton(view: UsageViewMode, cx: &App) -> AnyElement {
    let bar_bg = overlay_strong(cx);
    let soft_bg = overlay(cx);
    let card_bg = card_surface(cx);
    let border = cx.theme().border;

    let bar = |width: f32, height: f32| {
        div().h(px(height)).w(px(width)).flex_none().rounded(px(height / 2.0)).bg(bar_bg)
    };
    let track = || div().h(px(4.0)).w_full().flex_none().rounded_full().bg(bar_bg);

    match view {
        UsageViewMode::Daily => {
            let provider_group = || {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(7.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(bar(110.0, 12.0))
                            .child(div().flex_1())
                            .child(bar(56.0, 12.0)),
                    )
                    .child(track())
                    .child(bar(150.0, 8.0))
            };
            div()
                .mt(px(20.0))
                .flex()
                .items_start()
                .gap(px(28.0))
                .child(
                    div()
                        .w(px(290.0))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap(px(18.0))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(6.0))
                                .child(bar(96.0, 9.0))
                                .child(bar(150.0, 26.0))
                                .child(bar(180.0, 9.0)),
                        )
                        .child(provider_group())
                        .child(provider_group())
                        .child(provider_group()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(320.0))
                        .flex()
                        .flex_col()
                        .gap(px(10.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .child(bar(90.0, 12.0))
                                .child(div().flex_1())
                                .child(bar(150.0, 10.0)),
                        )
                        .child(div().h(px(CHART_HEIGHT)).w_full().corner(px(8.0)).bg(soft_bg))
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .child(bar(40.0, 8.0))
                                .child(bar(40.0, 8.0))
                                .child(bar(40.0, 8.0)),
                        ),
                )
                .into_any_element()
        }
        UsageViewMode::Monthly | UsageViewMode::Projects => {
            let row = || {
                div()
                    .py(px(13.0))
                    .flex()
                    .flex_col()
                    .gap(px(7.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(bar(110.0, 12.0))
                            .child(div().flex_1())
                            .child(bar(64.0, 12.0)),
                    )
                    .child(bar(210.0, 8.0))
                    .child(track())
            };
            let mut card = div()
                .mt(px(20.0))
                .px(px(20.0))
                .corner(rems(0.75))
                .border_1()
                .border_color(border)
                .bg(card_bg)
                .flex()
                .flex_col()
                .child(
                    div().py(px(13.0)).flex().items_center().gap(px(16.0)).child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(5.0))
                            .child(bar(90.0, 12.0))
                            .child(bar(170.0, 8.0)),
                    ),
                );
            for _ in 0..4 {
                card = card.child(row());
            }
            card.into_any_element()
        }
    }
}
