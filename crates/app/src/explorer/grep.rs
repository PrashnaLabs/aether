use aether_ui::theme::Corners;
use gpui_kit::rems;
use std::ops::Range;
use std::path::Path;
use std::time::Duration;

use aether_explorer::ExplorerState;
use aether_ui::{field, file_icon, file_icon_color, icon_chip};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, StyledExt,
    button::ButtonVariants,
    h_flex,
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    scroll::ScrollableElement,
    v_flex,
};
use gpui_kit::{
    AnyElement, ClipboardItem, Context, Entity, FocusHandle, InteractiveElement, KeyDownEvent,
    Role, ScrollStrategy, SharedString, UniformListScrollHandle, Window, div, prelude::*, px,
    transparent_black, uniform_list,
};

use crate::app::ChatApp;
use crate::git_panel::step;

/// The match list's scroll and focus, and its selected row.
struct GrepNav {
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    selected: usize,
}

/// 1-indexed char column of byte offset `byte` in `line`.
fn char_col(line: &str, byte: usize) -> usize {
    line[..byte].chars().count() + 1
}

impl ChatApp {
    /// Runs workspace grep for what the grep field holds now.
    pub(crate) fn run_explorer_grep(&mut self, cx: &mut Context<Self>) {
        if !self.is_agent_chat(&self.chats[self.active]) {
            return;
        }
        let query = self.explorer_grep_input.read(cx).value().to_string();
        let explorer = self.active_project_explorer(cx);
        explorer.update(cx, |exp, _| exp.grep_options.query = query);
        self.spawn_grep(&explorer, cx);
    }

    /// Greps for `query` once typing pauses for 200 ms.
    pub(crate) fn queue_explorer_grep(&mut self, query: String, cx: &mut Context<Self>) {
        let explorer = self.active_project_explorer(cx);
        let generation = explorer.update(cx, |exp, _| {
            exp.grep_options.query = query;
            exp.grep_generation += 1;
            exp.grep_generation
        });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(200)).await;
            this.update(cx, |this, cx| {
                if explorer.read(cx).grep_generation == generation {
                    this.spawn_grep(&explorer, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Greps `explorer` off the UI thread; a newer run drops this one's results.
    pub(super) fn spawn_grep(&mut self, explorer: &Entity<ExplorerState>, cx: &mut Context<Self>) {
        let job = explorer.update(cx, |exp, _| exp.begin_grep());
        cx.notify();
        let Some((generation, search)) = job else {
            return;
        };
        let task = cx.background_executor().spawn(async move { search() });
        let explorer = explorer.clone();
        cx.spawn(async move |this, cx| {
            let matches = task.await;
            this.update(cx, |_, cx| {
                explorer.update(cx, |exp, _| exp.finish_grep(generation, matches));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Renders the workspace text grep pane.
    pub(super) fn render_explorer_grep_pane(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let explorer_entity = self.active_project_explorer(cx);

        let explorer = explorer_entity.read(cx);
        let count = explorer.grep_results.len();
        let running = explorer.grep_running;
        let case_sensitive = explorer.grep_options.case_sensitive;
        let is_regex = explorer.grep_options.is_regex;
        let whole_word = explorer.grep_options.whole_word;
        let nav = window.use_keyed_state("explorer-grep-nav", cx, |_, cx| GrepNav {
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle().tab_stop(true),
            selected: 0,
        });
        let (scroll, focus) = (nav.read(cx).scroll.clone(), nav.read(cx).focus.clone());
        let focused = focus.is_focused(window);

        let theme = cx.theme();
        let border = theme.border;
        let muted = theme.muted_foreground;

        v_flex()
            .flex_1()
            .min_h_0()
            .child(
                // Search input + options
                v_flex()
                    .p_3()
                    .gap_2()
                    .border_b_1()
                    .border_color(border)
                    .child(
                        h_flex()
                            .w_full()
                            .gap_1p5()
                            .child(div().flex_1().child(
                                field(&self.explorer_grep_input).small().icon(IconName::Search),
                            ))
                            .child(
                                aether_ui::button("run-grep-btn")
                                    .primary()
                                    .small()
                                    .label("Find")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.run_explorer_grep(cx);
                                    })),
                            ),
                    )
                    .child(
                        // Match options toggles
                        h_flex()
                            .gap_1()
                            .child(
                                icon_chip("toggle-case", case_sensitive, cx)
                                    .icon(Lucide::CaseSensitive)
                                    .tooltip("Match Case")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.active_project_explorer(cx).update(cx, |exp, _| {
                                            exp.grep_options.case_sensitive =
                                                !exp.grep_options.case_sensitive;
                                        });
                                        this.run_explorer_grep(cx);
                                    })),
                            )
                            .child(
                                icon_chip("toggle-word", whole_word, cx)
                                    .icon(Lucide::WholeWord)
                                    .tooltip("Match Whole Word")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.active_project_explorer(cx).update(cx, |exp, _| {
                                            exp.grep_options.whole_word =
                                                !exp.grep_options.whole_word;
                                        });
                                        this.run_explorer_grep(cx);
                                    })),
                            )
                            .child(
                                icon_chip("toggle-regex", is_regex, cx)
                                    .icon(Lucide::Regex)
                                    .tooltip("Use Regular Expression")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.active_project_explorer(cx).update(cx, |exp, _| {
                                            exp.grep_options.is_regex = !exp.grep_options.is_regex;
                                        });
                                        this.run_explorer_grep(cx);
                                    })),
                            ),
                    ),
            )
            .child(
                // Results summary & list
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .child(h_flex().px_3().py_1p5().text_xs().text_color(muted).child(if running {
                        "Searching…".to_string()
                    } else {
                        format!("{count} matches found")
                    }))
                    .child(
                        div()
                            .id("grep-results")
                            .flex_1()
                            .min_h_0()
                            .track_focus(&focus)
                            .role(Role::List)
                            .aria_label("Matches")
                            .on_key_down({
                                let nav = nav.clone();
                                cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                                    this.grep_key(ev, &nav, window, cx)
                                })
                            })
                            .child(
                                uniform_list(
                                    "grep-results-rows",
                                    count,
                                    cx.processor(move |this, range: Range<usize>, _, cx| {
                                        this.render_grep_rows(range, focused, &nav, cx)
                                    }),
                                )
                                .track_scroll(&scroll)
                                .size_full()
                                .p_1(),
                            )
                            .vertical_scrollbar(&scroll),
                    ),
            )
            .into_any_element()
    }

    /// The match rows in `range`; `focused` rings the selected one.
    fn render_grep_rows(
        &mut self,
        range: Range<usize>,
        focused: bool,
        nav: &Entity<GrepNav>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let explorer = self.active_project_explorer(cx);
        let view = cx.entity();
        let results = &explorer.read(cx).grep_results;
        let selected = nav.read(cx).selected.min(results.len().saturating_sub(1));
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let primary = theme.primary;
        let ring = theme.ring;

        let start = range.start;
        results
            .get(range)
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(offset, item)| {
                let ix = start + offset;
                let line_num = item.line_number;
                let col = char_col(&item.line_text, item.match_indices.0);
                let snippet = item.line_text.trim().to_string();
                let icon = file_icon(&item.path);
                let icon_color = file_icon_color(&item.path, cx);
                let filename = item
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "file".to_string());
                let is_selected = ix == selected;

                h_flex()
                    .id(("grep-row", ix))
                    .role(Role::ListItem)
                    .aria_label(SharedString::from(format!(
                        "{filename}, line {line_num}, {snippet}"
                    )))
                    .aria_selected(is_selected)
                    .when(is_selected, |r| r.aria_active_descendant())
                    .h(px(48.))
                    .w_full()
                    .px_2()
                    .corner(rems(0.375))
                    .border_1()
                    .border_color(transparent_black())
                    .when(is_selected && focused, |r| r.border_color(ring))
                    .when(is_selected, |r| r.bg(primary.opacity(0.12)))
                    .cursor_pointer()
                    .hover(|r| r.bg(theme.foreground.opacity(0.06)))
                    .on_click(cx.listener({
                        let nav = nav.clone();
                        move |this, _, window, cx| {
                            nav.update(cx, |nav, _| nav.selected = ix);
                            this.open_grep_hit(ix, window, cx);
                        }
                    }))
                    .context_menu({
                        let (view, path) = (view.clone(), item.path.clone());
                        move |menu: PopupMenu, _, _| grep_menu(menu, &view, &path, line_num, col)
                    })
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .child(Icon::new(icon).size(px(13.)).text_color(icon_color))
                                    .child(div().text_xs().font_semibold().child(filename))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(format!(":{}", line_num)),
                                    ),
                            )
                            .child(
                                div()
                                    .font_family(theme.mono_font_family.clone())
                                    .text_xs()
                                    .text_color(muted)
                                    .truncate()
                                    .child(snippet),
                            ),
                    )
                    .into_any_element()
            })
            .collect()
    }

    /// Arrows, Home and End move the match selection; Enter opens it.
    fn grep_key(
        &mut self,
        ev: &KeyDownEvent,
        nav: &Entity<GrepNav>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ev.keystroke.modifiers.modified() {
            return;
        }
        let len = self.active_project_explorer(cx).read(cx).grep_results.len();
        let ix = nav.read(cx).selected.min(len.saturating_sub(1));
        let key = ev.keystroke.key.as_str();
        if key == "enter" {
            cx.stop_propagation();
            self.open_grep_hit(ix, window, cx);
        } else if let Some(to) = step(ix, len, key) {
            cx.stop_propagation();
            nav.update(cx, |nav, _| nav.selected = to);
            nav.read(cx).scroll.scroll_to_item(to, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    /// Opens match `ix` with the cursor on the match.
    fn open_grep_hit(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let explorer = self.active_project_explorer(cx);
        let Some(hit) = explorer.read(cx).grep_results.get(ix) else {
            return;
        };
        let (path, line) = (hit.path.clone(), hit.line_number);
        let col = char_col(&hit.line_text, hit.match_indices.0);
        self.open_explorer_file_at(path, Some(line), Some(col), window, cx);
    }
}

/// Right-click menu of a match row.
fn grep_menu(
    menu: PopupMenu,
    view: &Entity<ChatApp>,
    path: &Path,
    line: usize,
    col: usize,
) -> PopupMenu {
    let (view, open, text) = (view.clone(), path.to_path_buf(), path.display().to_string());
    menu.item(PopupMenuItem::new("Open").icon(IconName::FileText).on_click(move |_, window, cx| {
        view.update(cx, |this, cx| {
            this.open_explorer_file_at(open.clone(), Some(line), Some(col), window, cx)
        })
    }))
    .item(
        PopupMenuItem::new("Copy path").icon(IconName::Copy).on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
        }),
    )
}

#[cfg(test)]
#[path = "../../tests/unit/explorer/grep.rs"]
mod tests;
