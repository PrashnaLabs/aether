use aether_ui::theme::Corners;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use aether_agent::SavedChat;
use aether_agent::usage_history::{
    self, MONTHLY_WINDOW, PricingStatus, UsageHistory, UsageProvider, UsageWindow, WINDOW_CHOICES,
};
use aether_ui::{ChoiceMenu, ChoiceOption, Segmented, composer_chip, overlay};
use chrono::{Local, NaiveDate};
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, StyledExt, button::ButtonVariants, h_flex,
    tooltip::Tooltip, v_flex,
};
use gpui_kit::{AnyElement, App, Context, Div, Hsla, Role, SharedString, div, prelude::*, px, rgb};

use crate::app::{ChatApp, Page};

mod chart;
mod daily;
mod format;
mod list;
mod monthly;
mod projects;
mod skeleton;

use daily::{usage_metric_strip, usage_quality_panel};
use format::{format_count, format_day_short, format_month_short, window_choice_label};
use skeleton::usage_skeleton;

/// Rendered chart height, matching Waku and T3 Code's `h-56` plot.
const CHART_HEIGHT: f32 = 224.0;
/// Sliver above the top gridline so a peak's 2px stroke is not shaved off.
const CHART_PLOT_TOP: f32 = 8.0;
const CHART_TICKS: usize = 4;
/// Width of the y-axis label gutter.
const CHART_GUTTER: f32 = 56.0;
/// Dashboard max width so the two-column daily chart and 6-provider breakdown fit comfortably.
const USAGE_MAX_WIDTH: f32 = 1040.0;
/// A snapshot older than this rescans when the page is next opened.
const USAGE_RESCAN_AFTER: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum UsageViewMode {
    #[default]
    Daily,
    Monthly,
    Projects,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum UsageMetric {
    #[default]
    Cost,
    Tokens,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum UsageBreakdown {
    #[default]
    Model,
    Day,
}

pub(crate) fn usage_provider_icon_path(provider: UsageProvider) -> &'static str {
    aether_core::Provider::ALL
        .into_iter()
        .find(|&p| UsageProvider::from_aether(p) == provider)
        .map_or("", aether_ui::provider_icon_path)
}

pub(crate) fn usage_provider_color(provider: UsageProvider, dark: bool) -> Hsla {
    match (provider, dark) {
        (UsageProvider::Claude, true) => rgb(0xE07A5F).into(),
        (UsageProvider::Claude, false) => rgb(0xB64A2B).into(),
        (UsageProvider::Codex, true) => rgb(0x60A5FA).into(),
        (UsageProvider::Codex, false) => rgb(0x2563EB).into(),
        (UsageProvider::Pi, true) => rgb(0x34D399).into(),
        (UsageProvider::Pi, false) => rgb(0x047857).into(),
        (UsageProvider::OpenCode, true) => rgb(0xA78BFA).into(),
        (UsageProvider::OpenCode, false) => rgb(0x7C3AED).into(),
        (UsageProvider::Antigravity, true) => rgb(0x22D3EE).into(),
        (UsageProvider::Antigravity, false) => rgb(0x0E7490).into(),
        (UsageProvider::OpenRouter, true) => rgb(0xFB7185).into(),
        (UsageProvider::OpenRouter, false) => rgb(0xBE123C).into(),
    }
}

fn usage_provider_colors(cx: &App) -> [Hsla; UsageProvider::COUNT] {
    let dark = cx.theme().is_dark();
    UsageProvider::ALL.map(|p| usage_provider_color(p, dark))
}

fn provider_mark(provider: UsageProvider, size_px: f32, color: Hsla) -> impl IntoElement {
    Icon::empty()
        .path(usage_provider_icon_path(provider))
        .size(px(size_px))
        .text_color(color)
        .flex_none()
}

fn provider_short_label(provider: UsageProvider) -> &'static str {
    match provider {
        UsageProvider::Claude => "Claude",
        UsageProvider::Codex => "Codex",
        UsageProvider::Pi => "Pi",
        UsageProvider::OpenCode => "OpenCode",
        UsageProvider::Antigravity => "AGY",
        UsageProvider::OpenRouter => "OpenRouter",
    }
}

impl ChatApp {
    pub(crate) fn toggle_usage_page(&mut self, cx: &mut Context<Self>) {
        if self.page == Page::Usage {
            self.open_page(Page::Chat, cx);
        } else {
            self.open_page(Page::Usage, cx);
        }
    }

    pub(crate) fn effective_usage_window(&self) -> UsageWindow {
        match self.usage_view {
            UsageViewMode::Monthly => MONTHLY_WINDOW,
            UsageViewMode::Daily | UsageViewMode::Projects => self.usage_window,
        }
    }

    pub(crate) fn ensure_usage_history(&mut self, force: bool, cx: &mut Context<Self>) {
        let window = self.effective_usage_window();
        let satisfied = self.usage_history.as_ref().is_some_and(|history| history.window == window)
            && self
                .usage_history_scanned_at
                .is_some_and(|scanned| scanned.elapsed() < USAGE_RESCAN_AFTER);
        if self.usage_history_pending_for == Some(window) {
            return;
        }
        if !force && satisfied {
            return;
        }
        self.usage_history_pending_for = Some(window);
        self.usage_history_generation += 1;
        let generation = self.usage_history_generation;
        let project_roots = self.known_project_roots();
        let saved_chats = self.saved_chat_snapshots();
        let fallback_prices = self.model_prices_snapshot();
        cx.spawn(async move |this, cx| {
            let history = cx
                .background_executor()
                .spawn(async move {
                    usage_history::scan_cached(
                        window,
                        &project_roots,
                        &saved_chats,
                        &fallback_prices,
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.usage_history_generation != generation {
                    return;
                }
                this.usage_history_pending_for = None;
                this.usage_chart_hover = None;
                this.usage_history_scanned_at = Some(Instant::now());
                this.usage_history = Some(history);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn known_project_roots(&self) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> =
            self.chats.iter().filter_map(|c| c.folder(&self.settings.workspace_root)).collect();
        if !self.settings.workspace_root.is_empty() {
            roots.push(PathBuf::from(&self.settings.workspace_root));
        }
        roots.sort();
        roots.dedup();
        roots
    }

    fn saved_chat_snapshots(&self) -> Vec<SavedChat> {
        self.chats
            .iter()
            .filter(|c| !c.messages.is_empty())
            .map(|c| SavedChat {
                id: c.id,
                title: c.title.to_string(),
                messages: c.messages.clone(),
                pinned: c.pinned,
                archived: c.archived,
                provider: c.provider,
                provider_model: c.provider_model.clone(),
                cli_session: c.cli_session.clone(),
                access: c.access_mode(),
                effort: c.effort_level().to_owned(),
                cwd: c.cwd_str().to_owned(),
                project: c.project_str().to_owned(),
                worktree_path: c.worktree_path_str().to_owned(),
                worktree_branch: c.worktree_branch_str().to_owned(),
                parent: c.parent,
                compaction_threshold_percent: c.compaction_threshold_percent,
                lead: c.lead,
                lead_seen: c.lead_seen,
                pending_deliveries: vec![],
                handoff_from: c.handoff_from,
            })
            .collect()
    }

    pub(crate) fn set_usage_window(&mut self, window: UsageWindow, cx: &mut Context<Self>) {
        if self.usage_window == window {
            return;
        }
        self.usage_window = window;
        self.ensure_usage_history(false, cx);
        cx.notify();
    }

    pub(crate) fn set_usage_view(&mut self, view: UsageViewMode, cx: &mut Context<Self>) {
        if self.usage_view == view {
            return;
        }
        self.usage_view = view;
        self.usage_expanded_models = None;
        self.ensure_usage_history(false, cx);
        cx.notify();
    }

    pub(crate) fn render_usage(&self, cx: &mut Context<Self>) -> AnyElement {
        let pending = self.usage_history_pending_for.is_some();
        let expected = self.effective_usage_window();
        let history = self.usage_history.as_ref().filter(|history| {
            matches!(
                (history.window, expected),
                (
                    UsageWindow::TrailingDays(_) | UsageWindow::ThisMonth | UsageWindow::LastMonth,
                    UsageWindow::TrailingDays(_) | UsageWindow::ThisMonth | UsageWindow::LastMonth
                ) | (UsageWindow::Months(_), UsageWindow::Months(_))
            )
        });
        let range = history
            .map(|h| (h.since_day, h.until_day))
            .unwrap_or_else(|| expected.bounds(Local::now().date_naive()));
        let fills_viewport =
            matches!(self.usage_view, UsageViewMode::Monthly | UsageViewMode::Projects);

        let mut content = div()
            .w_full()
            .max_w(px(USAGE_MAX_WIDTH))
            .mx_auto()
            .px_6()
            .py_8()
            .flex()
            .flex_col()
            .when(fills_viewport, |el| el.h_full().min_h_0())
            .child(self.render_usage_top_bar(cx))
            .child(self.render_usage_controls_bar(range, pending, cx));

        let Some(history) = history else {
            content = content.child(usage_skeleton(self.usage_view, cx));
            return v_flex()
                .id("usage-page")
                .size_full()
                .when(!fills_viewport, |p| p.overflow_y_scroll())
                .child(content)
                .into_any_element();
        };

        if !history.errors.is_empty() || history.pricing == PricingStatus::Unavailable {
            content = content.child(usage_notices(history, cx));
        }

        content = match self.usage_view {
            UsageViewMode::Daily => content
                .child(
                    div()
                        .mt(px(20.0))
                        .flex()
                        .items_start()
                        .gap(px(28.0))
                        .child(self.render_usage_summary(history, cx))
                        .child(self.render_usage_chart_column(history, cx)),
                )
                .child(usage_metric_strip(history, cx))
                .child(
                    div()
                        .mt(px(24.0))
                        .flex()
                        .items_start()
                        .gap(px(28.0))
                        .child(self.render_usage_breakdown(history, cx))
                        .child(usage_quality_panel(history, cx)),
                ),
            UsageViewMode::Monthly => content.child(self.render_usage_month_list(history, cx)),
            UsageViewMode::Projects => content.child(self.render_usage_projects(history, cx)),
        };

        let footer_text = format!(
            "{} files scanned ({} unchanged) · {} records in {:.1}s",
            format_count(history.scanned_files as u64),
            format_count(history.skipped_files as u64),
            format_count(history.records),
            history.scan_duration.as_secs_f64()
        );
        content = content.child(
            div()
                .mt(px(18.0))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(SharedString::from(footer_text)),
        );

        v_flex()
            .id("usage-page")
            .size_full()
            .when(!fills_viewport, |p| p.overflow_y_scroll())
            .child(content)
            .into_any_element()
    }

    fn render_usage_top_bar(&self, cx: &mut Context<Self>) -> Div {
        h_flex()
            .gap_2()
            .items_center()
            .child(
                aether_ui::button("usage-back")
                    .ghost()
                    .small()
                    .icon(IconName::ArrowLeft)
                    .tooltip("Back to chat")
                    .on_click(cx.listener(|this, _, _, cx| this.open_page(Page::Chat, cx))),
            )
            .child(div().flex_1().text_2xl().font_semibold().child("Usage"))
    }

    fn render_usage_controls_bar(
        &self,
        range: (NaiveDate, NaiveDate),
        pending: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let monthly = self.usage_view == UsageViewMode::Monthly;
        let theme = cx.theme();
        let border = theme.border;
        let muted = theme.muted_foreground;
        let ring = theme.ring;
        let hover_bg = overlay(cx);

        let view_idx = match self.usage_view {
            UsageViewMode::Daily => 0,
            UsageViewMode::Monthly => 1,
            UsageViewMode::Projects => 2,
        };
        let view_options = Segmented::new("usage-view-mode")
            .compact()
            .option("Daily")
            .option("Monthly")
            .option("Projects")
            .selected(view_idx)
            .on_change(cx.listener(|this, idx, _, cx| {
                let view = match *idx {
                    0 => UsageViewMode::Daily,
                    1 => UsageViewMode::Monthly,
                    _ => UsageViewMode::Projects,
                };
                this.set_usage_view(view, cx);
            }));

        let window_selector = (!monthly).then(|| {
            let selected = self.usage_window;
            let options = WINDOW_CHOICES
                .iter()
                .map(|&w| ChoiceOption {
                    mark: None,
                    icon: None,
                    image: None,
                    title: window_choice_label(w).into(),
                    description: None,
                    checked: w == selected,
                    disabled: false,
                })
                .collect();
            self.usage_window_menu
                .update(cx, |m, cx| m.sync(options, Some("Time window".into()), 180., cx));
            let trigger = composer_chip("usage-window-selector", false, cx)
                .label(window_choice_label(selected))
                .icon(IconName::ChevronDown)
                .h(px(28.0))
                .px(px(10.0))
                .border_1()
                .border_color(border)
                .corner(px(7.0));
            ChoiceMenu::new(&self.usage_window_menu, trigger).into_any_element()
        });

        let refresh_tip: SharedString = if pending {
            "Scanning transcripts…".into()
        } else {
            "Rescan usage transcripts".into()
        };
        let refresh = div()
            .id("usage-refresh")
            .tab_index(0)
            .role(Role::Button)
            .aria_label(refresh_tip.clone())
            .focus_visible(move |el| el.border_color(ring))
            .h(px(28.0))
            .px(px(8.0))
            .corner(px(7.0))
            .border_1()
            .border_color(border)
            .flex()
            .items_center()
            .cursor_pointer()
            .hover(|el| el.bg(hover_bg))
            .tooltip(move |window, cx| Tooltip::new(refresh_tip.clone()).build(window, cx))
            .child(
                Icon::new(if pending { IconName::LoaderCircle } else { IconName::RotateCw })
                    .size(px(13.0))
                    .text_color(muted),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.ensure_usage_history(true, cx);
            }));

        let range_label = if monthly {
            format!("{} – {}", format_month_short(range.0), format_month_short(range.1))
        } else {
            format!("{} – {}", format_day_short(range.0), format_day_short(range.1))
        };

        div()
            .mt(px(10.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(muted)
                    .child(SharedString::from(range_label)),
            )
            .child(view_options)
            .children(window_selector)
            .child(refresh)
    }
}

fn card_surface(cx: &App) -> Hsla {
    let theme = cx.theme();
    if theme.is_dark() { theme.secondary.opacity(0.5) } else { theme.background }
}

fn usage_notices(history: &UsageHistory, cx: &App) -> Div {
    let theme = cx.theme();
    let mut notice = div()
        .mt(px(14.0))
        .px(px(12.0))
        .py(px(8.0))
        .corner(px(8.0))
        .border_1()
        .border_color(theme.border)
        .flex()
        .flex_col()
        .gap(px(3.0))
        .text_xs()
        .text_color(theme.muted_foreground);
    for error in &history.errors {
        notice = notice.child(SharedString::from(error.clone()));
    }
    if history.pricing == PricingStatus::Unavailable {
        notice = notice.child(
            "Model pricing table is currently unavailable; only provider-reported costs are shown.",
        );
    }
    notice
}

#[cfg(test)]
#[path = "../../tests/unit/usage_view/mod.rs"]
mod tests;
