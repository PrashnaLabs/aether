use std::sync::atomic::Ordering;

use gpui_kit::component::Disableable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{ActiveTheme, IconName, h_flex, v_flex};
use gpui_kit::{AnyElement, Context, div, prelude::*, px};

use super::worktree_preview_branch;
use crate::app::sidebar::folder_label;
use crate::app::{Chat, ChatApp, WorktreeJob, project_dir};

impl ChatApp {
    /// Above the composer: worktree progress, a failed create, a note, or a removed or missing worktree.
    pub(in crate::app) fn worktree_note(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use gpui_kit::assets::IconName as Lucide;
        let ix = self.active;
        let chat = &self.chats[ix];
        let muted = cx.theme().muted_foreground;
        let row = || h_flex().px_2().gap_2().items_center().text_xs().text_color(muted);
        let icon = |name: Lucide| gpui_kit::component::Icon::new(name).size(px(14.)).flex_none();
        let dismiss = |id: &'static str, cx: &mut Context<Self>, clear: fn(&mut Chat)| {
            aether_ui::button(id)
                .ghost()
                .xsmall()
                .icon(IconName::Close)
                .accessibility_label("Dismiss")
                .tooltip("Dismiss")
                .on_click(cx.listener(move |this, _, _, cx| {
                    clear(&mut this.chats[this.active]);
                    cx.notify();
                }))
        };
        if let Some(job) = &chat.worktree_job {
            let (text, cancelable) = match job {
                WorktreeJob::Creating(_, cancel) if cancel.load(Ordering::Relaxed) => {
                    ("Canceling…".to_string(), false)
                }
                WorktreeJob::Creating(base, _) => {
                    (format!("Creating a worktree from {base}…"), true)
                }
                WorktreeJob::Checking => ("Checking the worktree…".into(), false),
                WorktreeJob::Removing => ("Removing the worktree…".into(), false),
                WorktreeJob::Switching => ("Switching branches…".into(), false),
            };
            return Some(
                row()
                    .child(icon(Lucide::GitFork))
                    .child(div().flex_1().min_w_0().child(text))
                    .when(cancelable, |row| {
                        row.child(
                            aether_ui::button("worktree-cancel")
                                .ghost()
                                .xsmall()
                                .label("Cancel")
                                .on_click(cx.listener(|this, _, _, cx| this.cancel_worktree(cx))),
                        )
                    })
                    .into_any_element(),
            );
        }
        if let Some(error) = chat.worktree_error.clone() {
            return Some(
                h_flex()
                    .px_2()
                    .gap_2()
                    .items_start()
                    .text_xs()
                    .text_color(muted)
                    .child(
                        icon(Lucide::CircleAlert)
                            .mt(px(1.))
                            .text_color(aether_ui::theme::danger_text(cx)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_color(cx.theme().foreground).child(
                                "Couldn't start in a worktree, so your message wasn't sent.",
                            ))
                            .child(error),
                    )
                    .child(
                        aether_ui::button("worktree-local")
                            .ghost()
                            .xsmall()
                            .label("Switch to Local")
                            .on_click(cx.listener(|this, _, _, cx| this.choose_worktree(None, cx))),
                    )
                    .child(dismiss("worktree-dismiss", cx, |chat| chat.worktree_error = None))
                    .into_any_element(),
            );
        }
        if let Some(info) = chat.worktree_info.clone() {
            return Some(
                row()
                    .child(icon(Lucide::Info))
                    .child(div().flex_1().min_w_0().child(info))
                    .child(dismiss("worktree-info-dismiss", cx, |chat| chat.worktree_info = None))
                    .into_any_element(),
            );
        }
        let root = &self.settings.workspace_root;
        let project = project_dir(chat, root);
        if let Some(base) = chat.new_worktree.clone() {
            let custom = self.worktree_name_input.read(cx).value();
            let preview = worktree_preview_branch(&custom, &self.composer.read(cx).value());
            return Some(
                row()
                    .child(icon(Lucide::GitFork))
                    .child(div().flex_none().child(format!("From {base}")))
                    .child(icon(Lucide::ArrowRight))
                    .child(div().flex_none().text_color(muted).child(preview))
                    .into_any_element(),
            );
        }
        if self.worktree_missing(ix) {
            return Some(
                row()
                    .child(icon(Lucide::FolderX))
                    .child(div().flex_1().min_w_0().child(format!(
                        "Worktree folder is missing. Branch {} is kept.",
                        chat.worktree_branch
                    )))
                    .child(
                        aether_ui::button("worktree-clean-up")
                            .ghost()
                            .xsmall()
                            .label("Clean up")
                            .tooltip("Clears git's record of the missing folder")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.remove_worktree(ix, window, cx)
                            })),
                    )
                    .into_any_element(),
            );
        }
        if !chat.worktree_removed() {
            return None;
        }
        Some(
            row()
                .child(icon(Lucide::FolderX))
                .child(
                    div().flex_1().min_w_0().child(format!(
                        "Worktree removed. Branch {} is kept.",
                        chat.worktree_branch
                    )),
                )
                .child(
                    aether_ui::button("continue-in-project")
                        .ghost()
                        .xsmall()
                        .label(format!(
                            "Continue in {}",
                            folder_label(&project.display().to_string())
                        ))
                        .tooltip("Copies this chat into the project folder")
                        .disabled(self.missing_folders.contains(&project))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.continue_in_project(ix, window, cx)
                        })),
                )
                .into_any_element(),
        )
    }
}
