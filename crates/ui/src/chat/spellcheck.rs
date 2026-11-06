use std::{ops::Range, sync::OnceLock, time::Duration};

use gpui_kit::component::{ActiveTheme, input::TextareaState};
use gpui_kit::{
    App, Context, Entity, Render, SharedString, Subscription, Task, UnderlineStyle, Window, canvas,
    point, prelude::*, px,
};
use spellbook::Dictionary;

fn dictionary() -> &'static Dictionary {
    static DICTIONARY: OnceLock<Dictionary> = OnceLock::new();
    DICTIONARY.get_or_init(|| {
        Dictionary::new(
            include_str!("../../assets/spelling/en_US.aff"),
            include_str!("../../assets/spelling/en_US.dic"),
        )
        .expect("bundled English spelling dictionary must parse")
    })
}

/// UTF-8 ranges for prose words; leave code, paths, mentions and URLs alone.
fn prose_words(text: &str) -> Vec<Range<usize>> {
    let mut words = Vec::new();
    let mut code_ticks = 0;
    let mut offset = 0;
    for token in text.split_inclusive(char::is_whitespace) {
        let start = offset;
        offset += token.len();
        let ticks = token.chars().filter(|c| *c == '`').count();
        if ticks > 0 {
            if code_ticks == 0 {
                let first = token.find('`').unwrap();
                let run = token[first..].chars().take_while(|c| *c == '`').count();
                if ticks < run * 2 {
                    code_ticks = run;
                }
            } else if ticks >= code_ticks {
                code_ticks = 0;
            }
            continue;
        }
        if code_ticks > 0 || token.contains(['/', '\\', '@', '_', ':', '=']) {
            continue;
        }
        let word = token.trim_matches(|c: char| !c.is_alphabetic());
        if word.len() < 2
            || !word.chars().all(|c| c.is_alphabetic() || c == '\'' || c == '’')
            || word.chars().skip(1).any(char::is_uppercase)
        {
            continue;
        }
        let begin = start + token.find(word).unwrap();
        words.push(begin..begin + word.len());
    }
    words
}

fn misspellings(text: &str) -> Vec<Range<usize>> {
    let dictionary = dictionary();
    prose_words(text)
        .into_iter()
        .filter(|range| !dictionary.check(&text[range.clone()].replace('’', "'")))
        .collect()
}

/// An independent overlay keeps dictionary work and text checking off the typing path.
pub(super) struct Spellcheck {
    input: Entity<TextareaState>,
    text: SharedString,
    checked: SharedString,
    ranges: Vec<Range<usize>>,
    task: Option<Task<()>>,
    _subscription: Subscription,
}

impl Spellcheck {
    pub(super) fn new(input: Entity<TextareaState>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&input, |this, _, cx| {
            this.check(cx);
            cx.notify();
        });
        let mut this = Self {
            input,
            text: SharedString::default(),
            checked: SharedString::default(),
            ranges: Vec::new(),
            task: None,
            _subscription: subscription,
        };
        this.check(cx);
        this
    }

    fn check(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value();
        if text == self.text {
            return;
        }
        self.text = text.clone();
        self.ranges.clear();
        let executor = cx.background_executor().clone();
        self.task = Some(cx.spawn(async move |this, cx| {
            executor.timer(Duration::from_millis(300)).await;
            let source = text.clone();
            let ranges = executor.spawn(async move { misspellings(&source) }).await;
            let _ = this.update(cx, |this, cx| {
                if this.text == text {
                    this.checked = text;
                    this.ranges = ranges;
                    cx.notify();
                }
            });
        }));
    }
}

impl Render for Spellcheck {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let input = self.input.clone();
        let checked = self.checked.clone();
        let ranges = self.ranges.clone();
        let color = cx.theme().danger;
        canvas(
            |_, _, _| (),
            move |bounds, _, window, cx: &mut App| {
                let input = input.read(cx);
                if input.value() != checked {
                    return;
                }
                let underline =
                    UnderlineStyle { thickness: px(1.), color: Some(color), wavy: true };
                let bounds = bounds.intersect(&input.input_bounds());
                window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
                    for range in &ranges {
                        // Per-character bounds also handle words wrapping across lines.
                        for (index, ch) in checked[range.clone()].char_indices() {
                            let start = range.start + index;
                            let Some(b) = input.range_to_bounds(&(start..start + ch.len_utf8()))
                            else {
                                continue;
                            };
                            if b.size.width > px(0.) {
                                window.paint_underline(
                                    point(b.left(), b.bottom() - px(2.)),
                                    b.size.width,
                                    &underline,
                                );
                            }
                        }
                    }
                });
            },
        )
        .absolute()
        .inset_0()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/spellcheck.rs"]
mod tests;
