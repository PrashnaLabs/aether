use super::*;
use crate::app::{Page, SidebarView};
use aether_agent::Agent;
use aether_core::{Provider, Settings};
use gpui_kit::{AnyWindowHandle, Modifiers, TestAppContext, VisualTestContext};

fn open(cx: &mut TestAppContext, size: Option<(f32, f32)>) -> (AnyWindowHandle, Entity<ChatApp>) {
    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    let options = gpui_kit::WindowOptions {
        window_bounds: size.map(|(w, h)| {
            gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds {
                origin: Default::default(),
                size: gpui_kit::size(px(w), px(h)),
            })
        }),
        ..Default::default()
    };
    cx.update(|cx| {
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| {
                ChatApp::with_state(
                    Agent::new().unwrap(),
                    Settings {
                        provider: Provider::OpenRouter,
                        workspace_root: std::env::temp_dir().to_string_lossy().into_owned(),
                        ..Settings::default()
                    },
                    None,
                    None,
                    false,
                    window,
                    cx,
                )
            })
        })
        .unwrap()
    })
}

fn draw(visual: &mut VisualTestContext, app: &Entity<ChatApp>, edit: impl FnOnce(&mut ChatApp)) {
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            edit(app);
            cx.notify();
        });
        let _ = window.draw(cx);
    });
}

fn click(visual: &mut VisualTestContext, selector: &'static str) {
    let bounds = visual.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} missing"));
    visual.simulate_click(bounds.center(), Modifiers::default());
}

#[gpui_kit::test]
fn status_bar_stays_at_the_bottom_with_the_sidebar_collapsed_or_not(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    let mut visual = VisualTestContext::from_window(window, cx);
    for collapsed in [false, true] {
        draw(&mut visual, &app, |app| app.settings.sidebar_collapsed = collapsed);
        let viewport = visual.update(|window, _| window.viewport_size());
        let bounds = visual.debug_bounds("status-bar").expect("status bar must be visible");
        assert!(bounds.size.height > px(0.));
        assert!((bounds.bottom() - viewport.height).abs() < px(2.));
        assert!(bounds.size.width >= viewport.width - px(2.));
    }
}

#[gpui_kit::test]
fn status_bar_has_nav_buttons(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    let mut visual = VisualTestContext::from_window(window, cx);
    for on in [false, true] {
        draw(&mut visual, &app, |app| {
            app.settings.notes_enabled = on;
            app.settings.hackernews_enabled = on;
        });
        for id in ["nav-direct", "nav-archive", "nav-terminal", "nav-settings"] {
            assert!(visual.debug_bounds(id).is_some(), "{id} missing");
        }
        assert_eq!(visual.debug_bounds("nav-notes").is_some(), on);
        assert_eq!(visual.debug_bounds("nav-hackernews").is_some(), on);
    }
}

#[gpui_kit::test]
fn nav_click_opens_page(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    let mut visual = VisualTestContext::from_window(window, cx);
    draw(&mut visual, &app, |_| {});
    click(&mut visual, "nav-settings");
    assert_eq!(app.read_with(&visual, |app, _| app.page), Page::Settings);
    draw(&mut visual, &app, |_| {});
    click(&mut visual, "nav-settings");
    assert_eq!(app.read_with(&visual, |app, _| app.page), Page::Chat);
}

#[gpui_kit::test]
fn direct_count_badge_shows(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    let mut visual = VisualTestContext::from_window(window, cx);
    draw(&mut visual, &app, |_| {});
    let direct = app.read_with(&visual, |app, _| {
        app.chats.iter().filter(|c| !c.archived && !c.is_agent()).count()
    });
    assert!(direct > 0);
    assert_eq!(app.read_with(&visual, |app, _| app.direct_chat_count()), direct);
    assert!(visual.debug_bounds("nav-direct-count").is_some());
    draw(&mut visual, &app, |app| app.chats.iter_mut().for_each(|c| c.archived = true));
    assert_eq!(app.read_with(&visual, |app, _| app.direct_chat_count()), 0);
    assert!(visual.debug_bounds("nav-direct-count").is_none());
}

#[gpui_kit::test]
fn bar_has_no_back_or_usage_nav(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    let mut visual = VisualTestContext::from_window(window, cx);
    for view in [
        SidebarView::Projects,
        SidebarView::DirectChats,
        SidebarView::Archived,
        SidebarView::Outside,
    ] {
        draw(&mut visual, &app, |app| app.sidebar_view = view);
        for id in ["nav-back", "nav-usage"] {
            assert!(visual.debug_bounds(id).is_none(), "{id} in {view:?}");
        }
    }
}

#[gpui_kit::test]
fn sidebar_footer_back_only_in_sub_views(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    let mut visual = VisualTestContext::from_window(window, cx);
    draw(&mut visual, &app, |app| app.sidebar_view = SidebarView::Projects);
    assert!(visual.debug_bounds("sidebar-back").is_none());
    for view in [SidebarView::DirectChats, SidebarView::Archived, SidebarView::Outside] {
        draw(&mut visual, &app, |app| app.sidebar_view = view);
        click(&mut visual, "sidebar-back");
        assert_eq!(app.read_with(&visual, |app, _| app.sidebar_view), SidebarView::Projects);
    }
}

#[test]
fn sidebar_has_no_nav_footer() {
    let source = include_str!("../../../src/app/sidebar.rs");
    let source = source.split("#[cfg(test)]").next().unwrap();
    for gone in [".nav(", "chats_button(", "outside_button("] {
        assert!(!source.contains(gone), "sidebar still has {gone}");
    }
}

#[gpui_kit::test]
fn outside_button_follows_archive_and_opens_its_view(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    let mut visual = VisualTestContext::from_window(window, cx);
    draw(&mut visual, &app, |_| {});
    assert!(visual.debug_bounds("nav-outside").is_none(), "hidden with nobody outside");
    draw(&mut visual, &app, |app| {
        app.external = vec![aether_agent::ExternalAgent {
            provider: Provider::Claude,
            pid: Some(42),
            started: String::new(),
            elapsed: String::new(),
            cwd: "/tmp/proj".into(),
            session: String::new(),
            title: String::new(),
            state: aether_agent::ExternalState::Idle,
            exact: true,
            origin: String::new(),
        }]
    });
    let outside = visual.debug_bounds("nav-outside").expect("nav-outside missing");
    let archive = visual.debug_bounds("nav-archive").unwrap();
    let terminal = visual.debug_bounds("nav-terminal").unwrap();
    assert!(archive.right() <= outside.left() && outside.right() <= terminal.left());
    assert!(visual.debug_bounds("nav-outside-count").is_some());
    assert!(visual.debug_bounds("nav-outside-active").is_none());
    click(&mut visual, "nav-outside");
    assert_eq!(app.read_with(&visual, |app, _| app.sidebar_view), SidebarView::Outside);
    draw(&mut visual, &app, |_| {});
    assert!(visual.debug_bounds("nav-outside-active").is_some());
    assert!(visual.debug_bounds("nav-archive-active").is_none());
}

#[gpui_kit::test]
fn nav_works_with_sidebar_collapsed(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    let mut visual = VisualTestContext::from_window(window, cx);
    draw(&mut visual, &app, |app| app.settings.sidebar_collapsed = true);
    click(&mut visual, "nav-settings");
    assert_eq!(app.read_with(&visual, |app, _| app.page), Page::Settings);
}

#[gpui_kit::test]
fn nav_stays_whole_in_a_narrow_window(cx: &mut TestAppContext) {
    let (window, app) = open(cx, Some((320., 600.)));
    let mut visual = VisualTestContext::from_window(window, cx);
    draw(&mut visual, &app, |app| app.settings.sidebar_collapsed = true);
    let viewport = visual.update(|window, _| window.viewport_size());
    assert!(viewport.width <= px(330.));
    let settings = visual.debug_bounds("nav-settings").expect("nav-settings missing");
    assert!(settings.right() <= viewport.width);
    assert!(settings.size.width >= px(20.));
}

fn plan(limits: &[(&str, f32, &str)]) -> aether_agent::usage::PlanUsage {
    aether_agent::usage::PlanUsage {
        plan_name: "System default".into(),
        limits: limits
            .iter()
            .map(|(label, left, reset)| aether_agent::usage::PlanLimit::new(*label, *left, *reset))
            .collect(),
        ..Default::default()
    }
}

fn seed(app: &mut ChatApp) {
    app.provider_plan_usages = [
        (
            Provider::Claude,
            plan(&[
                ("Session", 75., "resets in 3h 55m"),
                ("Weekly", 34., "resets in 3d 21h"),
                ("Fable", 100., "resets in 3d 21h"),
            ]),
        ),
        (Provider::Antigravity, plan(&[("Weekly (Gemini)", 99., "resets in 6d 2h")])),
        (Provider::Codex, plan(&[])),
    ]
    .into();
}

fn openrouter() -> aether_agent::usage::PlanUsage {
    aether_agent::usage::PlanUsage::parse_openrouter(
        r#"{"data":{"total_credits":12.34,"total_usage":1.5}}"#,
    )
    .unwrap()
}

fn seed_all(app: &mut ChatApp) {
    seed(app);
    app.provider_plan_usages.insert(Provider::OpenRouter, openrouter());
}

fn open_usage_pop(visual: &mut VisualTestContext, app: &Entity<ChatApp>) {
    draw(visual, app, seed);
    click(visual, "usage-strip");
    draw(visual, app, seed);
    draw(visual, app, seed);
    assert!(visual.debug_bounds("usage-pop").is_some(), "usage popover should open");
}

#[test]
fn headline_picks_highest_used_then_soonest_reset() {
    let claude = plan(&[("Session", 75., "resets in 3h 55m"), ("Weekly", 34., "resets in 3d 21h")]);
    assert_eq!(usage::headline(&claude), Some((66., "3d 21h".into(), None)));
    let tie = plan(&[
        ("Weekly", 50., "resets in 3d 21h"),
        ("Other", 50., "resets soon"),
        ("Session", 50., "resets in 4h 59m"),
    ]);
    assert_eq!(usage::headline(&tie).unwrap().1, "4h 59m");
    let gemini = plan(&[("Weekly (Gemini)", 99., "resets in 6d 2h")]);
    assert_eq!(usage::headline(&gemini), Some((1., "6d 2h".into(), Some("Gemini wk".into()))));
    assert_eq!(usage::headline(&plan(&[])), None);
}

#[test]
fn short_label_for_non_session_weekly() {
    assert_eq!(usage::short_label("Weekly (Gemini)").as_deref(), Some("Gemini wk"));
    assert_eq!(usage::short_label("Gemini Models · Weekly Limit").as_deref(), Some("Gemini wk"));
    assert_eq!(usage::short_label("Session (Claude/GPT)").as_deref(), Some("Claude/GPT 5h"));
    for plain in ["Session", "Weekly", "Credits"] {
        assert_eq!(usage::short_label(plain), None, "{plain}");
    }
}

#[test]
fn updated_ago_formats() {
    let now = 1_000_000_000;
    assert_eq!(usage::updated_ago(now, now), "just now");
    assert_eq!(usage::updated_ago(now, now - 59_000), "just now");
    assert_eq!(usage::updated_ago(now, now - 5 * 60_000), "5m ago");
    assert_eq!(usage::updated_ago(now, now - 2 * 3_600_000), "2h ago");
    assert_eq!(usage::updated_ago(now, now - 3 * 86_400_000), "3d ago");
    assert_eq!(usage::updated_ago(now, now + 5_000), "just now");
}

#[gpui_kit::test]
fn strip_lists_only_providers_with_usage(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    let mut visual = VisualTestContext::from_window(window, cx);
    draw(&mut visual, &app, seed);
    let claude = visual.debug_bounds("usage-strip-claude").expect("claude item");
    let gemini = visual.debug_bounds("usage-strip-antigravity").expect("antigravity item");
    assert!(claude.right() <= gemini.left());
    assert!(visual.debug_bounds("usage-strip-codex").is_none(), "no headline, no item");
    assert!(visual.debug_bounds("usage-strip-openrouter").is_none());
    assert!(visual.debug_bounds("usage-none").is_none());
    let nav = visual.debug_bounds("nav-settings").unwrap();
    assert!(nav.right() <= claude.left());
    draw(&mut visual, &app, seed_all);
    assert!(visual.debug_bounds("usage-strip-openrouter").is_none(), "openrouter is popover-only");
    assert!(visual.debug_bounds("usage-pct-claude").is_some());
    assert!(visual.debug_bounds("usage-pct-openrouter").is_none(), "balance only, no %");
}

#[test]
fn openrouter_shows_balance_not_percent() {
    assert_eq!(
        usage::gauge(Provider::OpenRouter, &openrouter()),
        Some((None, "$12.34".into(), None))
    );
    let claude = plan(&[("Weekly", 34., "resets in 3d 21h")]);
    assert_eq!(usage::gauge(Provider::Claude, &claude), Some((Some(66.), "3d 21h".into(), None)));
    assert_eq!(usage::gauge(Provider::Claude, &plan(&[])), None);
}

#[test]
fn opencode_without_limits_is_pay_as_you_go() {
    let zen = aether_agent::usage::PlanUsage::parse_opencode(
        r#"{"data":{"workspace":{"id":"w","usageURL":"u"},"budgets":[]}}"#,
    )
    .unwrap();
    assert_eq!(usage::gauge(Provider::OpenCode, &zen), Some((None, "Pay-as-you-go".into(), None)));
    let go = plan(&[("Session", 70., "resets in 2h 0m")]);
    assert_eq!(usage::gauge(Provider::OpenCode, &go), Some((Some(30.), "2h 0m".into(), None)));
}

#[gpui_kit::test]
fn rows_sort_by_used_and_openrouter_card_has_no_bars(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    let mut visual = VisualTestContext::from_window(window, cx);
    let hot = |app: &mut ChatApp| {
        seed_all(app);
        app.provider_plan_usages
            .insert(Provider::Antigravity, plan(&[("Weekly (Gemini)", 5., "resets in 6d 2h")]));
    };
    draw(&mut visual, &app, hot);
    click(&mut visual, "usage-strip");
    draw(&mut visual, &app, hot);
    draw(&mut visual, &app, hot);
    let top = |visual: &mut VisualTestContext, id| visual.debug_bounds(id).unwrap().top();
    let (agy, claude, router) = (
        top(&mut visual, "usage-row-antigravity"),
        top(&mut visual, "usage-row-claude"),
        top(&mut visual, "usage-row-openrouter"),
    );
    assert!(agy < claude && claude < router, "95% > 66% > balance");
    assert!(visual.debug_bounds("usage-row-codex").is_none());
    assert!(visual.debug_bounds("usage-row-openrouter-pct").is_none());
    assert!(visual.debug_bounds("usage-row-claude-pct").is_some());
    click(&mut visual, "usage-row-openrouter");
    draw(&mut visual, &app, hot);
    assert!(visual.debug_bounds("usage-card-limit-1").is_some());
    assert!(visual.debug_bounds("usage-card-bar-0").is_none(), "no fake bars");
    click(&mut visual, "usage-row-claude");
    draw(&mut visual, &app, hot);
    assert!(visual.debug_bounds("usage-card-bar-0").is_some());
}

#[gpui_kit::test]
fn narrow_window_card_replaces_list(cx: &mut TestAppContext) {
    let (window, app) = open(cx, Some((660., 700.)));
    cx.update(crate::actions::bind_keys);
    let mut visual = VisualTestContext::from_window(window, cx);
    draw(&mut visual, &app, |app| {
        app.settings.sidebar_collapsed = true;
        seed(app);
    });
    open_usage_pop(&mut visual, &app);
    click(&mut visual, "usage-row-claude");
    draw(&mut visual, &app, seed);
    assert!(visual.debug_bounds("usage-card").is_some());
    assert!(visual.debug_bounds("usage-pop").is_none(), "card takes the list's place");
    visual.simulate_keystrokes("escape");
    draw(&mut visual, &app, seed);
    assert!(visual.debug_bounds("usage-pop").is_some());
    assert!(visual.debug_bounds("usage-card").is_none());
}

#[gpui_kit::test]
fn strip_with_no_usage_draws_without_separators(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    let mut visual = VisualTestContext::from_window(window, cx);
    draw(&mut visual, &app, |app| app.provider_plan_usages.clear());
    assert!(visual.debug_bounds("usage-none").is_some());
    for p in ["claude", "codex", "antigravity", "openrouter"] {
        assert!(
            visual.debug_bounds(Box::leak(format!("usage-strip-{p}").into_boxed_str())).is_none()
        );
    }
    click(&mut visual, "usage-strip");
    draw(&mut visual, &app, |app| app.provider_plan_usages.clear());
    draw(&mut visual, &app, |app| app.provider_plan_usages.clear());
    assert!(visual.debug_bounds("usage-pop").is_some());
    assert!(visual.debug_bounds("usage-row-claude").is_none());
}

#[gpui_kit::test]
fn strip_has_no_used_label_or_refresh(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    let mut visual = VisualTestContext::from_window(window, cx);
    draw(&mut visual, &app, seed);
    assert!(visual.debug_bounds("usage-refresh").is_none());
    let strip = visual.debug_bounds("usage-strip").unwrap();
    let first = visual.debug_bounds("usage-strip-claude").unwrap();
    assert!(first.left() - strip.left() < px(2.), "leading label in {strip:?}");
    open_usage_pop(&mut visual, &app);
    assert!(visual.debug_bounds("usage-pop").is_some());
}

#[gpui_kit::test]
fn wide_row_click_keeps_row_focus(cx: &mut TestAppContext) {
    let (window, app) = open(cx, Some((1280., 800.)));
    cx.update(crate::actions::bind_keys);
    let mut visual = VisualTestContext::from_window(window, cx);
    open_usage_pop(&mut visual, &app);
    click(&mut visual, "usage-row-claude");
    draw(&mut visual, &app, seed);
    let focused = |visual: &mut VisualTestContext| {
        visual.update(|window, cx| {
            app.read(cx).usage_pop.rows.iter().position(|row| row.is_focused(window))
        })
    };
    assert_eq!(focused(&mut visual), Some(0));
    visual.simulate_keystrokes("down");
    draw(&mut visual, &app, seed);
    assert_eq!(focused(&mut visual), Some(2));
}

#[gpui_kit::test]
fn popover_is_compact_only(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    let mut visual = VisualTestContext::from_window(window, cx);
    open_usage_pop(&mut visual, &app);
    assert!(visual.debug_bounds("usage-row-claude").is_some());
    assert!(visual.debug_bounds("usage-row-antigravity").is_some());
    assert!(visual.debug_bounds("usage-row-codex").is_none());
    assert!(visual.debug_bounds("usage-pop-refresh").is_some());
    assert!(visual.debug_bounds("usage-details").is_some());
    for gone in ["usage-mode", "usage-manage", "usage-row-claude-limits"] {
        assert!(visual.debug_bounds(gone).is_none(), "{gone} still drawn");
    }
    let source = include_str!("../../../src/app/status_bar/usage.rs");
    let source = source.split("#[cfg(test)]").next().unwrap();
    for gone in ["all agents", "Manage Accounts", "Segmented", "Detailed"] {
        assert!(!source.contains(gone), "usage.rs still has {gone}");
    }
}

#[gpui_kit::test]
fn card_has_no_back_or_manage(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    cx.update(crate::actions::bind_keys);
    let mut visual = VisualTestContext::from_window(window, cx);
    open_usage_pop(&mut visual, &app);
    click(&mut visual, "usage-row-claude");
    draw(&mut visual, &app, seed);
    assert!(visual.debug_bounds("usage-card").is_some());
    for gone in ["usage-card-back", "usage-card-manage"] {
        assert!(visual.debug_bounds(gone).is_none(), "{gone} still drawn");
    }
    visual.simulate_keystrokes("escape");
    draw(&mut visual, &app, seed);
    assert!(visual.debug_bounds("usage-card").is_none());
    assert!(visual.debug_bounds("usage-pop").is_some());
}

#[gpui_kit::test]
fn provider_row_opens_card_with_all_limits(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    cx.update(crate::actions::bind_keys);
    let mut visual = VisualTestContext::from_window(window, cx);
    open_usage_pop(&mut visual, &app);
    assert!(visual.debug_bounds("usage-card").is_none());
    click(&mut visual, "usage-row-claude");
    draw(&mut visual, &app, seed);
    assert_eq!(app.read_with(&visual, |app, _| app.usage_pop.card), Some(Provider::Claude));
    assert!(visual.debug_bounds("usage-card").is_some());
    for ix in 0..3 {
        let id = Box::leak(format!("usage-card-limit-{ix}").into_boxed_str());
        assert!(visual.debug_bounds(id).is_some(), "{id} missing");
    }
    assert!(visual.debug_bounds("usage-card-limit-3").is_none());
    visual.simulate_keystrokes("escape");
    draw(&mut visual, &app, seed);
    assert!(visual.debug_bounds("usage-card").is_none(), "escape returns to the list");
    assert!(visual.debug_bounds("usage-pop").is_some());
    click(&mut visual, "usage-row-claude");
    draw(&mut visual, &app, seed);
    click(&mut visual, "usage-row-claude");
    draw(&mut visual, &app, seed);
    assert!(visual.debug_bounds("usage-card").is_none(), "second click closes the card");
    visual.simulate_keystrokes("escape");
    draw(&mut visual, &app, seed);
    assert!(visual.debug_bounds("usage-pop").is_none(), "second escape closes");
    open_usage_pop(&mut visual, &app);
    visual.simulate_click(gpui_kit::point(px(400.), px(10.)), Modifiers::default());
    draw(&mut visual, &app, seed);
    assert!(visual.debug_bounds("usage-pop").is_none(), "outside click closes");
}

#[gpui_kit::test]
fn usage_details_opens_usage_page(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    let mut visual = VisualTestContext::from_window(window, cx);
    open_usage_pop(&mut visual, &app);
    // skip the history scan: its tokio work trips gpui's determinism check
    app.update(&mut visual, |app, _| {
        app.usage_history_pending_for = Some(app.effective_usage_window())
    });
    click(&mut visual, "usage-details");
    assert_eq!(app.read_with(&visual, |app, _| app.page), Page::Usage);
    assert!(!app.read_with(&visual, |app, _| app.usage_pop.open));
}

#[test]
fn plan_usage_records_when_it_arrived() {
    let source = include_str!("../../../src/app/providers.rs");
    assert_eq!(source.matches("provider_plan_usages.insert").count(), 1);
    assert!(source.contains("plan_usage_at.insert"));
}

fn sample(cpu: f32, used_gb: u64, total_gb: u64) -> Option<crate::sysinfo::Sample> {
    Some(crate::sysinfo::Sample {
        cpu_percent: cpu,
        mem_used: used_gb << 30,
        mem_total: total_gb << 30,
    })
}

#[gpui_kit::test]
fn status_bar_hides_resources_without_sample(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    let mut visual = VisualTestContext::from_window(window, cx);
    draw(&mut visual, &app, |app| app.sys_sample = None);
    assert!(visual.debug_bounds("sys-cpu").is_none());
    assert!(visual.debug_bounds("sys-mem").is_none());
}

#[gpui_kit::test]
fn status_bar_shows_cpu_ram_with_sample(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    let mut visual = VisualTestContext::from_window(window, cx);
    draw(&mut visual, &app, |app| app.sys_sample = sample(42.7, 12, 16));
    assert!(visual.debug_bounds("sys-cpu").is_some());
    assert!(visual.debug_bounds("sys-mem").is_some());
    assert_eq!(super::resource_text(&sample(42.7, 12, 16).unwrap()), ("43%".into(), "12.0/16 GB".into()));
}

#[test]
fn direct_label_carries_the_count() {
    assert_eq!(super::direct_label(3), "Direct Chats · 3");
    assert_eq!(super::direct_label(0), "Direct Chats");
}

#[gpui_kit::test]
fn escape_closes_the_popover_without_leaving_the_page(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    cx.update(crate::actions::bind_keys);
    let mut visual = VisualTestContext::from_window(window, cx);
    app.update(&mut visual, |app, cx| app.open_page(Page::Settings, cx));
    open_usage_pop(&mut visual, &app);
    click(&mut visual, "usage-row-claude");
    draw(&mut visual, &app, seed);
    visual.simulate_keystrokes("escape");
    draw(&mut visual, &app, seed);
    assert!(visual.debug_bounds("usage-card").is_none(), "escape returns to the list first");
    assert!(visual.debug_bounds("usage-pop").is_some());
    visual.simulate_keystrokes("escape");
    draw(&mut visual, &app, seed);
    assert!(visual.debug_bounds("usage-pop").is_none(), "second escape closes");
    assert_eq!(app.read_with(&visual, |app, _| app.page), Page::Settings);
}

// simulate_keystrokes sends no key-up, and a keyboard click needs one
fn press(visual: &mut VisualTestContext, key: &str) {
    let keystroke = gpui_kit::Keystroke::parse(key).unwrap();
    let (is_held, prefer_character_input) = (false, false);
    let down =
        gpui_kit::KeyDownEvent { keystroke: keystroke.clone(), is_held, prefer_character_input };
    visual.simulate_event(down);
    visual.simulate_event(gpui_kit::KeyUpEvent { keystroke });
}

#[gpui_kit::test]
fn usage_strip_and_rows_work_from_the_keyboard(cx: &mut TestAppContext) {
    let (window, app) = open(cx, None);
    cx.update(crate::actions::bind_keys);
    let mut visual = VisualTestContext::from_window(window, cx);
    draw(&mut visual, &app, seed);
    visual.update(|window, cx| app.read(cx).usage_pop.strip.clone().focus(window, cx));
    draw(&mut visual, &app, seed);
    press(&mut visual, "enter");
    draw(&mut visual, &app, seed);
    draw(&mut visual, &app, seed);
    assert!(visual.debug_bounds("usage-pop").is_some(), "enter on the strip opens");
    let focused = |visual: &mut VisualTestContext| {
        visual.update(|window, cx| {
            app.read(cx).usage_pop.rows.iter().position(|row| row.is_focused(window))
        })
    };
    // rows: claude 0, antigravity 2, details 5; sorted claude (66%) then agy (1%)
    for (key, want) in [("down", 0), ("down", 2), ("down", 5), ("down", 5), ("up", 2)]
    {
        visual.simulate_keystrokes(key);
        draw(&mut visual, &app, seed);
        assert_eq!(focused(&mut visual), Some(want), "{key}");
    }
    for _ in 0..2 {
        visual.simulate_keystrokes("up");
        draw(&mut visual, &app, seed);
    }
    assert_eq!(focused(&mut visual), Some(0));
    press(&mut visual, "enter");
    draw(&mut visual, &app, seed);
    assert_eq!(app.read_with(&visual, |app, _| app.usage_pop.card), Some(Provider::Claude));
}

#[gpui_kit::test]
fn usage_strip_hugs_its_content(cx: &mut TestAppContext) {
    let (window, app) = open(cx, Some((1280., 800.)));
    let mut visual = VisualTestContext::from_window(window, cx);
    draw(&mut visual, &app, seed);
    let strip = visual.debug_bounds("usage-strip").unwrap();
    let last = visual.debug_bounds("usage-strip-antigravity").unwrap();
    assert!(strip.right() - last.right() < px(2.), "{strip:?} vs {last:?}");
    assert!(strip.size.width < px(400.), "{strip:?}");
}

#[gpui_kit::test]
fn mid_width_clips_resources_before_the_clock(cx: &mut TestAppContext) {
    for w in [640., 560., 440., 400.] {
        let (window, app) = open(cx, Some((w, 600.)));
        let mut visual = VisualTestContext::from_window(window, cx);
        draw(&mut visual, &app, |app| {
            app.settings.sidebar_collapsed = true;
            seed_all(app);
            app.sys_sample = sample(42.7, 12, 16);
        });
        let viewport = visual.update(|window, _| window.viewport_size());
        for id in ["nav-direct", "nav-settings"] {
            let nav = visual.debug_bounds(id).unwrap();
            assert!(nav.right() <= viewport.width && nav.size.width >= px(20.), "{w}: {id}");
        }
        let clock = visual.debug_bounds("status-clock").expect("clock");
        let row = visual.debug_bounds("sys-resources").expect("resources row");
        let cpu = visual.debug_bounds("sys-cpu").expect("cpu");
        let clock_clipped = clock.right() > viewport.width;
        let cpu_clipped = row.size.width < px(1.) || cpu.right() > row.right() + px(0.5);
        assert!(!clock_clipped || cpu_clipped, "{w}: clock clipped first {clock:?} {row:?}");
        if w >= 440. {
            assert!(!clock_clipped, "{w}: clock whole {clock:?}");
        }
        if w == 440. {
            assert!(cpu_clipped, "{w}: cpu gives way {row:?} {cpu:?}");
        }
    }
}
