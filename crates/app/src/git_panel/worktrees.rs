//! The Worktrees section: every worktree of the active chat's repo, each
//! opened in a new chat, revealed, copied or removed, and creating and
//! pruning them from the "…" menu.

use aether_ui::theme::Corners;
use std::path::{Path, PathBuf};

use aether_core::home_dir;
use aether_git::worktree::{self, Start};
use aether_git::{ChangeKind, Listed, Status};
use aether_ui::{row_style, theme};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, h_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, ClipboardItem, Context, Entity, Hsla, PathPromptOptions, Role,
    SharedString, Window, div, prelude::*, px, rems,
};

use super::Feedback;
use super::list::ROW_HEIGHT;
use super::picker::{PickMode, pick_title};
use super::widgets::hover_actions;
use crate::app::{ChatApp, display_path};

/// How old a listing gets before a status read lists again, in ms; making,
/// removing or pruning one lists at once.
const STALE: i64 = 10_000;

/// A repo's worktrees as last listed, and when.
pub(super) struct Trees {
    at: i64,
    /// `None` while the first listing runs; a re-list keeps the last good one up.
    list: Option<Result<Vec<Listed>, SharedString>>,
}

/// A worktree command the panel is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TreeJob {
    Create,
    Prune,
}

type Act = fn(&mut ChatApp, &str, &mut Window, &mut Context<ChatApp>);

pub(super) fn tree_key(tree: &Listed) -> SharedString {
    format!("worktree-{}", tree.path.display()).into()
}

pub(super) fn tree_name(tree: &Listed) -> String {
    tree.path
        .file_name()
        .map_or_else(|| tree.path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// Its branch, else "detached a1b2c3d".
pub(super) fn tree_branch(tree: &Listed) -> String {
    match &tree.branch {
        Some(branch) => branch.clone(),
        None => format!("detached {}", &tree.head[..tree.head.len().min(7)]).trim_end().into(),
    }
}

/// Its state, in the order the row shows it; the main one's icon marks it.
pub(super) fn tree_flags(tree: &Listed, current: bool) -> Vec<&'static str> {
    [
        (current, "current"),
        (tree.dirty == Some(true), "changes"),
        (tree.locked.is_some(), "locked"),
        (tree.missing, "missing"),
    ]
    .into_iter()
    .filter_map(|(on, flag)| on.then_some(flag))
    .collect()
}

/// A worktree row in words.
pub(super) fn tree_label(tree: &Listed, current: bool) -> String {
    let mut parts = vec![tree_name(tree), format!("on {}", tree_branch(tree))];
    if tree.main {
        parts.push("main worktree".into());
    }
    parts.extend(tree_flags(tree, current).into_iter().map(|flag| match flag {
        "changes" => "uncommitted changes".to_string(),
        "missing" => "folder missing".to_string(),
        "locked" => match tree.locked.as_deref() {
            Some(why) if !why.is_empty() => format!("locked: {why}"),
            _ => "locked".into(),
        },
        flag => flag.to_string(),
    }));
    let keys = match (tree.missing, tree.remove_action().is_ok()) {
        (true, true) => ". Press Delete to prune it",
        (true, false) => "",
        (false, true) => ". Press Enter to open it in a new chat, Delete to remove it",
        (false, false) => ". Press Enter to open it in a new chat",
    };
    format!("{}{keys}", parts.join(", "))
}

/// Where `branch` is checked out among `trees`: "here" in `top`, else "in <folder>".
pub(super) fn checked_out_at(trees: &[Listed], top: &Path, branch: &str) -> Option<String> {
    let tree = trees.iter().find(|t| t.branch.as_deref() == Some(branch))?;
    Some(if tree.path == top { "here".into() } else { format!("in {}", tree_name(tree)) })
}

/// The worktree picker's title: what Enter makes.
pub(super) fn tree_pick_title(mode: &PickMode) -> String {
    match mode {
        PickMode::Switch => "Create worktree".into(),
        mode => pick_title(mode).replacen("Create branch", "Create worktree on a new branch", 1),
    }
}

/// Whether the "…" menu's Create worktree… and Prune worktrees can run.
pub(super) fn tree_menu(status: &Status, busy: bool, job: Option<TreeJob>) -> (bool, bool) {
    let free = !busy && job.is_none();
    (free && status.commit.is_some(), free)
}

/// What the prune confirm says; `missing` is how many listed folders are gone.
pub(super) fn prune_prompt(missing: usize) -> String {
    let listed = match missing {
        0 => "None listed here are gone.".to_string(),
        1 => "1 listed here is gone.".into(),
        n => format!("{n} listed here are gone."),
    };
    format!(
        "Clears git's records of worktrees whose folders are gone. {listed} Locked ones stay. \
         A worktree on a drive that isn't connected right now counts as gone."
    )
}

/// The line after a prune.
pub(super) fn prune_note(pruned: &[String]) -> String {
    match pruned {
        [] => "Nothing to prune".into(),
        [one] => format!("Pruned worktree {one}"),
        many => format!("Pruned {} worktrees", many.len()),
    }
}

/// The "…" menu's worktree items; `enabled` is from [`tree_menu`].
pub(super) fn tree_menu_items(
    menu: PopupMenu,
    view: &Entity<ChatApp>,
    (create, prune): (bool, bool),
) -> PopupMenu {
    let (make, clear) = (view.clone(), view.clone());
    menu.separator()
        .item(
            PopupMenuItem::new("Create worktree…")
                .icon(Lucide::GitFork)
                .disabled(!create)
                .on_click(move |_, window, cx| {
                    make.update(cx, |this, cx| this.open_tree_picker(window, cx))
                }),
        )
        .item(PopupMenuItem::new("Prune worktrees").icon(Lucide::Eraser).disabled(!prune).on_click(
            move |_, window, cx| clear.update(cx, |this, cx| this.confirm_prune(window, cx)),
        ))
}

fn flag_color(flag: &str, cx: &App) -> Hsla {
    match flag {
        "current" => theme::info_text(cx),
        "changes" => theme::mango_text(cx),
        "missing" => theme::danger_text(cx),
        _ => cx.theme().muted_foreground,
    }
}

impl ChatApp {
    /// The active folder's worktrees as last listed; empty until a listing lands.
    pub(crate) fn trees(&self) -> &[Listed] {
        self.git_folder()
            .and_then(|folder| self.git.trees.get(&folder))
            .and_then(|trees| trees.list.as_ref()?.as_ref().ok())
            .map_or(&[], Vec::as_slice)
    }

    pub(crate) fn trees_listed(&self) -> bool {
        self.git_folder()
            .and_then(|folder| self.git.trees.get(&folder))
            .is_some_and(|trees| trees.list.is_some())
    }

    fn tree_at(&self, key: &str) -> Option<Listed> {
        self.trees().iter().find(|t| tree_key(t) == key).cloned()
    }

    /// Where `branch` is checked out, for the worktree picker.
    pub(crate) fn branch_tree(&self, branch: &str) -> Option<String> {
        checked_out_at(self.trees(), &self.git_status()?.top, branch)
    }

    /// aether's folder for the active repo's new worktrees, as shown.
    pub(super) fn tree_slot(&self) -> Option<String> {
        let main = self.trees().iter().find(|t| t.main)?;
        let slot = worktree::root()?.join(worktree::repo_dir(&main.path));
        Some(display_path(&slot.display().to_string(), &home_dir()))
    }

    /// Lists the active folder's worktrees again once a status read lands
    /// on a stale listing, off the UI thread.
    pub(crate) fn sync_trees(&mut self, cx: &mut Context<Self>) {
        let (Some(folder), Some(at)) =
            (self.git_folder(), self.git_loaded().filter(|l| l.status.is_ok()).map(|l| l.at))
        else {
            return;
        };
        if self.git.trees.get(&folder).is_some_and(|t| at - t.at < STALE) {
            return;
        }
        let list = self.git.trees.remove(&folder).and_then(|t| t.list).filter(Result::is_ok);
        self.git.trees.insert(folder.clone(), Trees { at, list });
        let task = self.agent.repo_worktrees(folder.clone());
        cx.spawn(async move |this, cx| {
            let read = task.await;
            this.update(cx, |this, cx| {
                if let Some(trees) = this.git.trees.get_mut(&folder)
                    && trees.at == at
                {
                    trees.list = Some(read.map_err(SharedString::from));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Lists every repo's worktrees again on the next paint.
    pub(crate) fn relist_trees(&mut self) {
        for trees in self.git.trees.values_mut() {
            trees.at = i64::MIN / 2;
        }
    }

    pub(crate) fn open_tree_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.git.tree_job.is_none() {
            self.open_picker_as(true, window, cx);
        }
    }

    fn open_tree_chat(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tree) = self.tree_at(key).filter(|t| !t.missing) {
            self.open_worktree_chat(tree.path, tree.main, window, cx);
        }
    }

    fn reveal_tree(&mut self, key: &str, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(tree) = self.tree_at(key).filter(|t| !t.missing) {
            cx.reveal_path(&tree.path);
        }
    }

    fn copy_tree_path(&mut self, key: &str, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(tree) = self.tree_at(key) {
            cx.write_to_clipboard(ClipboardItem::new_string(tree.path.display().to_string()));
        }
    }

    /// Remove… or Prune: aether's own worktrees and the user's go through the
    /// same checks and confirms, refused while an agent works in them.
    pub(super) fn remove_tree(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tree) = self.tree_at(key).filter(|t| t.remove_action().is_ok()) {
            self.start_remove(tree.path, tree.repo, window, cx);
        }
    }

    /// "Choose folder…": the worktree goes in a new subfolder of a folder the user picks.
    pub(super) fn choose_tree_folder(
        &mut self,
        start: Start,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folder = self.git_folder();
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Create worktree here".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut paths))) = rx.await else {
                return;
            };
            let Some(parent) = paths.pop() else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                if this.git_folder() == folder {
                    this.create_tree(start, Some(parent), window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Checks `start` out in a new worktree, in aether's folder or a new
    /// subfolder of `parent`, then offers a chat in it.
    pub(super) fn create_tree(
        &mut self,
        start: Start,
        parent: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(folder) = self.git_folder().filter(|_| self.git.tree_job.is_none()) else {
            return;
        };
        self.git.tree_job = Some(TreeJob::Create);
        self.git.feedback = None;
        let task = self.agent.add_worktree(folder.clone(), start, parent);
        cx.spawn_in(window, async move |this, cx| {
            let made = task.await;
            this.update_in(cx, |this, window, cx| {
                this.git.tree_job = None;
                this.worktrees_changed(cx);
                match made {
                    Ok(path) => this.offer_tree_chat(folder, path, window, cx),
                    Err(e) => {
                        this.git.feedback = Some(Feedback {
                            folder: Some(folder),
                            error: true,
                            text: "Couldn't create the worktree".into(),
                            detail: Some(e.into()),
                            remote: false,
                        })
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn offer_tree_chat(
        &mut self,
        folder: PathBuf,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shown = display_path(&path.display().to_string(), &home_dir());
        self.git.feedback = Some(Feedback {
            folder: Some(folder),
            error: false,
            text: format!("Created worktree {shown}").into(),
            detail: None,
            remote: false,
        });
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let (view, path) = (view.clone(), path.clone());
            alert
                .confirm()
                .title("Worktree created")
                .description(format!("{shown} is ready. Open it in a new chat?"))
                .ok_text("Open in new chat")
                .cancel_text("Not now")
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        this.open_worktree_chat(path.clone(), false, window, cx);
                        this.chats[this.active].fresh_worktree = Some(path.clone());
                    });
                    true
                })
        });
    }

    /// Asks before pruning: it also drops worktrees on drives that are just unplugged.
    pub(super) fn confirm_prune(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(folder) = self.git_folder().filter(|_| self.git.tree_job.is_none()) else {
            return;
        };
        let missing = self.trees().iter().filter(|t| t.missing && t.locked.is_none()).count();
        let text = prune_prompt(missing);
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let (view, folder) = (view.clone(), folder.clone());
            alert
                .confirm()
                .title("Prune worktrees?")
                .description(text.clone())
                .ok_text("Prune")
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| this.prune_trees(folder.clone(), window, cx));
                    true
                })
        });
    }

    /// Prunes in `folder`'s repo, the one the confirm was for, not the active chat's.
    fn prune_trees(&mut self, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.git.tree_job.is_some() {
            return;
        }
        self.git.tree_job = Some(TreeJob::Prune);
        self.git.feedback = None;
        let task = self.agent.prune_worktrees(folder.clone());
        cx.spawn_in(window, async move |this, cx| {
            let pruned = task.await;
            this.update(cx, |this, cx| {
                this.git.tree_job = None;
                this.worktrees_changed(cx);
                this.git.feedback = Some(match pruned {
                    Ok(names) => Feedback {
                        folder: Some(folder),
                        error: false,
                        text: prune_note(&names).into(),
                        detail: (names.len() > 1).then(|| names.join("\n").into()),
                        remote: false,
                    },
                    Err(e) => Feedback {
                        folder: Some(folder),
                        error: true,
                        text: "Couldn't prune worktrees".into(),
                        detail: Some(e.into()),
                        remote: false,
                    },
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// A worktree row: folder, branch, path and state, with Open in new chat,
    /// Reveal and Remove on hover and in its menu. Enter opens it in a new
    /// chat, Delete removes it.
    pub(super) fn tree_row(
        &self,
        ix: usize,
        key: &SharedString,
        view: &Entity<Self>,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
        let (Some(tree), Some(status)) = (self.trees().get(ix), self.git_status()) else {
            return div().into_any_element();
        };
        let current = tree.path == status.top;
        let mut tree = tree.clone();
        // the panel's own read is newer than the listing
        if current {
            tree.dirty = Some(status.changes.iter().any(|c| c.kind != ChangeKind::Untracked));
        }
        let focus = self.git.focus.get(key).cloned();
        let focused = focus.as_ref().is_some_and(|f| f.is_focused(window));
        let removing = self.removing.contains(&tree.path);
        let muted = cx.theme().muted_foreground;
        let group = SharedString::from(format!("git-group-{key}"));
        let show_action = focused || removing || self.git.cursor.as_ref() == Some(key);
        let shown = display_path(&tree.path.display().to_string(), &home_dir());
        let (name, branch, flags) =
            (tree_name(&tree), tree_branch(&tree), tree_flags(&tree, current));
        let remove = tree.remove_action();
        let reveal = if cfg!(target_os = "macos") { "Reveal in Finder" } else { "Show in folder" };
        let button = |id: &str, icon: Lucide, tip: SharedString, off: bool, act: Act| {
            let (view, key) = (view.clone(), key.clone());
            aether_ui::button(SharedString::from(format!("git-tree-{id}-{key}")))
                .ghost()
                .xsmall()
                .icon(icon)
                .accessibility_label(format!("{tip}: {name}"))
                .tooltip(tip)
                .tab_stop(false)
                .disabled(off)
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    view.update(cx, |this, cx| act(this, &key, window, cx));
                })
        };
        let remove_button = button(
            "remove",
            if tree.missing { Lucide::Eraser } else { Lucide::Trash },
            remove.clone().map_or_else(SharedString::from, SharedString::from),
            remove.is_err(),
            Self::remove_tree,
        )
        .loading(removing);
        let click = {
            let (view, key) = (view.clone(), key.clone());
            move |ev: &ClickEvent, window: &mut Window, cx: &mut App| {
                let keyboard = matches!(ev, ClickEvent::Keyboard(_));
                view.update(cx, |this, cx| {
                    if keyboard {
                        this.open_tree_chat(&key, window, cx);
                    } else {
                        this.git.cursor = Some(key.clone());
                        cx.notify();
                    }
                });
            }
        };
        let menu = {
            let (view, key, missing, remove) =
                (view.clone(), key.clone(), tree.missing, remove.clone());
            move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
                let item = |label: SharedString, icon: Lucide, off: bool, act: Act| {
                    let (view, key) = (view.clone(), key.clone());
                    PopupMenuItem::new(label).icon(Icon::new(icon)).disabled(off).on_click(
                        move |_, window, cx| {
                            view.update(cx, |this, cx| act(this, &key, window, cx))
                        },
                    )
                };
                let menu = menu
                    .item(item(
                        "Open in new chat".into(),
                        Lucide::MessageSquarePlus,
                        missing,
                        Self::open_tree_chat,
                    ))
                    .item(item(reveal.into(), Lucide::FolderOpen, missing, Self::reveal_tree))
                    .item(item("Copy path".into(), Lucide::Copy, false, Self::copy_tree_path))
                    .separator();
                match &remove {
                    Ok(label) => {
                        menu.item(item((*label).into(), Lucide::Trash, false, Self::remove_tree))
                    }
                    Err(why) => menu
                        .item(item(
                            "Remove worktree…".into(),
                            Lucide::Trash,
                            true,
                            Self::remove_tree,
                        ))
                        .label(why.clone()),
                }
            }
        };
        let mut tip = format!("{}\n{branch}", tree.path.display());
        if tree.main {
            tip.push_str("\nMain worktree");
        }
        if let Some(why) = tree.locked.as_deref().filter(|why| !why.is_empty()) {
            tip.push_str(&format!("\nLocked: {why}"));
        }
        if tree.missing {
            tip.push_str("\nThe folder is gone; prune it to clear git's record");
        }
        row_style(
            h_flex()
                .id(SharedString::from(format!("git-row-{key}")))
                .when_some(focus, |el, focus| el.track_focus(&focus))
                .role(Role::Button)
                .aria_label(tree_label(&tree, current)),
            false,
            cx,
        )
        .group(group.clone())
        .w_full()
        .h(rems(ROW_HEIGHT))
        .pl_3()
        .pr_2()
        .gap_2()
        .items_center()
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .child(
            Icon::new(if tree.main { Lucide::House } else { Lucide::GitFork })
                .size(rems(0.875))
                .flex_none()
                .text_color(muted),
        )
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .gap_1p5()
                .items_baseline()
                .overflow_hidden()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .when(tree.missing, |el| el.text_color(muted))
                        .child(name.clone()),
                )
                .child(div().min_w_0().truncate().text_xs().text_color(muted).child(branch))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_middle()
                        .text_xs()
                        .text_color(muted)
                        .child(shown),
                ),
        )
        .child(hover_actions(
            group,
            show_action,
            h_flex().gap_2().children(flags.into_iter().map(|flag| {
                let color = flag_color(flag, cx);
                div()
                    .flex_none()
                    .px_1()
                    .corner(px(4.))
                    .bg(color.opacity(0.12))
                    .text_color(color)
                    .text_xs()
                    .child(flag)
            })),
            h_flex()
                .child(button(
                    "open",
                    Lucide::MessageSquarePlus,
                    "Open in new chat".into(),
                    tree.missing,
                    Self::open_tree_chat,
                ))
                .child(button(
                    "reveal",
                    Lucide::FolderOpen,
                    reveal.into(),
                    tree.missing,
                    Self::reveal_tree,
                ))
                .child(remove_button),
            cx,
        ))
        .on_click(click)
        .on_key_down(self.row_keys(key, view))
        .context_menu(menu)
        .into_any_element()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/git_panel/worktrees.rs"]
mod tests;
