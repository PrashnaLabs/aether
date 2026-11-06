use aether_ui::theme::Corners;
use gpui_kit::rems;
use std::path::Path;

use aether_agent::usage_history::{ProjectSlice, UsageHistory, UsageProvider};
use aether_core::home_dir;
use aether_ui::field;
use gpui_kit::component::ActiveTheme;
use gpui_kit::{AnyElement, Context, Div, FontWeight, Hsla, SharedString, div, prelude::*, px};

use crate::app::ChatApp;

use super::format::{
    count_noun, format_day_short, format_percent, format_tokens_compact, usage_project_path,
};
use super::list::{
    rank_by_cost, usage_headline_value, usage_list_empty_row, usage_models_drawer,
    usage_provider_values, usage_split_bar, usage_value_label,
};
use super::{card_surface, usage_provider_colors};

impl ChatApp {
    pub(super) fn render_usage_projects(
        &self,
        history: &UsageHistory,
        cx: &mut Context<Self>,
    ) -> Div {
        let by_cost = rank_by_cost(history);
        let filter = self.usage_project_filter.read(cx).value().trim().to_ascii_lowercase();
        let filtered: Vec<(usize, &ProjectSlice)> = history
            .projects
            .iter()
            .enumerate()
            .filter(|(_, project)| {
                if filter.is_empty() {
                    return true;
                }
                let (name, _) = self.usage_project_identity(project);
                name.to_ascii_lowercase().contains(&filter)
                    || project.path.to_ascii_lowercase().contains(&filter)
            })
            .collect();
        let peak = filtered
            .iter()
            .map(|(_, p)| if by_cost { p.cost_usd } else { p.total_tokens as f64 })
            .fold(0.0_f64, f64::max);

        let caption = if filter.is_empty() {
            format!(
                "{} · {} tokens · {}",
                count_noun(history.projects.len() as u64, "project"),
                format_tokens_compact(history.total_tokens as f64),
                count_noun(history.sessions, "session")
            )
        } else {
            format!(
                "{} of {}",
                filtered.len(),
                count_noun(history.projects.len() as u64, "project")
            )
        };

        let colors = usage_provider_colors(cx);
        let body: AnyElement = if history.projects.is_empty() {
            div()
                .px(px(20.0))
                .child(usage_list_empty_row("No agent usage in this window.", cx))
                .into_any_element()
        } else if filtered.is_empty() {
            div()
                .px(px(20.0))
                .child(usage_list_empty_row("No projects match your filter.", cx))
                .into_any_element()
        } else {
            let count = filtered.len();
            let mut rows = div().px(px(20.0)).flex().flex_col();
            for (pos, (index, project)) in filtered.into_iter().enumerate() {
                let last = pos + 1 == count;
                rows = rows.child(
                    self.render_usage_project_row(index, project, peak, by_cost, colors, last, cx),
                );
            }
            div()
                .id("usage-projects-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(rows)
                .into_any_element()
        };

        let theme = cx.theme();
        let border = theme.border;
        let fg = theme.foreground;
        let muted = theme.muted_foreground;
        let card_bg = card_surface(cx);

        div()
            .mt(px(20.0))
            .flex_1()
            .min_h_0()
            .pb(px(8.0))
            .corner(rems(0.75))
            .border_1()
            .border_color(border)
            .bg(card_bg)
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .px(px(20.0))
                    .py(px(13.0))
                    .border_b_1()
                    .border_color(border)
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
                                    .text_color(fg)
                                    .child("By project"),
                            )
                            .child(
                                div()
                                    .mt(px(3.0))
                                    .flex()
                                    .items_baseline()
                                    .gap(px(6.0))
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(fg)
                                            .child(SharedString::from(usage_headline_value(
                                                history, by_cost,
                                            ))),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(SharedString::from(format!("· {caption}"))),
                                    ),
                            ),
                    )
                    .child(div().w(px(240.0)).flex_none().child(field(&self.usage_project_filter))),
            )
            .child(body)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_usage_project_row(
        &self,
        index: usize,
        project: &ProjectSlice,
        peak: f64,
        by_cost: bool,
        colors: [Hsla; UsageProvider::COUNT],
        last: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let (name, path_caption) = self.usage_project_identity(project);
        let row_value = if by_cost { project.cost_usd } else { project.total_tokens as f64 };
        let mut caption_parts = Vec::new();
        if by_cost && project.cost_share > 0.0 {
            caption_parts.push(format!("{} of cost", format_percent(project.cost_share)));
        }
        caption_parts
            .push(format!("{} tokens", format_tokens_compact(project.total_tokens as f64)));
        caption_parts.push(count_noun(project.sessions, "session"));
        if let Some(last_day) = project.last_day {
            caption_parts.push(format!("last active {}", format_day_short(last_day)));
        }
        let row_id = SharedString::from(format!(
            "usage-project-models-{}-{index}",
            self.usage_history_generation
        ));
        let expanded = self.usage_expanded_models.as_ref() == Some(&row_id);
        let models_control = self.usage_models_control(row_id, &project.top_models, expanded, cx);

        let theme = cx.theme();
        let border = theme.border;
        let fg = theme.foreground;
        let muted = theme.muted_foreground;

        div()
            .w_full()
            .py(px(13.0))
            .when(!last, |el| el.border_b_1().border_color(border))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg)
                            .child(SharedString::from(name)),
                    )
                    .when_some(path_caption, |el, path| {
                        el.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(muted)
                                .child(SharedString::from(path)),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex_none()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg)
                            .child(SharedString::from(usage_value_label(
                                project.cost_usd,
                                project.total_tokens,
                                by_cost,
                            ))),
                    ),
            )
            .child(
                div()
                    .mt(px(2.0))
                    .text_xs()
                    .text_color(muted)
                    .truncate()
                    .child(SharedString::from(caption_parts.join(" · "))),
            )
            .child(div().mt(px(9.0)).child(usage_split_bar(
                colors,
                if peak <= 0.0 { 0.0 } else { (row_value / peak) as f32 },
                &project.by_provider,
                by_cost,
                cx,
            )))
            .child(
                div()
                    .mt(px(7.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(usage_provider_values(&project.by_provider, by_cost, cx))
                    .child(div().flex_1())
                    .when_some(models_control, |el, control| el.child(control)),
            )
            .when(expanded, |el| {
                el.child(usage_models_drawer(&project.top_models, project.cost_usd, cx))
            })
    }

    fn usage_project_identity(&self, project: &ProjectSlice) -> (String, Option<String>) {
        if project.path.is_empty() {
            return ("Other sessions".to_owned(), None);
        }
        let path = Path::new(&project.path);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| project.path.clone());
        let home = home_dir();
        let home_opt = (!home.as_os_str().is_empty()).then_some(home.as_path());
        (name, Some(usage_project_path(path, home_opt)))
    }
}
