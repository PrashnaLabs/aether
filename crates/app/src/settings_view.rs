use crate::settings_search::{SettingItem, search_settings, tab_match_count};
use aether_core::{
    AccessMode, Appearance, ChatLayout, CornerStyle, DateFormat, IconPack, NotesEditor, OrgHire,
    OrgLayout, Reasoning, Settings, THEME_FAMILIES, TerminalCursor, TerminalTheme, ThemeFamily,
    TileLayout, TimeFormat, ToolPolicy, WatchCorner, family_index,
};
use aether_driver::{DriverStartOptions, ProviderProbe};
use aether_git::Listed;
use aether_ui::schemes::{scheme_thumbnail, thumbnail_sides};
use aether_ui::theme::Corners;
use aether_ui::{
    AgentSelector, CAROUSEL_CARD_WIDTH, CardCarousel, CardRow, ChoiceCard, Segmented, SettingsPage,
    SettingsRow, SettingsSection, field, field_area, field_fill, overlay_strong,
};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::select::Select;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable, StyledExt,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    h_flex,
    slider::Slider,
    switch::Switch,
    v_flex,
};
use gpui_kit::rems;
use gpui_kit::{
    Anchor, Context, PathPromptOptions, Role, SharedString, Window, div, prelude::*, px,
};

use crate::app::{CLIS, ChatApp, display_path};
use crate::assets::{PREVIEW_ICONS, active_icon_pack, pack_icon_path};

const SETTINGS_DROPDOWN_WIDTH: f32 = 280.;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum SettingsTab {
    #[default]
    Appearance,
    BottomBar,
    Tiling,
    Composer,
    Model,
    Providers,
    Worktrees,
    Attachments,
    Tools,
    Organization,
    Terminal,
}

impl SettingsTab {
    pub(crate) const ALL: [Self; 11] = [
        Self::Appearance,
        Self::BottomBar,
        Self::Tiling,
        Self::Composer,
        Self::Model,
        Self::Providers,
        Self::Worktrees,
        Self::Attachments,
        Self::Tools,
        Self::Organization,
        Self::Terminal,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::BottomBar => "Bottom Bar",
            Self::Tiling => "Tiling",
            Self::Composer => "Composer",
            Self::Model => "Direct Chat",
            Self::Providers => "Providers",
            Self::Worktrees => "Worktrees",
            Self::Attachments => "Attachments",
            Self::Tools => "Tools",
            Self::Organization => "Organization",
            Self::Terminal => "Terminal",
        }
    }
}

pub(crate) const SETTINGS_GROUPS: [(&str, &[SettingsTab]); 2] = [
    (
        "General",
        &[
            SettingsTab::Appearance,
            SettingsTab::BottomBar,
            SettingsTab::Tiling,
            SettingsTab::Composer,
            SettingsTab::Model,
        ],
    ),
    (
        "Developer",
        &[
            SettingsTab::Providers,
            SettingsTab::Worktrees,
            SettingsTab::Attachments,
            SettingsTab::Tools,
            SettingsTab::Organization,
            SettingsTab::Terminal,
        ],
    ),
];

fn next_settings_tab(current: SettingsTab, next: bool) -> SettingsTab {
    let index = SettingsTab::ALL.iter().position(|&tab| tab == current).unwrap_or(0);
    SettingsTab::ALL
        [(index + if next { 1 } else { SettingsTab::ALL.len() - 1 }) % SettingsTab::ALL.len()]
}

impl ChatApp {
    pub(crate) fn render_settings(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        use crate::actions::{SettingsDismiss, SettingsSearch};
        let query = self.settings_search_input.read(cx).value();
        let searching = !query.trim().is_empty();
        let matches = if searching { search_settings(&query) } else { Vec::new() };
        let mut page =
            SettingsPage::new(if searching { "Search Results" } else { self.settings_tab.label() })
                .sidebar(self.render_settings_sidebar(&matches, searching, window, cx))
                .subtitle(if searching {
                    format!("{} matches for \"{}\"", matches.len(), query)
                } else {
                    self.settings_tab.description().to_string()
                });
        if searching {
            page = page.action(
                aether_ui::button("clear-settings-search")
                    .ghost()
                    .small()
                    .label("Clear")
                    .accessibility_label("Clear settings search")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.clear_settings_search(window, cx)),
                    ),
            );
            if matches.is_empty() {
                page = page.child(
                    v_flex()
                        .id("settings-empty")
                        .debug_selector(|| "settings-empty".into())
                        .items_center()
                        .gap_3()
                        .py_8()
                        .child(div().text_lg().font_medium().child("No settings found"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("Try another search or clear the filter."),
                        )
                        .child(
                            aether_ui::button("empty-clear-settings-search")
                                .debug_selector(|| "settings-empty-clear".into())
                                .small()
                                .label("Clear search")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.clear_settings_search(window, cx)
                                })),
                        ),
                );
            } else {
                for tab in SettingsTab::ALL {
                    if tab_match_count(tab, &matches) == 0 {
                        continue;
                    }
                    page = page.child(
                        v_flex()
                            .gap_4()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .child(div().text_lg().font_semibold().child(tab.label()))
                                    .children(self.settings_category_action(tab, cx)),
                            )
                            .child(self.render_settings_category(tab, Some(&matches), cx)),
                    );
                }
            }
        } else {
            if let Some(action) = self.settings_category_action(self.settings_tab, cx) {
                page = page.action(action);
            }
            page = page.child(self.render_settings_category(self.settings_tab, None, cx));
        }
        div()
            .id("settings-root")
            .size_full()
            .key_context("Settings")
            .on_action(cx.listener(|this, _: &SettingsDismiss, window, cx| {
                this.dismiss_settings(window, cx)
            }))
            .on_action(cx.listener(|this, _: &SettingsSearch, window, cx| {
                this.settings_search_input.update(cx, |input, cx| input.focus(window, cx));
            }))
            .child(page)
    }

    fn settings_category_action(
        &self,
        tab: SettingsTab,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        match tab {
            SettingsTab::Providers => Some(
                (aether_ui::button("refresh-providers")
                    .accessibility_label("Refresh provider status")
                    .ghost()
                    .small()
                    .icon(IconName::RefreshCw)
                    .label("Refresh")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.probe_providers(&CLIS, window, cx)),
                    ))
                .into_any_element(),
            ),
            SettingsTab::Worktrees => Some(
                (aether_ui::button("refresh-worktrees")
                    .accessibility_label("Refresh worktrees")
                    .ghost()
                    .small()
                    .icon(IconName::RefreshCw)
                    .label("Refresh")
                    .loading(self.listing_worktrees)
                    .on_click(cx.listener(|this, _, _, cx| this.load_worktrees(cx))))
                .into_any_element(),
            ),
            _ => None,
        }
    }

    fn render_theme_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.settings.appearance;
        let selected = family_index(active);
        let cards = THEME_FAMILIES.iter().enumerate().map(|(ix, family)| {
            let shown = family.shown(active);
            let (left, right) = thumbnail_sides(family, shown);
            ChoiceCard::new(format!("theme-card-{}", shown.slug()), shown.label())
                .selected(ix == selected)
                .preview(scheme_thumbnail(left, right))
                .when(family.variants.len() > 1, |card| {
                    card.label_element(theme_variant_picker(family, shown, ix == selected, cx))
                })
                .on_click(
                    cx.listener(move |this, _, window, cx| this.set_appearance(shown, window, cx)),
                )
        });
        div().id("theme").debug_selector(|| "theme".into()).child(
            SettingsSection::new("Theme")
                .icon(Icon::new(gpui_kit::assets::IconName::Palette))
                .child(
                    div()
                        .p_4()
                        .child(CardCarousel::new("theme-carousel", selected).children(cards)),
                ),
        )
    }

    fn render_icon_pack_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.settings.icon_pack;
        let pending = active != active_icon_pack();
        let fg = cx.theme().foreground;
        let cards = IconPack::ALL.map(|pack| {
            let preview = h_flex()
                .h(px(56.))
                .w_full()
                .flex_wrap()
                .gap_1()
                .items_center()
                .content_center()
                .justify_center()
                .text_color(fg)
                .children(
                    PREVIEW_ICONS
                        .map(|name| Icon::empty().path(pack_icon_path(pack, name)).size_4()),
                );
            ChoiceCard::new(format!("icon-pack-card-{}", pack.slug()), pack.label())
                .selected(pack == active)
                .preview(preview)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.update_settings(cx, |s| s.icon_pack = pack);
                }))
        });
        div().id("icon-pack").debug_selector(|| "icon-pack".into()).child(
            SettingsSection::new("Icon pack")
                .icon(Icon::new(gpui_kit::assets::IconName::Grid2x2))
                .child(
                    v_flex()
                        .p_4()
                        .gap_2()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div().text_xs().text_color(cx.theme().muted_foreground).child(
                                        if pending {
                                            "Restart to apply."
                                        } else {
                                            "Applies on next launch."
                                        },
                                    ),
                                )
                                .when(pending, |row| {
                                    row.child(
                                        aether_ui::button("icon-pack-restart")
                                            .debug_selector(|| "icon-pack-restart".into())
                                            .small()
                                            .label("Restart now")
                                            .on_click(|_, _, cx| cx.restart()),
                                    )
                                }),
                        )
                        .child(CardRow::new(cards)),
                ),
        )
    }

    fn render_corners_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.settings.corner_style;
        let line = cx.theme().muted_foreground;
        let cards = CornerStyle::ALL.map(|style| {
            let preview = div().h(px(76.)).w_full().relative().child(
                div()
                    .absolute()
                    .top(px(24.))
                    .left(px(40.))
                    .right_0()
                    .bottom_0()
                    .border_t_2()
                    .border_l_2()
                    .border_color(line)
                    .rounded_tl(px(16. * style.scale())),
            );
            ChoiceCard::new(format!("corners-card-{}", style.label().to_lowercase()), style.label())
                .selected(style == active)
                .preview(preview)
                .on_click(
                    cx.listener(move |this, _, window, cx| {
                        this.set_corner_style(style, window, cx)
                    }),
                )
        });
        div().id("corners").debug_selector(|| "corners".into()).child(
            SettingsSection::new("Corners")
                .icon(Icon::new(gpui_kit::assets::IconName::SquareRoundCorner))
                .child(div().p_4().child(CardRow::new(cards))),
        )
    }

    fn render_settings_category(
        &self,
        tab: SettingsTab,
        matches: Option<&[&SettingItem]>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let settings = &self.settings;
        let muted = cx.theme().muted_foreground;
        let show = |id: &str| {
            matches.is_none_or(|items| items.iter().any(|item| item.tab == tab && item.id == id))
        };
        let mut page = v_flex().gap_8();
        match tab {
            SettingsTab::Appearance => {
                page = page.when(show("theme"), |page| page.child(self.render_theme_section(cx)));
                page = page
                    .when(show("icon-pack"), |page| page.child(self.render_icon_pack_section(cx)));
                page =
                    page.when(show("corners"), |page| page.child(self.render_corners_section(cx)));
                page = page.when(show("font") || show("font-size"), |page| {
                    page.child(
                        SettingsSection::new("Text")
                            .icon(Icon::new(gpui_kit::assets::IconName::ALargeSmall))
                            .when(show("font"), |section| {
                                section.child(
                                    div().id("font").debug_selector(|| "font".into()).child(
                                        SettingsRow::new("Font")
                                            .description("Default follows the system.")
                                            .control(
                                                Select::new(&self.font_select)
                                                    .w(px(SETTINGS_DROPDOWN_WIDTH))
                                                    .h(px(40.))
                                                    .px(px(12.))
                                                    .corner(px(10.))
                                                    .bg(aether_ui::field_fill(cx))
                                                    .border_color(gpui_kit::transparent_black())
                                                    .menu_width(px(SETTINGS_DROPDOWN_WIDTH))
                                                    .search_placeholder("Search a font"),
                                            ),
                                    ),
                                )
                            })
                            .when(show("font-size"), |section| {
                                section.child(
                                    div()
                                        .id("font-size")
                                        .debug_selector(|| "font-size".into())
                                        .child(
                                            SettingsRow::new("Font size")
                                                .description("Scales text across the app.")
                                                .control(settings_dropdown(
                                                    "font-size",
                                                    "App font size",
                                                    settings.font_size_index(),
                                                    ["Small", "Default", "Large", "Extra large"],
                                                    cx.listener(|this, ix: &usize, window, cx| {
                                                        this.set_font_size(
                                                            Settings::FONT_SIZES[*ix],
                                                            window,
                                                            cx,
                                                        );
                                                    }),
                                                    cx,
                                                )),
                                        ),
                                )
                            }),
                    )
                });
                page = page.when(show("window-opacity") || show("window-blur"), |page| {
                    page.child(
                        SettingsSection::new("Window")
                            .icon(Icon::new(gpui_kit::assets::IconName::AppWindow))
                            .when(show("window-opacity"), |section| {
                                section.child(
                                    div()
                                        .id("window-opacity")
                                        .debug_selector(|| "window-opacity".into())
                                        .child(
                                            SettingsRow::new("Opacity")
                                                .description("How see-through the window is.")
                                                .control(
                                                    h_flex()
                                                        .gap_3()
                                                        .items_center()
                                                        .child(div().w(px(160.)).child(
                                                            Slider::new(&self.opacity_slider),
                                                        ))
                                                        .child(
                                                            div()
                                                                .w(px(40.))
                                                                .text_sm()
                                                                .text_color(muted)
                                                                .child(format!(
                                                                    "{}%",
                                                                    settings
                                                                        .clamped_window_opacity()
                                                                )),
                                                        ),
                                                ),
                                        ),
                                )
                            })
                            .when(cfg!(target_os = "macos") && show("window-blur"), |section| {
                                section.child(
                                    div()
                                        .id("window-blur")
                                        .debug_selector(|| "window-blur".into())
                                        .child(
                                            SettingsRow::new("Blur window")
                                                .description(
                                                    "Blur what shows through a translucent window.",
                                                )
                                                .control(
                                                    Switch::new("window-blur-setting")
                                                        .checked(settings.window_blur)
                                                        .disabled(
                                                            settings.clamped_window_opacity()
                                                                >= 100,
                                                        )
                                                        .accessibility_label("Blur window")
                                                        .on_click(cx.listener(
                                                            |this, checked: &bool, window, cx| {
                                                                this.set_window_blur(
                                                                    *checked, window, cx,
                                                                );
                                                            },
                                                        )),
                                                ),
                                        ),
                                )
                            }),
                    )
                });
                let watch_corner = show("watch-corner") && settings.watch_enabled;
                page = page.when(show("notifications") || show("watch-enabled") || watch_corner, |page| { page.child(SettingsSection::new("Appearance").icon(Icon::new(gpui_kit::assets::IconName::Settings))
                        .when(show("notifications"), |section| { section.child(div().id("notifications").debug_selector(|| "notifications".into()).child(SettingsRow::new("Notifications")
                                .description(
                                    "Notify when a chat finishes, fails or needs you while you're looking elsewhere.",
                                )
                                .control(
                                    Switch::new("notifications-setting")
                                        .checked(settings.notifications)
                                        .accessibility_label("Desktop notifications")
                                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                            let checked = *checked;
                                            this.update_settings(cx, |s| s.notifications = checked);
                                        })),
                                ))) })
                        .when(show("watch-enabled"), |section| { section.child(div().id("watch-enabled").debug_selector(|| "watch-enabled".into()).child(SettingsRow::new("Watch list")
                                .description("Shows the corner button, its shortcut, and Watch in session and terminal menus.")
                                .control(
                                    Switch::new("watch-enabled-setting")
                                        .checked(settings.watch_enabled)
                                        .accessibility_label("Watch list")
                                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                            this.set_watch_enabled(*checked, cx);
                                        })),
                                ))) })
                        .when(watch_corner, |section| { section.child(div().id("watch-corner").debug_selector(|| "watch-corner".into()).child(SettingsRow::new("Watch-list button")
                                .description("Which window corner holds the watch-list button.")
                                .control(
                                    settings_dropdown(
                                        "watch-corner", "Watch-list button position",
                                        WatchCorner::ALL.iter().position(|c| *c == settings.watch_corner).unwrap_or(0),
                                        WatchCorner::ALL.map(|c| c.label()),
                                        cx.listener(|this, ix: &usize, _, cx| {
                                            this.set_watch_corner(WatchCorner::ALL[*ix], cx);
                                        }),
                                        cx,
                                    ),
                                ))) })) });
                let chat_detail = aether_core::ChatDetail::ALL
                    .iter()
                    .position(|d| *d == settings.chat_detail)
                    .unwrap_or(1);
                page = page.when(show("show-hooks") || show("show-running-agents") || show("chat-detail"), |page| { page.child(SettingsSection::new("Chat").icon(Icon::new(gpui_kit::assets::IconName::MessageSquare))
                        .when(show("chat-detail"), |section| section.child(div().id("chat-detail").debug_selector(|| "chat-detail".into()).child(SettingsRow::new("Detail level")
                                .description("Compact hides thinking. Verbose opens every step, tool output and thought, and keeps finished replies unfolded.")
                                .control(settings_dropdown(
                                    "chat-detail",
                                    "Detail level",
                                    chat_detail,
                                    aether_core::ChatDetail::ALL.map(|d| d.label()),
                                    cx.listener(|this, ix: &usize, _, cx| {
                                        this.update_settings(cx, |s| {
                                            s.chat_detail = aether_core::ChatDetail::ALL[*ix]
                                        });
                                    }),
                                    cx,
                                )),
                        )))
                        .when(show("show-running-agents"), |section| { section.child(div().id("show-running-agents").debug_selector(|| "show-running-agents".into()).child(SettingsRow::new("Show running agents")
                                .description("List running chats in the sidebar's Agents section.")
                                .control(
                                    Switch::new("show-running-agents-setting")
                                        .checked(settings.show_running_agents)
                                        .accessibility_label("Show running agents")
                                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                            let checked = *checked;
                                            this.update_settings(cx, |s| s.show_running_agents = checked);
                                        })),
                                ))) })
                        .when(show("show-hooks"), |section| section.child(div().id("show-hooks").debug_selector(|| "show-hooks".into()).child(SettingsRow::new("Show hooks")
                                .description("List hook runs in a reply's steps. Failed hooks always show.")
                                .control(
                                    Switch::new("show-hooks-setting")
                                        .checked(settings.show_hooks)
                                        .accessibility_label("Show hooks")
                                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                            let checked = *checked;
                                            this.update_settings(cx, |s| s.show_hooks = checked);
                                        })),
                                ))))) });
                let sidebar_style = aether_core::SidebarStyle::ALL
                    .iter()
                    .position(|s| *s == settings.sidebar_style)
                    .unwrap_or(0);
                page = page.when(show("sidebar-style"), |page| {
                    page.child(SettingsSection::new("Sidebar").icon(Icon::new(gpui_kit::assets::IconName::PanelLeft)).child(
                        div().id("sidebar-style").debug_selector(|| "sidebar-style".into()).child(
                            SettingsRow::new("Style")
                                .description(
                                    "Default shows project folders. Minimal lists chats by recency or project. Focus groups them by what they need from you.",
                                )
                                .control(settings_dropdown(
                                    "sidebar-style",
                                    "Sidebar style",
                                    sidebar_style,
                                    aether_core::SidebarStyle::ALL.map(|s| s.label()),
                                    cx.listener(|this, ix: &usize, _, cx| {
                                        this.update_settings(cx, |s| {
                                            s.sidebar_style = aether_core::SidebarStyle::ALL[*ix]
                                        });
                                    }),
                                    cx,
                                )),
                        ),
                    ))
                });
                let chat_layout =
                    ChatLayout::ALL.iter().position(|l| *l == settings.chat_layout).unwrap_or(0);
                let right_panel_share = [0.3_f32, 0.4, 0.5, 0.6, 0.7]
                    .iter()
                    .position(|w| *w == settings.right_panel_share)
                    .unwrap_or(3);
                page = page.when(show("chat-layout") || show("org-layout") || show("right-panel-width"), |page| {
                    page.child(
                        SettingsSection::new("Chat layout")
                            .icon(Icon::new(gpui_kit::assets::IconName::PanelLeft))
                            .when(show("chat-layout"), |section| {
                                section.child(
                                    div()
                                        .id("chat-layout")
                                        .debug_selector(|| "chat-layout".into())
                                        .child(
                                            SettingsRow::new("Chat layout")
                                                .description("Tabbed uses tabs for tools. Sidebar keeps chat visible with tools in a right panel.")
                                                .control(settings_dropdown(
                                                    "chat-layout",
                                                    "Chat layout",
                                                    chat_layout,
                                                    ChatLayout::ALL.map(|l| l.label()),
                                                    cx.listener(|this, ix: &usize, _, cx| {
                                                        this.update_settings(cx, |s| s.chat_layout = ChatLayout::ALL[*ix]);
                                                    }),
                                                    cx,
                                                )),
                                        ),
                                )
                            })
                            .when(show("org-layout") && (settings.org_enabled || matches.is_some()), |section| section.child(self.org_layout_row(cx)))
                            .when(show("right-panel-width") && settings.chat_layout != ChatLayout::Tabbed, |section| {
                                section.child(
                                    div()
                                        .id("right-panel-width")
                                        .debug_selector(|| "right-panel-width".into())
                                        .child(
                                            SettingsRow::new("Right panel share")
                                                .description("Right panel's share of the width in Sidebar and Split layouts. Chat takes the rest. Drag the divider to resize too.")
                                                .control(settings_dropdown(
                                                    "right-panel-width",
                                                    "Right panel share",
                                                    right_panel_share,
                                                    ["30%", "40%", "50%", "60%", "70%"],
                                                    cx.listener(|this, ix: &usize, _, cx| {
                                                        this.update_settings(cx, |s| s.right_panel_share = [0.3, 0.4, 0.5, 0.6, 0.7][*ix]);
                                                    }),
                                                    cx,
                                                )),
                                        ),
                                )
                            }),
                    )
                });
            }
            SettingsTab::BottomBar => {
                let mut entry_cards = Vec::new();
                for (key, label, mark) in [
                    ("nav-direct", "Direct chats", gpui_kit::assets::IconName::MessageCircle),
                    ("nav-archive", "Archive", gpui_kit::assets::IconName::Archive),
                    ("nav-outside", "Outside agents", gpui_kit::assets::IconName::Laptop),
                    ("nav-terminal", "Terminal", gpui_kit::assets::IconName::SquareTerminal),
                    ("nav-notes", "Notes", gpui_kit::assets::IconName::BookOpen),
                    ("nav-board", "Board", gpui_kit::assets::IconName::LayoutDashboard),
                    ("nav-hackernews", "Hacker News", gpui_kit::assets::IconName::Flame),
                    ("nav-settings", "Settings", gpui_kit::assets::IconName::Settings),
                    ("usage", "Agent usage", gpui_kit::assets::IconName::Bot),
                    ("cpu", "CPU", gpui_kit::assets::IconName::Cpu),
                    ("memory", "Memory", gpui_kit::assets::IconName::MemoryStick),
                    ("clock", "Date and time", gpui_kit::assets::IconName::Timer),
                ] {
                    let selected = !settings.bottom_bar_hidden.iter().any(|k| k == key);
                    entry_cards.push(
                        div().w(px(120.)).flex_none().child(
                            ChoiceCard::new(format!("bottom-bar-{key}"), label)
                                .selected(selected)
                                .preview(
                                    h_flex()
                                        .h(px(48.))
                                        .w_full()
                                        .justify_center()
                                        .items_center()
                                        .child(Icon::new(mark).size(px(22.))),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.update_settings(cx, |s| {
                                        let selected =
                                            !s.bottom_bar_hidden.iter().any(|k| k == key);
                                        s.bottom_bar_hidden.retain(|k| k != key);
                                        if selected {
                                            s.bottom_bar_hidden.push(key.to_string());
                                        }
                                    });
                                })),
                        ),
                    );
                }
                let entries = SettingsSection::new("Visible entries")
                    .icon(Icon::new(gpui_kit::assets::IconName::Eye))
                    .child(h_flex().flex_wrap().gap_3().p_4().children(entry_cards));
                page = page.when(show("bottom-bar-entries"), |page| {
                    page.child(div().id("bottom-bar-entries").child(entries))
                });
                let agents = SettingsSection::new("Agent usage").icon(Icon::new(gpui_kit::assets::IconName::Bot))
                    .child(div().px_3().py_3().text_sm().text_color(cx.theme().muted_foreground)
                        .child("Choose which agents appear in the bottom bar. Select at least one. The usage popover shows all agents."));
                let cards = [
                    aether_core::Provider::Claude,
                    aether_core::Provider::Codex,
                    aether_core::Provider::Antigravity,
                    aether_core::Provider::OpenRouter,
                    aether_core::Provider::OpenCode,
                ]
                .into_iter()
                .map(|provider| {
                    let selected = !settings.bottom_bar_hidden_agents.contains(&provider);
                    ChoiceCard::new(
                        format!("bottom-bar-agent-{}", provider.key()),
                        provider.label(),
                    )
                    .selected(selected)
                    .preview(
                        h_flex()
                            .h(px(48.))
                            .w_full()
                            .justify_center()
                            .items_center()
                            .child(aether_ui::provider_icon(provider).size(px(22.))),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.update_settings(cx, |s| {
                            let selected = !s.bottom_bar_hidden_agents.contains(&provider);
                            if selected && s.bottom_bar_hidden_agents.len() >= 4 {
                                return;
                            }
                            s.bottom_bar_hidden_agents.retain(|p| *p != provider);
                            if selected {
                                s.bottom_bar_hidden_agents.push(provider);
                            }
                        });
                    }))
                });
                let agents = agents.child(div().p_3().child(
                    CardCarousel::new("bottom-bar-agent-cards", 0).compact().children(cards),
                ));
                page = page.when(show("bottom-bar-agents"), |page| {
                    page.child(div().id("bottom-bar-agents").child(agents))
                });

                let current_time_format_ix = match settings.status_bar_time_format {
                    TimeFormat::Hour12 => 0,
                    TimeFormat::Hour24 => 1,
                };
                let current_date_format_ix = match settings.status_bar_date_format {
                    DateFormat::Short => 0,
                    DateFormat::Numeric => 1,
                    DateFormat::Full => 2,
                    DateFormat::Hidden => 3,
                };
                page = page.when(
                    show("status-bar-time-format")
                        || show("status-bar-date-format")
                        || show("status-bar-seconds")
                        || show("status-bar-hn"),
                    |page| {
                        page.child(
                            SettingsSection::new("Date and Time")
                                .icon(Icon::new(gpui_kit::assets::IconName::Timer))
                                .when(show("status-bar-time-format"), |section| {
                                    section.child(
                                        div()
                                            .id("status-bar-time-format")
                                            .debug_selector(|| "status-bar-time-format".into())
                                            .child(
                                                SettingsRow::new("Time format")
                                                    .description("Choose 12-hour (with AM/PM) or 24-hour clock.")
                                                    .control(
                                                        settings_dropdown(
                                                            "status-bar-time-format",
                                                            "Time format",
                                                            current_time_format_ix,
                                                            TimeFormat::ALL.map(|fmt| fmt.label()),
                                                            cx.listener(|this, ix: &usize, _, cx| {
                                                                let fmt = TimeFormat::ALL[*ix];
                                                                this.update_settings(cx, |s| s.status_bar_time_format = fmt);
                                                            }),
                                                            cx,
                                                        ),
                                                    ),
                                            ),
                                    )
                                })
                                .when(show("status-bar-date-format"), |section| {
                                    section.child(
                                        div()
                                            .id("status-bar-date-format")
                                            .debug_selector(|| "status-bar-date-format".into())
                                            .child(
                                                SettingsRow::new("Date format")
                                                    .description("Display style for the globally visible date.")
                                                    .control(
                                                        settings_dropdown(
                                                            "status-bar-date-format",
                                                            "Date format",
                                                            current_date_format_ix,
                                                            DateFormat::ALL.map(|fmt| fmt.label()),
                                                            cx.listener(|this, ix: &usize, _, cx| {
                                                                let fmt = DateFormat::ALL[*ix];
                                                                this.update_settings(cx, |s| s.status_bar_date_format = fmt);
                                                            }),
                                                            cx,
                                                        ),
                                                    ),
                                            ),
                                    )
                                })
                                .when(show("status-bar-seconds"), |section| {
                                    section.child(
                                        div()
                                            .id("status-bar-seconds")
                                            .debug_selector(|| "status-bar-seconds".into())
                                            .child(
                                                SettingsRow::new("Display seconds")
                                                    .description("Include seconds in the status bar clock.")
                                                    .control(
                                                        Switch::new("status-bar-seconds-setting")
                                                            .checked(settings.status_bar_show_seconds)
                                                            .accessibility_label("Display seconds in status bar")
                                                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                                                let checked = *checked;
                                                                this.update_settings(cx, |s| s.status_bar_show_seconds = checked);
                                                            })),
                                                    ),
                                            ),
                                    )
                                }),
                        )
                    },
                );
            }
            SettingsTab::Tiling => {
                let stack = settings.tile_layout == TileLayout::Stack;
                let (fg, border) = (cx.theme().foreground, cx.theme().border);
                let shortcuts: Vec<_> = [
                    (&crate::actions::CycleTileLayout as &dyn gpui_kit::Action, "Next layout"),
                    (&crate::actions::FocusSessionLeft, "Focus the pane that way (any arrow)"),
                    (&crate::actions::SwapSessionLeft, "Swap with the pane that way (any arrow)"),
                    (&crate::actions::PromoteSession, "Make the focused pane the main pane"),
                    (&crate::actions::ToggleSessionFloat, "Float or dock the focused pane"),
                    (&crate::actions::NarrowMainPane, "Narrow the main pane"),
                    (&crate::actions::WidenMainPane, "Widen the main pane"),
                ]
                .into_iter()
                .filter_map(|(action, what)| Some((crate::actions::key_hint(action, cx)?, what)))
                .collect();
                page = page.when(
                    show("tile-layout") || show("tile-ratio") || show("tile-gap") || show("tile-shortcuts"),
                    |page| {
                        page.child(
                            SettingsSection::new("Tiling")
                                .when(show("tile-layout"), |section| {
                                    section.child(div().id("tile-layout").debug_selector(|| "tile-layout".into()).child(
                                        SettingsRow::new("Layout")
                                            .description("How docked session panes share the window.")
                                            .stacked()
                                            .control(
                                                v_flex()
                                                    .gap_2()
                                                    .child(h_flex().gap_2().children(TileLayout::ALL.into_iter().enumerate().map(|(ix, layout)| {
                                                        let on = settings.tile_layout == layout;
                                                        let color = if on { fg } else { muted };
                                                        aether_ui::button(("tile-layout", ix))
                                                            .ghost()
                                                            .selected(on)
                                                            .flex_1()
                                                            .h(px(72.))
                                                            .corner(px(10.))
                                                            .border_1()
                                                            .border_color(if on { fg.opacity(0.3) } else { border })
                                                            .tooltip(layout.hint())
                                                            .child(
                                                                v_flex()
                                                                    .items_center()
                                                                    .gap_2()
                                                                    .text_xs()
                                                                    .text_color(color)
                                                                    .child(tile_glyph(layout, color))
                                                                    .child(layout.label()),
                                                            )
                                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                                this.update_settings(cx, |s| s.tile_layout = layout);
                                                            }))
                                                    })))
                                                    .child(div().text_xs().text_color(muted).child(settings.tile_layout.hint())),
                                            ),
                                    ))
                                })
                                .when(show("tile-ratio"), |section| {
                                    section.child(div().id("tile-ratio").debug_selector(|| "tile-ratio".into()).when(!stack, |d| d.opacity(0.6)).child(
                                        SettingsRow::new("Main pane width")
                                            .description(if stack { "Share of the width the first pane takes." } else { "Stack layout only." })
                                            .control(settings_dropdown(
                                                "tile-ratio",
                                                "Main pane width",
                                                ((settings.tile_ratio.clamp(0.3, 0.8) * 100. - 30.) / 5.).round() as usize,
                                                (30..=80).step_by(5).map(|p| format!("{p}%")),
                                                cx.listener(|this, ix: &usize, _, cx| {
                                                    this.update_settings(cx, |s| s.tile_ratio = (30 + ix * 5) as f32 / 100.);
                                                }),
                                                cx,
                                            )),
                                    ))
                                })
                                .when(show("tile-gap"), |section| {
                                    section.child(div().id("tile-gap").debug_selector(|| "tile-gap".into()).child(
                                        SettingsRow::new("Gap")
                                            .description("Space between docked panes.")
                                            .control(settings_dropdown(
                                                "tile-gap",
                                                "Gap",
                                                (settings.tile_gap.clamp(0., 24.) / 4.).round() as usize,
                                                (0..=24).step_by(4).map(|p| format!("{p} px")),
                                                cx.listener(|this, ix: &usize, _, cx| {
                                                    this.update_settings(cx, |s| s.tile_gap = (ix * 4) as f32);
                                                }),
                                                cx,
                                            )),
                                    ))
                                })
                                .when(show("tile-shortcuts") && !shortcuts.is_empty(), |section| {
                                    section.child(div().id("tile-shortcuts").debug_selector(|| "tile-shortcuts".into()).child(
                                        SettingsRow::new("Shortcuts")
                                            .description("While a session pane has focus.")
                                            .stacked()
                                            .control(v_flex().gap_1p5().children(shortcuts.into_iter().map(|(keys, what)| {
                                                h_flex()
                                                    .justify_between()
                                                    .text_sm()
                                                    .child(what)
                                                    .child(
                                                        div()
                                                            .px_1p5()
                                                            .corner(px(4.))
                                                            .border_1()
                                                            .border_color(border)
                                                            .text_xs()
                                                            .text_color(muted)
                                                            .child(keys),
                                                    )
                                            }))),
                                    ))
                                }),
                        )
                    },
                );
            }
            SettingsTab::Composer => {
                page = page.when(show("voice-enabled") || show("voice-model"), |page| {
                    page.child(
                        SettingsSection::new("Voice")
                            .icon(Icon::new(gpui_kit::assets::IconName::Mic))
                            .when(show("voice-enabled"), |section| {
                                section.child(
                                    div()
                                        .id("voice-enabled")
                                        .debug_selector(|| "voice-enabled".into())
                                        .child(
                                            SettingsRow::new("Dictation")
                                                .description(
                                                    "Shows the mic button in the composer and turns on its shortcut.",
                                                )
                                                .control(
                                                    Switch::new("voice-enabled-setting")
                                                        .checked(settings.voice_enabled)
                                                        .accessibility_label("Dictation")
                                                        .on_click(cx.listener(
                                                            |this, checked: &bool, _, cx| {
                                                                this.set_voice_enabled(*checked, cx)
                                                            },
                                                        )),
                                                ),
                                        ),
                                )
                            })
                            .when(show("voice-model"), |section| {
                                section.child(
                                    div()
                                        .id("voice-model")
                                        .debug_selector(|| "voice-model".into())
                                        .child(
                                            SettingsRow::new("Speech model")
                                                .description(self.voice_model_status())
                                                .control(self.voice_model_control(cx)),
                                        ),
                                )
                            }),
                    )
                });
            }
            SettingsTab::Model => {
                use gpui_kit::assets::IconName as Lucide;
                page = page.when(show("api-key"), |page| { page.child(SettingsSection::new("OpenRouter API key").icon(Icon::new(Lucide::KeyRound)).when(show("api-key"), |section| { section.child(div().id("api-key").debug_selector(|| "api-key".into()).child(SettingsRow::new("API key")
                            .description(
                                "Stored in your system keychain, never in the settings file.",
                            )
                            .stacked()
                            .control(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(div().flex_1().child(field(&self.key_input).aria_label("OpenRouter API key")))
                                            .child(
                                                aether_ui::button("save-key")
                                                    .primary()
                                                    .small()
                                                    .label("Save")
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.save_key(window, cx)
                                                        },
                                                    )),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(self.key_status.clone()),
                                    ),
                            ))) })) });
                page = page
                    .when(show("default-model"), |page| {
                        page.child(
                            SettingsSection::new("Default model")
                                .icon(aether_ui::provider_icon(aether_core::Provider::OpenRouter))
                                .child(
                                    div().id("default-model")
                                        .debug_selector(|| "default-model".into())
                                        .child(
                                            SettingsRow::new("OpenRouter model")
                                                .description("Used for new OpenRouter chats. You can choose another model in the composer.")
                                                .stacked()
                                                .control(AgentSelector::new(&self.default_model)),
                                        ),
                                ),
                        )
                    })
                    .when(show("reasoning") || show("web-search"), |page| {
                        page.child(
                            SettingsSection::new("Response behavior")
                                .icon(Icon::new(Lucide::Lightbulb))
                                .when(show("reasoning"), |section| {
                                    let cards = [
                                        (Reasoning::Off, "Off", Lucide::CircleSlash),
                                        (Reasoning::Low, "Low", Lucide::Zap),
                                        (Reasoning::Medium, "Medium", Lucide::Gauge),
                                        (Reasoning::High, "High", Lucide::Lightbulb),
                                    ].map(|(effort, label, icon)| {
                                        ChoiceCard::new(format!("reasoning-{}", label.to_lowercase()), label)
                                            .selected(settings.reasoning == effort)
                                            .preview(
                                                h_flex().h(px(48.)).justify_center()
                                                    .child(Icon::new(icon).size(px(22.))),
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.update_settings(cx, |s| s.reasoning = effort);
                                            }))
                                    });
                                    let hint = match settings.reasoning {
                                        Reasoning::Off => "No reasoning requested. Some models still reason by default.",
                                        Reasoning::Low => "Less reasoning for faster responses and lower cost.",
                                        Reasoning::Medium => "A balance of reasoning depth, response time and cost.",
                                        Reasoning::High => "More reasoning for complex tasks. Responses can take longer and cost more.",
                                    };
                                    section.child(
                                        div().id("reasoning")
                                            .debug_selector(|| "reasoning".into())
                                            .child(
                                                SettingsRow::new("Reasoning effort")
                                                    .description("How much the OpenRouter model thinks before answering.")
                                                    .stacked()
                                                    .control(
                                                        v_flex().gap_3()
                                                            .child(
                                                                h_flex().w_full().max_w(px(560.)).gap_3()
                                                                    .children(cards.into_iter().map(|card| {
                                                                        div().flex_1().min_w_0().child(card)
                                                                    })),
                                                            )
                                                            .child(div().text_xs().text_color(muted).child(hint)),
                                                    ),
                                            ),
                                    )
                                })
                                .when(show("web-search"), |section| {
                                    section.child(
                                        div().id("web-search")
                                            .debug_selector(|| "web-search".into())
                                            .child(
                                                SettingsRow::new("Web search")
                                                    .icon(Icon::new(Lucide::Search))
                                                    .description("Allow DuckDuckGo searches. Also available from the Search chip in the composer.")
                                                    .control(
                                                        Switch::new("web-search-setting")
                                                            .checked(settings.web_search)
                                                            .accessibility_label("Web search")
                                                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                                                this.update_settings(cx, |s| s.web_search = *checked);
                                                            })),
                                                    ),
                                            ),
                                    )
                                }),
                        )
                    })
                    .when(show("instructions"), |page| {
                        page.child(
                            SettingsSection::new("Custom instructions")
                                .icon(Icon::new(Lucide::TextQuote))
                                .child(
                                    div().id("instructions")
                                        .debug_selector(|| "instructions".into())
                                        .child(
                                            SettingsRow::new("Instructions")
                                                .description("Added to every conversation. Set your preferred tone, format or conventions here.")
                                                .stacked()
                                                .control(field_area(&self.instructions).aria_label("Custom instructions")),
                                        ),
                                ),
                        )
                    });
            }
            SettingsTab::Providers => {
                let mut coding_agents = SettingsSection::new("Coding agents")
                    .icon(Icon::new(gpui_kit::assets::IconName::Bot));
                if matches.is_none() {
                    let cards = CLIS.into_iter().map(|provider| {
                        let installed =
                            self.probes.get(&provider).is_some_and(|probe| probe.installed);
                        let status = if self.probes.get(&provider).is_none() {
                            "Checking…"
                        } else if installed {
                            "Available"
                        } else {
                            "Not installed"
                        };
                        ChoiceCard::new(
                            format!("provider-card-{}", provider.key()),
                            provider.label(),
                        )
                        .selected(self.settings_provider == provider)
                        .preview(
                            v_flex()
                                .h(px(76.))
                                .w_full()
                                .gap_2()
                                .items_center()
                                .justify_center()
                                .child(aether_ui::provider_icon(provider).size(px(24.)))
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(
                                            Icon::new(if installed {
                                                gpui_kit::assets::IconName::Check
                                            } else {
                                                gpui_kit::assets::IconName::Circle
                                            })
                                            .size(px(10.)),
                                        )
                                        .child(status),
                                ),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.settings_provider = provider;
                            cx.notify();
                        }))
                    });
                    coding_agents = coding_agents.child(
                        div().p_4().child(
                            CardCarousel::new(
                                "provider-cards",
                                CLIS.iter().position(|p| *p == self.settings_provider).unwrap_or(0),
                            )
                            .compact()
                            .children(cards),
                        ),
                    );
                }
                let root = settings.workspace_root.clone();
                page = page

                    .when(show("workspace"), |page| { page.child(SettingsSection::new("Workspace").icon(Icon::new(gpui_kit::assets::IconName::Folder)).when(show("workspace"), |section| { section.child(div().id("workspace").debug_selector(|| "workspace".into()).child(SettingsRow::new("Folder")
                                .description(
                                    "Coding agents run here. Defaults to your home folder.",
                                )
                                .control(
                                    h_flex()
                                        .gap_2()
                                        .child(
                                            div()
                                                .max_w(px(280.))
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .text_ellipsis_start()
                                                .text_sm()
                                                .text_color(muted)
                                                .child(if root.is_empty() {
                                                    "Home folder".to_string()
                                                } else {
                                                    root.clone()
                                                }),
                                        )
                                        .when(!root.is_empty(), |row| {
                                            row.child(
                                                aether_ui::button("reset-workspace")
                                                    .ghost()
                                                    .small()
                                                    .label("Reset")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.update_settings(cx, |s| {
                                                            s.workspace_root.clear()
                                                        })
                                                    })),
                                            )
                                        })
                                        .child(
                                            aether_ui::button("choose-workspace")
                                                .small()
                                                .label("Choose…")
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.choose_workspace(window, cx)
                                                })),
                                        ),
                                ))) })) })
                    .when(show("claude") || show("codex") || show("pi") || show("opencode") || show("antigravity"), |page| { page.child(CLIS.into_iter().filter(|p| show(p.key()) && (matches.is_some() || *p == self.settings_provider)).fold(
                        coding_agents,
                        |section, p| {
                            let binary = DriverStartOptions::for_provider(settings, p, "").binary;
                            let access = settings.default_access(p);
                            let entity = cx.entity();
                            let access_picker = settings_dropdown_trigger(
                                SharedString::from(format!("default-access-{}", p.key())),
                                access.label(), cx,
                            )
                            .accessibility_label(format!("{} default access mode", p.label()))
                            .dropdown_menu_with_anchor(
                                Anchor::TopLeft,
                                move |mut menu: PopupMenu, _, _| {
                                    menu = settings_dropdown_menu(menu);
                                    for &mode in AccessMode::for_provider(p) {
                                        let entity = entity.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new(mode.label())
                                                .checked(mode == access)
                                                .on_click(move |_, _, cx| {
                                                    entity.update(cx, |this, cx| {
                                                        this.update_settings(cx, |s| {
                                                            s.provider_access
                                                                .insert(p.key().into(), mode);
                                                        });
                                                    });
                                                }),
                                        );
                                    }
                                    menu
                                },
                            );
                            section.child(
                                SettingsRow::new(p.label())
                                    .icon(aether_ui::provider_icon(p))
                                    .description(provider_status(self.probes.get(&p), &binary))
                                    .stacked()
                                    .control(
                                        v_flex().gap_4()
                                            .child(h_flex().gap_3()
                                            .child(labeled_input(
                                                "Binary path",
                                                &self.binary_inputs[&p],
                                                muted,
                                            ))
                                            .child(labeled_input(
                                                "Default model",
                                                &self.model_inputs[&p],
                                                muted,
                                            )))
                                            .child(
                                                v_flex()
                                                    .gap_1()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(muted)
                                                            .child("Default access mode"),
                                                    )
                                                    .child(h_flex().child(access_picker)),
                                            ),
                                    ),
                            )
                        },
                    )) });
            }
            SettingsTab::Worktrees => {
                page = page.child(self.worktrees_section(cx));
            }
            SettingsTab::Attachments => {
                page = page.child(self.render_stored_files(None, cx));
            }
            SettingsTab::Tools => {
                let notes_ids = [
                    "notes-directory",
                    "notes-editor",
                    "notes-scope",
                    "notes-nvim",
                    "notes-tmux",
                    "notes-session",
                    "notes-socket",
                ];
                let expanded = self.notes_advanced_open || matches.is_some();
                let show_note = |id: &str| settings.notes_enabled && expanded && show(id);
                page = page.when(show("notes-enabled") || notes_ids.into_iter().any(show_note), |page| {
                    let mut section = SettingsSection::new("Notes").icon(Icon::new(gpui_kit::assets::IconName::BookOpen)).when(show("notes-enabled"), |section| section.child(
                        div().id("notes-enabled").debug_selector(|| "notes-enabled".into()).child(
                            SettingsRow::new("Enable notes")
                                .description("Shows Notes in the sidebar and palette, its shortcuts, and Save to note in menus.")
                                .control(
                                    h_flex().gap_2().items_center().child(Switch::new("notes-enabled-setting")
                                        .checked(settings.notes_enabled)
                                        .accessibility_label("Enable notes")
                                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                            let checked = *checked;
                                            this.notes_advanced_open = false;
                                            this.update_settings(cx, |s| s.notes_enabled = checked);
                                        })))
                                        .when(settings.notes_enabled, |row| row.child(
                                            aether_ui::button("notes-advanced-toggle").debug_selector(|| "notes-advanced-toggle".into()).ghost().small()
                                                .icon(if expanded { gpui_kit::assets::IconName::ChevronUp } else { gpui_kit::assets::IconName::ChevronDown })
                                                .accessibility_label("Advanced notes settings")
                                                .tooltip(if expanded { "Hide advanced settings" } else { "Show advanced settings" })
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.notes_advanced_open = !this.notes_advanced_open;
                                                    cx.notify();
                                                })))),
                                ),
                        ),
                    ));
                    for (id, key, title, description) in [
                        ("notes-directory", "directory", "Notes directory", "Changing this folder leaves existing files in place. Save or close modified notes first."),
                        ("notes-nvim", "nvim", "Neovim executable", "Binary path or command resolved using PATH."),
                        ("notes-tmux", "tmux", "tmux executable", "Binary path or command resolved using PATH."),
                        ("notes-session", "session", "tmux session", "Dedicated session for the notes terminal."),
                        ("notes-socket", "socket", "Neovim socket", "Empty uses $XDG_RUNTIME_DIR/notes-nvim.sock, or /tmp/notes-nvim.sock."),
                    ] {
                        if show_note(id) {
                            section = section.child(div().id(id).debug_selector(move || id.into()).child(
                                SettingsRow::new(title).description(description).stacked()
                                    .control(field(&self.notes_setting_inputs[key]).aria_label(title)),
                            ));
                        }
                    }
                    section = section.when(show_note("notes-editor"), |section| section.child(
                        div().id("notes-editor").debug_selector(|| "notes-editor".into()).child(
                            SettingsRow::new("Default editor")
                                .description("Context menus can always open a note using either editor.")
                                .control(Segmented::new("notes-editor-picker")
                                    .option("Aether").option("Neovim/tmux")
                                    .selected(usize::from(settings.notes.editor == NotesEditor::Neovim))
                                    .on_change(cx.listener(|this, ix: &usize, _, cx| {
                                        let mut notes = this.settings.notes.clone();
                                        notes.editor = if *ix == 0 { NotesEditor::Aether } else { NotesEditor::Neovim };
                                        this.update_notes_settings(notes, cx);
                                    }))),
                        ),
                    ));
                    section = section.when(show_note("notes-scope"), |section| section.child(
                        div().id("notes-scope").debug_selector(|| "notes-scope".into()).child(
                            SettingsRow::new("Default scope")
                                .description("Choose global notes or notes for the current project.")
                                .control(Segmented::new("notes-scope-picker")
                                    .option("Global").option("Current project")
                                    .selected(usize::from(settings.notes.project_scope))
                                    .on_change(cx.listener(|this, ix: &usize, _, cx| {
                                        let mut notes = this.settings.notes.clone();
                                        notes.project_scope = *ix == 1;
                                        this.update_notes_settings(notes, cx);
                                    }))),
                        ),
                    ));
                    page.child(section.when_some(self.notes.error.clone(), |section, error| {
                        section.child(div().text_sm().text_color(aether_ui::theme::danger_text(cx)).child(error))
                    }))
                });
                page = page.when(show("hackernews-enabled"), |page| page.child(SettingsSection::new("Hacker News").icon(Icon::new(gpui_kit::assets::IconName::Flame)).child(
                    div().id("hackernews-enabled").debug_selector(|| "hackernews-enabled".into()).child(
                        SettingsRow::new("Enable Hacker News")
                            .description("Shows Hacker News in the sidebar and palette, and its shortcuts.")
                            .control(
                                Switch::new("hackernews-enabled-setting")
                                    .checked(settings.hackernews_enabled)
                                    .accessibility_label("Enable Hacker News")
                                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                        let checked = *checked;
                                        this.update_settings(cx, |s| s.hackernews_enabled = checked);
                                    })),
                            ),
                    ),
                )));
                page = page.when(show("web_search") || show("load_skill") || show("plan-first"), |page| { page.child([
                        ("web_search", "Web search", "Searches DuckDuckGo, so it asks by default."),
                        ("load_skill", "Load skill", "Reads one of your enabled skills."),
                    ]
                    .into_iter()
                    .filter(|(tool, _, _)| show(tool))
                    .fold(SettingsSection::new("Tool approvals").icon(Icon::new(gpui_kit::assets::IconName::Wrench)), |section, (tool, title, description)| {
                        let policy = settings.tool_policy(tool);
                        section.child(
                            SettingsRow::new(title)
                                .description(description)
                                .control(
                                    Segmented::new(SharedString::from(format!("policy-{tool}")))
                                        .option("Ask")
                                        .option("Allow")
                                        .selected(usize::from(policy == ToolPolicy::Allow))
                                        .on_change(cx.listener(move |this, ix: &usize, _, cx| {
                                            let policy =
                                                if *ix == 0 { ToolPolicy::Ask } else { ToolPolicy::Allow };
                                            this.update_settings(cx, |s| {
                                                s.tool_approvals.insert(tool.into(), policy);
                                            });
                                        })),
                                ),
                        )
                    })
                    .when(show("plan-first"), |section| { section.child(div().id("plan-first").debug_selector(|| "plan-first".into()).child(SettingsRow::new("Plan first")
                            .description(
                                "The model proposes a plan and waits for your approval before doing the \
                                 work. Same as the Plan chip.",
                            )
                            .control(
                                Switch::new("plan-mode-setting")
                                    .checked(settings.plan_mode)
                                    .accessibility_label("Plan first")
                                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                        let checked = *checked;
                                        this.update_settings(cx, |s| s.plan_mode = checked);
                                    })),
                            ))) })) });
            }
            SettingsTab::Terminal => {
                let raw_font = settings.terminal_font_family.trim().to_string();
                let font_warning = if raw_font.is_empty() {
                    None
                } else {
                    let installed = aether_terminal::view::installed_font_names_cached(cx);
                    let effective = aether_terminal::view::resolve_terminal_family(
                        &raw_font,
                        installed,
                        cx.theme().mono_font_family.as_ref(),
                    );
                    (effective.as_ref() != raw_font).then(|| {
                            let nerds = aether_terminal::view::installed_nerd_fonts(installed);
                            if nerds.is_empty() {
                                format!(
                                    "Font {raw_font:?} isn't installed, so the theme font is used and icons stay tofu. Install one: brew install --cask font-jetbrains-mono-nerd-font, then use JetBrainsMono Nerd Font."
                                )
                            } else {
                                format!(
                                    "Font {raw_font:?} isn't installed, using {} instead. Installed Nerd fonts: {}.",
                                    effective,
                                    nerds.join(", ")
                                )
                            }
                        })
                };
                let current_theme = settings.terminal_theme;
                let entity = cx.entity().clone();
                let theme_picker =
                    settings_dropdown_trigger("terminal-theme-dropdown", current_theme.label(), cx)
                        .accessibility_label("Terminal color theme")
                        .dropdown_menu_with_anchor(
                            Anchor::TopRight,
                            move |mut menu: PopupMenu, _, _| {
                                menu = settings_dropdown_menu(menu);
                                for theme in TerminalTheme::ALL {
                                    let entity = entity.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(theme.label())
                                            .checked(theme == current_theme)
                                            .on_click(move |_, _, cx| {
                                                entity.update(cx, |this, cx| {
                                                    this.update_settings(cx, |s| {
                                                        s.terminal_theme = theme
                                                    });
                                                });
                                            }),
                                    );
                                }
                                menu
                            },
                        );
                page = page.when(show("terminal-theme") || show("terminal-font-size") || show("terminal-font-family") || show("terminal-cursor") || show("terminal-meta"), |page| { page.child(SettingsSection::new("Terminal").icon(Icon::new(gpui_kit::assets::IconName::Terminal))
                            .when(show("terminal-theme"), |section| { section.child(div().id("terminal-theme").debug_selector(|| "terminal-theme".into()).child(SettingsRow::new("Color theme")
                                    .description(
                                        "Terminal palette. Auto pairs Mocha for dark and Latte for light appearance.",
                                    )
                                    .control(theme_picker))) })
                            .when(show("terminal-font-size"), |section| { section.child(div().id("terminal-font-size").debug_selector(|| "terminal-font-size".into()).child(SettingsRow::new("Font size")
                                    .description("Terminal text size, independent of the app UI font size.")
                                    .control(
                                        settings_dropdown(
                                            "terminal-font-size", "Terminal font size", settings.terminal_font_size_index(),
                                            Settings::TERMINAL_FONT_SIZES.map(|size| format!("{size}px")),
                                            cx.listener(|this, ix: &usize, _, cx| {
                                                let size = Settings::TERMINAL_FONT_SIZES[*ix];
                                                this.update_settings(cx, |s| s.terminal_font_size = size);
                                            }),
                                            cx,
                                        ),
                                    ))) })
                            .when(show("terminal-font-family"), |section| { section.child(div().id("terminal-font-family").debug_selector(|| "terminal-font-family".into()).child(SettingsRow::new("Font family")
                                    .description(
                                        "System monospace for the terminal. Use a Nerd Font (e.g. MesloLGS NF, \
                                         JetBrainsMono Nerd Font — no space in JetBrainsMono) for prompt icons. \
                                         Empty uses the theme font; symbol fonts are always tried as fallbacks. \
                                         Restart the app after installing a font.",
                                    )
                                    .control(
                                        div()
                                            .w(px(280.))
                                            .child(aether_ui::field(&self.terminal_font_input).aria_label("Terminal font family")),
                                    ))) })
                            .children(font_warning.filter(|_| show("terminal-font-family")).map(|w| {
                                SettingsRow::new("Font not found").description(w)
                            }))
                            .when(show("terminal-cursor"), |section| { section.child(div().id("terminal-cursor").debug_selector(|| "terminal-cursor".into()).child(SettingsRow::new("Cursor style")
                                    .description("Programs like Neovim can still set their own.")
                                    .control(
                                        settings_dropdown(
                                            "terminal-cursor-setting", "Terminal cursor style",
                                            TerminalCursor::ALL.iter().position(|c| *c == settings.terminal_cursor).unwrap_or(0),
                                            TerminalCursor::ALL.map(|c| c.label()),
                                            cx.listener(|this, ix: &usize, _, cx| {
                                                this.update_settings(cx, |s| s.terminal_cursor = TerminalCursor::ALL[*ix]);
                                            }),
                                            cx,
                                        ),
                                    ))) })
                            .when(show("terminal-meta"), |section| { section.child(div().id("terminal-meta").debug_selector(|| "terminal-meta".into()).child(SettingsRow::new("Use Option as Meta key")
                                    .description(
                                        "Option+key sends Esc+key, for word moves and deletes in the shell. \
                                         Off, Option types its character, like é.",
                                    )
                                    .control(
                                        Switch::new("terminal-option-meta-setting")
                                            .checked(settings.terminal_option_as_meta)
                                            .accessibility_label("Use Option as Meta key in terminal")
                                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                                let checked = *checked;
                                                this.update_settings(cx, |s| s.terminal_option_as_meta = checked);
                                            })),
                                    ))) })) });
            }
            SettingsTab::Organization => {
                page = page.child(self.org_settings(&show, matches.is_some(), cx));
            }
        }
        page
    }

    fn org_layout_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let layout = self.settings.org_layout;
        div().id("org-layout").debug_selector(|| "org-layout".into()).child(
            SettingsRow::new("Organization layout").description(layout.hint()).control(
                settings_dropdown(
                    "org-layout",
                    "Organization layout",
                    OrgLayout::ALL.iter().position(|l| *l == layout).unwrap_or(0),
                    OrgLayout::ALL.map(|l| l.label()),
                    cx.listener(|this, ix: &usize, _, cx| {
                        this.update_settings(cx, |s| s.org_layout = OrgLayout::ALL[*ix]);
                    }),
                    cx,
                ),
            ),
        )
    }

    /// Enable toggle, then layout, template and the default rules once on (or while searching).
    fn org_settings(
        &self,
        show: &dyn Fn(&str) -> bool,
        searching: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        use crate::org::model::TEMPLATES;
        let s = &self.settings;
        let on = s.org_enabled || searching;
        let row = |id: &'static str, title: &'static str, description: String, control| {
            div()
                .id(id)
                .debug_selector(move || id.into())
                .child(SettingsRow::new(title).description(description).control(control))
        };
        let pool = match s.org_pool {
            None => format!("Auto ({})", self.org_pool_cap()),
            Some(_) => "Auto from machine".into(),
        };
        let budgets = [5., 10., 25., 50., 100., 200., 500.];
        let general = SettingsSection::new("Organization")
            .icon(Icon::new(gpui_kit::assets::IconName::Users))
            .when(show("org-enabled"), |section| {
                section.child(row(
                    "org-enabled",
                    "Enable organizations",
                    "Shows the Organization view, sidebar section and status bar item. Turning it off pauses every org; nothing is deleted.".into(),
                    Switch::new("org-enabled-setting")
                        .checked(s.org_enabled)
                        .accessibility_label("Enable organizations")
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            let checked = *checked;
                            this.update_settings(cx, |s| s.org_enabled = checked);
                        }))
                        .into_any_element(),
                ))
            })
            .when(on && !searching, |section| section.child(self.org_layout_row(cx)))
            .when(on && show("org-template"), |section| {
                section.child(row(
                    "org-template",
                    "Default template",
                    "Used by Set up organization.".into(),
                    settings_dropdown(
                        "org-template",
                        "Default template",
                        TEMPLATES.iter().position(|t| t.id == s.org_template).unwrap_or(1),
                        TEMPLATES.map(|t| t.name),
                        cx.listener(|this, ix: &usize, _, cx| {
                            this.update_settings(cx, |s| s.org_template = TEMPLATES[*ix].id.into());
                        }),
                        cx,
                    )
                    .into_any_element(),
                ))
            });
        let rules = SettingsSection::new("Default rules")
            .icon(Icon::new(gpui_kit::assets::IconName::Wrench))
            .when(show("org-pool"), |section| {
                section.child(row(
                    "org-pool",
                    "Worker pool",
                    "Most workers running at once across every head.".into(),
                    settings_dropdown(
                        "org-pool",
                        "Worker pool",
                        s.org_pool.map_or(0, |n| n.clamp(1, 8)),
                        std::iter::once(pool).chain((1..=8).map(|n| n.to_string())),
                        cx.listener(|this, ix: &usize, _, cx| {
                            let pool = (*ix > 0).then_some(*ix);
                            this.update_settings(cx, |s| s.org_pool = pool);
                        }),
                        cx,
                    )
                    .into_any_element(),
                ))
            })
            .when(show("org-builds"), |section| {
                section.child(row(
                    "org-builds",
                    "Builds at a time",
                    "Builds are serialized; shared target dirs clobber when parallel.".into(),
                    settings_dropdown(
                        "org-builds",
                        "Builds at a time",
                        s.org_builds.clamp(1, 4) - 1,
                        (1..=4).map(|n| n.to_string()),
                        cx.listener(|this, ix: &usize, _, cx| {
                            this.update_settings(cx, |s| s.org_builds = ix + 1);
                        }),
                        cx,
                    )
                    .into_any_element(),
                ))
            })
            .when(show("org-workers-per-head"), |section| {
                section.child(row(
                    "org-workers-per-head",
                    "Workers per head",
                    "Worker cap a new head starts with.".into(),
                    settings_dropdown(
                        "org-workers-per-head",
                        "Workers per head",
                        s.org_workers_per_head.clamp(1, 6) - 1,
                        (1..=6).map(|n| n.to_string()),
                        cx.listener(|this, ix: &usize, _, cx| {
                            this.update_settings(cx, |s| s.org_workers_per_head = ix + 1);
                        }),
                        cx,
                    )
                    .into_any_element(),
                ))
            })
            .when(show("org-hire"), |section| {
                section.child(row(
                    "org-hire",
                    "Hiring policy",
                    "Rules: the request has a card, the head is under its cap, the pool has a slot.".into(),
                    settings_dropdown(
                        "org-hire",
                        "Hiring policy",
                        OrgHire::ALL.iter().position(|h| *h == s.org_hire).unwrap_or(0),
                        OrgHire::ALL.map(|h| h.label()),
                        cx.listener(|this, ix: &usize, _, cx| {
                            this.update_settings(cx, |s| s.org_hire = OrgHire::ALL[*ix]);
                        }),
                        cx,
                    )
                    .into_any_element(),
                ))
            })
            .when(show("org-budget"), |section| {
                section.child(row(
                    "org-budget",
                    "Org budget per day",
                    "Warns you when the org's spend today reaches it.".into(),
                    settings_dropdown(
                        "org-budget",
                        "Org budget per day",
                        budgets.iter().position(|b| *b == s.org_budget_usd).unwrap_or(2),
                        budgets.map(|b| format!("${b}")),
                        cx.listener(move |this, ix: &usize, _, cx| {
                            this.update_settings(cx, |s| s.org_budget_usd = budgets[*ix]);
                        }),
                        cx,
                    )
                    .into_any_element(),
                ))
            })
            .when(show("org-rotate"), |section| {
                section.child(row(
                    "org-rotate",
                    "Rotate a head's session at",
                    "Context bars turn amber here. Rotate session starts the head fresh with the same role.".into(),
                    settings_dropdown(
                        "org-rotate",
                        "Rotate session at",
                        usize::from(s.org_rotate_at.clamp(50, 95) - 50) / 5,
                        (50..=95).step_by(5).map(|p| format!("{p}% context")),
                        cx.listener(|this, ix: &usize, _, cx| {
                            this.update_settings(cx, |s| s.org_rotate_at = 50 + *ix as u8 * 5);
                        }),
                        cx,
                    )
                    .into_any_element(),
                ))
            });
        v_flex().gap_8().child(general).when(on, |page| page.child(rules))
    }
}

impl ChatApp {
    /// Linked worktrees, with who uses each and a way to remove or prune it.
    fn worktrees_section(&self, cx: &mut Context<Self>) -> SettingsSection {
        use gpui_kit::assets::IconName as Lucide;
        let section = SettingsSection::new("Worktrees");
        let Some(list) = &self.worktrees else {
            return section.child(SettingsRow::new("Looking for worktrees…"));
        };
        if list.is_empty() {
            return section.child(
                SettingsRow::new("No worktrees yet.")
                    .description("New worktree chats keep their checkouts here."),
            );
        }
        let home = aether_core::home_dir();
        list.iter().enumerate().fold(section, |section, (k, listed)| {
            let path = listed.path.display().to_string();
            let name = listed.branch.clone().unwrap_or_else(|| {
                listed
                    .path
                    .file_name()
                    .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned())
            });
            let repo = listed
                .repo
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let shown = aether_ui::middle_truncate(&display_path(&path, &home), 56);
            let reveal = listed.path.clone();
            let (label, off) = row_action(listed, self.worktree_running(&listed.path));
            let (tree, project) = (listed.path.clone(), listed.repo.clone());
            let remove = aether_ui::button(("worktree-remove", k))
                .small()
                .label(label)
                .accessibility_label(format!("{}: {name}", off.as_deref().unwrap_or(label)))
                .when_some(off.clone(), |button, why| button.tooltip(why))
                .disabled(off.is_some())
                .loading(self.removing.contains(&listed.path))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.start_remove(tree.clone(), project.clone(), window, cx)
                }));
            section.child(
                SettingsRow::new(name.clone())
                    .icon(gpui_kit::component::Icon::new(Lucide::GitFork))
                    .description(format!("{} · {repo} · {shown}", self.worktree_status(listed)))
                    .control(
                        h_flex()
                            .gap_2()
                            .child(
                                aether_ui::button(("worktree-reveal", k))
                                    .ghost()
                                    .small()
                                    .icon(IconName::FolderOpen)
                                    .accessibility_label(format!("Reveal {name}"))
                                    .tooltip(if listed.missing {
                                        "Folder missing"
                                    } else {
                                        "Reveal in Finder"
                                    })
                                    .disabled(listed.missing)
                                    .on_click(move |_, _, cx| cx.reveal_path(&reveal)),
                            )
                            .child(remove),
                    ),
            )
        })
    }
}

/// A Worktrees row's Remove or Prune label, and why it's off: the git panel's
/// rules, and never while an agent works there.
fn row_action(listed: &Listed, running: bool) -> (&'static str, Option<String>) {
    match listed.remove_action() {
        Ok(label) if running => {
            (label, Some("An agent is working here. Stop it to remove the worktree".into()))
        }
        Ok(label) => (label, None),
        Err(why) => ("Remove worktree…", Some(why)),
    }
}

fn tile_glyph(layout: TileLayout, color: gpui_kit::Hsla) -> impl IntoElement {
    let rects: &[[f32; 4]] = match layout {
        TileLayout::Manual => &[[1., 1., 11., 20.], [13.5, 1., 17.5, 20.]],
        TileLayout::Stack => &[[1., 1., 17., 20.], [20., 1., 11., 9.5], [20., 11.5, 11., 9.5]],
        TileLayout::Columns => &[[1., 1., 9., 20.], [11.5, 1., 9., 20.], [22., 1., 9., 20.]],
        TileLayout::Grid => &[
            [1., 1., 14.5, 9.5],
            [16.5, 1., 14.5, 9.5],
            [1., 11.5, 14.5, 9.5],
            [16.5, 11.5, 14.5, 9.5],
        ],
        TileLayout::Monocle => &[[1., 1., 30., 3.], [1., 5., 30., 16.]],
    };
    div().relative().flex_none().w(px(32.)).h(px(22.)).children(rects.iter().enumerate().map(
        |(i, [x, y, w, h])| {
            div()
                .absolute()
                .left(px(*x))
                .top(px(*y))
                .w(px(*w))
                .h(px(*h))
                .corner(px(1.5))
                .bg(color.opacity(if i == 0 { 0.9 } else { 0.4 }))
        },
    ))
}

fn settings_dropdown_trigger(
    id: impl Into<gpui_kit::ElementId>,
    value: impl Into<SharedString>,
    cx: &gpui_kit::App,
) -> Button {
    let fill = field_fill(cx);
    let hover = overlay_strong(cx);
    aether_ui::button(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(fill)
                .foreground(cx.theme().foreground)
                .hover(hover)
                .active(hover),
        )
        .bg(fill)
        .cursor_pointer()
        .w(px(SETTINGS_DROPDOWN_WIDTH))
        .h(px(40.))
        .px(px(12.))
        .corner(px(10.))
        .label(value)
        .dropdown_caret(true)
}

fn settings_dropdown_menu(menu: PopupMenu) -> PopupMenu {
    menu.min_w(px(SETTINGS_DROPDOWN_WIDTH)).max_w(px(SETTINGS_DROPDOWN_WIDTH))
}

fn theme_variant_picker(
    family: &ThemeFamily,
    shown: Appearance,
    selected: bool,
    cx: &mut Context<ChatApp>,
) -> gpui_kit::AnyElement {
    let variants = family.variants;
    let entity = cx.entity().clone();
    let name = shown.label().rsplit_once(' ').map_or(shown.label(), |(name, _)| name);
    let color = if selected { cx.theme().foreground } else { cx.theme().muted_foreground };
    let id = format!("theme-variant-{}", family.default.slug());
    aether_ui::button(SharedString::from(id.clone()))
        .debug_selector(|| id)
        .custom(ButtonCustomVariant::new(cx).foreground(color))
        .small()
        .px_0()
        .max_w(px(CAROUSEL_CARD_WIDTH))
        .label(shown.label())
        .dropdown_caret(true)
        .accessibility_label(format!("Choose {name} variant"))
        .dropdown_menu_with_anchor(Anchor::TopCenter, move |mut menu: PopupMenu, _, _| {
            menu = settings_dropdown_menu(menu);
            for &variant in variants {
                let entity = entity.clone();
                menu = menu.item(
                    PopupMenuItem::new(variant.label()).checked(variant == shown).on_click(
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.set_appearance(variant, window, cx);
                            });
                        },
                    ),
                );
            }
            menu
        })
        .into_any_element()
}

/// A compact settings picker with the current value and checked menu options.
fn settings_dropdown(
    id: &'static str,
    label: &'static str,
    selected: usize,
    options: impl IntoIterator<Item = impl Into<SharedString>>,
    on_change: impl Fn(&usize, &mut Window, &mut gpui_kit::App) + 'static,
    cx: &gpui_kit::App,
) -> impl IntoElement {
    let options: Vec<SharedString> = options.into_iter().map(Into::into).collect();
    let on_change = std::rc::Rc::new(on_change);
    settings_dropdown_trigger(id, options[selected].clone(), cx)
        .accessibility_label(label)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu: PopupMenu, _, _| {
            menu = settings_dropdown_menu(menu);
            for (index, option) in options.iter().enumerate() {
                let on_change = on_change.clone();
                menu = menu.item(
                    PopupMenuItem::new(option.clone())
                        .checked(index == selected)
                        .on_click(move |_, window, cx| on_change(&index, window, cx)),
                );
            }
            menu
        })
}

/// Settings status line for a CLI: where it was found, or why it wasn't.
fn provider_status(probe: Option<&ProviderProbe>, binary: &str) -> String {
    match probe {
        None => "Checking…".into(),
        Some(ProviderProbe { path: Some(path), models, .. }) => {
            let noun = if models.len() == 1 { "model" } else { "models" };
            format!("Found at {} · {} {noun}", path.display(), models.len())
        }
        Some(ProviderProbe { installed: true, .. }) => "Running as a background service".into(),
        Some(_) if !binary.is_empty() => format!("Not found at {binary}"),
        Some(_) => "Not found on PATH".into(),
    }
}

fn labeled_input(
    label: &'static str,
    input: &gpui_kit::Entity<gpui_kit::component::input::InputState>,
    muted: gpui_kit::Hsla,
) -> impl IntoElement {
    v_flex()
        .flex_1()
        .min_w_0()
        .gap_1()
        .child(div().text_xs().text_color(muted).child(label))
        .child(field(input).aria_label(label))
}

impl ChatApp {
    /// Asks for a folder and makes it the coding agents' workspace.
    fn choose_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.pop() else {
                return;
            };
            this.update(cx, |this, cx| {
                this.update_settings(cx, |s| s.workspace_root = path.display().to_string())
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings_view.rs"]
mod tests;

impl SettingsTab {
    fn description(self) -> &'static str {
        match self {
            Self::Appearance => "Customize how the app looks.",
            Self::BottomBar => "Choose bottom bar entries, agent usage and clock formats.",
            Self::Tiling => "Arrange docked session panes in the window.",
            Self::Composer => "Set up dictation in the message box.",
            Self::Model => "Set your default model, response behavior and custom instructions.",
            Self::Providers => "Configure coding agents and their workspace.",
            Self::Worktrees => "Manage isolated checkouts used by your chats.",
            Self::Attachments => "Files saved from chats across every project.",
            Self::Tools => "Control tool permissions and planning.",
            Self::Organization => "Run a project as a lead, persistent heads and their workers.",
            Self::Terminal => "Set the terminal's colors, font, cursor and keys.",
        }
    }

    fn icon(self) -> gpui_kit::assets::IconName {
        use gpui_kit::assets::IconName as Lucide;
        match self {
            Self::Appearance => Lucide::Palette,
            Self::BottomBar => Lucide::PanelBottom,
            Self::Tiling => Lucide::LayoutDashboard,
            Self::Composer => Lucide::MessageSquare,
            Self::Model => Lucide::MessageCircle,
            Self::Providers => Lucide::Bot,
            Self::Worktrees => Lucide::GitFork,
            Self::Attachments => Lucide::Paperclip,
            Self::Tools => Lucide::Wrench,
            Self::Organization => Lucide::Users,
            Self::Terminal => Lucide::SquareTerminal,
        }
    }
}

impl ChatApp {
    pub(crate) fn cycle_settings_tab(&mut self, next: bool, cx: &mut Context<Self>) {
        self.select_settings_tab(next_settings_tab(self.settings_tab, next), cx);
    }

    fn select_settings_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        self.settings_tab = tab;
        if tab == SettingsTab::Worktrees {
            self.load_worktrees(cx);
        }
        if tab == SettingsTab::Attachments {
            self.load_stored_files(cx);
        }
        cx.notify();
    }

    fn clear_settings_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_search_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn dismiss_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings_search_input.read(cx).value().is_empty() {
            self.clear_settings_search(window, cx);
        } else {
            self.open_page(crate::app::Page::Chat, cx);
            self.composer.update(cx, |input, cx| input.focus(window, cx));
        }
    }

    fn render_settings_sidebar(
        &self,
        matches: &[&SettingItem],
        searching: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        use crate::actions::{SettingsNext, SettingsPrevious};
        let muted = cx.theme().muted_foreground;
        let ring = cx.theme().ring;
        let mut categories = v_flex()
            .id("settings-categories")
            .role(Role::TabList)
            .aria_label("Settings categories")
            .key_context("SettingsSidebar")
            .track_focus(&self.settings_sidebar_focus)
            .tab_stop(true)
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .corner(rems(0.5))
            .focus_visible(move |style| style.border_color(ring))
            .gap_5()
            .on_action(
                cx.listener(|this, _: &SettingsNext, _, cx| this.cycle_settings_tab(true, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SettingsPrevious, _, cx| this.cycle_settings_tab(false, cx)),
            );
        for (label, tabs) in SETTINGS_GROUPS {
            let mut group = v_flex()
                .gap_1()
                .child(div().px_2().pb_1().text_xs().font_medium().text_color(muted).child(label));
            for &tab in tabs {
                let count = tab_match_count(tab, matches);
                let label = if searching {
                    format!("{} · {} matches", tab.label(), count)
                } else {
                    tab.label().into()
                };
                group = group.child(
                    h_flex()
                        .id(tab.label())
                        .w_full()
                        .h_9()
                        .px_3()
                        .gap_2()
                        .corner(rems(0.5))
                        .text_sm()
                        .cursor_pointer()
                        .role(Role::Tab)
                        .aria_label(label)
                        .aria_selected(tab == self.settings_tab)
                        .when(tab == self.settings_tab, |item| {
                            item.bg(cx.theme().accent).text_color(cx.theme().accent_foreground)
                        })
                        .hover(|item| item.bg(cx.theme().secondary))
                        .child(Icon::new(tab.icon()).size_4())
                        .child(tab.label())
                        .when(searching, |button| {
                            button.child(
                                div()
                                    .ml_auto()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(count.to_string()),
                            )
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_settings_tab(tab, cx);
                            // A category selection exits search; search results remain grouped until selection.
                            this.settings_search_input
                                .update(cx, |input, cx| input.set_value("", window, cx));
                            this.settings_sidebar_focus.focus(window, cx);
                        })),
                );
            }
            categories = categories.child(group);
        }
        v_flex()
            .id("settings-sidebar")
            .debug_selector(|| "settings-sidebar".into())
            .size_full()
            .min_h_0()
            .overflow_y_scroll()
            .p_4()
            .when(
                cfg!(target_os = "macos")
                    && self.settings.sidebar_collapsed
                    && !window.is_fullscreen(),
                |el| el.pt(px(40.)),
            )
            .gap_5()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .child(
                aether_ui::button("settings-back")
                    .ghost()
                    .w_full()
                    .h_9()
                    .px_3()
                    .justify_start()
                    .accessibility_label("Back to chat")
                    .child(
                        h_flex()
                            .w_full()
                            .justify_start()
                            .gap_2()
                            .child(Icon::new(IconName::ArrowLeft).size_4().flex_none())
                            .child(div().min_w_0().text_ellipsis().child("Back to Chat")),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_page(crate::app::Page::Chat, cx);
                        this.composer.update(cx, |input, cx| input.focus(window, cx));
                    })),
            )
            .child(
                field(&self.settings_search_input)
                    .icon(IconName::Search)
                    .aria_label("Search settings"),
            )
            .child(categories)
    }
}
