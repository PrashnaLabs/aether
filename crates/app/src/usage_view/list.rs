use aether_agent::usage_history::{ProviderDay, UsageHistory, UsageProvider};
use aether_ui::theme::Corners;
use aether_ui::{overlay, overlay_strong};
use gpui_kit::component::{ActiveTheme, Icon, IconName, tooltip::Tooltip};
use gpui_kit::gpui::relative;
use gpui_kit::{
    AnyElement, App, Context, Div, FontWeight, Hsla, Role, SharedString, div, prelude::*, px,
    transparent_black,
};

use crate::app::ChatApp;

use super::format::{count_noun, format_percent, format_tokens_compact, format_usd};
use super::{provider_mark, usage_provider_color};

impl ChatApp {
    pub(super) fn usage_models_control(
        &self,
        id: SharedString,
        top_models: &[(String, f64)],
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let label = usage_top_models_label(top_models)?;
        let model_count = top_models.len() as u64;
        let tip = SharedString::from(format!("Show {}", count_noun(model_count, "model")));
        let muted = cx.theme().muted_foreground;
        let hover_bg = overlay(cx);
        let active_bg = overlay_strong(cx);
        let ring = cx.theme().ring;
        let toggle_id = id.clone();

        Some(
            div()
                .id(SharedString::from(format!("{id}-trigger")))
                .tab_index(0)
                .role(Role::Button)
                .aria_label(tip.clone())
                .aria_expanded(expanded)
                .border_1()
                .border_color(transparent_black())
                .focus_visible(move |el| el.border_color(ring))
                .h(px(22.0))
                .max_w(px(300.0))
                .px(px(6.0))
                .corner(px(5.0))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(5.0))
                .cursor_pointer()
                .text_xs()
                .text_color(muted)
                .when(expanded, |el| el.bg(active_bg))
                .hover(|el| el.bg(hover_bg))
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .child(div().min_w_0().truncate().child(SharedString::from(label)))
                .child(
                    Icon::new(if expanded { IconName::ChevronUp } else { IconName::ChevronDown })
                        .size(px(10.0))
                        .text_color(muted),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.usage_expanded_models.as_ref() == Some(&toggle_id) {
                        this.usage_expanded_models = None;
                    } else {
                        this.usage_expanded_models = Some(toggle_id.clone());
                    }
                    cx.notify();
                }))
                .into_any_element(),
        )
    }
}

pub(super) fn rank_by_cost(history: &UsageHistory) -> bool {
    history.cost_usd > 0.0
}

pub(super) fn usage_headline_value(history: &UsageHistory, by_cost: bool) -> String {
    if by_cost {
        format_usd(history.cost_usd)
    } else {
        format_tokens_compact(history.total_tokens as f64)
    }
}

pub(super) fn usage_value_label(cost_usd: f64, total_tokens: u64, by_cost: bool) -> String {
    if by_cost { format_usd(cost_usd) } else { format_tokens_compact(total_tokens as f64) }
}

pub(super) fn usage_list_header(title: String, caption: String, total: String, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .px(px(20.0))
        .py(px(13.0))
        .border_b_1()
        .border_color(theme.border)
        .flex()
        .items_center()
        .gap(px(16.0))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground)
                        .child(SharedString::from(title)),
                )
                .child(
                    div()
                        .mt(px(3.0))
                        .truncate()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(SharedString::from(caption)),
                ),
        )
        .child(
            div()
                .flex_none()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.foreground)
                .child(SharedString::from(total)),
        )
}

pub(super) fn usage_list_empty_row(message: &'static str, cx: &App) -> Div {
    div()
        .py(px(24.0))
        .flex()
        .justify_center()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(message)
}

pub(super) fn usage_split_bar(
    colors: [Hsla; UsageProvider::COUNT],
    length: f32,
    by_provider: &[ProviderDay; UsageProvider::COUNT],
    by_cost: bool,
    cx: &App,
) -> Div {
    let values: [f64; UsageProvider::COUNT] =
        std::array::from_fn(|i| usage_provider_value(&by_provider[i], by_cost));
    let sum: f64 = values.iter().sum();
    let length = if length > 0.0 { length.clamp(0.02, 1.0) } else { 0.0 };
    let mut bar = div().h_full().w(relative(length)).rounded_full().overflow_hidden().flex();
    if sum > 0.0 {
        for (index, value) in values.into_iter().enumerate() {
            if value > 0.0 {
                bar = bar.child(div().h_full().w(relative((value / sum) as f32)).bg(colors[index]));
            }
        }
    }
    div().h(px(4.0)).w_full().rounded_full().bg(overlay(cx)).child(bar)
}

pub(super) fn usage_provider_value(entry: &ProviderDay, by_cost: bool) -> f64 {
    if by_cost { entry.cost_usd } else { entry.total_tokens as f64 }
}

pub(super) fn usage_provider_values(
    by_provider: &[ProviderDay; UsageProvider::COUNT],
    by_cost: bool,
    cx: &App,
) -> Div {
    let dark = cx.theme().is_dark();
    let muted = cx.theme().muted_foreground;
    let mut row = div().flex().flex_wrap().items_center().gap(px(14.0));
    for provider in UsageProvider::ALL {
        let entry = by_provider[provider.index()];
        if entry.total_tokens == 0 && entry.cost_usd <= 0.0 {
            continue;
        }
        let color = usage_provider_color(provider, dark);
        row = row.child(
            div()
                .flex()
                .items_center()
                .gap(px(5.0))
                .child(provider_mark(provider, 11.0, color))
                .child(div().text_xs().text_color(muted).child(SharedString::from(
                    usage_value_label(entry.cost_usd, entry.total_tokens, by_cost),
                ))),
        );
    }
    row
}

pub(crate) fn usage_top_models_label(top_models: &[(String, f64)]) -> Option<String> {
    if top_models.is_empty() {
        return None;
    }
    let mut label =
        top_models.iter().take(2).map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(" · ");
    if top_models.len() > 2 {
        label.push_str(&format!(" · +{}", top_models.len() - 2));
    }
    Some(label)
}

pub(super) fn usage_models_drawer(top_models: &[(String, f64)], total_cost: f64, cx: &App) -> Div {
    let theme = cx.theme();
    let border = theme.border;
    let fg = theme.foreground;
    let muted = theme.muted_foreground;
    let bg = overlay(cx);

    let mut drawer = div()
        .mt(px(8.0))
        .px(px(12.0))
        .py(px(6.0))
        .corner(px(8.0))
        .border_1()
        .border_color(border)
        .bg(bg)
        .flex()
        .flex_col();
    for (index, (name, cost_usd)) in top_models.iter().enumerate() {
        drawer = drawer.child(
            div()
                .py(px(5.0))
                .when(index > 0, |el| el.border_t_1().border_color(border))
                .flex()
                .items_center()
                .gap(px(12.0))
                .text_xs()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(fg)
                        .child(SharedString::from(name.clone())),
                )
                .when(total_cost > 0.0, |el| {
                    el.child(
                        div()
                            .flex_none()
                            .text_color(fg)
                            .child(SharedString::from(format_usd(*cost_usd))),
                    )
                    .child(
                        div()
                            .w(px(52.0))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .text_color(muted)
                            .child(SharedString::from(format_percent(*cost_usd / total_cost))),
                    )
                }),
        );
    }
    drawer
}

#[cfg(test)]
#[path = "../../tests/unit/usage_view/list.rs"]
mod tests;
