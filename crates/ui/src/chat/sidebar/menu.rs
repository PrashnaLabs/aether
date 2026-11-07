use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName,
    menu::{PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{App, ClipboardItem, Context, Entity, SharedString, Window, div, prelude::*};

use super::state::SidebarState;
use super::{ChatItem, ChatLine, RowHandler};

fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") { "Reveal in Finder" } else { "Show in folder" }
}

pub(super) fn track_menu(
    state: &Entity<SidebarState>,
    key: &SharedString,
) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
    let (state, key) = (state.clone(), key.clone());
    move |open, _, cx| {
        state.update(cx, |s, cx| {
            if *open {
                s.open_menu = Some(key.clone());
            } else if s.open_menu.as_ref() == Some(&key) {
                s.open_menu = None;
            }
            cx.notify();
        })
    }
}

fn copy_item(label: &'static str, text: SharedString) -> PopupMenuItem {
    PopupMenuItem::new(label).icon(IconName::Copy).on_click(move |_, _, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()))
    })
}

/// A removed worktree's row can't reveal its folder; the menu item stays as
/// a disabled placeholder so the row keeps its shape.
fn reveal_disabled(line: &ChatLine) -> bool {
    line.removed
}

fn reveal_item(path: SharedString, disabled: bool) -> PopupMenuItem {
    PopupMenuItem::new(reveal_label())
        .icon(IconName::FolderOpen)
        .disabled(disabled)
        .on_click(move |_, _, cx| cx.reveal_path(std::path::Path::new(&*path)))
}

#[cfg(test)]
fn project_menu_labels(has_page: bool, has_files: bool, has_hide: bool) -> Vec<&'static str> {
    let mut labels = Vec::new();
    if has_page {
        labels.push("Open project page");
    }
    labels.push("New agent chat here");
    if has_files {
        labels.push("Browse files");
    }
    labels.extend([reveal_label(), "Copy path"]);
    if has_hide {
        labels.push("Remove from sidebar");
    }
    labels
}

pub(super) fn project_menu(
    menu: PopupMenu,
    missing: bool,
    path: SharedString,
    on_new: RowHandler,
    on_files: Option<RowHandler>,
    on_hide: Option<RowHandler>,
    on_page: Option<RowHandler>,
) -> PopupMenu {
    let mut menu = menu;
    if let Some(on_page) = on_page {
        menu = menu
            .item(
                PopupMenuItem::new("Open project page")
                    .icon(Lucide::LayoutDashboard)
                    .on_click(move |ev, window, cx| on_page(ev, window, cx)),
            )
            .separator();
    }
    let mut menu = menu.item(
        PopupMenuItem::new("New agent chat here")
            .icon(IconName::Plus)
            .disabled(missing)
            .on_click(move |ev, window, cx| on_new(ev, window, cx)),
    );
    if let Some(on_files) = on_files {
        menu = menu.item(
            PopupMenuItem::new("Browse files")
                .icon(IconName::Folder)
                .disabled(missing)
                .on_click(move |ev, window, cx| on_files(ev, window, cx)),
        );
    }
    let menu = menu.item(reveal_item(path.clone(), missing)).item(copy_item("Copy path", path));
    match on_hide {
        Some(on_hide) => menu.separator().item(
            PopupMenuItem::element(|_, cx| {
                v_flex().child("Remove from sidebar").child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Chats and files are kept"),
                )
            })
            .icon(IconName::EyeOff)
            .on_click(move |ev, window, cx| on_hide(ev, window, cx)),
        ),
        // Worktree headers offer no hide: it would nuke the parent project.
        None => menu,
    }
}

/// Row menu, shared by right-click and the row's `…` button.
pub(super) fn chat_menu(
    menu: PopupMenu,
    chat: &ChatItem,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let (rename, duplicate, pin, archive, delete) = (
        chat.on_rename.clone(),
        chat.on_duplicate.clone(),
        chat.on_pin.clone(),
        chat.on_archive.clone(),
        chat.on_delete.clone(),
    );
    let menu = menu
        .item(
            PopupMenuItem::new("Rename")
                .icon(IconName::CaseSensitive)
                .on_click(move |ev, window, cx| rename(ev, window, cx)),
        )
        .item(
            PopupMenuItem::new("Duplicate")
                .icon(IconName::Copy)
                .on_click(move |ev, window, cx| duplicate(ev, window, cx)),
        )
        .item(
            PopupMenuItem::new(if chat.pinned { "Unpin" } else { "Pin" })
                .icon(IconName::Star)
                .on_click(move |ev, window, cx| pin(ev, window, cx)),
        );
    let menu = match chat.handoff.clone() {
        Some((targets, on_pick)) => {
            let id = chat.id;
            let icon = Some(Icon::new(Lucide::Forward));
            menu.submenu_with_icon(icon, "Hand off to", window, cx, move |mut menu, _, _| {
                for &target in &targets {
                    let on_pick = on_pick.clone();
                    menu = menu.item(
                        PopupMenuItem::new(target.label())
                            .icon(crate::provider_icon(target))
                            .on_click(move |_, window, cx| on_pick(id, target, window, cx)),
                    );
                }
                menu
            })
        }
        _ => menu,
    };
    let menu = match chat.on_watch.clone().filter(|_| !chat.archived) {
        Some(on_watch) => {
            let id = chat.id;
            menu.item(
                PopupMenuItem::new(if chat.watched { "Unwatch" } else { "Watch" })
                    .icon(if chat.watched { IconName::EyeOff } else { IconName::Eye })
                    .on_click(move |_, window, cx| on_watch(id, window, cx)),
            )
        }
        None => menu,
    };
    let menu = match chat.line.as_ref() {
        Some(line) => menu
            .separator()
            .item(reveal_item(line.path.clone(), reveal_disabled(line)))
            .item(copy_item("Copy path", line.path.clone())),
        None => menu.separator(),
    };
    let menu = menu.item(copy_item("Copy title", chat.title.clone()));
    let menu = match chat.on_remove_worktree.clone() {
        Some(remove) => menu.item(
            PopupMenuItem::new("Remove worktree…")
                .icon(Lucide::FolderX)
                .on_click(move |ev, window, cx| remove(ev, window, cx)),
        ),
        None => menu,
    };
    menu.separator()
        .item(
            PopupMenuItem::new(if chat.archived { "Unarchive" } else { "Archive" })
                .icon(if chat.archived {
                    Icon::new(Lucide::ArchiveRestore)
                } else {
                    Icon::new(IconName::Inbox)
                })
                .on_click(move |ev, window, cx| archive(ev, window, cx)),
        )
        .item(
            PopupMenuItem::new("Delete")
                .icon(Lucide::Trash)
                .on_click(move |ev, window, cx| delete(ev, window, cx)),
        )
}

#[cfg(test)]
#[path = "../../../tests/unit/chat/sidebar/menu.rs"]
mod tests;
