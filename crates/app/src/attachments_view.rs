use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use aether_core::{attachments_dir, is_image, mime_for};
use aether_ui::theme::Corners;
use aether_ui::{AttachmentView, attachment_strip};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{ButtonVariant, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable, checkbox::Checkbox, h_flex, v_flex};
use gpui_kit::{AnyElement, App, Context, SharedString, Window, div, prelude::*, px};

use crate::app::{ChatApp, project_dir};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum ProjectFilter {
    #[default]
    All,
    Project(PathBuf),
    RemovedChats,
}

#[derive(Clone, Debug)]
pub(crate) struct StoredFile {
    pub chat: i64,
    pub path: PathBuf,
    pub size: u64,
    pub modified: std::time::SystemTime,
}

/// Every file under the attachments folder, newest first.
pub(crate) fn scan(root: &Path) -> Vec<StoredFile> {
    let mut files: Vec<StoredFile> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|dir| {
            let chat = dir.file_name().to_str()?.parse::<i64>().ok()?;
            Some(std::fs::read_dir(dir.path()).into_iter().flatten().flatten().filter_map(
                move |f| {
                    let meta = f.metadata().ok().filter(|m| m.is_file())?;
                    Some(StoredFile {
                        chat,
                        path: f.path(),
                        size: meta.len(),
                        modified: meta.modified().ok()?,
                    })
                },
            ))
        })
        .flatten()
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.modified));
    files
}

/// Stored name is `{n}-{name}`.
fn shown_name(path: &Path) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    name.split_once('-').map_or(name, |(_, rest)| rest).to_string()
}

impl ChatApp {
    fn confirm_delete_stored_files(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        let title = if paths.len() == 1 {
            format!("Delete \"{}\"?", shown_name(&paths[0]))
        } else {
            format!("Delete {} attachments?", paths.len())
        };
        let app = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let app = app.clone();
            let paths = paths.clone();
            alert.confirm()
                .title(SharedString::from(title.clone()))
                .description("This permanently deletes the saved files from the config folder. Messages using them will show File missing.")
                .ok_text("Delete")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _, cx| {
                    app.update(cx, |app, cx| {
                        app.stored_files_error = None;
                        for path in &paths {
                            app.delete_stored_file(path.clone(), cx);
                        }
                    });
                    true
                })
        });
    }

    pub(crate) fn load_stored_files(&mut self, cx: &mut Context<Self>) {
        let Some(root) = attachments_dir() else { return };
        cx.spawn(async move |this, cx| {
            let files = cx.background_executor().spawn(async move { scan(&root) }).await;
            this.update(cx, |this, cx| {
                this.stored_file_picks.retain(|path| files.iter().any(|f| &f.path == path));
                this.stored_files = files;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn delete_stored_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if let Err(error) = std::fs::remove_file(&path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            self.stored_files_error =
                Some(format!("Couldn't delete {}: {error}", shown_name(&path)).into());
            cx.notify();
            return;
        }
        self.stored_files.retain(|f| f.path != path);
        self.stored_file_picks.remove(&path);
        self.missing_attachments.insert(path);
        cx.notify();
    }

    fn stored_file_project(&self, file: &StoredFile) -> Option<PathBuf> {
        self.chats
            .iter()
            .find(|chat| chat.id == file.chat)
            .map(|chat| project_dir(chat, &self.settings.workspace_root))
    }

    fn visible_stored_files(&self, project: Option<&Path>) -> Vec<&StoredFile> {
        self.stored_files
            .iter()
            .filter(|file| {
                let dir = self.stored_file_project(file);
                if let Some(project) = project {
                    return dir.as_deref() == Some(project);
                }
                match &self.stored_files_project_filter {
                    ProjectFilter::All => true,
                    ProjectFilter::Project(project) => dir.as_ref() == Some(project),
                    ProjectFilter::RemovedChats => dir.is_none(),
                }
            })
            .collect()
    }

    fn stored_files_filter(&self, cx: &mut Context<Self>) -> AnyElement {
        let projects: BTreeSet<PathBuf> =
            self.stored_files.iter().filter_map(|file| self.stored_file_project(file)).collect();
        let mut options = vec![(ProjectFilter::All, SharedString::from("All projects"))];
        options.extend(projects.into_iter().map(|dir| {
            let name = self.project_name(&dir);
            (ProjectFilter::Project(dir), name)
        }));
        if self.stored_files.iter().any(|file| self.stored_file_project(file).is_none()) {
            options.push((ProjectFilter::RemovedChats, "Removed chats".into()));
        }
        let current = self.stored_files_project_filter.clone();
        let label = match &current {
            ProjectFilter::All => "All projects".into(),
            ProjectFilter::Project(dir) => self.project_name(dir),
            ProjectFilter::RemovedChats => "Removed chats".into(),
        };
        let app = cx.entity();
        aether_ui::button("attachments-project-filter")
            .debug_selector(|| "attachments-project-filter".into())
            .ghost()
            .small()
            .label(label)
            .dropdown_caret(true)
            .accessibility_label("Attachments project filter")
            .dropdown_menu(move |mut menu, _, _| {
                for (value, label) in &options {
                    let app = app.clone();
                    let value = value.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label.clone()).checked(current == value).on_click(
                            move |_, _, cx| {
                                app.update(cx, |app, cx| {
                                    app.stored_files_project_filter = value.clone();
                                    cx.notify();
                                });
                            },
                        ),
                    );
                }
                menu
            })
            .into_any_element()
    }

    /// The settings gallery is flat; project dashboards have a fixed scope.
    pub(crate) fn render_stored_files(
        &self,
        project: Option<&Path>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let files = self.visible_stored_files(project);
        let paths: Vec<PathBuf> = files.iter().map(|f| f.path.clone()).collect();
        let selected: Vec<PathBuf> =
            paths.iter().filter(|path| self.stored_file_picks.contains(*path)).cloned().collect();
        let count = selected.len();
        let clear_paths = paths.clone();
        let toolbar = (count > 0).then(|| {
            h_flex()
                .id("attachments-selection-bar")
                .debug_selector(|| "attachments-selection-bar".into())
                .role(gpui_kit::Role::Toolbar)
                .aria_label("Selected attachments")
                .w_full()
                .gap(px(6.))
                .items_center()
                .flex_wrap()
                .p(px(6.))
                .corner(px(9.))
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .child(
                    div()
                        .px(px(4.))
                        .text_sm()
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .child(format!("{count} selected")),
                )
                .when(count < paths.len(), |bar| {
                    bar.child(
                        aether_ui::button("attachments-select-all")
                            .debug_selector(|| "attachments-select-all".into())
                            .ghost()
                            .small()
                            .label("Select all")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.stored_file_picks.extend(paths.iter().cloned());
                                cx.notify();
                            })),
                    )
                })
                .child(div().flex_1())
                .child(
                    aether_ui::button("attachments-delete-selected")
                        .debug_selector(|| "attachments-delete-selected".into())
                        .danger()
                        .small()
                        .h(px(28.))
                        .icon(IconName::Trash)
                        .label("Delete")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.confirm_delete_stored_files(selected.clone(), window, cx);
                        })),
                )
                .child(
                    aether_ui::button("attachments-clear-selection")
                        .debug_selector(|| "attachments-clear-selection".into())
                        .ghost()
                        .small()
                        .icon(IconName::X)
                        .accessibility_label("Clear selection")
                        .tooltip("Clear selection")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.stored_file_picks.retain(|path| !clear_paths.contains(path));
                            cx.notify();
                        })),
                )
        });
        let app = cx.weak_entity();
        let views: Vec<AttachmentView> = files
            .iter()
            .map(|f| AttachmentView {
                name: shown_name(&f.path).into(),
                path: f.path.clone(),
                image: is_image(mime_for(&f.path)),
                size: f.size,
                missing: false,
            })
            .collect();
        let open_app = app.clone();
        let open: Rc<dyn Fn(&Path, &mut Window, &mut App)> = Rc::new(move |path, window, cx| {
            open_app.update(cx, |app, cx| app.preview_image(path, window, cx)).ok();
        });
        v_flex()
            .gap(px(18.))
            .children(
                project
                    .is_none()
                    .then(|| h_flex().items_center().child(self.stored_files_filter(cx))),
            )
            .children(toolbar)
            .children(
                self.stored_files_error
                    .clone()
                    .map(|error| div().text_sm().text_color(cx.theme().danger).child(error)),
            )
            .when(files.is_empty(), |gallery| {
                gallery.child(
                    div()
                        .p(px(30.))
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("No saved attachments."),
                )
            })
            .child(
                h_flex()
                    .id("stored-files-grid")
                    .debug_selector(|| "stored-files-grid".into())
                    .flex_wrap()
                    .gap(px(8.))
                    .children(views.into_iter().map(|view| {
                        let path = view.path.clone();
                        let picked = self.stored_file_picks.contains(&path);
                        let tile_group =
                            SharedString::from(format!("stored-file-{}", path.display()));
                        let remove_app = app.clone();
                        let remove_path = path.clone();
                        let remove: Rc<dyn Fn(usize, &mut Window, &mut App)> =
                            Rc::new(move |_, window, cx| {
                                remove_app
                                    .update(cx, |app, cx| {
                                        app.confirm_delete_stored_files(
                                            vec![remove_path.clone()],
                                            window,
                                            cx,
                                        );
                                    })
                                    .ok();
                            });
                        v_flex()
                            .id(SharedString::from(path.to_string_lossy().into_owned()))
                            .group(tile_group.clone())
                            .debug_selector(|| {
                                format!(
                                    "stored-file-{}",
                                    path.file_name().unwrap().to_string_lossy()
                                )
                            })
                            .flex_none()
                            .relative()
                            .w(px(if view.image { 102. } else { 226. }))
                            .p(px(2.))
                            .corner(px(12.))
                            .border_1()
                            .border_color(if picked { cx.theme().ring } else { cx.theme().border })
                            .child(attachment_strip(
                                vec![view.clone()],
                                px(96.),
                                Some(open.clone()),
                                Some(remove),
                                false,
                                cx,
                            ))
                            .child(
                                div()
                                    .id("selection-slot")
                                    .debug_selector(|| {
                                        format!(
                                            "stored-file-select-{}",
                                            path.file_name().unwrap().to_string_lossy()
                                        )
                                    })
                                    .absolute()
                                    .top(px(6.))
                                    .left(px(6.))
                                    .p(px(2.))
                                    .corner(px(4.))
                                    .bg(cx.theme().background.opacity(0.9))
                                    .when(count == 0, |slot| {
                                        slot.opacity(0.).group_hover(tile_group, |s| s.opacity(1.))
                                    })
                                    .on_click(|_, _, cx| cx.stop_propagation())
                                    .child(
                                        Checkbox::new("select")
                                            .checked(picked)
                                            .tab_stop(count > 0)
                                            .aria_label(SharedString::from(format!(
                                                "Select {}",
                                                view.name
                                            )))
                                            .on_click(cx.listener(
                                                move |this, checked: &bool, _, cx| {
                                                    if *checked {
                                                        this.stored_file_picks.insert(path.clone());
                                                    } else {
                                                        this.stored_file_picks.remove(&path);
                                                    }
                                                    cx.notify();
                                                },
                                            )),
                                    ),
                            )
                    })),
            )
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "../tests/unit/attachments_view.rs"]
mod tests;
