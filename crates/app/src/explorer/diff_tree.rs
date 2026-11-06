use aether_ui::theme::Corners;
use gpui_kit::rems;
use std::collections::BTreeMap;

use aether_ui::{FileEdits, file_icon, file_icon_color, reveal_file};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme, Icon, IconName, StyledExt, h_flex, v_flex};
use gpui_kit::{AnyElement, Context, Role, SharedString, div, prelude::*, px};

use crate::app::ChatApp;

/// A folder's children while the file list is built.
#[derive(Default)]
struct Dir {
    dirs: BTreeMap<String, Dir>,
    files: BTreeMap<String, usize>,
}

/// One row of the Diff file tree: `file` is the index into the reply's files.
struct Line {
    depth: usize,
    name: String,
    file: Option<usize>,
}

fn lines(dir: &Dir, depth: usize, out: &mut Vec<Line>) {
    for (name, sub) in &dir.dirs {
        out.push(Line { depth, name: name.clone(), file: None });
        lines(sub, depth + 1, out);
    }
    for (name, &file) in &dir.files {
        out.push(Line { depth, name: name.clone(), file: Some(file) });
    }
}

/// Folders first, then files, like the explorer's file tree.
fn tree_lines(files: &[FileEdits]) -> Vec<Line> {
    let mut root = Dir::default();
    for (ix, file) in files.iter().enumerate() {
        let mut parts: Vec<&str> = file.path.split('/').filter(|p| !p.is_empty()).collect();
        let Some(name) = parts.pop() else { continue };
        let mut dir = &mut root;
        for part in parts {
            dir = dir.dirs.entry(part.to_string()).or_default();
        }
        dir.files.insert(name.to_string(), ix);
    }
    let mut out = Vec::new();
    lines(&root, 0, &mut out);
    out
}

impl ChatApp {
    /// The Diff sidebar: the reply's changed files as a tree; picking one scrolls its diff into view.
    pub(super) fn render_explorer_diff_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, foreground) = (theme.muted_foreground, theme.foreground);
        let folder_color = aether_ui::theme::mango_text(cx);
        let Some((files, scroll)) = self.reply_diff_files().filter(|(files, _)| !files.is_empty())
        else {
            return crate::git_panel::note(
                Lucide::FileDiff,
                muted,
                "No edits to show",
                "Open a reply's changed files to see their diffs here.",
                cx,
            )
            .into_any_element();
        };
        let rows = tree_lines(&files).into_iter().enumerate().map(|(ix, line)| {
            let row = h_flex()
                .id(("diff-tree-row", ix))
                .role(Role::TreeItem)
                .aria_label(SharedString::from(line.name.clone()))
                .aria_level(line.depth + 1)
                .h(px(24.))
                .w_full()
                .items_center()
                .px_2()
                .pl(px(line.depth as f32 * 14. + 8.))
                .corner(rems(0.375))
                .cursor_pointer()
                .text_xs()
                .gap_1p5()
                .hover(|r| r.bg(foreground.opacity(0.06)));
            match line.file {
                None => row
                    .child(Icon::new(IconName::ChevronDown).size(px(12.)).text_color(muted))
                    .child(Icon::new(IconName::FolderOpen).size(px(14.)).text_color(folder_color))
                    .child(div().truncate().font_medium().child(line.name))
                    .into_any_element(),
                Some(file_ix) => {
                    let edits = &files[file_ix];
                    let path = std::path::Path::new(&edits.path);
                    let (added, removed) = (edits.added, edits.removed);
                    let (files, scroll, target) =
                        (files.clone(), scroll.clone(), edits.path.clone());
                    row.on_click(cx.listener(move |_, _, _, cx| {
                        reveal_file(&scroll, &files, Some(&target));
                        cx.notify();
                    }))
                    .child(div().w(px(12.)))
                    .child(
                        Icon::new(file_icon(path))
                            .size(px(14.))
                            .text_color(file_icon_color(path, cx)),
                    )
                    .child(div().flex_1().truncate().child(line.name))
                    .child(
                        h_flex()
                            .gap_1()
                            .when(added > 0, |r| {
                                r.child(
                                    div()
                                        .text_color(aether_ui::theme::success_text(cx))
                                        .child(format!("+{added}")),
                                )
                            })
                            .when(removed > 0, |r| {
                                r.child(
                                    div()
                                        .text_color(aether_ui::theme::danger_text(cx))
                                        .child(format!("\u{2212}{removed}")),
                                )
                            }),
                    )
                    .into_any_element()
                }
            }
        });
        v_flex()
            .id("diff-tree")
            .role(Role::Tree)
            .aria_label("Changed files")
            .flex_1()
            .min_h_0()
            .p_1()
            .overflow_y_scroll()
            .children(rows)
            .into_any_element()
    }
}
