use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, StyledExt,
    button::ButtonVariants,
    h_flex,
    menu::{ContextMenuExt, DropdownMenu, PopupMenu},
    v_flex,
};
use gpui_kit::{
    Anchor, AnyElement, App, FocusHandle, Hsla, KeyDownEvent, Role, SharedString, Window, div,
    prelude::*, px, relative, rems,
};

use super::chat_row::status_mark;
use super::chrome::{nest, reveal_on_hover, row_style, swallow_click, tabular, tooltip};
use super::crew::Bucket;
use super::menu::project_menu;
use super::{
    ChatStatus, CollapseHandler, LONG_BRANCH, NEST, ProjectHeader, RowHandler, middle_truncate,
};
use crate::theme::{danger_text, mango_text, success_text};

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

const LONG_PATH: usize = 32;

/// `path` with the home folder shortened to `~`, for the no-git line.
pub fn tilde_path(path: &str) -> String {
    let home = aether_core::home_dir();
    match std::path::Path::new(path).strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.to_string(),
    }
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
    pub(super) active: bool,
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
            active,
            on_collapse,
            on_new,
            on_files,
            on_hide,
            on_page,
            now,
        } = self;
        let ProjectHeader {
            name,
            path,
            count,
            collapsed,
            status,
            missing,
            branch,
            no_git,
            added,
            deleted,
            ahead,
            needs_you,
            failed,
        } = header;
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
        let chip = |n: usize, mark: AnyElement, color: Hsla| {
            (n > 0).then(|| {
                h_flex().gap_1().items_center().text_color(color).child(mark).child(n.to_string())
            })
        };
        let chips = h_flex()
            .absolute()
            .right_0()
            .gap_2()
            .items_center()
            .text_xs()
            .font_features(tabular())
            .when(missing, |el| el.text_color(muted).child("Missing"))
            .children(chip(needs_you, status_mark(ChatStatus::Waiting, false, cx), mango_text(cx)))
            .children(chip(failed, status_mark(ChatStatus::Failed, false, cx), danger_text(cx)))
            .when(collapsed && Bucket::of(status) == Bucket::Working, |el| {
                el.child(status.badge(("project-status", id), Some(now), cx))
            })
            .map(|el| {
                if show_actions {
                    el.opacity(0.)
                } else {
                    el.group_hover(group.clone(), |s| s.opacity(0.))
                }
            });
        let diff = added > 0 || deleted > 0;
        let no_git = (branch.is_none() && no_git).then(|| {
            h_flex()
                .ml(-(rems(0.8125) + rems(0.5)))
                .h(px(15.))
                .gap_1()
                .items_center()
                .text_size(px(10.5))
                .text_color(muted.opacity(0.7))
                .child(
                    div().size(px(6.)).mx(px(3.)).rounded_full().bg(muted.opacity(0.7)).flex_none(),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(middle_truncate(&tilde_path(&path), LONG_PATH)),
                )
        });
        let git = branch.map(|branch| {
            h_flex()
                .ml(-(rems(0.8125) + rems(0.5)))
                .flex_wrap()
                .h(px(15.))
                .overflow_hidden()
                .gap_x(px(7.))
                .items_center()
                .text_size(px(10.5))
                .text_color(muted)
                .font_features(tabular())
                .child(
                    h_flex()
                        .min_w_0()
                        .max_w_full()
                        .gap_1()
                        .items_center()
                        .child(Icon::new(Lucide::GitBranch).size(px(12.)).flex_none())
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(middle_truncate(&branch, LONG_BRANCH)),
                        ),
                )
                .when(diff, |el| {
                    el.child(
                        h_flex()
                            .flex_none()
                            .gap(px(6.))
                            .child(div().text_color(success_text(cx)).child(format!("+{added}")))
                            .child(div().text_color(danger_text(cx)).child(format!("−{deleted}"))),
                    )
                })
                .when(ahead > 0, |el| el.child(div().flex_none().child(format!("↑{ahead}"))))
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
            active,
            cx,
        )
        .when(active, |row| row.bg(cx.theme().sidebar_accent))
        .group(group)
        .relative()
        .w_full()
        .when(spaced, |row| row.mt_0p5())
        .items_start()
        .gap_2()
        .px_2()
        .py_1()
        .text_sm()
        .text_color(muted)
        .child(
            h_flex()
                .flex_none()
                .h(rems(1.25))
                .gap_2()
                .items_center()
                .child(
                    Icon::new(project_chevron_icon(collapsed)).size(rems(0.6875)).text_color(muted),
                )
                .child(
                    Icon::new(project_folder_icon(collapsed)).size(rems(0.8125)).text_color(color),
                ),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    h_flex().h(rems(1.25)).gap_2().items_center().child(
                        div()
                            .id(("project-name", id))
                            .text_color(if missing { muted } else { cx.theme().sidebar_foreground })
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_semibold()
                            .tooltip(tooltip(name_tip))
                            .child(name),
                    ),
                )
                .children(git)
                .children(no_git),
        )
        .child(
            div()
                .relative()
                .flex_none()
                .h(rems(1.25))
                .flex()
                .items_center()
                .child(chips)
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
