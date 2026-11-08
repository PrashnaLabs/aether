use aether_ui::theme::Corners;
use gpui_kit::rems;
use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::ops::Range;
use std::path::{Path, PathBuf};

use aether_git::{ChangeKind, Stat};
use aether_ui::{field, file_icon, file_icon_color};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, StyledExt, WindowExt,
    button::{ButtonVariant, ButtonVariants},
    h_flex,
    input::InputState,
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    notification::Notification,
    scroll::ScrollableElement,
    v_flex,
};
use gpui_kit::{
    AnyElement, ClipboardItem, Context, Entity, FocusHandle, InteractiveElement, KeyDownEvent,
    Role, ScrollStrategy, SharedString, UniformListScrollHandle, Window, div, prelude::*, px,
    transparent_black, uniform_list,
};

use super::buffers::language_from_path;
use crate::app::ChatApp;
use crate::git_panel::step;

/// The tree's scroll and focus, and the selection last scrolled into view.
struct TreeNav {
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    shown: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
enum TreeStep {
    Focus(usize),
    Toggle,
    Open,
}

/// What a key does on tree row `ix`: arrows, Home and End move, Left and
/// Right close, open or climb folders, Enter and Space open.
fn tree_step(rows: &[(usize, Option<bool>)], ix: usize, key: &str) -> Option<TreeStep> {
    let (depth, open) = *rows.get(ix)?;
    match (key, open) {
        ("enter" | "space", None) => Some(TreeStep::Open),
        ("enter" | "space", Some(_)) | ("right", Some(false)) | ("left", Some(true)) => {
            Some(TreeStep::Toggle)
        }
        ("right", Some(true)) => {
            rows.get(ix + 1).filter(|row| row.0 > depth).map(|_| TreeStep::Focus(ix + 1))
        }
        ("left", _) => rows[..ix].iter().rposition(|row| row.0 < depth).map(TreeStep::Focus),
        _ => step(ix, rows.len(), key).map(TreeStep::Focus),
    }
}

/// Right-click menu of a tree row; `folder` is whether a folder is open, `None` for a file.
fn tree_menu(
    menu: PopupMenu,
    view: &Entity<ChatApp>,
    path: &Path,
    root: &Path,
    folder: Option<bool>,
    notes: bool,
) -> PopupMenu {
    if notes && folder.is_none() {
        let listing = if root.file_name().is_some_and(|name| name == ".trash")
            || path.starts_with(root.join(".trash"))
        {
            crate::notes::Listing::Trash
        } else {
            crate::notes::Listing::All
        };
        return crate::notes::view::note_context(menu, view.clone(), path.to_path_buf(), listing);
    }
    type Act = fn(&mut ChatApp, PathBuf, &mut Window, &mut Context<ChatApp>);
    let act = |label: &'static str, act: Act| {
        let (view, path) = (view.clone(), path.to_path_buf());
        PopupMenuItem::new(label).on_click(move |_, window, cx| {
            view.update(cx, |this, cx| act(this, path.clone(), window, cx))
        })
    };
    let menu = match folder {
        Some(open) => menu
            .item(act(if open { "Collapse" } else { "Expand" }, |this, path, _, cx| {
                this.toggle_tree_dir(&path, cx)
            }))
            .when(!notes, |menu| {
                menu.item(act("New agent chat here", ChatApp::new_chat_in).icon(IconName::Bot))
            })
            .separator()
            .item(
                act(
                    if notes { "New note here…" } else { "New project note…" },
                    ChatApp::new_note_here,
                )
                .icon(IconName::BookOpen),
            )
            .when(!notes, |menu| {
                menu.item(act("New File…", ChatApp::ask_new_file).icon(Lucide::FilePlus))
            })
            .item(act("New Folder…", ChatApp::ask_new_folder).icon(Lucide::FolderPlus)),
        None => menu.item(act("Open", ChatApp::open_explorer_file).icon(IconName::FileText)),
    };
    let copy = |label: &'static str, text: String| {
        PopupMenuItem::new(label).icon(IconName::Copy).on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
        })
    };
    let reveal = path.to_path_buf();
    let reveal_label =
        if cfg!(target_os = "macos") { "Reveal in Finder" } else { "Show in folder" };
    menu.separator()
        .item(
            PopupMenuItem::new(reveal_label)
                .icon(IconName::FolderOpen)
                .on_click(move |_, _, cx| cx.reveal_path(&reveal)),
        )
        .item(copy("Copy path", path.display().to_string()))
        .item(copy(
            "Copy relative path",
            path.strip_prefix(root).unwrap_or(path).display().to_string(),
        ))
        .separator()
        .item(act("Rename…", ChatApp::ask_rename_path).icon(Lucide::Pencil))
        .when(!notes, |menu| {
            menu.item(act("Delete", ChatApp::confirm_delete_path).icon(Lucide::Trash))
        })
}

/// What a name dialog does with the name typed; `false` keeps the dialog up.
type NameOp = fn(&mut ChatApp, &Path, &str, &mut Window, &mut Context<ChatApp>) -> bool;

impl ChatApp {
    /// Renders the directory tree panel.
    pub(crate) fn render_explorer_tree_pane(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let explorer = self.active_project_explorer(cx);
        let nav = window.use_keyed_state("explorer-tree-nav", cx, |_, cx| TreeNav {
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle().tab_stop(true),
            shown: None,
        });
        let (scroll, focus) = (nav.read(cx).scroll.clone(), nav.read(cx).focus.clone());
        if self.page == crate::app::Page::Notes
            && std::mem::take(&mut self.notes.focus_tree)
            && self.notes.form.is_none()
        {
            focus.focus(window, cx);
        }
        let tree = &explorer.read(cx).tree_state;
        let (root, count, selected) = (tree.root.clone(), tree.rows().len(), tree.selected.clone());
        if nav.read(cx).shown != selected {
            let rows = tree.rows();
            if let Some(ix) = selected.as_ref().and_then(|s| rows.iter().position(|r| &r.path == s))
            {
                scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
            }
            nav.update(cx, |nav, _| nav.shown = selected);
        }
        let focused = focus.is_focused(window);
        let border = cx.theme().border;
        let folder_color = aether_ui::theme::mango_text(cx);
        let header_button = |id: &'static str, icon: Lucide, tip: &'static str| {
            aether_ui::button(id).ghost().xsmall().icon(icon).tooltip(tip).accessibility_label(tip)
        };

        v_flex()
            .flex_1()
            .min_h_0()
            .child(
                // Root folder breadcrumb and file creation actions
                h_flex()
                    .px_3()
                    .py_2()
                    .items_center()
                    .gap_1p5()
                    .border_b_1()
                    .border_color(border)
                    .child(Icon::new(IconName::FolderOpen).size(px(14.)).text_color(folder_color))
                    .child(
                        div().flex_1().text_xs().font_medium().truncate().child(
                            root.file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| "Workspace".to_string()),
                        ),
                    )
                    .child(header_button("tree-new-file", Lucide::FilePlus, "New File").on_click({
                        let r = root.clone();
                        cx.listener(move |this, _, window, cx| {
                            if this.page == crate::app::Page::Notes {
                                this.new_note_here(r.clone(), window, cx)
                            } else {
                                this.ask_new_file(r.clone(), window, cx)
                            }
                        })
                    }))
                    .child(
                        header_button("tree-new-folder", Lucide::FolderPlus, "New Folder")
                            .on_click({
                                let r = root.clone();
                                cx.listener(move |this, _, window, cx| {
                                    this.ask_new_folder(r.clone(), window, cx)
                                })
                            }),
                    ),
            )
            .child(
                div()
                    .id("explorer-tree")
                    .debug_selector(|| "explorer-tree".into())
                    .flex_1()
                    .min_h_0()
                    .track_focus(&focus)
                    .role(Role::Tree)
                    .aria_label("Files")
                    .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                        this.tree_key(ev, window, cx)
                    }))
                    .child(
                        uniform_list(
                            "explorer-tree-rows",
                            count,
                            cx.processor(move |this, range: Range<usize>, _, cx| {
                                this.render_tree_rows(range, focused, cx)
                            }),
                        )
                        .track_scroll(&scroll)
                        .size_full()
                        .p_1(),
                    )
                    .vertical_scrollbar(&scroll),
            )
            .into_any_element()
    }

    /// The tree rows in `range`; `focused` rings the selected one.
    fn render_tree_rows(
        &mut self,
        range: Range<usize>,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let explorer = self.active_project_explorer(cx);
        let view = cx.entity();
        let notes = self.page == crate::app::Page::Notes;
        let git_changes: HashMap<PathBuf, (char, ChangeKind, Stat)> = self
            .git_status()
            .filter(|_| !notes)
            .map(|s| {
                s.changes
                    .iter()
                    .map(|c| (s.top.join(&c.path), (c.kind.letter(), c.kind, c.stat)))
                    .collect()
            })
            .unwrap_or_default();

        let exp = explorer.read(cx);
        let root = exp.root.clone();
        let tree = &exp.tree_state;
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let folder_color = aether_ui::theme::mango_text(cx);
        let primary = theme.primary;
        let ring = theme.ring;

        let start = range.start;
        let rows = tree.rows().get(range).unwrap_or_default();
        rows.iter()
            .enumerate()
            .map(|(offset, node)| {
                let ix = start + offset;
                let path = node.path.clone();
                let name = node.name.clone();
                let folder = node.is_dir.then(|| tree.is_expanded(&path));
                let is_selected = tree.selected.as_ref() == Some(&path);
                let menu = {
                    let (view, path, root) = (view.clone(), path.clone(), root.clone());
                    move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
                        tree_menu(menu, &view, &path, &root, folder, notes)
                    }
                };

                let row = h_flex()
                    .id(("tree-row", ix))
                    .role(Role::TreeItem)
                    .aria_label(SharedString::from(name.clone()))
                    .aria_level(node.depth + 1)
                    .aria_selected(is_selected)
                    .when_some(folder, |r, open| r.aria_expanded(open))
                    .when(is_selected, |r| r.aria_active_descendant())
                    .h(px(24.))
                    .border_1()
                    .border_color(transparent_black())
                    .when(is_selected && focused, |r| r.border_color(ring))
                    .w_full()
                    .items_center()
                    .px_2()
                    .pl(px(node.depth as f32 * 14. + 8.))
                    .corner(rems(0.375))
                    .cursor_pointer()
                    .text_xs()
                    .when(exp.is_ignored(&path), |r| r.opacity(0.5))
                    .when(is_selected, |r| r.bg(primary.opacity(0.12)))
                    .hover(|r| r.bg(theme.foreground.opacity(0.06)));

                if let Some(open) = folder {
                    let chevron = if open { IconName::ChevronDown } else { IconName::ChevronRight };
                    let folder_icon = if open { IconName::FolderOpen } else { IconName::Folder };
                    let p_toggle = path.clone();
                    let dir_has_git_changes = git_changes.keys().any(|p| p.starts_with(&path));

                    row.child(
                        h_flex()
                            .id(("tree-toggle", ix))
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .gap_1p5()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_tree_dir(&p_toggle, cx);
                            }))
                            .child(Icon::new(chevron).size(px(12.)).text_color(muted))
                            .child(Icon::new(folder_icon).size(px(14.)).text_color(folder_color))
                            .child(
                                div()
                                    .truncate()
                                    .font_medium()
                                    .when(dir_has_git_changes, |d| {
                                        d.text_color(aether_ui::theme::warning_text(cx))
                                    })
                                    .child(name),
                            )
                            .when(dir_has_git_changes, |r| {
                                r.child(div().w(px(5.)).h(px(5.)).rounded_full().bg(folder_color))
                            }),
                    )
                    .context_menu(menu)
                    .into_any_element()
                } else {
                    let p_open = path.clone();
                    let is_dirty = exp.tabs.iter().any(|t| t.path == path && t.is_dirty);

                    let git_change = git_changes.get(&path).cloned();
                    let git_color = git_change.as_ref().map(|(_, kind, _)| match kind {
                        ChangeKind::Modified => aether_ui::theme::warning_text(cx),
                        ChangeKind::Added | ChangeKind::Untracked => {
                            aether_ui::theme::success_text(cx)
                        }
                        ChangeKind::Deleted | ChangeKind::Conflicted => {
                            aether_ui::theme::danger_text(cx)
                        }
                        ChangeKind::Renamed | ChangeKind::Copied => aether_ui::theme::info_text(cx),
                        ChangeKind::TypeChanged => muted,
                    });

                    let icon = file_icon(&path);
                    let default_icon_color = file_icon_color(&path, cx);
                    row.child(
                        h_flex()
                            .id(("tree-file", ix))
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .gap_1p5()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if this.page == crate::app::Page::Notes {
                                    this.open_note(p_open.clone(), None, window, cx);
                                } else {
                                    this.open_explorer_file(p_open.clone(), window, cx);
                                }
                            }))
                            .child(div().w(px(12.)))
                            .child(
                                Icon::new(icon)
                                    .size(px(14.))
                                    .text_color(git_color.unwrap_or(default_icon_color)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .when(is_dirty, |d| d.font_semibold())
                                    .when_some(git_color, |d, c| d.text_color(c))
                                    .child(name),
                            )
                            .when_some(git_change, |r, (code, _, stat)| {
                                let c = git_color.unwrap_or(muted);
                                let mut badge = h_flex()
                                    .id(("tree-stat", ix))
                                    .items_center()
                                    .gap_1()
                                    .cursor_pointer()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_bold()
                                            .text_color(c)
                                            .child(code.to_string()),
                                    );

                                if let Stat::Lines(added, removed) = stat {
                                    if added > 0 {
                                        badge = badge.child(
                                            div()
                                                .text_xs()
                                                .font_medium()
                                                .text_color(aether_ui::theme::success_text(cx))
                                                .child(format!("+{added}")),
                                        );
                                    }
                                    if removed > 0 {
                                        badge = badge.child(
                                            div()
                                                .text_xs()
                                                .font_medium()
                                                .text_color(aether_ui::theme::danger_text(cx))
                                                .child(format!("\u{2212}{removed}")),
                                        );
                                    }
                                }
                                r.child(badge)
                            })
                            .when(is_dirty, |r| {
                                r.child(div().w(px(7.)).h(px(7.)).rounded_full().bg(primary))
                            }),
                    )
                    .context_menu(menu)
                    .into_any_element()
                }
            })
            .collect()
    }

    /// Opens or closes the folder at `path` and selects it.
    fn toggle_tree_dir(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !self.notes_tree_path_allowed(path, cx) {
            return;
        }
        self.active_project_explorer(cx).update(cx, |exp, _| {
            exp.tree_state.toggle_expand(path);
            exp.tree_state.select(path.to_path_buf());
        });
        cx.notify();
    }

    fn tree_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if ev.keystroke.modifiers.modified() {
            return;
        }
        let explorer = self.active_project_explorer(cx);
        let (step, path) = {
            let tree = &explorer.read(cx).tree_state;
            let rows = tree.rows();
            let shape: Vec<(usize, Option<bool>)> = rows
                .iter()
                .map(|r| (r.depth, r.is_dir.then(|| tree.is_expanded(&r.path))))
                .collect();
            let ix = tree
                .selected
                .as_ref()
                .and_then(|s| rows.iter().position(|r| &r.path == s))
                .unwrap_or(0);
            let Some(step) = tree_step(&shape, ix, &ev.keystroke.key) else {
                return;
            };
            let target = match step {
                TreeStep::Focus(to) => to,
                _ => ix,
            };
            (step, rows[target].path.clone())
        };
        cx.stop_propagation();
        match step {
            TreeStep::Focus(_) => {
                explorer.update(cx, |exp, _| exp.tree_state.select(path));
                cx.notify();
            }
            TreeStep::Toggle => self.toggle_tree_dir(&path, cx),
            TreeStep::Open => {
                if self.page == crate::app::Page::Notes {
                    self.open_note(path, None, window, cx);
                } else {
                    self.open_explorer_file(path, window, cx);
                }
            }
        }
    }

    // The shared Explorer permits symlink folders; notes operations must stay in
    // their canonical tree, including generic folder creation and renaming.
    fn notes_tree_path_allowed(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        if self.page != crate::app::Page::Notes {
            return true;
        }
        let root = self.notes_explorer(cx).read(cx).root.clone();
        let allowed = path.starts_with(&root)
            && aether_core::canonicalize(path).is_ok_and(|canonical| canonical == path);
        if !allowed {
            self.notes.error = Some("Symlink note paths are not allowed.".into());
            cx.notify();
        }
        allowed
    }

    /// Asks for a file name, then makes it in `dir` and opens it.
    fn ask_new_file(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let op: NameOp = |this, dir, name, window, cx| {
            let made = aether_explorer::create_file(dir, name);
            let Some(path) = this.tree_op_done(made, &format!("create {name}"), window, cx) else {
                return false;
            };
            this.open_explorer_file(path, window, cx);
            true
        };
        self.ask_tree_name(dir, "New File", "Create", String::new(), op, window, cx);
    }

    /// Asks for a folder name, then makes it in `dir`.
    fn ask_new_folder(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if !self.notes_tree_path_allowed(&dir, cx) {
            return;
        }
        let op: NameOp = |this, dir, name, window, cx| {
            let made = aether_explorer::create_folder(dir, name);
            this.tree_op_done(made, &format!("create {name}"), window, cx).is_some()
        };
        self.ask_tree_name(dir, "New Folder", "Create", String::new(), op, window, cx);
    }

    /// Asks for a new name for `path`, then renames it and the tabs open on it.
    fn ask_rename_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if !self.notes_tree_path_allowed(&path, cx) {
            return;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let op: NameOp = |this, path, name, window, cx| {
            if path.file_name() == Some(OsStr::new(name)) {
                return true;
            }
            let renamed = aether_explorer::rename(path, name);
            if let Ok(to) = &renamed {
                let explorer = this.active_project_explorer(cx);
                explorer.update(cx, |exp, _| exp.retarget_tabs(path, to));
                let moved: Vec<_> = explorer
                    .read(cx)
                    .tabs
                    .iter()
                    .filter(|t| t.path.starts_with(to))
                    .map(|t| (t.id, language_from_path(&t.path)))
                    .collect();
                for (id, lang) in moved {
                    if let Some((editor, _)) = this.explorer_editors.get(&id) {
                        editor.update(cx, |e, cx| e.set_highlighter(lang, cx));
                    }
                }
            }
            this.tree_op_done(renamed, &format!("rename to {name}"), window, cx).is_some()
        };
        self.ask_tree_name(path, "Rename", "Rename", name, op, window, cx);
    }

    /// A dialog asking for a name, prefilled with `initial`; OK hands it to `op`.
    fn ask_tree_name(
        &mut self,
        path: PathBuf,
        title: &'static str,
        ok: &'static str,
        initial: String,
        op: NameOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(initial, window, cx);
            input
        });
        let (view, first) = (cx.entity(), Cell::new(true));
        aether_ui::theme::open_alert(window, cx, move |alert, window, cx| {
            if first.replace(false) {
                input.update(cx, |input, cx| input.focus(window, cx));
            }
            let (view, input, path) = (view.clone(), input.clone(), path.clone());
            alert
                .confirm()
                .title(title)
                .child(field(&input).small().aria_label(title))
                .ok_text(ok)
                .on_ok(move |_, window, cx| {
                    let name = input.read(cx).value().trim().to_string();
                    !name.is_empty()
                        && view.update(cx, |this, cx| op(this, &path, &name, window, cx))
                })
        });
    }

    /// Refreshes the tree onto `done`'s new path, or tells the user why it failed.
    fn tree_op_done(
        &mut self,
        done: std::io::Result<PathBuf>,
        what: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<PathBuf> {
        let path = done
            .inspect_err(|e| {
                window.push_notification(Notification::error(format!("Couldn't {what}: {e}")), cx)
            })
            .ok()?;
        let explorer = self.active_project_explorer(cx);
        self.refresh_explorer(&explorer, cx);
        explorer.update(cx, |exp, _| exp.reveal(&path));
        cx.notify();
        Some(path)
    }

    /// Asks, then moves `path` to the Trash and closes its tabs.
    fn confirm_delete_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().to_string();
        let title = SharedString::from(format!("Move {name} to the Trash?"));
        let detail = if path.is_dir() {
            "Everything in it goes too; its open tabs close without saving."
        } else {
            "Its tab closes without saving."
        };
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let (view, path) = (view.clone(), path.clone());
            alert
                .confirm()
                .title(title.clone())
                .description(detail)
                .ok_text("Move to Trash")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| this.delete_path(path.clone(), window, cx));
                    true
                })
        });
    }

    /// Moves `path` to the Trash off the UI thread, then closes its tabs.
    fn delete_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let explorer = self.active_project_explorer(cx);
        let task = cx.background_executor().spawn({
            let path = path.clone();
            async move { aether_explorer::trash(&path) }
        });
        cx.spawn_in(window, async move |this, cx| {
            let trashed = task.await;
            this.update_in(cx, |this, window, cx| {
                if let Err(e) = trashed {
                    let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy();
                    let note = format!("Couldn't move {name} to the Trash: {e}");
                    window.push_notification(Notification::error(note), cx);
                    return;
                }
                let gone: Vec<PathBuf> = explorer
                    .read(cx)
                    .tabs
                    .iter()
                    .map(|t| t.path.clone())
                    .filter(|p| p.starts_with(&path))
                    .collect();
                this.drop_explorer_tabs(&explorer, &gone, false, window, cx);
                this.refresh_explorer(&explorer, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
#[path = "../../tests/unit/explorer/tree.rs"]
mod tests;
