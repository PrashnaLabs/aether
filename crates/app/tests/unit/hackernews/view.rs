use super::*;
use aether_agent::Agent;
use aether_core::Settings;
use gpui_kit::TestAppContext;

#[gpui_kit::test]
fn news_reader_is_separate_from_agent_browser(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    let (window, app) = cx.update(|cx| {
        let (window, app) =
            gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| {
                    ChatApp::with_state(
                        Agent::new().unwrap(),
                        Settings::default(),
                        None,
                        None,
                        false,
                        window,
                        cx,
                    )
                })
            })
            .unwrap();
        (window, app)
    });
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.open_page(Page::HackerNews, cx);
            let side_tab = app.side_tab;
            assert_ne!(app.hn_browser.entity_id(), app.browser.entity_id());
            for url in ["https://example.com/article", "https://news.ycombinator.com/item?id=42"] {
                app.open_hn_link(url, window, cx);
                assert!(app.hn_reader_open);
                assert_eq!(app.page, Page::HackerNews);
                assert_eq!(app.side_tab, side_tab);
                assert!(!app.browser_open);
                app.close_hn_reader(cx);
                assert!(!app.hn_reader_open);
                assert!(!app.hn_browser.read(cx).native_visible(cx));
            }
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn scrolling_near_bottom_requests_next_page(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    let (window, app) = cx.update(|cx| {
        gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| {
                ChatApp::with_state(
                    Agent::new().unwrap(),
                    Settings::default(),
                    None,
                    None,
                    false,
                    window,
                    cx,
                )
            })
        })
        .unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.page = Page::HackerNews;
            app.hackernews.state = HnState::Loaded;
            app.hackernews.stories = (0..30)
                .map(|id| {
                    let mut story = super::super::types::sample_stories()[0].clone();
                    story.id = id;
                    story
                })
                .collect();
            // An exhausted ID snapshot keeps this UI test entirely offline.
            app.hackernews.story_ids.clear();
            app.hackernews.has_more = true;
            cx.notify();
        });
        let _ = window.draw(cx);
        window.simulate_next_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            assert!(!app.hackernews.loading_more);
            assert_eq!(app.hackernews.page, 0, "top of a full feed must not fetch ahead");
            let scroll = &app.hackernews.scroll;
            assert!(scroll.max_offset().y > px(500.));
            scroll.set_offset(gpui_kit::point(px(0.), -scroll.max_offset().y));
            cx.notify();
        });
        let _ = window.draw(cx);
        window.simulate_next_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let model = &app.read(cx).hackernews;
        assert!(
            model.loading_more || model.page == 1,
            "reaching the bottom must load the next page without a click"
        );
    });
}
