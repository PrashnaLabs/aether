use aether_core::{DateFormat, TimeFormat};
use aether_ui::theme::{Corners, danger_text, warning_text};
use chrono::Local;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::{Icon, IconName, Selectable, StyledExt, h_flex};
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, Hsla, MouseButton, SharedString, Window, div,
    prelude::*, px,
};

use super::{ChatApp, Page, SidebarView};

mod usage;

const GB: f64 = (1u64 << 30) as f64;

pub(super) fn resource_text(s: &crate::sysinfo::Sample) -> (String, String) {
    (
        format!("{:.0}%", s.cpu_percent),
        format!("{:.1}/{:.0} GB", s.mem_used as f64 / GB, s.mem_total as f64 / GB),
    )
}

fn resource_color(pct: f32, base: Hsla, cx: &App) -> Hsla {
    if pct >= 95. {
        danger_text(cx)
    } else if pct >= 80. {
        warning_text(cx)
    } else {
        base
    }
}

fn resource_item(
    id: &'static str,
    icon: Lucide,
    text: String,
    pct: f32,
    tip: String,
    cx: &App,
) -> impl IntoElement {
    let color = resource_color(pct, cx.theme().muted_foreground, cx);
    h_flex()
        .id(id)
        .debug_selector(move || id.into())
        .flex_none()
        .items_center()
        .gap(px(4.))
        .text_color(color)
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
        })
        .child(gpui_kit::svg().path(icon.path()).size(px(12.)).text_color(color))
        .child(text)
}

pub(crate) use usage::UsagePop;

pub(super) fn direct_label(count: usize) -> String {
    if count > 0 { format!("Direct Chats · {count}") } else { "Direct Chats".into() }
}

pub(crate) const STATUS_BAR: f32 = 30.;

fn sep(cx: &App) -> Div {
    div().w(px(1.)).h(px(14.)).bg(cx.theme().tokens.border).flex_none()
}

type NavClick = fn(&mut ChatApp, &mut Window, &mut Context<ChatApp>);

fn nav_button(
    id: &'static str,
    icon: impl Into<Icon>,
    label: impl Into<SharedString>,
    active: bool,
    view: &Entity<ChatApp>,
    on_click: impl Fn(&mut ChatApp, &mut Window, &mut Context<ChatApp>) + 'static,
    cx: &App,
) -> Div {
    let (view, label) = (view.clone(), label.into());
    div().debug_selector(move || id.into()).relative().flex_none().child(
        aether_ui::icon_chip(id, active, cx)
            .selected(active)
            .when(active, |chip| chip.debug_selector(move || format!("{id}-active")))
            .icon(icon)
            .accessibility_label(label.clone())
            .tooltip(label)
            .on_click(move |_, window, cx| view.update(cx, |this, cx| on_click(this, window, cx))),
    )
}

fn badged_nav_button(
    id: &'static str,
    icon: impl Into<Icon>,
    label: impl Into<SharedString>,
    active: bool,
    count: usize,
    view: &Entity<ChatApp>,
    on_click: NavClick,
    cx: &App,
) -> Div {
    let badge_view = view.clone();
    nav_button(id, icon, label, active, view, on_click, cx)
        .flex()
        .items_end()
        .w(px(34.))
        .h(px(28.))
        .when(count > 0, |slot| {
            let badge = SharedString::from(format!("{id}-count"));
            slot.child(
                aether_ui::count_badge(badge.clone(), count, cx)
                    .debug_selector(move || badge.into())
                    .on_click(move |_, window, cx| {
                        badge_view.update(cx, |this, cx| on_click(this, window, cx))
                    }),
            )
        })
}

impl ChatApp {
    fn render_status_nav(&self, view: &Entity<ChatApp>, cx: &App) -> Div {
        let (page, sidebar) = (self.page, self.sidebar_view);
        let (outside, direct) = (self.external.len(), self.direct_chat_count());
        let toggle = |target: Page| {
            move |this: &mut ChatApp, window: &mut Window, cx: &mut Context<ChatApp>| {
                if this.page == target {
                    this.open_page(Page::Chat, cx);
                    this.composer.update(cx, |c, cx| c.focus(window, cx));
                } else if target == Page::Notes {
                    this.open_notes(window, cx);
                } else {
                    this.open_page(target, cx);
                }
            }
        };
        h_flex()
            .flex_none()
            .items_center()
            .gap_1()
            .child(badged_nav_button(
                "nav-direct",
                Lucide::MessageCircle,
                direct_label(direct),
                sidebar == SidebarView::DirectChats,
                direct,
                view,
                |this, window, cx| this.switch_sidebar_to_direct_chats(window, cx),
                cx,
            ))
            .child(nav_button(
                "nav-archive",
                Lucide::Archive,
                "Archive",
                sidebar == SidebarView::Archived,
                view,
                |this, _, cx| this.switch_sidebar_to_archive(cx),
                cx,
            ))
            .when(outside > 0 || sidebar == SidebarView::Outside, |row| {
                row.child(badged_nav_button(
                    "nav-outside",
                    Lucide::Laptop,
                    aether_ui::outside_aria(outside),
                    sidebar == SidebarView::Outside,
                    outside,
                    view,
                    |this, _, cx| this.switch_sidebar_to_outside(cx),
                    cx,
                ))
            })
            .child(nav_button(
                "nav-terminal",
                IconName::SquareTerminal,
                "Terminal",
                page == Page::Terminal,
                view,
                |this, window, cx| this.toggle_terminal_page(window, cx),
                cx,
            ))
            .when(self.settings.notes_enabled, |row| {
                row.child(nav_button(
                    "nav-notes",
                    IconName::BookOpen,
                    "Notes",
                    page == Page::Notes,
                    view,
                    toggle(Page::Notes),
                    cx,
                ))
            })
            .when(self.settings.hackernews_enabled, |row| {
                row.child(nav_button(
                    "nav-hackernews",
                    Lucide::Flame,
                    "Hacker News",
                    page == Page::HackerNews,
                    view,
                    |this, window, cx| this.toggle_hackernews_page(window, cx),
                    cx,
                ))
            })
            .child(nav_button(
                "nav-settings",
                IconName::Settings,
                "Settings",
                page == Page::Settings,
                view,
                toggle(Page::Settings),
                cx,
            ))
    }

    pub(crate) fn direct_chat_count(&self) -> usize {
        self.chats.iter().filter(|c| !c.archived && !c.is_agent()).count()
    }

    pub(crate) fn render_global_status_bar(
        &self,
        view: &Entity<ChatApp>,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let border = theme.tokens.border;
        let bg = theme.tokens.sidebar;
        let fg = theme.tokens.muted_foreground;
        let primary_fg = theme.tokens.foreground;

        // Live Date and Time configured by user settings
        let now = Local::now();
        let show_seconds = self.settings.status_bar_show_seconds;
        let time_str = match (self.settings.status_bar_time_format, show_seconds) {
            (TimeFormat::Hour12, true) => now.format("%-I:%M:%S %p").to_string(),
            (TimeFormat::Hour12, false) => now.format("%-I:%M %p").to_string(),
            (TimeFormat::Hour24, true) => now.format("%H:%M:%S").to_string(),
            (TimeFormat::Hour24, false) => now.format("%H:%M").to_string(),
        };

        let date_str = match self.settings.status_bar_date_format {
            DateFormat::Short => Some(now.format("%a, %b %-d").to_string()),
            DateFormat::Numeric => Some(now.format("%Y-%m-%d").to_string()),
            DateFormat::Full => Some(now.format("%A, %B %-d, %Y").to_string()),
            DateFormat::Hidden => None,
        };

        let full_tooltip = format!(
            "{}\nTimezone: {}\nUTC: {}",
            now.format("%A, %B %-d, %Y · %-I:%M:%S %p"),
            now.offset(),
            now.naive_utc().format("%Y-%m-%d %H:%M:%S UTC")
        );

        let clock_click_view = view.clone();
        let nav = self.render_status_nav(view, cx);
        let usage = self.render_usage_strip(view, window, cx);
        let resources = self.sys_sample.map(|s| {
            let (cpu, mem) = resource_text(&s);
            let mem_pct = s.mem_used as f32 / s.mem_total.max(1) as f32 * 100.;
            let tip = format!(
                "CPU {:.1}%\nMemory {:.2} of {:.2} GB ({mem_pct:.0}%)",
                s.cpu_percent,
                s.mem_used as f64 / GB,
                s.mem_total as f64 / GB
            );
            let mut row = h_flex()
                .debug_selector(|| "sys-resources".into())
                .min_w_0()
                .overflow_hidden()
                .items_center()
                .gap(px(10.));
            // drops before the clock when narrow
            row.style().flex_shrink = Some(100.);
            row.child(resource_item("sys-cpu", Lucide::Cpu, cpu, s.cpu_percent, tip.clone(), cx))
                .child(resource_item("sys-mem", Lucide::MemoryStick, mem, mem_pct, tip, cx))
        });

        h_flex()
            .id("global-status-bar")
            .debug_selector(|| "status-bar".into())
            .flex_none()
            .h(px(STATUS_BAR))
            .w_full()
            .bg(bg)
            .border_t_1()
            .border_color(border)
            .px(px(10.))
            .items_center()
            .justify_between()
            .gap(px(8.))
            .text_size(px(11.))
            .text_color(fg)
            .child(nav)
            .child(sep(cx))
            .child(usage)
            .when(resources.is_some(), |row| row.child(sep(cx)))
            .children(resources)
            .child(sep(cx))
            // Right Side: Global Date & Time Clock
            .child(
                h_flex()
                    .min_w_0()
                    .overflow_hidden()
                    .items_center()
                    .gap(px(8.))
                    // Globally Visible Time & Date widget
                    .child(
                        h_flex()
                            .id("global-clock-widget")
                            .debug_selector(|| "status-clock".into())
                            .cursor_pointer()
                            .px(px(6.))
                            .py(px(2.))
                            .corner(px(4.))
                            .items_center()
                            .gap(px(6.))
                            .hover(|s| s.bg(theme.tokens.accent.opacity(0.15)))
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(full_tooltip.clone())
                                    .build(window, cx)
                            })
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                clock_click_view.update(cx, |this, cx| {
                                    this.update_settings(cx, |s| {
                                        s.status_bar_show_seconds = !s.status_bar_show_seconds;
                                    });
                                });
                            })
                            .when_some(date_str, |row, date| {
                                row.child(
                                    gpui_kit::svg()
                                        .path(Lucide::Calendar.path())
                                        .size(px(12.))
                                        .text_color(fg),
                                )
                                .child(div().font_medium().text_color(primary_fg).child(date))
                                .child(div().text_color(border).child("·"))
                            })
                            .child(
                                gpui_kit::svg()
                                    .path(Lucide::Clock.path())
                                    .size(px(12.))
                                    .text_color(fg),
                            )
                            .child(div().font_medium().text_color(primary_fg).child(time_str)),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/status_bar.rs"]
mod tests;
