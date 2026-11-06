use super::*;

#[test]
fn markdown_extensions_are_case_insensitive() {
    for name in ["README.md", "guide.markdown", "NOTES.MD", "doc.mdown", "doc.mkd"] {
        assert!(is_markdown_file(Path::new(name)), "{name}");
    }
    for name in ["main.rs", "notes.txt", "Markdown", "image.png"] {
        assert!(!is_markdown_file(Path::new(name)), "{name}");
    }
}

#[gpui_kit::test]
fn markdown_preview_preserves_edits_and_line_links_open_source(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = std::env::temp_dir().join(format!("aether-markdown-preview-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.as_path().join("README.md");
    std::fs::write(&path, "# Hello\n\n- native preview\n").unwrap();
    let window = cx.add_window(|window, cx| {
        let mut settings = aether_core::Settings::default();
        settings.provider = aether_core::Provider::Codex;
        settings.workspace_root = dir.as_path().display().to_string();
        let mut app = ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            settings,
            None,
            None,
            false,
            window,
            cx,
        );
        app.open_explorer_file(path.clone(), window, cx);
        app
    });
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("explorer-markdown-preview").is_some());
    let toggle = visual.debug_bounds("markdown-preview-toggle").unwrap();
    visual.simulate_click(toggle.center(), gpui_kit::Modifiers::default());
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            let editor = app.active_explorer_editor(window, cx).unwrap();
            editor.update(cx, |editor, cx| editor.replace_all("# Unsaved heading", window, cx));
        });
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("explorer-markdown-preview").is_none());
    let toggle = visual.debug_bounds("markdown-preview-toggle").unwrap();
    visual.simulate_click(toggle.center(), gpui_kit::Modifiers::default());
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("explorer-markdown-preview").is_some());
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            let editor = app.active_explorer_editor(window, cx).unwrap();
            assert_eq!(editor.read(cx).text().to_string(), "# Unsaved heading");
            app.open_explorer_file_at(path.clone(), Some(2), None, window, cx);
        });
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("explorer-markdown-preview").is_none());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "# Hello\n\n- native preview\n");
    std::fs::remove_dir_all(dir).unwrap();
}
