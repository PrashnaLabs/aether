use crate::theme::Corners;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, StyledExt,
    button::ButtonVariants,
    h_flex,
    menu::{ContextMenuExt, DropdownMenu, PopupMenu},
};
use gpui_kit::{
    Anchor, AnyElement, App, FocusHandle, KeyDownEvent, Role, SharedString, Window, div,
    prelude::*, px, relative, rems,
};

use super::chrome::{nest, reveal_on_hover, row_style, swallow_click, tabular, tooltip};
use super::menu::project_menu;
use super::{ChatStatus, CollapseHandler, NEST, ProjectHeader, RowHandler};

pub fn project_aria(
    name: &str,
    count: usize,
    worktree: bool,
    missing: bool,
    collapsed: bool,
    status: ChatStatus,
) -> String {
    let chats = if count == 1 { "1 chat".to_string() } else { format!("{count} chats") };
    let mut aria = format!("{name}, {chats}");
    if worktree {
        aria.push_str(", worktree");
    }
    if missing {
        aria.push_str(", folder not found");
    } else if collapsed && status != ChatStatus::Idle {
        aria.push_str(&format!(", {}", status.label()));
    }
    aria
}

pub fn project_chevron_icon(collapsed: bool) -> IconName {
    if collapsed { IconName::ChevronRight } else { IconName::ChevronDown }
}

pub fn project_folder_icon(collapsed: bool) -> IconName {
    if collapsed { IconName::FolderClosed } else { IconName::FolderOpen }
}

pub(super) struct ProjectRow {
    pub(super) id: usize,
    pub(super) header: ProjectHeader,
    pub(super) spaced: bool,
    pub(super) guide: bool,
    pub(super) nested: bool,
    pub(super) worktree: bool,
    pub(super) on_collapse: CollapseHandler,
    pub(super) on_new: RowHandler,
    pub(super) on_files: Option<RowHandler>,
    pub(super) on_hide: Option<RowHandler>,
    pub(super) on_page: Option<RowHandler>,
    pub(super) now: i64,
}

impl ProjectRow {
    pub(super) fn render(
        self,
        focus: Option<FocusHandle>,
        show_actions: bool,
        on_menu: impl Fn(&bool, &mut Window, &mut App) + 'static,
        nav: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
        group: SharedString,
        cx: &App,
    ) -> AnyElement {
        let Self {
            id,
            header,
            spaced,
            guide,
            nested,
            worktree,
            on_collapse,
            on_new,
            on_files,
            on_hide,
            on_page,
            now,
        } = self;
        let ProjectHeader { name, path, count, collapsed, status, missing } = header;
        let muted = cx.theme().muted_foreground;
        let color = if missing || worktree { muted } else { super::project_color(&name, cx) };
        let aria = project_aria(&name, count, worktree, missing, collapsed, status);
        let name_tip: SharedString =
            if missing { format!("Folder not found: {path}").into() } else { path.clone() };
        let new_tip: SharedString = if missing {
            "Folder not found".into()
        } else {
            format!("New agent chat in {name}").into()
        };
        let menu = {
            let (path, on_new, on_files, on_hide, on_page) =
                (path.clone(), on_new.clone(), on_files.clone(), on_hide.clone(), on_page.clone());
            move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
                project_menu(
                    menu,
                    missing,
                    path.clone(),
                    on_new.clone(),
                    on_files.clone(),
                    on_hide.clone(),
                    on_page.clone(),
                )
            }
        };
        let context = menu.clone();
        let count_label = div()
            .absolute()
            .right_0()
            .px_1p5()
            .py_0p5()
            .corner(px(4.))
            .bg(cx.theme().muted.opacity(0.3))
            .text_color(muted)
            .text_xs()
            .font_features(tabular())
            .child(if missing { "Missing".to_string() } else { count.to_string() })
            .map(|el| {
                if show_actions {
                    el.opacity(0.)
                } else {
                    el.group_hover(group.clone(), |s| s.opacity(0.))
                }
            });
        let actions = reveal_on_hover(
            h_flex().flex_none().justify_end().items_center().gap_1(),
            group.clone(),
            show_actions,
        )
        .when_some(on_files, |act, on_files| {
            act.child(
                crate::components::button(("project-files", id))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Folder)
                    .accessibility_label("Browse files")
                    .tooltip(format!("Browse files in {name}"))
                    .disabled(missing)
                    .on_click(move |ev, window, cx| {
                        cx.stop_propagation();
                        on_files(ev, window, cx)
                    }),
            )
        })
        .child(
            crate::components::button(("project-new", id))
                .ghost()
                .xsmall()
                .icon(IconName::Plus)
                .accessibility_label(new_tip.clone())
                .tooltip(new_tip)
                .disabled(missing)
                .on_click(move |ev, window, cx| {
                    cx.stop_propagation();
                    on_new(ev, window, cx)
                }),
        )
        .child(swallow_click(
            ("project-menu-slot", id),
            crate::components::button(("project-menu", id))
                .ghost()
                .xsmall()
                .icon(IconName::Ellipsis)
                .accessibility_label(format!("Project actions for {name}"))
                .tooltip("Project actions")
                .dropdown_menu_with_anchor(Anchor::TopRight, menu)
                .on_open_change(on_menu),
        ));
        let toggle = on_collapse.clone();
        let row = row_style(
            h_flex()
                .id(("project", id))
                .when_some(focus, |el, focus| el.track_focus(&focus))
                .role(Role::Button)
                .aria_label(aria)
                .aria_expanded(!collapsed),
            false,
            cx,
        )
        .group(group)
        .relative()
        .w_full()
        .when(spaced, |row| row.mt_0p5())
        .gap_2()
        .px_2()
        .py_1()
        .text_sm()
        .text_color(muted)
        .child(
            Icon::new(project_chevron_icon(collapsed))
                .size(rems(0.6875))
                .text_color(muted)
                .flex_none(),
        )
        .child(
            Icon::new(project_folder_icon(collapsed))
                .size(rems(0.8125))
                .text_color(color)
                .flex_none(),
        )
        .child(
            div()
                .id(("project-name", id))
                .text_color(color)
                .flex_1()
                .min_w_0()
                .truncate()
                .font_medium()
                .tooltip(tooltip(name_tip))
                .child(name),
        )
        .when(collapsed && status != ChatStatus::Idle, |row| {
            row.child(status.badge(("project-status", id), Some(now), cx))
        })
        .child(
            div()
                .relative()
                .flex_none()
                .h(rems(1.25))
                .flex()
                .items_center()
                .child(count_label)
                .child(actions),
        )
        .when(guide, |row| {
            row.child(
                div()
                    .absolute()
                    .left(rems(NEST))
                    .ml(px(-1.))
                    .w(px(1.))
                    .top(relative(0.5))
                    .mt(rems(0.4375))
                    .bottom(px(-1.))
                    .bg(cx.theme().sidebar_border),
            )
        })
        .on_click(move |_, window, cx| toggle(&!collapsed, window, cx))
        .on_key_down(move |ev, window, cx| {
            let fold = match ev.keystroke.key.as_str() {
                "left" => true,
                "right" => false,
                _ => return nav(ev, window, cx),
            };
            if ev.keystroke.modifiers.modified() {
                return;
            }
            cx.stop_propagation();
            if fold != collapsed {
                on_collapse(&fold, window, cx);
            }
        })
        .context_menu(context)
        .into_any_element();
        nest(row, nested, cx)
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/chat/sidebar/project_row.rs"]
mod tests;
