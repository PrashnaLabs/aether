use super::*;
use aether_core::{Provider, Settings};
use gpui_kit::TestAppContext;

#[gpui_kit::test]
fn notes_quick_open_renders_and_closes_for_openrouter(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    let root = std::env::temp_dir().join(format!("aether-notes-quick-open-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut settings = Settings {
        provider: Provider::OpenRouter,
        workspace_root: root.display().to_string(),
        ..Settings::default()
    };
    settings.notes.directory = root.join("notes").display().to_string();
    let window = cx.add_window(|window, cx| {
        ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            settings,
            None,
            None,
            false,
            window,
            cx,
        )
    });
    window
        .update(cx, |app, window, cx| {
            let project = app.active_project_explorer(cx);
            project.update(cx, |exp, _| exp.quick_open_open = true);
            app.open_notes(window, cx);
            app.open_quick_open(window, cx);
            let notes = app.notes.explorer.clone().unwrap();
            assert!(notes.read(cx).quick_open_open);
            assert!(app.render_quick_open_overlay(window, cx).is_some());
            app.close_quick_open(window, cx);
            assert!(!notes.read(cx).quick_open_open);
            assert!(project.read(cx).quick_open_open, "notes close preserves project state");
            assert!(app.render_quick_open_overlay(window, cx).is_none());
        })
        .unwrap();
    let _ = std::fs::remove_dir_all(root);
}
