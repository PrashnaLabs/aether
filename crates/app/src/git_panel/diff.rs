use std::path::PathBuf;
use std::rc::Rc;

use aether_core::hunk_header;
use aether_git::{Change, ChangeKind, Commit, GitOp, HunkOp, STDOUT_CAP, Stash, apply_hunk};
use aether_ui::{DiffChange, DiffLines, DiffRow, parse_diff, tabular, theme};
use futures::future::Either;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::scroll::{ScrollableElement, ScrollbarAxis};
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, StyledExt, h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, KeyDownEvent, ListHorizontalSizingBehavior,
    SharedString, UniformListScrollHandle, Window, div, point, prelude::*, px, rems, uniform_list,
};

use super::rows::{commit_key, describe, file_key, follow, split_path, stash_key};
use super::widgets::{letter_chip, note, stat_element};
use super::{Feedback, Job, OpenDiff};
use crate::app::{ChatApp, format_full};

/// Diffs past either limit wait behind "Show anyway".
const DIFF_LINES: usize = 2_000;
const DIFF_BYTES: usize = 512 << 10;

/// What the diff view shows: a changed file, or a history commit or a stash, read-only.
#[derive(Clone)]
pub(super) enum Target {
    File(Change),
    Commit(Commit),
    Stash(Stash),
}

/// A fetched diff, parsed off the UI thread.
pub(super) struct Diff {
    raw: SharedString,
    rows: Rc<Vec<DiffRow>>,
    lines: usize,
    binary: bool,
    /// Git's output stopped at the capture cap.
    cut: bool,
    /// The hunk action running, by the hunk's place in this diff.
    acting: Option<(usize, HunkOp)>,
}

impl Diff {
    fn too_large(&self) -> bool {
        self.lines > DIFF_LINES || self.raw.len() > DIFF_BYTES
    }
}

/// A modified file's text diff, shown whole, gets per-hunk actions; other
/// changes stage as whole files.
fn hunk_actions(change: &Change, diff: &Diff) -> bool {
    change.kind == ChangeKind::Modified && !diff.binary && !diff.cut && !diff.too_large()
}

/// Rows, line count and kind of a diff's text.
fn prepare_diff(raw: &str) -> (Vec<DiffRow>, usize, bool, bool) {
    let binary =
        raw.lines().any(|line| line.starts_with("Binary files ") || line == "GIT binary patch");
    // text drawing has no tab stops; four spaces keep indentation readable
    let rows = parse_diff(&raw.replace('\t', "    "));
    (rows, raw.lines().count(), binary, raw.len() as u64 >= STDOUT_CAP)
}

/// The diff view's way back to the list.
pub(super) fn back_button(cx: &mut Context<ChatApp>) -> Button {
    aether_ui::button("git-diff-back")
        .ghost()
        .small()
        .icon(IconName::ArrowLeft)
        .accessibility_label("Back to changes")
        .tooltip("Back to changes · Esc")
        .on_click(cx.listener(|this, _, _, cx| this.close_diff(cx)))
}

impl ChatApp {
    /// Keeps the open diff on its file as it moves between sections, and
    /// closes it once the file has no changes left.
    pub(super) fn follow_diff(&mut self, cx: &mut Context<Self>) {
        let Some(open) = &self.git.diff else {
            return;
        };
        if self.git_folder().as_ref() != Some(&open.folder) {
            return;
        }
        let Some(status) = self.git_status() else {
            return;
        };
        let change = match &open.target {
            Target::File(change) => change,
            // a commit never changes; it stays up until closed
            Target::Commit(_) => return,
            Target::Stash(stash) => {
                if !status.stashes.iter().any(|s| s.sha == stash.sha) {
                    self.git.diff = None;
                }
                return;
            }
        };
        let moved =
            follow(&status.changes, &open.key, &change.path).map(|ix| status.changes[ix].clone());
        match moved {
            Some(change) => {
                let open = self.git.diff.as_mut().expect("checked above");
                open.key = file_key(&change);
                open.target = Target::File(change);
                self.load_diff(cx);
            }
            None => self.git.diff = None,
        }
    }

    pub(super) fn open_diff(
        &mut self,
        key: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(folder), Some(status)) = (self.git_folder(), self.git_status()) else {
            return;
        };
        let target = if let Some(change) = status.changes.iter().find(|c| file_key(c) == key) {
            Target::File(change.clone())
        } else if let Some(commit) = self.history_commits().iter().find(|c| commit_key(c) == key) {
            Target::Commit(commit.clone())
        } else if let Some(stash) = status.stashes.iter().find(|s| stash_key(s) == key) {
            Target::Stash(stash.clone())
        } else {
            return;
        };
        self.git.cursor = Some(key.clone());
        let same = self.git.diff.as_ref().is_some_and(|d| d.key == key);
        if !same {
            self.git.diff = Some(OpenDiff {
                folder,
                key,
                target,
                diff: None,
                full: false,
                scroll: UniformListScrollHandle::new(),
            });
            self.load_diff(cx);
        }
        window.focus(&self.git.diff_focus, cx);
        cx.notify();
    }

    fn close_diff(&mut self, cx: &mut Context<Self>) {
        self.git.diff = None;
        self.git.refocus = true;
        cx.notify();
    }

    /// Fetches the open file's diff; the old text stays up until the new one lands.
    fn load_diff(&mut self, cx: &mut Context<Self>) {
        let (Some(open), Some(status)) = (&self.git.diff, self.git_status()) else {
            return;
        };
        let top = status.top.clone();
        let task = match &open.target {
            Target::File(change) => Either::Left(self.agent.git_diff(top, change.clone())),
            Target::Commit(commit) => {
                let sha = commit.sha.clone();
                Either::Right(
                    cx.background_executor().spawn(async move { aether_git::show(&top, &sha) }),
                )
            }
            Target::Stash(stash) => {
                let sha = stash.sha.clone();
                Either::Right(
                    cx.background_executor()
                        .spawn(async move { aether_git::stash_show(&top, &sha) }),
                )
            }
        };
        self.git.diff_run += 1;
        let run = self.git.diff_run;
        cx.spawn(async move |this, cx| {
            let fetched = task.await;
            let prepared = match fetched {
                Ok(raw) => {
                    let parse = cx.background_executor().spawn(async move {
                        let prepared = prepare_diff(&raw);
                        (raw, prepared)
                    });
                    Ok(parse.await)
                }
                Err(e) => Err(e),
            };
            this.update(cx, |this, cx| {
                if this.git.diff_run != run {
                    return;
                }
                if let Some(open) = &mut this.git.diff {
                    open.diff = Some(match prepared {
                        Ok((raw, (rows, lines, binary, cut))) => Ok(Diff {
                            raw: raw.into(),
                            rows: Rc::new(rows),
                            lines,
                            binary,
                            cut,
                            acting: None,
                        }),
                        Err(e) => Err(e.into()),
                    });
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Stages or unstages hunk `hunk` of the open diff; Revert asks first,
    /// since it rewrites the file with no undo.
    fn hunk_clicked(
        &mut self,
        hunk: usize,
        op: HunkOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some((folder, top)), Some(open)) = (self.git_target(), &self.git.diff) else {
            return;
        };
        let (Target::File(change), Some(Ok(diff)), true) =
            (&open.target, &open.diff, open.folder == folder)
        else {
            return;
        };
        let (path, raw) = (change.path.clone(), diff.raw.clone());
        if op != HunkOp::Revert {
            return self.run_hunk(folder, top, raw, hunk, op, window, cx);
        }
        if self.git.job.is_some() {
            return;
        }
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let (view, folder, top, raw) = (view.clone(), folder.clone(), top.clone(), raw.clone());
            alert
                .confirm()
                .title(format!("Revert this change in {path}?"))
                .description("This can't be undone.")
                .ok_text("Revert")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let (folder, top, raw) = (folder.clone(), top.clone(), raw.clone());
                    view.update(cx, |this, cx| {
                        this.run_hunk(folder, top, raw, hunk, HunkOp::Revert, window, cx)
                    });
                    true
                })
        });
    }

    /// Applies hunk `hunk` of `raw`, a diff read in `folder`'s repo at `top`,
    /// as the panel's one job, then re-reads the repo, which reloads the diff.
    fn run_hunk(
        &mut self,
        folder: PathBuf,
        top: PathBuf,
        raw: SharedString,
        hunk: usize,
        op: HunkOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = match op {
            HunkOp::Stage => "Couldn't stage the hunk",
            HunkOp::Unstage => "Couldn't unstage the hunk",
            HunkOp::Revert => "Couldn't revert the hunk",
        };
        if !self.can_run_in(&folder, &top, text, cx) {
            return;
        }
        if let Some(Some(Ok(diff))) = self.git.diff.as_mut().map(|open| &mut open.diff) {
            diff.acting = Some((hunk, op));
        }
        self.git.feedback = None;
        // no GitOp for hunks: an empty one holds the one-job lock without a file spinner
        self.git.job = Some(Job { op: GitOp::Stage(Vec::new()), done: false });
        // the focused hunk button goes away with its hunk
        window.focus(&self.git.diff_focus, cx);
        let task = cx.background_executor().spawn(async move { apply_hunk(&top, &raw, hunk, op) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.git.generation += 1;
                match result {
                    Ok(()) => {
                        if let Some(job) = &mut this.git.job {
                            job.done = true;
                        }
                    }
                    Err(e) => {
                        this.git.job = None;
                        if let Some(Some(Ok(diff))) = this.git.diff.as_mut().map(|o| &mut o.diff) {
                            diff.acting = None;
                        }
                        this.git.feedback = Some(Feedback {
                            folder: Some(folder),
                            error: true,
                            text: text.into(),
                            detail: Some(e.into()),
                            remote: false,
                        });
                    }
                }
                this.refresh_git(false, cx);
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Esc, arrows, Page Up/Down and Home/End in the open diff.
    fn git_diff_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if ev.keystroke.modifiers.modified() {
            return;
        }
        let key = ev.keystroke.key.as_str();
        if key == "escape" {
            cx.stop_propagation();
            return self.close_diff(cx);
        }
        let Some(open) = &self.git.diff else {
            return;
        };
        let state = open.scroll.0.borrow();
        let handle = &state.base_handle;
        let line = state.last_item_size.map_or(px(20.), |size| size.item.height);
        let page = (handle.bounds().size.height - line).max(line);
        let (offset, max) = (handle.offset(), handle.max_offset());
        let (x, y) = (-offset.x, -offset.y);
        let (x, y) = match key {
            "up" => (x, y - line),
            "down" => (x, y + line),
            "pageup" => (x, y - page),
            "pagedown" => (x, y + page),
            "home" => (x, px(0.)),
            "end" => (x, max.y),
            "left" => (x - line * 2., y),
            "right" => (x + line * 2., y),
            _ => return,
        };
        handle.set_offset(point(-x.clamp(px(0.), max.x), -y.clamp(px(0.), max.y)));
        // a focused hunk button scrolled out of view would drop focus
        window.focus(&self.git.diff_focus, cx);
        cx.stop_propagation();
        cx.notify();
    }

    /// The diff's rows as [`DiffLines`] draws them, with tab-stop hunk
    /// actions leading each `@@` row.
    fn hunk_lines(
        &self,
        diff: &Diff,
        scroll: &UniformListScrollHandle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows = diff.rows.clone();
        let digits = |side: fn(&DiffRow) -> Option<u32>| {
            rows.iter().filter_map(side).max().map(|n| n.to_string().len())
        };
        let old_digits = digits(|row| match row {
            DiffRow::Line { old, .. } => *old,
            _ => None,
        });
        let new_digits = digits(|row| match row {
            DiffRow::Line { new, .. } => *new,
            _ => None,
        });
        let widest = rows
            .iter()
            .enumerate()
            .max_by_key(|(_, row)| match row {
                DiffRow::Line { text, .. }
                | DiffRow::Hunk(text)
                | DiffRow::File(text)
                | DiffRow::Note(text) => text.len(),
            })
            .map(|(ix, _)| ix);
        let view = cx.entity();
        let list = uniform_list("git-diff-lines", rows.len(), move |range, _, cx| {
            let this = view.read(cx);
            let theme = cx.theme();
            let (fg, muted) = (theme.foreground, theme.muted_foreground);
            let (added_bg, removed_bg) = (theme.success.opacity(0.12), theme.danger.opacity(0.12));
            let (added_fg, removed_fg) = (theme::success_text(cx), theme::danger_text(cx));
            // ~0.6em per mono digit at text_xs, plus padding
            let number = move |digits: Option<usize>, n: Option<u32>| {
                digits.map(|d| {
                    div()
                        .flex_none()
                        .w(rems(0.45 * d as f32 + 0.75))
                        .pr_2()
                        .text_right()
                        .text_color(muted)
                        .children(n.map(|n| n.to_string()))
                })
            };
            range
                .map(|ix| match &rows[ix] {
                    DiffRow::Line { change, old, new, text } => {
                        let (bg, sign, sign_color) = match change {
                            DiffChange::Added => (Some(added_bg), "+", added_fg),
                            DiffChange::Removed => (Some(removed_bg), "−", removed_fg),
                            DiffChange::Context => (None, " ", muted),
                        };
                        h_flex()
                            .when_some(bg, |row, bg| row.bg(bg))
                            .children(number(old_digits, *old))
                            .children(number(new_digits, *new))
                            .child(
                                div()
                                    .flex_none()
                                    .w(rems(1.))
                                    .text_center()
                                    .text_color(sign_color)
                                    .child(sign),
                            )
                            .child(div().pr_3().text_color(fg).child(text.clone()))
                            .into_any_element()
                    }
                    DiffRow::Hunk(header) => {
                        let hunk = rows[..ix].iter().filter(|r| matches!(r, DiffRow::Hunk(_)));
                        this.hunk_row(hunk.count(), header, &view, cx)
                    }
                    DiffRow::File(text) | DiffRow::Note(text) => {
                        div().px_2().text_color(muted).child(text.clone()).into_any_element()
                    }
                })
                .collect::<Vec<_>>()
        });
        div()
            .relative()
            .size_full()
            .font_family(cx.theme().mono_font_family.clone())
            .text_xs()
            .line_height(rems(1.25))
            .whitespace_nowrap()
            .map(tabular)
            .child(
                list.with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
                    .with_width_from_item(widest)
                    .track_scroll(scroll)
                    .size_full(),
            )
            .scrollbar(scroll, ScrollbarAxis::Both)
            .into_any_element()
    }

    /// A hunk's `@@` row: Stage and Revert in a work-tree diff, Unstage in a
    /// staged one. Revert asks first: it writes the file, with no undo.
    fn hunk_row(
        &self,
        hunk: usize,
        header: &SharedString,
        view: &Entity<Self>,
        cx: &App,
    ) -> AnyElement {
        let Some(open) = &self.git.diff else {
            return div().into_any_element();
        };
        let Target::File(change) = &open.target else {
            return div().into_any_element();
        };
        let acting = match &open.diff {
            Some(Ok(diff)) => diff.acting,
            _ => None,
        };
        let busy = self.git.job.is_some();
        let line = hunk_header(header).map_or(0, |(_, _, new, _)| new);
        let ops: &[HunkOp] =
            if change.staged { &[HunkOp::Unstage] } else { &[HunkOp::Stage, HunkOp::Revert] };
        let theme = cx.theme();
        h_flex()
            .h(rems(1.25))
            .pl_1()
            .pr_2()
            .gap_0p5()
            .items_center()
            .bg(theme.muted.opacity(0.35))
            .text_color(theme.muted_foreground)
            .children(ops.iter().map(|&op| {
                let (id, icon, label, note) = match op {
                    HunkOp::Stage => {
                        ("git-hunk-stage", Icon::new(IconName::Plus), "Stage hunk", "")
                    }
                    HunkOp::Unstage => {
                        ("git-hunk-unstage", Icon::new(IconName::Minus), "Unstage hunk", "")
                    }
                    HunkOp::Revert => (
                        "git-hunk-revert",
                        Icon::new(Lucide::RotateCcw),
                        "Revert hunk",
                        ", discarding its edits…",
                    ),
                };
                let running = acting == Some((hunk, op));
                let view = view.clone();
                aether_ui::button((id, hunk))
                    .ghost()
                    .xsmall()
                    .icon(icon)
                    .accessibility_label(format!("{label} at line {line}{note}"))
                    .tooltip(format!("{label}{note}"))
                    .loading(running)
                    .disabled(busy && !running)
                    .on_click(move |_, window, cx| {
                        view.update(cx, |this, cx| this.hunk_clicked(hunk, op, window, cx))
                    })
            }))
            .child(div().pl_1().child(header.clone()))
            .into_any_element()
    }

    /// An open file's header: its name, status and stats, Copy diff, and Stage or Unstage.
    fn file_header(&self, open: &OpenDiff, change: &Change, cx: &mut Context<Self>) -> Div {
        let (muted, border) = (cx.theme().muted_foreground, cx.theme().sidebar_border);
        let (name, dir) = split_path(&change.path);
        let running = self.git.busy_with(change);
        let key = open.key.clone();
        let mut where_ = describe(change);
        if !dir.is_empty() {
            where_ = format!("{dir} · {where_}");
        }
        let action = if change.staged { "Unstage" } else { "Stage" };
        v_flex()
            .flex_none()
            .px_2()
            .py_1()
            .gap_0p5()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(back_button(cx))
                    .child(letter_chip(change.kind, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .font_medium()
                            .child(name.to_string()),
                    )
                    .child(stat_element(change.stat, cx))
                    .children(open.diff.as_ref().and_then(|d| d.as_ref().ok()).map(|diff| {
                        Clipboard::new("git-diff-copy")
                            .value(diff.raw.clone())
                            .tooltip("Copy diff")
                            .small()
                    })),
            )
            .child(
                h_flex()
                    .pl(rems(2.))
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_middle()
                            .text_xs()
                            .text_color(muted)
                            .child(where_),
                    )
                    .child(
                        aether_ui::button("git-diff-stage")
                            .ghost()
                            .xsmall()
                            .icon(if change.staged { IconName::Minus } else { IconName::Plus })
                            .label(action)
                            .accessibility_label(format!("{action} {}", change.path))
                            .loading(running)
                            .disabled(self.git.job.is_some() && !running)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.toggle_stage(&key, window, cx)
                            })),
                    ),
            )
    }

    /// The open file's, commit's or stash's diff, with a way back to the list.
    pub(super) fn git_diff_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(open) = &self.git.diff else {
            return div().into_any_element();
        };
        let (muted, border) = (cx.theme().muted_foreground, cx.theme().sidebar_border);
        let (header, change) = match &open.target {
            Target::File(change) => (self.file_header(open, change, cx), Some(change)),
            Target::Commit(commit) => (self.commit_header(commit, cx), None),
            Target::Stash(stash) => (self.stash_header(stash, &open.key, cx), None),
        };
        let body = match &open.diff {
            None => self.git_placeholder_rows(cx),
            Some(Err(e)) => note(
                Lucide::CircleAlert,
                theme::danger_text(cx),
                "Couldn't load the diff",
                e.clone(),
                cx,
            )
            .child(
                aether_ui::button("git-diff-retry")
                    .outline()
                    .small()
                    .mt_2()
                    .label("Retry")
                    .on_click(cx.listener(|this, _, _, cx| this.load_diff(cx))),
            )
            .into_any_element(),
            Some(Ok(diff)) if diff.binary => {
                note(Lucide::File, muted, "Binary file", "Its contents can't be shown as text.", cx)
                    .into_any_element()
            }
            Some(Ok(diff)) if diff.too_large() && !open.full => {
                let title = format!("Diff too large ({} lines)", format_full(diff.lines as u64));
                note(Lucide::FileText, muted, title, "Showing it all may take a moment.", cx)
                    .child(
                        aether_ui::button("git-diff-full")
                            .outline()
                            .small()
                            .mt_2()
                            .label("Show anyway")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(open) = &mut this.git.diff {
                                    open.full = true;
                                }
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
            Some(Ok(diff)) if diff.rows.is_empty() => {
                let (title, text) = match &open.target {
                    Target::File(_) => {
                        ("No content changes", "Only the file's mode or name changed.")
                    }
                    Target::Commit(_) => {
                        ("No file changes", "Git shows none for this commit, as for a merge.")
                    }
                    Target::Stash(_) => ("No file changes", "Git shows none for this stash."),
                };
                note(Lucide::FileText, muted, title, text, cx).into_any_element()
            }
            Some(Ok(diff)) => v_flex()
                .flex_1()
                .min_h_0()
                .child(if change.is_some_and(|change| hunk_actions(change, diff)) {
                    self.hunk_lines(diff, &open.scroll, cx)
                } else {
                    DiffLines::new("git-diff-lines", diff.rows.clone(), &open.scroll)
                        .into_any_element()
                })
                .when(diff.cut, |el| {
                    el.child(
                        div()
                            .flex_none()
                            .px_3()
                            .py_1()
                            .text_xs()
                            .text_color(muted)
                            .border_t_1()
                            .border_color(border)
                            .child("Diff cut at 1 MB. Open the file in your editor for the rest."),
                    )
                })
                .into_any_element(),
        };
        // a failed stage from this view shows here; the commit area is hidden
        let failed =
            self.feedback_here().is_some_and(|f| f.error).then(|| self.git_feedback(cx)).flatten();
        v_flex()
            .id("git-diff")
            .track_focus(&self.git.diff_focus)
            .flex_1()
            .min_h_0()
            .on_key_down(
                cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                    this.git_diff_key(ev, window, cx)
                }),
            )
            .child(header)
            .children(failed.map(|failed| {
                div().flex_none().p_3().border_b_1().border_color(border).child(failed)
            }))
            .child(body)
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/git_panel/diff.rs"]
mod tests;
