use super::*;

#[gpui_kit::test]
fn skill_instructions_preview_preserves_source(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        let mut app = ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            aether_core::Settings {
                provider: aether_core::Provider::OpenRouter,
                workspace_root: std::env::temp_dir().display().to_string(),
                ..Default::default()
            },
            None,
            None,
            false,
            window,
            cx,
        );
        app.open_page(Page::Skills, cx);
        app.edit_skill(None, window, cx);
        app.skill_editor.as_ref().unwrap().instructions.update(cx, |input, cx| {
            input.set_value("# Instructions\n\n**Keep this text**", window, cx)
        });
        app
    });
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    for preview in [true, false] {
        let button = visual.debug_bounds("skill-markdown-preview-toggle").unwrap();
        visual.simulate_click(button.center(), gpui_kit::Modifiers::default());
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(visual.debug_bounds("skill-markdown-preview-body").is_some(), preview);
        visual.read(|cx| {
            let editor = app.read(cx).skill_editor.as_ref().unwrap();
            assert_eq!(
                editor.instructions.read(cx).value(),
                "# Instructions\n\n**Keep this text**"
            );
        });
    }
}
