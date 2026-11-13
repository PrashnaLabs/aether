use super::*;

#[test]
fn parses_typed_counts() {
    assert_eq!(parse_count(" 2"), Some(2));
    assert_eq!(parse_count("-2"), None);
}

#[test]
fn hire_models_list_the_cheap_tier_first() {
    let list = hire_models(&[Provider::Claude, Provider::Codex]);
    let (claude, codex) = (models(Provider::Claude), models(Provider::Codex));
    assert_eq!(
        list[..2],
        [(Provider::Claude, claude[claude.len() - 1]), (Provider::Codex, codex[2])]
    );
    assert_eq!(list.len(), claude.len() + codex.len());
    assert_eq!(list.last(), Some(&(Provider::Claude, claude[0])));
}

/// Exercise the same modal wrapper and keyed focus lifetime as all three org
/// dialogs, without constructing ChatApp's unrelated services.
struct DialogHarness {
    open: bool,
    invoking: FocusHandle,
    destination: FocusHandle,
    at_destination: bool,
    page_focus_fallback: bool,
    first: FocusHandle,
    last: FocusHandle,
    show_last: bool,
    selector: Option<Entity<AgentSelectorState>>,
    picked: Option<String>,
    effort_picked: Option<String>,
    text_input: Option<Entity<InputState>>,
    text_area: Option<Entity<TextareaState>>,
}

impl gpui_kit::Render for DialogHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut content = div()
            .relative()
            .size_full()
            .child(div().id("invoking").track_focus(&self.invoking).tab_index(0).child("Open"));
        if self.at_destination {
            content = content.child(
                div()
                    .id("destination-input")
                    .track_focus(&self.destination)
                    .tab_index(0)
                    .child("Destination input"),
            );
        }
        if self.open {
            let panel = div()
                .w(px(520.))
                .child(div().id("first").track_focus(&self.first).tab_index(0).child("First"))
                .when(self.show_last, |d| {
                    d.child(div().id("last").track_focus(&self.last).tab_index(0).child("Last"))
                })
                .when_some(self.text_input.clone(), |d, input| d.child(field(&input)))
                .when_some(self.text_area.clone(), |d, area| d.child(field_area(&area)))
                .when_some(self.selector.clone(), |d, selector| {
                    d.child(
                        div()
                            .debug_selector(|| "selector-trigger".into())
                            .w(px(280.))
                            .child(dialog_agent_selector(&selector, window, cx)),
                    )
                });
            let view = cx.entity().downgrade();
            content = content.child(dialog_scrim(
                false,
                panel,
                "Hire a worker".into(),
                window,
                cx,
                move |_, cx| {
                    view.update(cx, |this, cx| {
                        this.open = false;
                        cx.notify();
                    })
                    .ok();
                },
            ));
        }
        content
    }
}

fn harness(cx: &mut gpui::TestAppContext) -> (Entity<DialogHarness>, &mut gpui::VisualTestContext) {
    cx.update(gpui_kit::init);
    let (root, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            // Match ChatApp's page-level fallback, which the modal must override
            // when a wizard step removes its focused control.
            cx.on_focus_lost(window, |this: &mut DialogHarness, window, cx| {
                if this.page_focus_fallback {
                    this.invoking.focus(window, cx);
                }
            })
            .detach();
            DialogHarness {
                open: false,
                invoking: cx.focus_handle().tab_stop(true),
                destination: cx.focus_handle().tab_stop(true),
                at_destination: false,
                page_focus_fallback: true,
                first: cx.focus_handle().tab_stop(true),
                last: cx.focus_handle().tab_stop(true),
                show_last: true,
                selector: None,
                picked: None,
                effort_picked: None,
                text_input: None,
                text_area: None,
            }
        });
        gpui_kit::base::Root::new(view, window, cx)
    });
    let view = cx.update(|_, cx| root.read(cx).view().clone().downcast::<DialogHarness>().unwrap());
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        let focus = view.read(cx).invoking.clone();
        focus.focus(window, cx);
        view.update(cx, |v, cx| {
            v.open = true;
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    (view, cx)
}

// Native keyboard clicks require the key-up that simulate_keystrokes omits.
fn press(visual: &mut gpui::VisualTestContext, key: &str) {
    let keystroke = gpui_kit::Keystroke::parse(key).unwrap();
    visual.simulate_event(gpui_kit::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    visual.simulate_event(gpui_kit::KeyUpEvent { keystroke });
}

#[gpui::test]
fn modal_initial_focus_tab_containment_and_escape_restore(cx: &mut gpui::TestAppContext) {
    let (view, cx) = harness(cx);
    cx.update(|window, cx| {
        assert!(!view.read(cx).invoking.is_focused(window));
        assert!(window.focused(cx).is_some());
    });
    for (key, first) in [
        ("tab", true),
        ("tab", false),
        ("tab", true),
        ("shift-tab", false),
        ("shift-tab", true),
        ("shift-tab", false),
    ] {
        press(cx, key);
        cx.update(|window, cx| {
            let dialog = view.read(cx);
            let expected = if first { &dialog.first } else { &dialog.last };
            assert!(
                expected.is_focused(window),
                "incorrect focus after {key}: actual {:?}, first {:?}, last {:?}, invoking {:?}",
                window.focused(cx),
                dialog.first,
                dialog.last,
                dialog.invoking
            );
        });
    }
    press(cx, "escape");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| {
        assert!(!view.read(cx).open);
        assert!(view.read(cx).invoking.is_focused(window));
    });
}

#[gpui::test]
fn success_dismissal_restores_focus_and_reopening_captures_invoker(cx: &mut gpui::TestAppContext) {
    let (view, cx) = harness(cx);
    for _ in 0..2 {
        // Model success paths remove the overlay without dispatching Cancel.
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.open = false;
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
        cx.update(|window, cx| assert!(view.read(cx).invoking.is_focused(window)));
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.open = true;
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
    }
}

fn selector_harness(
    cx: &mut gpui::TestAppContext,
) -> (Entity<DialogHarness>, &mut gpui::VisualTestContext) {
    selector_harness_with_efforts(cx, vec![])
}

fn selector_harness_with_efforts(
    cx: &mut gpui::TestAppContext,
    efforts: Vec<String>,
) -> (Entity<DialogHarness>, &mut gpui::VisualTestContext) {
    let (view, cx) = harness(cx);
    cx.update(|window, cx| {
        crate::actions::bind_keys(cx);
        let selector = cx.new(|cx| {
            let mut state = AgentSelectorState::new(window, cx);
            state.sync(
                vec![aether_ui::ProviderModels {
                    provider: Provider::Claude,
                    items: vec![aether_ui::ModelItem::custom("opus")],
                    hidden: false,
                }],
                vec![],
                Provider::Claude,
                "opus".into(),
                efforts,
                String::new(),
                cx,
            );
            state
        });
        view.update(cx, |v, cx| {
            cx.subscribe(&selector, |this, _, event: &AgentSelectorEvent, cx| {
                match event {
                    AgentSelectorEvent::Pick { model, .. } => this.picked = Some(model.to_string()),
                    AgentSelectorEvent::Effort(level) => this.effort_picked = Some(level.clone()),
                    _ => return,
                }
                cx.notify();
            })
            .detach();
            v.selector = Some(selector);
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    (view, cx)
}

fn selector_trigger(view: &Entity<DialogHarness>, cx: &mut gpui::VisualTestContext) -> FocusHandle {
    // Traverse the real native trigger, then activate it from the keyboard.
    for _ in 0..3 {
        press(cx, "tab");
    }
    let trigger = cx.update(|window, cx| {
        // Native controls install activation listeners when painted focused.
        window.draw(cx).clear(cx);
        let v = view.read(cx);
        assert!(!v.invoking.is_focused(window));
        assert!(!v.first.is_focused(window));
        assert!(!v.last.is_focused(window));
        window.focused(cx).expect("selector trigger focused")
    });
    trigger
}

fn assert_popup(view: &Entity<DialogHarness>, cx: &mut gpui::VisualTestContext) {
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(view.read(cx).open);
        assert!(
            window
                .context_stack()
                .iter()
                .any(|context| context.contains(aether_ui::AGENT_SELECTOR_CONTEXT)),
            "real popup must be in the rendered focus path"
        );
        let trap = gpui_kit::base::active_focus_trap(window, cx)
            .expect("modal trap remains active in popup");
        assert!(trap.contains_focused(window, cx));
    });
}

#[gpui::test]
fn nested_selector_escape_is_consumed_before_parent(cx: &mut gpui::TestAppContext) {
    let (view, cx) = selector_harness(cx);
    let trigger = selector_trigger(&view, cx);
    press(cx, "space");
    assert_popup(&view, cx);
    cx.update(|window, cx| {
        assert_ne!(window.focused(cx).as_ref(), Some(&trigger), "popup takes focus");
    });
    press(cx, "escape");
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(view.read(cx).open, "first Escape closes only the real popup");
        assert!(trigger.is_focused(window), "popup restores its native trigger");
    });
    press(cx, "escape");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| {
        assert!(!view.read(cx).open);
        assert!(view.read(cx).invoking.is_focused(window));
    });
}

#[gpui::test]
fn modal_exposes_a_named_native_dialog(cx: &mut gpui::TestAppContext) {
    use gpui_kit::Element as _;
    cx.update(|cx| {
        let focus = cx.focus_handle();
        let panel = dialog_panel(div(), "Hire a worker".into(), &focus).into_element();
        assert_eq!(panel.a11y_role(), Some(Role::Dialog));
        let mut node = gpui_kit::accesskit::Node::new(panel.a11y_role().unwrap());
        panel.write_a11y_info(&mut node);
        assert_eq!(node.label(), Some("Hire a worker"));
    });
}

#[gpui::test]
fn removing_a_focused_control_keeps_focus_in_the_dialog(cx: &mut gpui::TestAppContext) {
    let (view, cx) = harness(cx);
    cx.update(|window, cx| {
        let focus = view.read(cx).last.clone();
        focus.focus(window, cx);
        window.draw(cx).clear(cx);
        view.update(cx, |v, cx| {
            v.show_last = false;
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    press(cx, "tab");
    cx.update(|window, cx| assert!(view.read(cx).first.is_focused(window)));
    press(cx, "escape");
    cx.update(|_, cx| assert!(!view.read(cx).open));
}

#[gpui::test]
fn selector_pointer_activation_pick_and_scrim_dismissal(cx: &mut gpui::TestAppContext) {
    let (view, cx) = selector_harness(cx);
    let bounds = cx.debug_bounds("selector-trigger").expect("native trigger bounds");
    // GPUI's AccessKit Click fallback dispatches this same centre down/up pair.
    // This tests its native event path, not the unavailable OS adapter callback.
    let trigger = cx.update(|window, cx| {
        window.dispatch_event(
            gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                button: MouseButton::Left,
                position: bounds.center(),
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        let trigger = window.focused(cx).expect("pointer down focuses native trigger");
        window.dispatch_event(
            gpui_kit::PlatformInput::MouseUp(gpui_kit::MouseUpEvent {
                button: MouseButton::Left,
                position: bounds.center(),
                modifiers: Default::default(),
                click_count: 1,
            }),
            cx,
        );
        trigger
    });
    assert_popup(&view, cx);
    cx.update(|window, cx| assert!(!trigger.is_focused(window) && view.read(cx).open));
    press(cx, "enter");
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert_eq!(view.read(cx).picked.as_deref(), Some("opus"));
        assert!(view.read(cx).open);
        assert!(trigger.is_focused(window));
    });
    cx.simulate_click(gpui_kit::point(px(1.), px(1.)), Default::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| {
        assert!(!view.read(cx).open);
        assert!(view.read(cx).invoking.is_focused(window));
    });
}

#[gpui::test]
fn selector_tab_navigation_dismisses_popup_without_leaving_modal(cx: &mut gpui::TestAppContext) {
    for key in ["tab", "shift-tab"] {
        let (view, visual) = selector_harness(cx);
        let trigger = selector_trigger(&view, visual);
        press(visual, "space");
        assert_popup(&view, visual);
        press(visual, key);
        // Focus-out dismisses the popup after the focus transition is painted.
        visual.update(|window, cx| window.draw(cx).clear(cx));
        visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            assert!(view.read(cx).open);
            assert!(
                !window
                    .context_stack()
                    .iter()
                    .any(|context| context.contains(aether_ui::AGENT_SELECTOR_CONTEXT))
            );
            let trap = gpui_kit::base::active_focus_trap(window, cx).expect("focus stays in modal");
            assert!(trap.contains_focused(window, cx));
            assert!(!view.read(cx).invoking.is_focused(window));
            trigger.focus(window, cx);
        });
        // A popup left open would remove the trigger's on_click listener, so
        // this single activation also proves Tab really dismissed it.
        visual.update(|window, cx| window.draw(cx).clear(cx));
        press(visual, "space");
        assert_popup(&view, visual);
        press(visual, "escape");
        visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            assert!(view.read(cx).open);
            assert!(trigger.is_focused(window));
        });
        press(visual, "escape");
        visual.update(|window, cx| window.draw(cx).clear(cx));
        visual.update(|window, cx| {
            assert!(!view.read(cx).open);
            assert!(view.read(cx).invoking.is_focused(window));
        });
    }
}

#[gpui::test]
fn selector_tab_to_reasoning_keeps_popup_open_and_effort_operable(cx: &mut gpui::TestAppContext) {
    let (view, cx) = selector_harness_with_efforts(cx, vec!["low".into(), "high".into()]);
    let trigger = selector_trigger(&view, cx);
    press(cx, "space");
    assert_popup(&view, cx);
    press(cx, "tab");
    assert_popup(&view, cx);
    press(cx, "enter");
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        let view = view.read(cx);
        assert_eq!(view.effort_picked.as_deref(), Some(""), "first reasoning pill is Auto");
        assert!(view.picked.is_none(), "reasoning activation must not pick a model");
        assert!(view.open);
    });
    assert_popup(&view, cx);
    press(cx, "escape");
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(view.read(cx).open);
        assert!(trigger.is_focused(window));
    });
}

#[gpui::test]
fn navigation_dismissal_preserves_new_destination_focus(cx: &mut gpui::TestAppContext) {
    let (view, cx) = harness(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open = false;
            view.at_destination = true;
            // Navigation assigns destination focus before the closing modal's
            // keyed state is released on the next draw.
            view.destination.focus(window, cx);
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    cx.update(|window, cx| {
        assert!(!view.read(cx).open);
        assert!(
            view.read(cx).destination.is_focused(window),
            "modal release must preserve destination focus"
        );
        assert!(!view.read(cx).invoking.is_focused(window));
    });
}

#[gpui::test]
fn ordinary_dismissal_restores_own_or_absent_focus_without_page_fallback(
    cx: &mut gpui::TestAppContext,
) {
    for blur in [false, true] {
        let (view, visual) = harness(cx);
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.page_focus_fallback = false;
                view.open = false;
                cx.notify();
            });
            if blur {
                window.blur(cx);
            }
            window.draw(cx).clear(cx);
        });
        visual.update(|window, cx| {
            assert!(!view.read(cx).open);
            assert!(view.read(cx).invoking.is_focused(window), "restore invoker when blur={blur}");
        });
    }
}

fn text_harness(
    cx: &mut gpui::TestAppContext,
    multiline: bool,
) -> (
    Entity<DialogHarness>,
    &mut gpui::VisualTestContext,
    gpui_kit::component::input::AnyInputState,
    FocusHandle,
) {
    let (view, visual) = harness(cx);
    let (focused, panel) = visual.update(|window, cx| {
        let panel = window.focused(cx).expect("initial dialog panel focus");
        let input = new_input("Edited head title".into(), window, cx);
        let area = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx).auto_grow(3, 8);
            state.set_value("Edited hire task\nUnsaved role instructions", window, cx);
            state
        });
        let focused = if multiline {
            gpui_kit::component::input::AnyInputState::Textarea(area.clone())
        } else {
            gpui_kit::component::input::AnyInputState::Input(input.clone())
        };
        view.update(cx, |view, cx| {
            view.text_input = Some(input);
            view.text_area = Some(area);
            cx.notify();
        });
        window.draw(cx).clear(cx);
        focused.focus_handle(cx).focus(window, cx);
        window.draw(cx).clear(cx);
        (focused, panel)
    });
    visual.simulate_input(" unsaved");
    (view, visual, focused, panel)
}

fn assert_text_escape_preserves_edits(cx: &mut gpui::TestAppContext, multiline: bool) {
    let (view, visual, input, panel) = text_harness(cx, multiline);
    let before = visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(input.focus_handle(cx).is_focused(window));
        let before = input.value(cx);
        assert!(before.contains("unsaved"));
        before
    });
    press(visual, "escape");
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(view.read(cx).open, "first Escape must preserve unsaved form");
        assert_eq!(input.value(cx), before);
        assert!(panel.is_focused(window), "first Escape moves focus to dialog panel");
        let trap = gpui_kit::base::active_focus_trap(window, cx).expect("modal trap stays active");
        assert!(trap.contains_focused(window, cx));
    });
    press(visual, "escape");
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.update(|window, cx| {
        assert!(!view.read(cx).open, "second deliberate Escape dismisses dialog");
        assert!(view.read(cx).invoking.is_focused(window));
    });
}

#[gpui::test]
fn real_input_escape_preserves_unsaved_edits(cx: &mut gpui::TestAppContext) {
    assert_text_escape_preserves_edits(cx, false);
}

#[gpui::test]
fn real_textarea_escape_preserves_unsaved_edits(cx: &mut gpui::TestAppContext) {
    assert_text_escape_preserves_edits(cx, true);
}

#[gpui::test]
fn text_escape_retains_native_ime_unmarking(cx: &mut gpui::TestAppContext) {
    use gpui_kit::EntityInputHandler as _;
    let (view, visual, focused, panel) = text_harness(cx, false);
    let input = focused.as_input().unwrap().clone();
    let before = visual.update(|window, cx| {
        input.update(cx, |state, cx| {
            state.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
            assert!(state.marked_text_range(window, cx).is_some());
        });
        window.draw(cx).clear(cx);
        input.read(cx).value()
    });
    // Test the input engine's marked-text handling, not the OS IME adapter.
    press(visual, "escape");
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        input.update(cx, |state, cx| {
            assert!(state.marked_text_range(window, cx).is_none());
            assert_eq!(state.value(), before);
        });
        assert!(view.read(cx).open);
        assert!(panel.is_focused(window));
    });
}

#[gpui::test]
fn backdrop_still_dismisses_while_real_textarea_has_unsaved_edits(cx: &mut gpui::TestAppContext) {
    let (view, visual, _, _) = text_harness(cx, true);
    visual.simulate_click(gpui_kit::point(px(1.), px(1.)), Default::default());
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.update(|window, cx| {
        assert!(!view.read(cx).open);
        assert!(view.read(cx).invoking.is_focused(window));
    });
}
