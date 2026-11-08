use aether_ui::theme::Corners;
use std::path::{Path, PathBuf};

use aether_core::{ChatLayout, home_dir};
use aether_git::{Status, StatusError};
use aether_ui::{row_style, theme};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme, Icon, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, ElementId, FocusHandle, Hsla, Role, SharedString, div,
    img, prelude::*, px, transparent_black,
};

use super::PopView;
use super::model::{
    branch_spoken, changes_spoken, choose_app_label, copy_label, disabled_label, git_trouble,
    home_short, line_totals, pop_rows, thousands, view_title,
};
use crate::app::ChatApp;
use crate::open_in;

pub(super) struct Where {
    pub(super) folder: Option<PathBuf>,
    pub(super) project: Option<PathBuf>,
    pub(super) folder_ok: bool,
}

pub(super) fn row(
    id: impl Into<ElementId>,
    focus: &FocusHandle,
    label: SharedString,
    off: Option<&str>,
    cx: &App,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let label = match off {
        Some(reason) => disabled_label(&label, reason).into(),
        None => label,
    };
    let base = h_flex()
        .id(id)
        .track_focus(focus)
        .tab_index(0)
        .role(Role::ListItem)
        .aria_label(label)
        .gap(px(8.))
        .items_center()
        .px(px(10.))
        .py(px(8.))
        .text_sm();
    match off {
        None => row_style(base, false, cx),
        Some(_) => {
            let ring = cx.theme().ring;
            base.border_1()
                .border_color(transparent_black())
                .focus_visible(move |s| s.border_color(ring))
                .rounded(cx.theme().radius)
                .opacity(0.5)
        }
    }
}

pub(super) fn text(
    title: impl Into<SharedString>,
    sub: Option<SharedString>,
    cx: &App,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    v_flex()
        .flex_1()
        .min_w_0()
        .child(div().truncate().child(title.into()))
        .children(sub.map(|s| div().truncate().text_xs().text_color(muted).child(s)))
        .into_any_element()
}

pub(super) fn icon(name: Lucide, cx: &App) -> Icon {
    Icon::new(name).small().text_color(cx.theme().muted_foreground)
}

pub(super) fn chip(label: &'static str, cx: &App) -> gpui_kit::Div {
    let theme = cx.theme();
    div()
        .flex_none()
        .px(px(6.))
        .corner(px(6.))
        .bg(theme.muted)
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(label)
}

pub(crate) fn diff_stat(status: Option<&Status>, cx: &App) -> gpui_kit::Div {
    let muted = cx.theme().muted_foreground;
    let Some(status) = status else { return div().text_color(muted).child("\u{2014}") };
    let (added, removed) = line_totals(&status.changes);
    let part = |n: u32, sign: &str, color: Hsla| {
        div().text_color(if n > 0 { color } else { muted }).child(format!("{sign}{}", thousands(n)))
    };
    let (add, rem) = (theme::success_text(cx), theme::danger_text(cx));
    h_flex().gap(px(4.)).child(part(added, "+", add)).child(part(removed, "\u{2212}", rem))
}

impl ChatApp {
    pub(super) fn pop_where(&self) -> Where {
        self.where_of(self.active)
    }

    pub(super) fn where_of(&self, ix: usize) -> Where {
        let chat = &self.chats[ix];
        let folder = chat.folder(&self.settings.workspace_root);
        let folder_ok = folder.as_ref().is_some_and(|f| !self.missing_folders.contains(f))
            && !self.worktree_missing(ix)
            && !chat.worktree_removed();
        Where { folder, project: self.chat_project(ix), folder_ok }
    }

    pub(super) fn pop_body(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let at = self.pop_where();
        let view = self.workspace_pop.view;
        let mut body = match view {
            PopView::ChatLayout => {
                let handles = self.workspace_pop.rows(4, cx).to_vec();
                [
                    (ChatLayout::Tabbed, "Tabbed", Lucide::PanelBottom),
                    (ChatLayout::Sidebar, "Sidebar", Lucide::PanelRight),
                    (ChatLayout::Split, "Split", Lucide::Columns2),
                ]
                .into_iter()
                .enumerate()
                .map(|(ix, (layout, label, mark))| {
                    row(("pop-layout-option", ix), &handles[ix + 1], label.into(), None, cx)
                        .child(icon(mark, cx).when(layout == ChatLayout::Tabbed, |icon| {
                            icon.rotate(gpui_kit::Radians(std::f32::consts::PI))
                        }))
                        .child(text(label, None, cx))
                        .when(self.settings.chat_layout == layout, |r| {
                            r.child(icon(Lucide::Check, cx))
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.close_workspace_pop(window, cx);
                            this.update_settings(cx, |s| s.chat_layout = layout);
                        }))
                        .into_any_element()
                })
                .collect()
            }
            PopView::Location => self.pop_location(at, cx),
            PopView::Apps => self.pop_apps(at, cx),
            PopView::Branch => self.pop_branch(at, cx),
            PopView::Publish => self.pop_publish(cx),
            PopView::Remote => self.pop_remote(cx),
            PopView::Script(_) => self.pop_script_form(cx),
            PopView::Main => return self.pop_main(at, cx),
        };
        if let Some(title) = view_title(&view) {
            body.insert(0, self.pop_header(title, cx));
        }
        body
    }

    fn pop_header(&self, title: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let back = div()
            .id("pop-back")
            .track_focus(&self.workspace_pop.rows[0])
            .tab_index(0)
            .role(Role::Button)
            .aria_label("Back")
            .flex_none()
            .size(px(28.))
            .flex()
            .items_center()
            .justify_center();
        h_flex()
            .gap(px(6.))
            .items_center()
            .pb(px(2.))
            .child(
                row_style(back, false, cx)
                    .child(icon(Lucide::ArrowLeft, cx))
                    .on_click(cx.listener(|this, _, window, cx| this.pop_back(window, cx))),
            )
            .child(
                div()
                    .id("pop-title")
                    .role(Role::Heading)
                    .aria_label(title)
                    .text_sm()
                    .font_medium()
                    .child(title),
            )
            .into_any_element()
    }

    fn pop_main(&mut self, at: Where, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let repo = self.git_repo().is_some();
        let state = pop_rows(at.folder_ok, repo);
        let machine = self.workspace_pop.machine.clone().unwrap_or_else(|| "This computer".into());
        let home = home_dir();
        let place = at.folder.as_deref().map(|f| home_short(f, &home));
        let key_path = at.project.clone().or_else(|| at.folder.clone());
        let app_ix = key_path
            .as_deref()
            .and_then(|p| open_in::project_app(&self.open_apps, &self.settings, p));
        let app = app_ix.and_then(|ix| self.open_apps.get(ix));
        let open_label: SharedString =
            app.map_or("Open in…".into(), |a| format!("Open in {}", a.name).into());
        let app_mark = match app.and_then(|a| a.icon.clone()) {
            Some(path) => img(path).size(px(16.)).flex_none().into_any_element(),
            None => icon(app.map_or(Lucide::Folder, |a| a.glyph()), cx).into_any_element(),
        };
        let no_folder = (!at.folder_ok).then_some("Folder not found");
        let no_apps = self.open_apps.is_empty().then_some("No apps found");
        let open_off = no_apps.or(no_folder).or(app_ix.is_none().then_some("No apps found"));
        let can_open = open_off.is_none();
        let read = self.git_loaded().map(|l| &l.status);
        let not_repo = at.folder_ok && matches!(read, Some(Err(StatusError::NotRepo)));
        let trouble = git_trouble(read);
        let scripts = self.pop_project_scripts(&at);
        let g = 4 + scripts.len();
        let h = self.workspace_pop.rows(g + if not_repo { 4 } else { 6 }, cx).to_vec();
        let script_rows = self.pop_script_rows(&at, &state, scripts, &h[3..g], cx);
        let folder = at.folder;

        let machine_row = row("pop-machine", &h[0], machine.clone(), None, cx)
            .child(icon(Lucide::Laptop, cx))
            .child(text(machine, place.map(Into::into), cx))
            .child(icon(Lucide::ChevronRight, cx))
            .on_click(
                cx.listener(|this, _, window, cx| this.pop_view(PopView::Location, window, cx)),
            );

        let open_folder = folder.clone();
        let open_row = row("pop-open-in", &h[1], open_label.clone(), open_off, cx)
            .flex_1()
            .child(app_mark)
            .child(text(open_label, open_off.map(Into::into), cx))
            .when(can_open, |r| {
                r.on_click(cx.listener(move |this, _, window, cx| {
                    if let (Some(folder), Some(ix)) = (open_folder.clone(), app_ix) {
                        this.close_workspace_pop(window, cx);
                        this.open_project(folder, ix, window, cx);
                    }
                }))
            });
        let caret_label = choose_app_label(app.map(|a| a.name)).into();
        let caret = row("pop-open-in-apps", &h[2], caret_label, no_apps, cx)
            .child(icon(Lucide::ChevronDown, cx))
            .when(!self.open_apps.is_empty(), |r| {
                r.on_click(
                    cx.listener(|this, _, window, cx| this.pop_view(PopView::Apps, window, cx)),
                )
            });

        let browse_ix = g + if not_repo { 1 } else { 2 };
        let browse = row("pop-browse", &h[browse_ix], "Browse files".into(), no_folder, cx)
            .child(icon(Lucide::Folder, cx))
            .child(text("Browse files", no_folder.map(Into::into), cx))
            .when(at.folder_ok, |r| {
                r.on_click(cx.listener(|this, _, window, cx| {
                    this.close_workspace_pop(window, cx);
                    this.toggle_explorer_page(window, cx);
                }))
            });
        let resume_off = self.resume_command_of(self.active).is_none().then(|| {
            if !at.folder_ok {
                "Folder not found"
            } else if self.chats[self.active].busy() {
                "Reply still running"
            } else {
                "No CLI session yet"
            }
        });
        let resume =
            row("pop-resume", &h[h.len() - 2], "Resume in terminal".into(), resume_off, cx)
                .child(icon(Lucide::SquareTerminal, cx))
                .child(text("Resume in terminal", resume_off.map(Into::into), cx))
                .when(resume_off.is_none(), |r| {
                    r.on_click(cx.listener(|this, _, window, cx| {
                        this.close_workspace_pop(window, cx);
                        this.resume_in_terminal(this.active, window, cx);
                    }))
                })
                .into_any_element();
        let layout = row("pop-chat-layout", &h[h.len() - 1], "Chat layout".into(), None, cx)
            .child(icon(Lucide::LayoutDashboard, cx))
            .child(text("Chat layout", None, cx))
            .child(icon(Lucide::ChevronRight, cx))
            .on_click(
                cx.listener(|this, _, window, cx| this.pop_view(PopView::ChatLayout, window, cx)),
            )
            .into_any_element();
        let top_rows = [
            machine_row.into_any_element(),
            h_flex().child(open_row).child(caret).into_any_element(),
        ]
        .into_iter()
        .chain(script_rows);
        if let (true, Some(folder)) = (not_repo, folder) {
            let init = self.pop_init_row(&h[g], folder, cx);
            return top_rows.chain([init, browse.into_any_element(), resume, layout]).collect();
        }

        let reading = "Reading git status…";
        let unread = if self.git_folder().is_some() { reading } else { "Not a git repository" };
        let changes_off = state.reason.or(trouble).or((!state.git).then_some(unread));
        let branch = match self.git_status() {
            Some(s) => s.branch.clone().unwrap_or_else(|| "detached".into()),
            None => "\u{2014}".into(),
        };
        let linked = !self.chats[self.active].worktree_branch_str().is_empty();
        let branch_off = changes_off.or(self.git_status().is_none().then_some(reading));
        let branch_label = branch_spoken(&branch, linked).into();
        let branch_row = row("pop-branch", &h[g], branch_label, branch_off, cx)
            .child(icon(Lucide::GitBranch, cx))
            .child(text(branch, branch_off.map(Into::into), cx))
            .when(linked, |r| r.child(chip("worktree", cx)))
            .child(icon(Lucide::ChevronRight, cx))
            .when(branch_off.is_none(), |r| {
                r.on_click(cx.listener(|this, _, window, cx| this.open_branch_view(window, cx)))
            });

        let remote_row = self.pop_remote_row(&h[g + 1], branch_off, cx);
        let changes_aria = changes_spoken(self.git_status().filter(|_| changes_off.is_none()));
        let changes_row = row("pop-changes", &h[g + 3], changes_aria.into(), changes_off, cx)
            .child(icon(Lucide::FileDiff, cx))
            .child(text("Changes", changes_off.map(Into::into), cx))
            .child(diff_stat(self.git_status(), cx).flex_none().text_xs())
            .when(changes_off.is_none(), |r| {
                r.on_click(cx.listener(|this, _, window, cx| {
                    this.close_workspace_pop(window, cx);
                    this.toggle_changes(true, window, cx);
                }))
            });

        top_rows
            .chain([
                branch_row.into_any_element(),
                remote_row,
                browse.into_any_element(),
                changes_row.into_any_element(),
                resume,
                layout,
            ])
            .collect()
    }

    fn pop_location(&mut self, at: Where, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let home = home_dir();
        let mut paths: Vec<(&'static str, PathBuf)> = Vec::new();
        if let Some(folder) = at.folder.clone() {
            let project = at.project.clone().unwrap_or_else(|| folder.clone());
            if folder != project {
                paths.push(("Worktree", folder));
            }
            paths.push(("Project root", project));
        }
        let h = self.workspace_pop.rows(paths.len() + 3, cx)[1..].to_vec();
        let mut out: Vec<AnyElement> = paths
            .into_iter()
            .enumerate()
            .map(|(i, (title, path))| {
                let short = home_short(&path, &home);
                row(("pop-path", i), &h[i], copy_label(title, &short).into(), None, cx)
                    .child(text(title, Some(short.into()), cx))
                    .child(icon(Lucide::Copy, cx))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            path.display().to_string(),
                        ));
                    }))
                    .into_any_element()
            })
            .collect();
        let n = out.len();
        let target = at.folder.filter(|_| at.folder_ok);
        let reveal_to: Option<PathBuf> = target.clone();
        let off = target.is_none().then_some("Folder not found");
        out.push(
            row("pop-reveal", &h[n], "Reveal in Finder".into(), off, cx)
                .child(icon(Lucide::Folder, cx))
                .child(text("Reveal in Finder", off.map(Into::into), cx))
                .when_some(reveal_to, |r, path| {
                    r.on_click(cx.listener(move |this, _, window, cx| {
                        cx.reveal_path(&path);
                        this.close_workspace_pop(window, cx);
                    }))
                })
                .into_any_element(),
        );
        out.push(
            row("pop-terminal", &h[n + 1], "Open terminal here".into(), off, cx)
                .child(icon(Lucide::SquareTerminal, cx))
                .child(text("Open terminal here", off.map(Into::into), cx))
                .when(target.is_some(), |r| {
                    r.on_click(cx.listener(|this, _, window, cx| {
                        this.close_workspace_pop(window, cx);
                        this.open_terminal(window, cx);
                    }))
                })
                .into_any_element(),
        );
        out
    }

    fn pop_apps(&mut self, at: Where, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let key_path: Option<&Path> = at.project.as_deref().or(at.folder.as_deref());
        let current =
            key_path.and_then(|p| open_in::project_app(&self.open_apps, &self.settings, p));
        let h = self.workspace_pop.rows(self.open_apps.len().max(1) + 1, cx)[1..].to_vec();
        let off = key_path.is_none().then_some("Folder not found");
        if self.open_apps.is_empty() {
            return vec![
                row("pop-no-apps", &h[0], "No apps found".into(), Some("No apps found"), cx)
                    .child(text("No apps found", None, cx))
                    .into_any_element(),
            ];
        }
        self.open_apps
            .iter()
            .enumerate()
            .map(|(i, app)| {
                let mark = match app.icon.clone() {
                    Some(path) => img(path).size(px(16.)).flex_none().into_any_element(),
                    None => icon(app.glyph(), cx).into_any_element(),
                };
                let (id, project) = (app.id, key_path.map(Path::to_path_buf));
                row(("pop-app", i), &h[i], app.name.into(), off, cx)
                    .child(mark)
                    .child(text(app.name, off.map(Into::into), cx))
                    .when(current == Some(i), |r| r.child(icon(Lucide::Check, cx)))
                    .when_some(project, |r, project| {
                        r.on_click(cx.listener(move |this, _, window, cx| {
                            this.update_settings(cx, |s| open_in::pick_app(s, &project, id));
                            this.pop_view(PopView::Main, window, cx);
                        }))
                    })
                    .into_any_element()
            })
            .collect()
    }
}
