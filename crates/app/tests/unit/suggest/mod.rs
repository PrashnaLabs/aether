use super::*;

#[test]
fn trigger_parser_detects_all_triggers() {
    assert_eq!(find_trigger("/", 1), Some((0, '/', "".into())));
    assert_eq!(find_trigger("/compact", 8), Some((0, '/', "compact".into())));
    assert_eq!(find_trigger("$", 1), Some((0, '$', "".into())));
    assert_eq!(find_trigger("$cargo test", 11), Some((0, '$', "cargo test".into())));
    assert_eq!(find_trigger("hello @", 7), Some((6, '@', "".into())));
    assert_eq!(find_trigger("hello @git", 10), Some((6, '@', "git".into())));
    assert_eq!(find_trigger("file #Cargo", 11), Some((5, '#', "Cargo".into())));
    assert_eq!(find_trigger("plain text", 10), None);
}

#[test]
fn slash_commands_keep_app_commands_and_take_cli_ones_from_the_probe() {
    let source = concat!(
        include_str!("../../../src/suggest/slash.rs"),
        include_str!("../../../src/suggest/mod.rs")
    );
    for name in [
        "clear", "compact", "cost", "doctor", "help", "ask", "autoedit", "auto", "full", "readonly",
    ] {
        assert!(source.contains(&format!("\"{name}\"")), "{name}");
    }
    assert!(source.contains("format!(\"mode {name}\")"));
    assert!(source.contains("p.commands"));
    for fake in ["\"tokens\"", "\"pr\"", "\"bug\"", "\"teamwork-preview\""] {
        assert!(!source.contains(fake), "{fake}");
    }
}

#[test]
fn a_trigger_from_another_draft_is_refused() {
    assert!(trigger_fits("hello @", 6, 7));
    assert!(trigger_fits("/", 0, 1));
    assert!(!trigger_fits("hi", 6, 2));
    assert!(!trigger_fits("é@", 1, 3));
    assert!(!trigger_fits("hello @git", 6, 3));
}

#[gpui_kit::test]
fn slash_commands_offer_every_color_scheme(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            aether_core::Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        )
    });
    let app = window.root(cx).unwrap();
    app.read_with(cx, |app, _| {
        let all = app.slash_commands("theme rose-pine-moon");
        assert!(all.iter().any(|(label, _, _)| label.as_ref() == "/theme rose-pine-moon"));
    });
}

#[gpui_kit::test]
fn slash_commands_offer_btw_in_direct_chats(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            aether_core::Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        )
    });
    let app = window.root(cx).unwrap();
    app.read_with(cx, |app, _| {
        assert!(!app.chats[app.active].is_agent());
        assert!(app.slash_commands("btw").iter().any(|(label, _, _)| label.as_ref() == "/btw"));
    });
}

#[gpui_kit::test]
fn handoff_suggests_installed_providers_except_the_current(cx: &mut gpui_kit::TestAppContext) {
    use aether_core::Provider;
    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            aether_core::Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        )
    });
    window
        .update(cx, |app, _, _| {
            let ready = aether_driver::ProviderProbe { installed: true, ..Default::default() };
            for p in [Provider::Claude, Provider::Codex, Provider::Pi] {
                app.probes.insert(p, ready.clone());
            }
            app.chats[app.active].provider = Provider::Claude;
            app.chats[app.active].messages.push(aether_core::Message::user("hi"));
            let found: Vec<_> = app
                .slash_commands("handoff")
                .into_iter()
                .map(|(label, desc, _)| (label.to_string(), desc.to_string()))
                .collect();
            assert!(found.contains(&("/handoff codex".into(), "Continue in Codex".into())));
            assert!(found.iter().any(|(l, _)| l == "/handoff pi"));
            assert!(!found.iter().any(|(l, _)| l.contains("claude") || l.contains("opencode")));
            app.chats[app.active].messages.clear();
            assert!(app.slash_commands("handoff").is_empty());
        })
        .unwrap();
}
