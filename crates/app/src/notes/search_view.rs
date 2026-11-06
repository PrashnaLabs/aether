use super::search::{CONTEXT, Mode};
use super::*;
use crate::actions::{NotesSearchConfirm, NotesSearchDismiss, NotesSearchNext, NotesSearchPrev};
use aether_ui::theme::Corners;
use aether_ui::{Markdown, field, file_icon};
use gpui_kit::component::{ActiveTheme, Icon, Sizable, button::ButtonVariants, h_flex, v_flex};
use gpui_kit::rems;
use gpui_kit::{AnyElement, div, px};
impl ChatApp {
    pub(crate) fn render_notes_search(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.notes.search_ui.open {
            return None;
        }
        let state = &self.notes.search_ui;
        let theme = cx.theme();
        let border = theme.border;
        let muted = theme.muted_foreground;
        let selected = state.selected;
        let height = (f32::from(window.viewport_size().height) - 100.).clamp(240., 600.);
        let width = (f32::from(window.viewport_size().width) - 48.).clamp(320., 1000.);
        let mode = if self.notes.search.read(cx).value().starts_with("/s ") {
            Mode::Grep
        } else {
            state.mode
        };
        let mut modes = h_flex().gap_1();
        for (label, selected_mode) in [("Files", Mode::Files), ("Grep", Mode::Grep)] {
            modes = modes.child(
                aether_ui::button(label)
                    .xsmall()
                    .ghost()
                    .label(label)
                    .when(mode == selected_mode, |s| s.bg(cx.theme().accent))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.notes.search_ui.mode = selected_mode;
                        let query = this.notes.search.read(cx).value().to_string();
                        if let Some(query) = query.strip_prefix("/s ") {
                            this.notes.search.update(cx, |s, cx| s.set_value(query, window, cx));
                        }
                        this.notes.search.update(cx, |s, cx| s.focus(window, cx));
                        this.queue_notes_search(cx);
                    })),
            );
        }
        let results = if state.loading {
            div().p_4().text_sm().text_color(muted).child("Searching…").into_any_element()
        } else if state.results.is_empty() {
            div()
                .p_4()
                .text_sm()
                .text_color(muted)
                .child(
                    if mode == Mode::Grep && self.notes.search.read(cx).value().trim().is_empty() {
                        "Type text to search inside notes."
                    } else {
                        "No notes found."
                    },
                )
                .into_any_element()
        } else {
            gpui_kit::uniform_list(
                "notes-search-results",
                state.results.len(),
                cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                    let theme = cx.theme();
                    let muted = theme.muted_foreground;
                    let accent = theme.accent;
                    range
                        .filter_map(|index| {
                            let hit = this.notes.search_ui.results.get(index)?.clone();
                            Some(
                                v_flex()
                                    .id(("notes-search-result", index))
                                    .debug_selector(move || {
                                        format!("notes-search-result-{index}")
                                    })
                                    .w_full()
                                    .min_w_0()
                                    .h(px(if mode == Mode::Grep { 56. } else { 36. }))
                                    .flex_none()
                                    .px_3()
                                    .justify_center()
                                    .gap_1()
                                    .overflow_hidden()
                                    .cursor_pointer()
                                    .when(index == selected, |s| s.bg(accent))
                                    .hover(|s| s.bg(accent.opacity(0.7)))
                                    .child(
                                        h_flex()
                                            .w_full()
                                            .min_w_0()
                                            .gap_2()
                                            .child(
                                                div().flex_none().child(
                                                    Icon::new(file_icon(&hit.path)).size_4(),
                                                ),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .text_sm()
                                                    .truncate()
                                                    .child(hit.label),
                                            ),
                                    )
                                    .when_some(hit.line, |row, line| {
                                        row.child(
                                            div()
                                                .w_full()
                                                .pl_6()
                                                .text_xs()
                                                .text_color(muted)
                                                .truncate()
                                                .child(format!("{line}: {}", hit.snippet)),
                                        )
                                    })
                                    .on_click(cx.listener(
                                        move |this, event: &gpui_kit::ClickEvent, window, cx| {
                                            this.notes.search_ui.selected = index;
                                            this.notes
                                                .search
                                                .update(cx, |input, cx| input.focus(window, cx));
                                            this.preview_note_result(cx);
                                            if event.click_count() == 2 {
                                                this.open_note_result(window, cx);
                                            }
                                            cx.notify();
                                        },
                                    ))
                                    .into_any_element(),
                            )
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&state.scroll)
            .with_horizontal_sizing_behavior(gpui_kit::ListHorizontalSizingBehavior::FitList)
            .size_full()
            .into_any_element()
        };
        let mut preview = v_flex()
            .id("notes-search-preview")
            .debug_selector(|| "notes-search-preview".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .border_l_1()
            .border_color(border);
        if let Some(hit) = state.results.get(selected) {
            preview = preview.child(
                div()
                    .w_full()
                    .flex_none()
                    .px_4()
                    .py_2()
                    .text_xs()
                    .text_color(muted)
                    .border_b_1()
                    .border_color(border)
                    .truncate()
                    .child(
                        hit.line
                            .map(|line| format!("{} · line {line}", hit.label))
                            .unwrap_or_else(|| hit.label.clone()),
                    ),
            );
        }
        let body = if let Some(data) = &state.preview {
            if data.markdown {
                let view = cx.entity().downgrade();
                div()
                    .p_4()
                    .child(
                        Markdown::new("notes-search-document", data.text.clone().into()).on_reflow(
                            Some(std::rc::Rc::new(move |cx| {
                                view.update(cx, |_, cx| cx.notify()).ok();
                            })),
                        ),
                    )
                    .into_any_element()
            } else {
                let mut code =
                    v_flex().p_3().text_sm().font_family(cx.theme().mono_font_family.clone());
                for (index, line) in data.text.lines().take(2000).enumerate() {
                    let row = data.first_line + index;
                    code = code.child(
                        h_flex()
                            .gap_3()
                            .when(data.matched_line == Some(row), |s| s.bg(cx.theme().accent))
                            .child(
                                div()
                                    .w(px(40.))
                                    .flex_none()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(row.to_string()),
                            )
                            .child(div().min_w_0().whitespace_normal().child(line.to_string())),
                    );
                }
                code.into_any_element()
            }
        } else {
            div()
                .p_4()
                .text_sm()
                .text_color(muted)
                .child(if state.preview_loading {
                    "Loading preview…"
                } else {
                    "Select a result to preview it."
                })
                .into_any_element()
        };
        preview = preview.child(
            div()
                .id("notes-search-preview-scroll")
                .w_full()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(body),
        );
        if state.preview.as_ref().is_some_and(|p| p.truncated) {
            preview = preview.child(
                div()
                    .px_4()
                    .py_1()
                    .text_xs()
                    .text_color(muted)
                    .child("Preview shortened. Open the note for the full text."),
            );
        }
        if let Some(error) = &state.error {
            preview = preview.child(
                div()
                    .p_3()
                    .text_xs()
                    .text_color(aether_ui::theme::danger_text(cx))
                    .child(error.clone()),
            );
        }
        Some(
            div()
                .id("notes-search-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(cx.theme().background.opacity(0.65))
                .flex()
                .items_start()
                .justify_center()
                .pt(px(48.))
                .on_click(cx.listener(|this, _, window, cx| this.close_notes_search(window, cx)))
                .child(
                    v_flex()
                        .id("notes-search-modal")
                        .debug_selector(|| "notes-search-modal".into())
                        .key_context(CONTEXT)
                        .on_action(cx.listener(|this, _: &NotesSearchNext, _, cx| {
                            this.move_note_result(1, cx)
                        }))
                        .on_action(cx.listener(|this, _: &NotesSearchPrev, _, cx| {
                            this.move_note_result(-1, cx)
                        }))
                        .on_action(cx.listener(|this, _: &NotesSearchConfirm, window, cx| {
                            this.open_note_result(window, cx)
                        }))
                        .on_action(cx.listener(|this, _: &NotesSearchDismiss, window, cx| {
                            this.close_notes_search(window, cx)
                        }))
                        .w(px(width))
                        .h(px(height))
                        .bg(cx.theme().popover)
                        .border_1()
                        .border_color(border)
                        .corner(rems(0.5))
                        .shadow_lg()
                        .overflow_hidden()
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            div().p_3().flex_none().border_b_1().border_color(border).child(
                                field(&self.notes.search)
                                    .icon(gpui_kit::component::IconName::Search)
                                    .aria_label("Search all notes"),
                            ),
                        )
                        .child(
                            h_flex()
                                .debug_selector(|| "notes-search-toolbar".into())
                                .w_full()
                                .h_9()
                                .px_3()
                                .flex_none()
                                .border_b_1()
                                .border_color(border)
                                .justify_between()
                                .child(modes)
                                .child(
                                    div().text_xs().text_color(muted).child(format!(
                                        "{} results · All notes",
                                        state.results.len()
                                    )),
                                ),
                        )
                        .child(
                            h_flex()
                                .flex_1()
                                .min_h_0()
                                .overflow_hidden()
                                .w_full()
                                .child(
                                    v_flex()
                                        .debug_selector(|| "notes-search-results-pane".into())
                                        .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                                            // Result navigation must not start the window's text-selection drag.
                                            gpui_kit::base::GlobalState::suppress_text_selection(
                                                cx,
                                            );
                                            cx.stop_propagation();
                                        })
                                        .w(px(width * 0.4))
                                        .h_full()
                                        .min_h_0()
                                        .flex_none()
                                        .overflow_hidden()
                                        .py_1()
                                        .child(results),
                                )
                                .child(preview),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .h_8()
                                .flex_none()
                                .px_3()
                                .border_t_1()
                                .border_color(border)
                                .text_xs()
                                .text_color(muted)
                                .child("↑↓ Preview    Enter Open    Esc Close"),
                        ),
                )
                .into_any_element(),
        )
    }
}
