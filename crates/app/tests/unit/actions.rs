#[gpui_kit::test]
fn slash_focus_is_limited_to_chat_outside_editable_controls(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::KeyContext;
    cx.update(|cx| {
        crate::actions::bind_keys(cx);
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let binding = keymap.bindings_for_action(&crate::actions::FocusComposer).next().unwrap();
        let predicate = binding.predicate().unwrap();
        let context = |name| KeyContext::parse(name).unwrap();
        assert!(predicate.eval(&[context("Chat")]));
        assert!(predicate.eval(&[context("AetherSession")]));
        assert!(!predicate.eval(&[context("Aether")]));
        for control in [
            "Input",
            "Editor",
            "Terminal",
            "PopupMenu",
            "ChoiceMenu",
            "BranchSelect",
            "AgentSelector",
            "WorkspaceSelect",
            "Select",
            "Command",
            "AskCard",
        ] {
            let stack = [context("Chat"), context("AetherSession"), context(control)];
            for depth in 1..=stack.len() {
                assert!(
                    !predicate.eval_inner(&stack[..depth], &stack),
                    "slash must remain available in {control}"
                );
            }
        }
    });
}

#[test]
fn cycle_chat_layout_action_is_registered() {
    let source = include_str!("../../src/actions.rs");
    assert!(source.contains("CycleChatLayout"));
}

#[test]
fn open_usage_keybinding_is_registered() {
    let source = include_str!("../../src/actions.rs");
    assert!(source.contains("KeyBinding::new(\"secondary-u\", OpenUsage, None)"));
}

#[test]
fn composer_history_keybindings_are_registered() {
    let source = include_str!("../../src/actions.rs");
    assert!(source.contains("KeyBinding::new(\"up\", ComposerHistoryPrev, history)"));
    assert!(source.contains("KeyBinding::new(\"down\", ComposerHistoryNext, history)"));
}

#[test]
fn branch_search_keybindings_are_registered() {
    let source = include_str!("../../src/actions.rs");
    for key in [
        "KeyBinding::new(\"up\", PopUp, Some(branch_field.as_str()))",
        "KeyBinding::new(\"down\", PopDown, Some(branch_field.as_str()))",
        "KeyBinding::new(\"enter\", PopConfirm, Some(branch_field.as_str()))",
    ] {
        assert!(source.contains(key), "{key}");
    }
}

#[test]
fn publish_name_keybindings_are_registered() {
    let source = include_str!("../../src/actions.rs");
    for key in [
        "KeyBinding::new(\"up\", PopUp, Some(publish_field.as_str()))",
        "KeyBinding::new(\"down\", PopDown, Some(publish_field.as_str()))",
        "KeyBinding::new(\"enter\", PopConfirm, Some(publish_field.as_str()))",
    ] {
        assert!(source.contains(key), "{key}");
    }
}

#[test]
fn script_form_keybindings_are_registered() {
    let source = include_str!("../../src/actions.rs");
    assert!(source.contains("KeyBinding::new(\"enter\", PopConfirm, Some(script_field.as_str()))"));
}

#[test]
fn menu_bar_keybindings_are_registered() {
    let source = include_str!("../../src/actions.rs");
    for key in [
        "KeyBinding::new(\"escape\", SelectDismiss, Some(select_field.as_str()))",
        "KeyBinding::new(\"enter\", QuickOpenConfirm, Some(quick_open.as_str()))",
        "KeyBinding::new(\"escape\", QuickOpenDismiss, Some(quick_open.as_str()))",
        "KeyBinding::new(\"escape\", StopGenerating, Some(generating.as_str()))",
        "KeyBinding::new(\"escape\", BackToChat, Some(\"Aether\"))",
        "KeyBinding::new(\"cmd-q\", Quit, None)",
    ] {
        assert!(source.contains(key), "{key}");
    }
}

#[test]
fn search_workspace_keybinding_is_registered() {
    let source = include_str!("../../src/actions.rs");
    assert!(source.contains("KeyBinding::new(\"secondary-shift-f\", SearchWorkspace, None)"));
}

#[test]
fn draft_editor_keybindings_are_registered() {
    let source = include_str!("../../src/actions.rs");
    for line in [
        r#"KeyBinding::new("ctrl-g", EditDraft, Some(composer.as_str()))"#,
        r#"KeyBinding::new("escape", DraftEditorDone, Some(draft.as_str()))"#,
        r#"KeyBinding::new("ctrl-g", DraftEditorDone, Some(draft.as_str()))"#,
        r#"KeyBinding::new("secondary-s", DraftEditorDone, Some(draft.as_str()))"#,
        r#"KeyBinding::new("secondary-enter", DraftEditorSend, Some(draft.as_str()))"#,
    ] {
        assert!(source.contains(line), "{line}");
    }
}

#[test]
fn dictation_keybinding_is_registered_and_shown() {
    let source = include_str!("../../src/actions.rs");
    let line = r#"KeyBinding::new("secondary-shift-m", ToggleVoice, Some(COMPOSER_CONTEXT))"#;
    assert!(source.contains(line));
    let chip = include_str!("../../src/app/composer/mod.rs");
    assert!(chip.contains("&ToggleVoice,"));
}

#[test]
fn draft_editor_copy_is_verbatim() {
    let source = include_str!("../../src/app/draft_editor.rs");
    for text in ["Edit draft", "Esc or Ctrl+G to return · ⌘↩ to send", "Discard changes"] {
        assert!(source.contains(text), "{text}");
    }
}

#[test]
fn ask_card_keybindings_are_registered() {
    let source = include_str!("../../src/actions.rs");
    for key in [
        "KeyBinding::new(\"enter\", AskPrimary, ask)",
        "KeyBinding::new(\"escape\", AskDismiss, ask)",
        "KeyBinding::new(\"up\", AskPrev, ask)",
        "KeyBinding::new(\"down\", AskNext, ask)",
        "KeyBinding::new(\"space\", AskToggle, ask)",
    ] {
        assert!(source.contains(key), "{key}");
    }
    assert_eq!(super::ASK_CONTEXT, "AskCard");
}

#[gpui_kit::test]
fn tiling_keys_win_in_a_session_composer(cx: &mut gpui_kit::TestAppContext) {
    use crate::actions::*;
    use gpui_kit::{Action, KeyContext, Keystroke};
    cx.update(|cx| {
        bind_keys(cx);
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let stack = ["Aether", "SessionWorkspace", "AetherSession", "Composer", "Input"]
            .map(|c| KeyContext::parse(c).unwrap());
        let mut keys: Vec<(&str, &dyn Action)> = vec![
            ("ctrl-alt-l", &CycleTileLayout),
            ("ctrl-alt-shift-left", &SwapSessionLeft),
            ("ctrl-alt-shift-right", &SwapSessionRight),
            ("ctrl-alt-shift-up", &SwapSessionUp),
            ("ctrl-alt-shift-down", &SwapSessionDown),
            ("ctrl-alt-enter", &PromoteSession),
            ("ctrl-alt-f", &ToggleSessionFloat),
        ];
        if cfg!(target_os = "macos") {
            keys.extend([
                ("ctrl-alt-[", &NarrowMainPane as &dyn Action),
                ("ctrl-alt-]", &WidenMainPane),
            ]);
        }
        for (key, action) in keys {
            let (bindings, _) =
                keymap.bindings_for_input(&[Keystroke::parse(key).unwrap()], &stack);
            assert!(bindings.first().is_some_and(|b| b.action().partial_eq(action)), "{key}");
        }
    });
}
