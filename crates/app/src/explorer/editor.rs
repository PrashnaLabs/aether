use std::path::{Path, PathBuf};
use std::rc::Rc;

use aether_explorer::{DiskConflict, ExplorerSidebarTab};
use aether_git::{Change, ChangeKind, Stat};
use aether_ui::theme::Corners;
use aether_ui::{
    DiffChange, DiffRow, Markdown, Tab, TabBar, file_icon, file_icon_color, parse_diff,
};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, StyledExt,
    button::ButtonVariants,
    h_flex,
    input::{Editor, RopeExt},
    kbd::Kbd,
    menu::PopupMenuItem,
    v_flex,
};
use gpui_kit::{
    AnyElement, Bounds, ClipboardItem, Context, SharedString, Window, canvas, div, fill, point,
    prelude::*, px, relative, size,
};

use super::{CloseEditorTab, EXPLORER_EDITOR_CONTEXT, NextEditorTab, PrevEditorTab};
use crate::actions::QuickOpen;
use crate::app::ChatApp;

impl ChatApp {
    /// The active file's diff rows for its git `change`, read off the UI thread once
    /// per git status read; the last rows for the same file stay up meanwhile.
    fn explorer_diff_rows(
        &mut self,
        file: &Path,
        top: PathBuf,
        change: Change,
        cx: &mut Context<Self>,
    ) -> Option<Result<Rc<Vec<DiffRow>>, SharedString>> {
        let key = (file.to_path_buf(), change.clone(), self.git_loaded().map_or(0, |l| l.at));
        if self.explorer_diff_key.as_ref() != Some(&key) {
            if self.explorer_diff_key.as_ref().is_none_or(|(f, ..)| f != file) {
                self.explorer_diff = None;
            }
            self.explorer_diff_key = Some(key.clone());
            let task = self.agent.git_diff(top, change);
            cx.spawn(async move |this, cx| {
                let rows = match task.await {
                    Ok(raw) => Ok(cx
                        .background_executor()
                        .spawn(async move { parse_diff(&raw.replace('\t', "    ")) })
                        .await),
                    Err(err) => Err(SharedString::from(err)),
                };
                this.update(cx, |this, cx| {
                    if this.explorer_diff_key == Some(key) {
                        this.explorer_diff = Some(rows.map(Rc::new));
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
        self.explorer_diff.clone()
    }

    /// The "Changed on disk" / "Deleted on disk" strip above the active editor.
    fn render_disk_banner(&self, conflict: DiskConflict, cx: &mut Context<Self>) -> AnyElement {
        let warning = aether_ui::theme::warning_text(cx);
        h_flex()
            .px_4()
            .py_1()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(warning.opacity(0.1))
            .text_xs()
            .child(Icon::new(Lucide::CircleAlert).size(px(13.)).text_color(warning))
            .child(div().flex_1().min_w_0().child(match conflict {
                DiskConflict::Changed => "Changed on disk",
                DiskConflict::Deleted => "Deleted on disk",
            }))
            .when(conflict == DiskConflict::Changed, |bar| {
                bar.child(
                    aether_ui::button("disk-reload")
                        .xsmall()
                        .primary()
                        .label("Reload")
                        .tooltip("Drop your edits and load the file from disk")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.reload_active_explorer_file(window, cx)
                        })),
                )
            })
            .child(
                aether_ui::button("disk-keep")
                    .xsmall()
                    .ghost()
                    .label("Keep mine")
                    .tooltip("Keep your text; saving overwrites the file")
                    .on_click(cx.listener(|this, _, _, cx| this.keep_active_explorer_file(cx))),
            )
            .into_any_element()
    }

    /// Renders the editor canvas including tab bar, gutter, editor, and status bar.
    pub(crate) fn render_explorer_editor_canvas(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let explorer_entity = self.active_project_explorer(cx);
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let sidebar_bg = cx.theme().sidebar;
        let folder_color = aether_ui::theme::mango_text(cx);
        let success_color = aether_ui::theme::success_text(cx);
        let danger_color = aether_ui::theme::danger_text(cx);
        let warning_color = aether_ui::theme::warning_text(cx);

        let explorer = explorer_entity.read(cx);
        let active_tab_idx = explorer.active_tab;
        let tabs: Vec<(PathBuf, bool)> =
            explorer.tabs.iter().map(|t| (t.path.clone(), t.is_dirty)).collect();
        let buffer_id = explorer.active_buffer().map(|b| b.id);
        let current_buf = explorer.active_buffer().map(|b| (b.detect_language(), b.conflict));
        let active_folder = explorer.active_folder();
        let root = explorer.root.clone();
        let labels = explorer.tab_labels();

        if tabs.is_empty() {
            // Empty state hero with continuous top bar
            return v_flex()
                .flex_1()
                .h_full()
                .min_w_0()
                .child(
                    TabBar::new("explorer-empty-bar")
                        .tabs_id("explorer-empty-tabs")
                        .height(px(40.))
                        .bg(sidebar_bg)
                        .border_color(border)
                        .info(div().text_xs().text_color(muted).child("No open files"))
                        .when(self.page == crate::app::Page::Notes, |bar| {
                            bar.action(aether_ui::window_controls_gap(window))
                        }),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_h_0()
                        .items_center()
                        .justify_center()
                        .gap_4()
                        .p_8()
                        .child(
                            Icon::new(IconName::FolderOpen).size(px(48.)).text_color(folder_color),
                        )
                        .child(div().text_lg().font_semibold().child("Project File Explorer"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child("Select a file from the explorer or use Quick Open."),
                        )
                        .child(
                            h_flex()
                                .gap_3()
                                .flex_wrap()
                                .justify_center()
                                .child(
                                    aether_ui::button("hero-quick-open")
                                        .primary()
                                        .small()
                                        .icon(Lucide::FileSearch)
                                        .label("Quick Open")
                                        .children(
                                            Kbd::binding_for_action(&QuickOpen, None, window)
                                                .map(|k| k.appearance(false)),
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_quick_open(window, cx);
                                        })),
                                )
                                .child(
                                    aether_ui::button("hero-grep")
                                        .ghost()
                                        .small()
                                        .icon(IconName::Search)
                                        .label("Workspace Grep")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.set_explorer_sidebar_tab(
                                                ExplorerSidebarTab::Grep,
                                                cx,
                                            );
                                        })),
                                ),
                        ),
                )
                .into_any_element();
        }

        let file_path =
            active_tab_idx.and_then(|i| tabs.get(i)).map(|(p, _)| p.clone()).unwrap_or_default();
        let (language, conflict) = current_buf.unwrap_or(("Plain Text", None));
        let editor = self.active_explorer_editor(window, cx);
        let markdown = is_markdown_file(&file_path);
        let preview =
            markdown && buffer_id.is_some_and(|id| !self.explorer_markdown_source.contains(&id));
        let cursor = editor.as_ref().filter(|_| !preview).map(|e| e.read(cx).cursor_position());

        let git_status = self.git_status();
        let active_git_change =
            git_status.filter(|_| self.page != crate::app::Page::Notes).and_then(|s| {
                s.changes
                    .iter()
                    .find(|c| s.top.join(&c.path) == file_path)
                    .map(|c| (s.top.clone(), c.clone()))
            });
        let diff = active_git_change
            .clone()
            .filter(|_| !preview)
            .and_then(|(top, change)| self.explorer_diff_rows(&file_path, top, change, cx));

        let view = cx.entity();
        let single = tabs.len() == 1;
        let mut tab_bar = TabBar::new("explorer-tab-bar")
            .tabs_id("explorer-tabs-container")
            .height(px(40.))
            .bg(sidebar_bg)
            .border_color(border)
            .when_some(active_tab_idx, |bar, ix| bar.selected_index(ix))
            .tabs(tabs.iter().zip(labels).enumerate().map(|(ix, ((path, dirty), label))| {
                let is_active = Some(ix) == active_tab_idx;
                let icon = file_icon(path);
                let icon_color = file_icon_color(path, cx);
                let path = path.clone();
                let key = path.display().to_string();
                let tip = path.strip_prefix(&root).unwrap_or(&path).display().to_string();
                let (close_path, view) = (path.clone(), view.clone());
                let dirty = *dirty;
                Tab::new(SharedString::from(format!("tab-{key}")), label)
                    .icon(icon)
                    .icon_color(icon_color)
                    .selected(is_active)
                    .dirty(dirty)
                    .tooltip(tip)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_active_explorer_tab(ix, window, cx);
                    }))
                    .on_close(
                        SharedString::from(format!("close-tab-{key}")),
                        "Close file",
                        cx.listener(move |this, _, window, cx| {
                            this.close_explorer_tabs(vec![close_path.clone()], window, cx);
                        }),
                    )
                    .context_menu(move |menu, _, _| {
                        let (close, others) =
                            ((view.clone(), path.clone()), (view.clone(), path.clone()));
                        let copy = path.clone();
                        let save_view = view.clone();
                        let revert_view = view.clone();
                        let (discuss_view, discuss_path) = (view.clone(), path.clone());
                        menu.item(PopupMenuItem::new("Close").icon(Lucide::Close).on_click(
                            move |_, window, cx| {
                                close.0.update(cx, |this, cx| {
                                    this.close_explorer_tabs(vec![close.1.clone()], window, cx)
                                })
                            },
                        ))
                        .item(PopupMenuItem::new("Close others").disabled(single).on_click(
                            move |_, window, cx| {
                                others.0.update(cx, |this, cx| {
                                    this.close_other_explorer_tabs(&others.1, window, cx)
                                })
                            },
                        ))
                        .separator()
                        .item(
                            PopupMenuItem::new("Save")
                                .icon(Lucide::Save)
                                .disabled(!dirty)
                                .on_click(move |_, window, cx| {
                                    save_view.update(cx, |this, cx| {
                                        this.set_active_explorer_tab(ix, window, cx);
                                        this.save_active_explorer_file(window, cx);
                                    })
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Revert")
                                .icon(Lucide::RotateCcw)
                                .disabled(!dirty)
                                .on_click(move |_, window, cx| {
                                    revert_view.update(cx, |this, cx| {
                                        this.set_active_explorer_tab(ix, window, cx);
                                        this.revert_active_explorer_file(window, cx);
                                    })
                                }),
                        )
                        .item(PopupMenuItem::new("Discuss").icon(IconName::Bot).on_click(
                            move |_, window, cx| {
                                discuss_view.update(cx, |this, cx| {
                                    this.set_active_explorer_tab(ix, window, cx);
                                    this.ask_agent_about_file(discuss_path.clone(), window, cx);
                                })
                            },
                        ))
                        .separator()
                        .item(
                            PopupMenuItem::new("Copy path").icon(Lucide::Copy).on_click(
                                move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copy.display().to_string(),
                                    ))
                                },
                            ),
                        )
                    })
            }));

        if markdown {
            tab_bar = tab_bar.action(
                aether_ui::button("markdown-preview-toggle")
                    .debug_selector(|| "markdown-preview-toggle".into())
                    .small()
                    .ghost()
                    .icon(if preview { Lucide::Pencil } else { Lucide::Eye })
                    .tooltip(if preview { "Edit Markdown source" } else { "Preview Markdown" })
                    .accessibility_label(if preview {
                        "Edit Markdown source"
                    } else {
                        "Preview Markdown"
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(id) = buffer_id {
                            if this.explorer_markdown_source.remove(&id) {
                                window.blur(cx);
                            } else {
                                this.explorer_markdown_source.insert(id);
                                if let Some(editor) = this.active_explorer_editor(window, cx) {
                                    editor.update(cx, |editor, cx| editor.focus(window, cx));
                                }
                            }
                        }
                        cx.notify();
                    })),
            );
        }

        v_flex()
            .key_context(EXPLORER_EDITOR_CONTEXT)
            .on_action(cx.listener(|this, _: &CloseEditorTab, window, cx| {
                this.close_active_explorer_tab(window, cx)
            }))
            .on_action(cx.listener(|this, _: &NextEditorTab, window, cx| {
                this.cycle_explorer_tab(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PrevEditorTab, window, cx| {
                this.cycle_explorer_tab(false, window, cx)
            }))
            .flex_1()
            .h_full()
            .min_w_0()
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|_this, ev: &gpui_kit::MouseUpEvent, window, cx| {
                    let at = ev.position;
                    cx.defer_in(window, move |this, _, cx| {
                        let explorer = this.active_project_explorer(cx);
                        this.file_selection_bar =
                            explorer.read(cx).active_buffer().and_then(|buffer| {
                                let (editor, _) = this.explorer_editors.get(&buffer.id)?;
                                (!editor.read(cx).selected_range().is_empty())
                                    .then(|| (at, buffer.path.clone()))
                            });
                        cx.notify();
                    });
                }),
            )
            .children(self.file_selection_bar.clone().map(|(at, path)| {
                gpui_kit::deferred(
                    gpui_kit::anchored()
                        .position(point(at.x, at.y - px(8.)))
                        .anchor(gpui_kit::Anchor::BottomCenter)
                        .snap_to_window_with_margin(px(8.))
                        .child(
                            h_flex()
                                .gap_1()
                                .occlude()
                                .p_1()
                                .corner(gpui_kit::rems(0.5))
                                .shadow_lg()
                                .bg(cx.theme().popover)
                                .border_1()
                                .border_color(border)
                                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                    this.file_selection_bar = None;
                                    cx.notify();
                                }))
                                .child(
                                    aether_ui::button("file-selection-ask-agent")
                                        .ghost()
                                        .xsmall()
                                        .icon(Lucide::MessageCircle)
                                        .label("Ask agent")
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.file_selection_bar = None;
                                            this.ask_agent_about_file(path.clone(), window, cx);
                                        })),
                                )
                                .child(
                                    aether_ui::button("file-selection-copy")
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Copy)
                                        .label("Copy")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            let explorer = this.active_project_explorer(cx);
                                            let text = explorer
                                                .read(cx)
                                                .active_buffer()
                                                .and_then(|buffer| {
                                                    this.explorer_editors.get(&buffer.id)
                                                })
                                                .map(|(editor, _)| {
                                                    editor.read(cx).selected_text().to_string()
                                                });
                                            if let Some(text) = text {
                                                cx.write_to_clipboard(ClipboardItem::new_string(
                                                    text,
                                                ));
                                            }
                                            this.file_selection_bar = None;
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
            }))
            .child(tab_bar.when(self.page == crate::app::Page::Notes, |bar| {
                bar.action(aether_ui::window_controls_gap(window))
            }))
            .when_some(conflict, |el, conflict| el.child(self.render_disk_banner(conflict, cx)))
            .child(if preview {
                let text =
                    editor.as_ref().map(|e| e.read(cx).text().to_string()).unwrap_or_default();
                let view = cx.entity().downgrade();
                div()
                    .id(("explorer-markdown-preview", buffer_id.unwrap_or_default()))
                    .debug_selector(|| "explorer-markdown-preview".into())
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .p_6()
                    .child(
                        div().w_full().max_w(px(900.)).mx_auto().child(
                            Markdown::new(
                                format!("explorer-markdown-{}", buffer_id.unwrap_or_default()),
                                text.into(),
                            )
                            .cwd(file_path.parent().map(|p| p.to_string_lossy().to_string().into()))
                            .on_reflow(Some(Rc::new(move |cx| {
                                view.update(cx, |_, cx| cx.notify()).ok();
                            }))),
                        ),
                    )
                    .into_any_element()
            } else {
                let marks = diff.and_then(Result::ok).filter(|rows| !rows.is_empty());
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .relative()
                    .children(editor.clone().map(|editor| {
                        let notes_on = self.settings.notes_enabled;
                        Editor::new(&editor)
                            .h(relative(1.))
                            .appearance(false)
                            .bordered(false)
                            .context_menu(move |menu, _, _| {
                                use gpui_kit::component::input::{Copy, Cut, Paste, SelectAll};
                                let menu = menu
                                    .menu("Cut", Box::new(Cut))
                                    .menu("Copy", Box::new(Copy))
                                    .menu("Paste", Box::new(Paste))
                                    .separator()
                                    .menu("Select all", Box::new(SelectAll));
                                if !notes_on {
                                    return menu;
                                }
                                menu.separator().menu(
                                    "Save selection to note…",
                                    Box::new(crate::actions::SaveSelectionToNote),
                                )
                            })
                    }))
                    .when_some(editor.zip(marks), |el, (editor, rows)| {
                        el.child(div().absolute().inset_0().child(canvas(
                            move |_bounds, _window, _cx| {},
                            move |_bounds, _, window, cx| {
                                let editor = editor.read(cx);
                                let Some(visible) = editor.visible_row_range() else {
                                    return;
                                };
                                for row in rows.iter() {
                                    let (line, color) = match row {
                                        DiffRow::Line {
                                            change: DiffChange::Added,
                                            new: Some(line),
                                            ..
                                        } => (*line, success_color),
                                        DiffRow::Line {
                                            change: DiffChange::Removed,
                                            old: Some(line),
                                            ..
                                        } => (*line, danger_color),
                                        _ => continue,
                                    };
                                    let row = (line as usize).saturating_sub(1);
                                    if !visible.contains(&row) {
                                        continue;
                                    }
                                    let start = editor.text().line_start_offset(row);
                                    let Some(at) = editor.range_to_bounds(&(start..start)) else {
                                        continue;
                                    };
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            point(at.origin.x - px(3.), at.origin.y),
                                            size(px(3.), at.size.height),
                                        ),
                                        color,
                                    ));
                                }
                            },
                        )))
                    })
                    .into_any_element()
            })
            .child({
                let active_git_change = active_git_change.map(|(_, change)| change);

                // Status bar at bottom
                h_flex()
                    .h(px(24.))
                    .px_4()
                    .items_center()
                    .gap_4()
                    .border_t_1()
                    .border_color(border)
                    .bg(sidebar_bg)
                    .text_xs()
                    .text_color(muted)
                    .when_some(cursor, |bar, at| {
                        bar.child(format!("Ln {}, Col {}", at.line + 1, at.character + 1))
                    })
                    .child(div().child("UTF-8"))
                    .child(div().child(language))
                    .when_some(active_git_change, |bar, change| {
                        let stat_str = match change.stat {
                            Stat::Lines(a, r) if a > 0 && r > 0 => {
                                format!("Git: {} (+{} \u{2212}{})", change.kind.label(), a, r)
                            }
                            Stat::Lines(a, _) if a > 0 => {
                                format!("Git: {} (+{})", change.kind.label(), a)
                            }
                            Stat::Lines(_, r) if r > 0 => {
                                format!("Git: {} (\u{2212}{})", change.kind.label(), r)
                            }
                            _ => format!("Git: {}", change.kind.label()),
                        };
                        let c = match change.kind {
                            ChangeKind::Modified => warning_color,
                            ChangeKind::Added | ChangeKind::Untracked => success_color,
                            ChangeKind::Deleted | ChangeKind::Conflicted => danger_color,
                            _ => muted,
                        };
                        bar.child(
                            h_flex()
                                .items_center()
                                .gap_1()
                                .text_color(c)
                                .child(Icon::new(Lucide::GitBranch).size(px(12.)).text_color(c))
                                .child(stat_str),
                        )
                    })
                    .child(div().flex_1())
                    .when_some(active_folder, |bar, folder| {
                        bar.child(
                            h_flex()
                                .items_center()
                                .gap_1p5()
                                .min_w_0()
                                .child(
                                    Icon::new(IconName::Folder)
                                        .size(px(12.))
                                        .text_color(folder_color),
                                )
                                .child(div().truncate().child(folder.display().to_string())),
                        )
                    })
            })
            .into_any_element()
    }
}

pub(super) fn is_markdown_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| {
        ["md", "markdown", "mdown", "mkd"].iter().any(|known| ext.eq_ignore_ascii_case(known))
    })
}

#[cfg(test)]
#[path = "../../tests/unit/explorer/editor.rs"]
mod tests;
