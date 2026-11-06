use super::*;

#[test]
fn checks_prose_without_flagging_code_or_paths() {
    let text = "Fix teh alignment in `wronng code` and ```rust\nlet wronng = true;\n```\nSee https://example.invalid @wronng src/wronng.rs snake_case camelCase API.";
    let bad: Vec<_> = misspellings(text).into_iter().map(|r| &text[r]).collect();
    assert_eq!(bad, ["teh"]);
}

#[test]
fn preserves_utf8_offsets_and_accepts_contractions() {
    let text = "🙂 Don’t change this misspelld word, please.";
    let bad: Vec<_> = misspellings(text).into_iter().map(|r| &text[r]).collect();
    assert_eq!(bad, ["misspelld"]);
}

#[test]
fn dictionary_supports_inflected_words() {
    assert!(misspellings("The buttons were aligned and the changes were committed.").is_empty());
}

#[gpui_kit::test]
fn checks_latest_draft_without_changing_text_or_selection(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        let input = cx.new(|cx| {
            let mut input = TextareaState::new(window, cx);
            input.set_value("teh", window, cx);
            input.set_selected_range(1..1, cx);
            input
        });
        Spellcheck::new(input, cx)
    });
    cx.run_until_parked();
    cx.background_executor.advance_clock(Duration::from_millis(350));
    cx.run_until_parked();
    window
        .update(cx, |spelling, window, cx| {
            assert_eq!(spelling.ranges, [0..3]);
            assert_eq!(spelling.input.read(cx).value(), "teh");
            assert_eq!(spelling.input.read(cx).selected_range(), 1..1);
            spelling.input.update(cx, |input, cx| input.set_value("wronng", window, cx));
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |spelling, window, cx| {
            assert!(
                spelling.ranges.is_empty(),
                "old ranges must clear as soon as the draft changes"
            );
            spelling.input.update(cx, |input, cx| input.set_value("correct", window, cx));
        })
        .unwrap();
    cx.run_until_parked();
    cx.background_executor.advance_clock(Duration::from_millis(350));
    cx.run_until_parked();
    window
        .update(cx, |spelling, _, cx| {
            assert_eq!(spelling.checked, "correct");
            assert!(
                spelling.ranges.is_empty(),
                "cancelled checks must not restore stale underlines"
            );
            assert_eq!(spelling.input.read(cx).value(), "correct");
        })
        .unwrap();
}
