use aether_ui::theme::Corners;
use chrono::Utc;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{ActiveTheme, IconName, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::{
    AnyElement, Context, ElementId, Entity, MouseButton, Window, div, prelude::*, px, rgb,
};

use super::types::{HnCategory, HnState, HnStory, PrototypeStateOverride};
use crate::app::{ChatApp, Page};

impl ChatApp {
    pub(crate) fn render_hackernews_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if self.hn_reader_open {
            let lights = self.settings.sidebar_collapsed;
            self.hn_browser.update(cx, |b, _| b.lights = lights);
            return v_flex()
                .id("hackernews-reader")
                .size_full()
                .child(div().flex_1().min_h_0().w_full().child(self.hn_browser.clone()));
        }
        // Sync query from search input
        let query = self.hn_search_input.read(cx).value();
        self.hackernews.search_query = query.to_string();

        let active_state = self.hackernews.effective_state();

        v_flex().id("hackernews-page").size_full().child(self.render_hn_header(window, cx)).child(
            div().flex_1().min_h_0().w_full().child(match active_state {
                HnState::Loading => self.render_hn_loading_skeleton(cx).into_any_element(),
                HnState::Error(err) => self.render_hn_error(&err, cx).into_any_element(),
                HnState::Empty => self.render_hn_empty(cx).into_any_element(),
                HnState::Fetching => v_flex()
                    .size_full()
                    .child(self.render_hn_fetching_banner(cx))
                    .child(self.render_hn_story_list(window, cx))
                    .into_any_element(),
                HnState::Loaded => self.render_hn_story_list(window, cx).into_any_element(),
            }),
        )
    }

    /// Header with branding, category filter tabs, search filter, and refresh action
    fn render_hn_header(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let lights = aether_ui::traffic_lights_gap(
            self.page == Page::HackerNews && self.settings.sidebar_collapsed,
            window,
            cx,
        );
        let theme = cx.theme();
        let border = theme.tokens.border;
        let active_cat = self.hackernews.category;
        let is_fetching = matches!(self.hackernews.effective_state(), HnState::Fetching);
        let is_searching = !self.hackernews.search_query.trim().is_empty();
        let view = cx.entity().clone();

        h_flex()
            .flex_none()
            .h(px(52.))
            .w_full()
            .px(px(20.))
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .items_center()
                    .gap(px(14.))
                    .child(lights)
                    // Hacker News Badge and Title
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .w(px(28.))
                                    .h(px(28.))
                                    .corner(px(6.))
                                    .bg(rgb(0xFF6600))
                                    .text_color(rgb(0xFFFFFF))
                                    .font_weight(gpui_kit::FontWeight::BOLD)
                                    .text_size(px(16.))
                                    .child("Y"),
                            )
                            .child(
                                v_flex()
                                    .gap(px(1.))
                                    .child(
                                        div()
                                            .font_semibold()
                                            .text_size(px(15.))
                                            .text_color(theme.tokens.foreground)
                                            .child("Hacker News"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(theme.tokens.muted_foreground)
                                            .child("Curated tech discussions, stories & projects"),
                                    ),
                            ),
                    )
                    .child(div().h(px(20.)).w(px(1.)).bg(border).mx(px(4.)))
                    // Category Filter Tabs
                    .child(h_flex().items_center().gap(px(4.)).children(
                        HnCategory::ALL.iter().map(|&cat| {
                            let selected = cat == active_cat;
                            let view = view.clone();
                            div()
                                .cursor_pointer()
                                .px(px(10.))
                                .py(px(5.))
                                .corner(px(6.))
                                .text_size(px(12.))
                                .font_medium()
                                .when(selected, |s| {
                                    s.bg(theme.tokens.accent.opacity(0.2)).text_color(rgb(0xFF6600))
                                })
                                .when(!selected, |s| {
                                    s.text_color(theme.tokens.muted_foreground).hover(|h| {
                                        h.bg(theme.tokens.accent.opacity(0.1))
                                            .text_color(theme.tokens.foreground)
                                    })
                                })
                                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                    view.update(cx, |this, cx| {
                                        this.select_hn_category(cat, window, cx);
                                    });
                                })
                                .child(cat.label())
                        }),
                    )),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap(px(8.))
                    // Real Search Input Field
                    .child(
                        div().w(px(180.)).child(
                            aether_ui::field(&self.hn_search_input)
                                .small()
                                .icon(IconName::Search)
                                .aria_label("Search Hacker News stories"),
                        ),
                    )
                    .when(is_searching, |row| {
                        row.child(
                            aether_ui::button("hn-clear-search-btn")
                                .ghost()
                                .small()
                                .icon(Lucide::X)
                                .tooltip("Clear search filter")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.hn_search_input.update(cx, |input, cx| {
                                        input.set_value("", window, cx);
                                    });
                                    this.hackernews.search_query.clear();
                                    cx.notify();
                                })),
                        )
                    })
                    // Live Refresh Button
                    .child(
                        aether_ui::button("hn-refresh")
                            .ghost()
                            .small()
                            .icon(Lucide::RefreshCw)
                            .tooltip(if is_fetching {
                                "Fetching stories..."
                            } else {
                                "Refresh Hacker News (Live API)"
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.fetch_hn_stories(window, cx);
                            })),
                    )
                    // Back to Chat Button (when in full page mode)
                    .when(self.page == Page::HackerNews, |row| {
                        row.child(
                            aether_ui::button("hn-back-chat")
                                .ghost()
                                .small()
                                .icon(Lucide::ArrowLeft)
                                .tooltip("Back to Chat (Esc)")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_page(Page::Chat, cx);
                                    this.composer.update(cx, |c, cx| c.focus(window, cx));
                                })),
                        )
                        .child(aether_ui::window_controls_gap(window))
                    }),
            )
    }

    /// Loading State: Renders modern skeleton cards
    fn render_hn_loading_skeleton(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let bar_bg = theme.tokens.muted.opacity(0.4);
        let pill_bg = theme.tokens.muted.opacity(0.3);

        let skeleton_card = |ix: usize| {
            h_flex()
                .gap(px(16.))
                .p(px(14.))
                .corner(px(8.))
                .border_1()
                .border_color(theme.tokens.border)
                .bg(theme.tokens.sidebar.opacity(0.3))
                .items_start()
                // Score placeholder
                .child(div().w(px(52.)).h(px(44.)).corner(px(6.)).bg(pill_bg).flex_none())
                // Content placeholder
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(8.))
                        .child(
                            div()
                                .h(px(14.))
                                .w(px(if ix.is_multiple_of(2) { 380. } else { 480. }))
                                .corner(px(4.))
                                .bg(bar_bg),
                        )
                        .child(
                            h_flex()
                                .gap(px(10.))
                                .items_center()
                                .child(div().h(px(10.)).w(px(70.)).corner(px(3.)).bg(pill_bg))
                                .child(div().h(px(10.)).w(px(90.)).corner(px(3.)).bg(pill_bg))
                                .child(div().h(px(10.)).w(px(110.)).corner(px(3.)).bg(pill_bg)),
                        ),
                )
        };

        v_flex()
            .size_full()
            .p(px(20.))
            .gap(px(10.))
            .child(
                h_flex()
                    .items_center()
                    .gap(px(8.))
                    .pb(px(8.))
                    .child(
                        gpui_kit::svg()
                            .path(Lucide::RefreshCw.path())
                            .size(px(14.))
                            .text_color(rgb(0xFF6600)),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_medium()
                            .text_color(theme.tokens.muted_foreground)
                            .child("Loading stories from Hacker News API..."),
                    ),
            )
            .child(skeleton_card(0))
            .child(skeleton_card(1))
            .child(skeleton_card(2))
            .child(skeleton_card(3))
            .child(skeleton_card(4))
            .child(skeleton_card(5))
    }

    /// Fetching State Banner: Appears above story list during background refresh
    fn render_hn_fetching_banner(&self, _cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .flex_none()
            .h(px(32.))
            .w_full()
            .px(px(20.))
            .items_center()
            .gap(px(8.))
            .bg(rgb(0xFF6600).opacity(0.12))
            .border_b_1()
            .border_color(rgb(0xFF6600).opacity(0.3))
            .text_size(px(12.))
            .text_color(rgb(0xFF6600))
            .font_medium()
            .child(
                gpui_kit::svg()
                    .path(Lucide::RefreshCw.path())
                    .size(px(13.))
                    .text_color(rgb(0xFF6600)),
            )
            .child(div().child("Refreshing feed with latest Hacker News submissions..."))
    }

    /// Error State: Informative error card with retry action
    fn render_hn_error(&self, err_msg: &str, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let border = theme.tokens.border;

        v_flex().size_full().items_center().justify_center().p(px(30.)).child(
            v_flex()
                .max_w(px(460.))
                .p(px(24.))
                .corner(px(12.))
                .border_1()
                .border_color(border)
                .bg(theme.tokens.sidebar)
                .items_center()
                .gap(px(14.))
                .child(
                    div()
                        .w(px(48.))
                        .h(px(48.))
                        .rounded_full()
                        .bg(rgb(0xEF4444).opacity(0.15))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            gpui_kit::svg()
                                .path(Lucide::ClockAlert.path())
                                .size(px(24.))
                                .text_color(rgb(0xEF4444)),
                        ),
                )
                .child(
                    div()
                        .font_semibold()
                        .text_size(px(16.))
                        .text_color(theme.tokens.foreground)
                        .child("Unable to Load Hacker News"),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(theme.tokens.muted_foreground)
                        .child(err_msg.to_string()),
                )
                .child(
                    h_flex()
                        .gap(px(10.))
                        .pt(px(6.))
                        .child(
                            aether_ui::button("hn-retry-error")
                                .primary()
                                .icon(Lucide::RefreshCw)
                                .child("Try Again")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.fetch_hn_stories(window, cx);
                                })),
                        )
                        .child(
                            aether_ui::button("hn-fallback-sample")
                                .ghost()
                                .child("Load Offline Sample")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.hackernews.load_sample();
                                    this.hackernews.state_override = PrototypeStateOverride::Auto;
                                    cx.notify();
                                })),
                        ),
                ),
        )
    }

    /// Empty State: When search filter yields 0 results or empty feed
    fn render_hn_empty(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let is_searching = !self.hackernews.search_query.trim().is_empty();

        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .p(px(30.))
            .gap(px(12.))
            .child(
                div()
                    .w(px(44.))
                    .h(px(44.))
                    .rounded_full()
                    .bg(theme.tokens.muted.opacity(0.2))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        gpui_kit::svg()
                            .path(Lucide::Newspaper.path())
                            .size(px(22.))
                            .text_color(theme.tokens.muted_foreground),
                    ),
            )
            .child(
                div().font_medium().text_size(px(15.)).text_color(theme.tokens.foreground).child(
                    if is_searching {
                        "No stories match your search"
                    } else {
                        "No Hacker News stories found"
                    },
                ),
            )
            .child(div().text_size(px(13.)).text_color(theme.tokens.muted_foreground).child(
                if is_searching {
                    "Try a different search keyword or clear the search filter."
                } else {
                    "Feed is currently empty. Try refreshing or switching categories."
                },
            ))
            .child(
                aether_ui::button("hn-reset-search")
                    .ghost()
                    .small()
                    .child(if is_searching { "Clear Search Filter" } else { "Refresh Stories" })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.hn_search_input.update(cx, |input, cx| {
                            input.set_value("", window, cx);
                        });
                        this.hackernews.search_query.clear();
                        this.fetch_hn_stories(window, cx);
                    })),
            )
    }

    /// Story List: Rendered loaded stories cards with pagination
    fn render_hn_story_list(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let stories = self.hackernews.filtered_stories();
        let now_ts = Utc::now().timestamp();
        let view = cx.entity().clone();
        let loading_more = self.hackernews.loading_more;
        let count = stories.len();
        let scroll = self.hackernews.scroll.clone();
        let generation = self.hackernews.generation;
        let sentinel_view = view.downgrade();
        let can_load = self.hackernews.can_load_more();

        v_flex()
            .id("hn-stories-container")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .p(px(20.))
            .gap(px(8.))
            .children(stories.iter().enumerate().map(|(ix, story)| {
                self.render_hn_story_card(ix + 1, story, now_ts, &view, window, cx)
            }))
            .child(
                gpui_kit::canvas(
                    move |_, window, _| {
                        if can_load && scroll.max_offset().y + scroll.offset().y <= px(500.) {
                            window.on_next_frame(move |window, cx| {
                                let _ = sentinel_view.update(cx, |this, cx| {
                                    if this.hackernews.generation == generation {
                                        this.fetch_hn_more_stories(window, cx);
                                    }
                                });
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .w_full()
                .h(px(1.)),
            )
            .when(loading_more, |container| {
                container.child(div().py_4().text_sm().child("Loading more stories…"))
            })
            .when_some(self.hackernews.pagination_error.clone(), |container, error| {
                container.child(
                    v_flex().gap_2().py_4().child(error).child(
                        aether_ui::button("hn-retry-more")
                            .ghost()
                            .small()
                            .label("Retry loading stories")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.hackernews.pagination_error = None;
                                this.fetch_hn_more_stories(window, cx);
                            })),
                    ),
                )
            })
            .when(!self.hackernews.has_more && count > 0, |container| {
                container.child(
                    div()
                        .w_full()
                        .py(px(16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.))
                        .text_color(cx.theme().tokens.muted_foreground)
                        .child(format!("✓ All {count} stories loaded")),
                )
            })
    }

    /// Single Story Card
    fn render_hn_story_card(
        &self,
        rank: usize,
        story: &HnStory,
        now_ts: i64,
        view: &Entity<Self>,
        _window: &mut Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let border = theme.tokens.border;
        let card_bg = theme.tokens.sidebar.opacity(0.4);
        let hover_bg = theme.tokens.accent.opacity(0.12);

        let id = story.id;
        let title = story.title.clone();
        let link_url = story.link_url();
        let discussion_url = story.discussion_url();
        let domain = story.domain();
        let score = story.score;
        let by = story.by.clone();
        let time_ago = story.time_ago(now_ts);
        let comments = story.comments_count;

        let discuss_view = view.clone();
        let discuss_title = title.clone();
        let discuss_url = link_url.clone();

        let open_view = view.clone();
        let open_url = link_url.clone();

        let comments_view = view.clone();
        let comments_url = discussion_url.clone();

        h_flex()
            .id(ElementId::Name(format!("hn-story-{id}").into()))
            .w_full()
            .p(px(12.))
            .corner(px(8.))
            .border_1()
            .border_color(border)
            .bg(card_bg)
            .hover(|s| s.bg(hover_bg).border_color(rgb(0xFF6600).opacity(0.4)))
            .items_center()
            .justify_between()
            .gap(px(14.))
            // Left: Rank & Upvote Score Pill
            .child(
                h_flex()
                    .items_center()
                    .gap(px(12.))
                    .flex_none()
                    // Rank
                    .child(
                        div()
                            .w(px(26.))
                            .text_align(gpui_kit::TextAlign::Right)
                            .text_size(px(13.))
                            .font_semibold()
                            .text_color(theme.tokens.muted_foreground)
                            .child(format!("{rank}.")),
                    )
                    // Score pill
                    .child(
                        div()
                            .px(px(8.))
                            .py(px(4.))
                            .corner(px(6.))
                            .bg(rgb(0xFF6600).opacity(0.15))
                            .text_color(rgb(0xFF6600))
                            .font_bold()
                            .text_size(px(12.))
                            .items_center()
                            .gap(px(3.))
                            .child(format!("▲ {score}")),
                    ),
            )
            // Center: Title, Domain, Metadata
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(4.))
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .cursor_pointer()
                                    .font_medium()
                                    .text_size(px(14.))
                                    .text_color(theme.tokens.foreground)
                                    .hover(|h| h.text_color(rgb(0xFF6600)))
                                    .on_mouse_down(MouseButton::Left, {
                                        let open_view = open_view.clone();
                                        let open_url = open_url.clone();
                                        move |_, window, cx| {
                                            open_view.update(cx, |this, cx| {
                                                this.open_hn_link(&open_url, window, cx);
                                            });
                                        }
                                    })
                                    .child(title),
                            )
                            // Domain tag
                            .child(
                                div()
                                    .px(px(6.))
                                    .py(px(1.5))
                                    .corner(px(4.))
                                    .bg(theme.tokens.muted.opacity(0.2))
                                    .text_size(px(11.))
                                    .text_color(theme.tokens.muted_foreground)
                                    .child(domain),
                            ),
                    )
                    // Metadata row
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(11.))
                            .text_color(theme.tokens.muted_foreground)
                            .child(format!("by {by}"))
                            .child(div().child("·"))
                            .child(time_ago)
                            .child(div().child("·"))
                            .child(
                                div()
                                    .cursor_pointer()
                                    .hover(|h| h.text_color(theme.tokens.foreground))
                                    .on_mouse_down(MouseButton::Left, {
                                        let comments_view = comments_view.clone();
                                        let comments_url = comments_url.clone();
                                        move |_, window, cx| {
                                            comments_view.update(cx, |this, cx| {
                                                this.open_hn_link(&comments_url, window, cx);
                                            });
                                        }
                                    })
                                    .child(format!("💬 {comments} comments")),
                            ),
                    ),
            )
            // Right: Interactive Action Buttons
            .child(
                h_flex()
                    .items_center()
                    .gap(px(6.))
                    .flex_none()
                    // 1. Discuss with Agent (AI pair programmer feature)
                    .child(
                        aether_ui::button(ElementId::Name(format!("hn-discuss-{id}").into()))
                            .ghost()
                            .small()
                            .icon(Lucide::BotMessageSquare)
                            .child("Ask Agent")
                            .tooltip("Discuss this Hacker News story with your AI agent")
                            .on_click({
                                let discuss_view = discuss_view.clone();
                                let discuss_title = discuss_title.clone();
                                let discuss_url = discuss_url.clone();
                                move |_, window, cx| {
                                    discuss_view.update(cx, |this, cx| {
                                        this.discuss_hn_story_with_agent(
                                            &discuss_title,
                                            &discuss_url,
                                            window,
                                            cx,
                                        );
                                    });
                                }
                            }),
                    )
                    // 2. Open in Built-in Browser
                    .child(
                        aether_ui::button(ElementId::Name(format!("hn-open-{id}").into()))
                            .ghost()
                            .small()
                            .icon(Lucide::Globe)
                            .tooltip("Open article in Aether's Browser tab")
                            .on_click({
                                let open_view = open_view.clone();
                                let open_url = open_url.clone();
                                move |_, window, cx| {
                                    open_view.update(cx, |this, cx| {
                                        this.open_hn_link(&open_url, window, cx);
                                    });
                                }
                            }),
                    )
                    // 3. Discussion Link
                    .child(
                        aether_ui::button(ElementId::Name(format!("hn-comments-{id}").into()))
                            .ghost()
                            .small()
                            .icon(Lucide::MessageSquare)
                            .tooltip("Open HN Comments discussion thread")
                            .on_click({
                                let comments_view = comments_view.clone();
                                let comments_url = comments_url.clone();
                                move |_, window, cx| {
                                    comments_view.update(cx, |this, cx| {
                                        this.open_hn_link(&comments_url, window, cx);
                                    });
                                }
                            }),
                    )
                    // 4. Copy URL to clipboard
                    .child(
                        aether_ui::button(ElementId::Name(format!("hn-copy-{id}").into()))
                            .ghost()
                            .small()
                            .icon(Lucide::Copy)
                            .tooltip("Copy article URL to clipboard")
                            .on_click({
                                let copy_url = link_url.clone();
                                move |_, _, cx| {
                                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                        copy_url.clone(),
                                    ));
                                }
                            }),
                    ),
            )
            .into_any_element()
    }

    /// User clicks "Ask Agent" on a Hacker News story
    pub(crate) fn discuss_hn_story_with_agent(
        &mut self,
        title: &str,
        url: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_page(Page::Chat, cx);
        let prompt = format!(
            "Let's discuss this Hacker News story:\nTitle: {title}\nLink: {url}\n\nCould you summarize the main points and let me know your thoughts?"
        );
        self.composer.update(cx, |comp, cx| {
            comp.set_value(prompt, window, cx);
            comp.focus(window, cx);
        });
        cx.notify();
    }

    /// User opens a story or discussion URL
    pub(crate) fn open_hn_link(&mut self, url: &str, _window: &mut Window, cx: &mut Context<Self>) {
        self.hn_browser.update(cx, |browser, cx| {
            browser.go(url, cx);
        });
        self.hn_reader_open = true;
        cx.notify();
    }

    pub(crate) fn close_hn_reader(&mut self, cx: &mut Context<Self>) {
        self.hn_reader_open = false;
        self.hn_browser.update(cx, |browser, _| browser.set_shown(false));
        cx.notify();
    }

    pub(crate) fn toggle_hackernews_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings.hackernews_enabled {
            return;
        }
        if self.page == Page::HackerNews {
            self.open_page(Page::Chat, cx);
            self.composer.update(cx, |c, cx| c.focus(window, cx));
        } else {
            self.open_page(Page::HackerNews, cx);
            if self.hackernews.stories.is_empty() {
                self.fetch_hn_stories(window, cx);
            }
        }
    }

    pub(crate) fn select_hn_category(
        &mut self,
        cat: HnCategory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.hackernews.category == cat {
            return;
        }

        self.hackernews.cache_current();
        self.hackernews.invalidate_requests();
        self.hackernews.category = cat;
        self.hackernews.scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
        if let Some(cached) = self.hackernews.category_cache.get(&cat).cloned()
            && cached.fetched_at.elapsed() < std::time::Duration::from_secs(300)
        {
            self.hackernews.stories = cached.stories;
            self.hackernews.story_ids = cached.ids;
            self.hackernews.page = cached.page;
            self.hackernews.has_more = cached.has_more;
            self.hackernews.state = if self.hackernews.stories.is_empty() && !cached.has_more {
                HnState::Empty
            } else {
                HnState::Loaded
            };
            self.hackernews.last_fetch = Some(cached.fetched_at);
            cx.notify();
            return;
        }
        self.hackernews.stories.clear();
        self.hackernews.last_fetch = None;
        self.fetch_hn_stories(window, cx);
    }

    pub(crate) fn fetch_hn_stories(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.hackernews.invalidate_requests();
        self.hackernews.state_override = PrototypeStateOverride::Auto;
        self.hackernews.state =
            if self.hackernews.stories.is_empty() { HnState::Loading } else { HnState::Fetching };
        cx.notify();
        let category = self.hackernews.category;
        let generation = self.hackernews.generation;
        cx.spawn(async move |this, cx| {
            let res = super::client::fetch_stories(category, 0, Vec::new()).await;
            this.update(cx, |this, cx| {
                if !this.hackernews.accepts_response(category, generation) {
                    return;
                }
                match res {
                    Ok(page) => this.hackernews.apply_page(0, page),
                    Err(err) => {
                        this.hackernews.state = HnState::Error(err);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn fetch_hn_more_stories(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.hackernews.can_load_more() {
            return;
        }
        self.hackernews.loading_more = true;
        cx.notify();
        let category = self.hackernews.category;
        let generation = self.hackernews.generation;
        let next_page = self.hackernews.page + 1;
        let ids = self.hackernews.story_ids.clone();
        cx.spawn(async move |this, cx| {
            let res = super::client::fetch_stories(category, next_page, ids).await;
            this.update(cx, |this, cx| {
                if !this.hackernews.accepts_response(category, generation) {
                    return;
                }
                this.hackernews.loading_more = false;
                match res {
                    Ok(page) => this.hackernews.apply_page(next_page, page),
                    Err(err) => this.hackernews.pagination_error = Some(err),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
#[path = "../../tests/unit/hackernews/view.rs"]
mod tests;
