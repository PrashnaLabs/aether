// No console window for release builds. `aether mcp` still talks over the pipes its parent passes.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod actions;
mod agents_view;
mod app;
mod assets;
mod attachments;
mod board;
mod browser;
mod explorer;
mod font_picker;
mod git_panel;
pub(crate) mod hackernews;
mod hitl;
mod icon_packs;
mod image_viewer;
mod menus;
mod notes;
mod notify;
mod open_in;
mod palette;
mod project_page;
mod projects;
mod session;
mod session_workspace;
mod settings;
mod settings_search;
#[cfg(test)]
#[path = "../tests/unit/settings_search.rs"]
mod settings_search_tests;
mod settings_view;
mod side_panel;
mod skills_view;
mod suggest;
mod sysinfo;
mod terminal;
mod usage_view;
mod workspace_popover;

use aether_agent::Agent;
use assets::AppAssets;
use gpui_kit::{App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};

fn main() {
    if std::env::args_os().nth(1).is_some_and(|a| a == "mcp") {
        std::process::exit(aether_agent::orchestrator::mcp::run_stdio());
    }
    #[cfg(all(target_os = "macos", debug_assertions))]
    notify::relaunch_in_dev_bundle();
    dotenvy::dotenv().ok();
    let agent = match Agent::new() {
        Ok(agent) => agent,
        Err(e) => {
            eprintln!("aether: couldn't start agent runtime: {e}");
            std::process::exit(1);
        }
    };
    agent.serve_orchestrator();

    gpui_kit::application().with_assets(AppAssets).run(move |cx: &mut App| {
        cx.set_app_identity("dev.aether.Aether", "Aether");
        gpui_kit::init(cx);
        actions::bind_keys(cx);
        menus::init(cx);
        let bounds = session_workspace::initial_window_bounds(cx)
            .unwrap_or_else(|| Bounds::centered(None, size(px(1100.), px(760.)), cx));
        if let Err(e) = gpui_kit::open_window(
            WindowOptions {
                app_id: Some("dev.aether.Aether".into()),
                icon: Some(std::sync::Arc::new(
                    image::load_from_memory(include_bytes!("../assets/app-icons/256.png"))
                        .unwrap_or_else(|e| {
                            eprintln!("aether: couldn't decode app icon: {e}");
                            std::process::exit(1);
                        })
                        .into_rgba8(),
                )),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                app_owns_titlebar_drag: cfg!(any(target_os = "macos", target_os = "linux", target_os = "windows")),
                window_decorations: cfg!(target_os = "linux")
                    .then_some(gpui_kit::WindowDecorations::Client),
                titlebar: Some(TitlebarOptions {
                    title: Some("Aether".into()),
                    appears_transparent: cfg!(any(target_os = "macos", target_os = "windows")),
                    traffic_light_position: cfg!(target_os = "macos")
                        .then(|| gpui_kit::point(px(12.), px(11.))),
                }),
                ..Default::default()
            },
            cx,
            |window, cx| {
                // Dropping a window aborts on Touch Bar Macs (zed#64819); quit, never close.
                window.on_window_should_close(cx, |_, cx| {
                    cx.quit();
                    false
                });
                cx.new(|cx| app::ChatApp::new(agent, window, cx))
            },
        ) {
            eprintln!("aether: couldn't open window: {e}");
            std::process::exit(1);
        }
        cx.activate(true);
    });
}
