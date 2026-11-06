use std::path::Path;

use aether_core::{Attachment, MAX_ATTACHMENTS, ProjectScript};
use aether_terminal::view::TerminalEvent;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{ActiveTheme, Icon, Sizable, h_flex};
use gpui_kit::{AnyElement, Context, Window, div, prelude::*, px};

use crate::app::{Chat, ChatApp, project_dir};
use crate::open_in::project_key;

pub(crate) struct SetupJob {
    pub(crate) queue: Vec<ProjectScript>,
    pub(crate) current: usize,
    pub(crate) text: String,
    pub(crate) attachments: Vec<Attachment>,
    pub(crate) failed: Option<SetupFailure>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SetupFailure {
    Code(i32),
    Stopped,
    NoStart,
    Blocked(&'static str),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SetupStep {
    Next(usize),
    Send,
    Failed(SetupFailure),
}

pub(crate) fn setup_queue(scripts: &[ProjectScript]) -> Vec<ProjectScript> {
    scripts.iter().filter(|s| s.on_new_worktree).cloned().collect()
}

pub(crate) fn after_exit(job: &SetupJob, code: Option<i32>) -> SetupStep {
    match code {
        Some(0) if job.current + 1 < job.queue.len() => SetupStep::Next(job.current + 1),
        Some(0) => SetupStep::Send,
        Some(code) => SetupStep::Failed(SetupFailure::Code(code)),
        None => SetupStep::Failed(SetupFailure::Stopped),
    }
}

pub(crate) fn setup_step(job: &SetupJob, ended: &TerminalEvent) -> Option<SetupStep> {
    let TerminalEvent::CommandEnded { key, code, stopped, failed_to_start } = ended else {
        return None;
    };
    if job.failed.is_some() || job.queue[job.current].name != key.as_ref() {
        return None;
    }
    Some(if *stopped {
        SetupStep::Failed(SetupFailure::Stopped)
    } else if *failed_to_start {
        SetupStep::Failed(SetupFailure::NoStart)
    } else {
        after_exit(job, *code)
    })
}

fn setup_reason(name: &str, why: SetupFailure) -> String {
    match why {
        SetupFailure::Code(code) => format!("{name} exited with code {code}"),
        SetupFailure::Stopped => format!("{name} stopped"),
        SetupFailure::NoStart => format!("{name} couldn't start"),
        SetupFailure::Blocked(why) => why.into(),
    }
}

pub(crate) fn setup_banner(job: &SetupJob) -> String {
    let name = &job.queue[job.current].name;
    match job.failed {
        None => format!("Setting up worktree… ({name})"),
        Some(why @ SetupFailure::Blocked(_)) => format!("Not sent: {}", setup_reason(name, why)),
        Some(why) => format!("Setup failed: {}", setup_reason(name, why)),
    }
}

pub(crate) fn rejoin(held: &str, current: &str) -> String {
    if current.trim().is_empty() { held.into() } else { format!("{held}\n\n{current}") }
}

pub(crate) fn restored(held: Vec<Attachment>, current: Vec<Attachment>) -> Vec<Attachment> {
    held.into_iter().chain(current).collect()
}

pub(crate) fn setup_owed(chat: &Chat) -> bool {
    chat.fresh_worktree.as_deref() == Some(Path::new(&chat.worktree_path))
}

impl ChatApp {
    pub(crate) fn begin_setup_or_submit(
        &mut self,
        ix: usize,
        text: String,
        attachments: Vec<Attachment>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chat = &self.chats[ix];
        let queue = if setup_owed(chat) {
            let key = project_key(&project_dir(chat, &self.settings.workspace_root));
            setup_queue(self.settings.project_scripts.get(&key).map_or(&[][..], Vec::as_slice))
        } else {
            Vec::new()
        };
        if queue.is_empty() {
            self.chats[ix].fresh_worktree = None;
            return self.submit(ix, text, attachments, window, cx);
        }
        self.chats[ix].setup =
            Some(SetupJob { queue, current: 0, text, attachments, failed: None });
        self.run_setup(ix, window, cx);
    }

    fn run_setup(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(job) = &self.chats[ix].setup else { return };
        let script = job.queue[job.current].clone();
        if !self.run_script(ix, &script, window, cx) {
            self.setup_failed(ix, SetupFailure::NoStart, window, cx);
        }
        cx.notify();
    }

    fn setup_failed(
        &mut self,
        ix: usize,
        why: SetupFailure,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(job) = &mut self.chats[ix].setup else { return };
        job.failed = Some(why);
        let reason = setup_reason(&job.queue[job.current].name, why);
        let body = format!("Its worktree setup failed: {reason}");
        self.deliver_failure_to_lead(ix, &body, window, cx);
    }

    pub(crate) fn setup_ended(
        &mut self,
        id: i64,
        ended: &TerminalEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.chat_ix(id) else { return };
        let Some(job) = self.chats[ix].setup.as_mut() else { return };
        let Some(step) = setup_step(job, ended) else { return };
        match step {
            SetupStep::Next(next) => {
                job.current = next;
                self.run_setup(ix, window, cx);
            }
            SetupStep::Send => self.setup_send_anyway(ix, window, cx),
            SetupStep::Failed(why) => self.setup_failed(ix, why, window, cx),
        }
        cx.notify();
    }

    pub(crate) fn setup_drawer_gone(&mut self, id: i64) -> bool {
        let job = self.chats.iter_mut().find(|c| c.id == id).and_then(|c| c.setup.as_mut());
        let Some(job) = job.filter(|j| j.failed.is_none()) else { return false };
        job.failed = Some(SetupFailure::Stopped);
        true
    }

    pub(crate) fn setup_retry(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(job) = &mut self.chats[ix].setup {
            job.failed = None;
            self.run_setup(ix, window, cx);
        }
    }

    pub(crate) fn setup_send_anyway(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(job) = self.chats[ix].setup.take() else { return };
        if let Some(why) = self.worktree_blocks_send(ix) {
            let failed = Some(SetupFailure::Blocked(why));
            self.chats[ix].setup = Some(SetupJob { failed, ..job });
            return cx.notify();
        }
        self.chats[ix].fresh_worktree = None;
        self.submit(ix, job.text, job.attachments, window, cx);
    }

    /// Setup stays owed, so the next send runs it again.
    pub(crate) fn setup_cancel(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(job) = self.chats[ix].setup.take() else { return };
        if job.failed.is_none()
            && let Some(folder) = self.folder_of(&self.chats[ix])
        {
            self.stop_script(&folder, &job.queue[job.current].name, cx);
        }
        if ix == self.active {
            let text = rejoin(&job.text, &self.composer.read(cx).value());
            self.composer.update(cx, |s, cx| {
                s.set_value(text, window, cx);
                s.focus(window, cx);
            });
        } else {
            self.chats[ix].draft = rejoin(&job.text, &self.chats[ix].draft);
        }
        let chat = &mut self.chats[ix];
        chat.pending = restored(job.attachments, std::mem::take(&mut chat.pending));
        if ix == self.active && chat.pending.len() > MAX_ATTACHMENTS {
            self.attach_error = Some(format!("Up to {MAX_ATTACHMENTS} attachments").into());
        }
        cx.notify();
    }

    pub(in crate::app) fn setup_note(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use gpui_kit::assets::IconName as Lucide;
        let ix = self.active;
        let job = self.chats[ix].setup.as_ref()?;
        let failed = job.failed.is_some();
        let rerun = failed && !matches!(job.failed, Some(SetupFailure::Blocked(_)));
        let name = job.queue[job.current].name.clone();
        let button = |id: &'static str, label: &'static str| {
            aether_ui::button(id).ghost().small().label(label)
        };
        let danger = aether_ui::theme::danger_text(cx);
        Some(
            h_flex()
                .px_2()
                .gap_2()
                .items_center()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(
                    Icon::new(if failed { Lucide::CircleAlert } else { Lucide::GitFork })
                        .size(px(14.))
                        .flex_none()
                        .when(failed, |icon| icon.text_color(danger)),
                )
                .child(div().flex_1().min_w_0().child(setup_banner(job)))
                .when(rerun, |row| {
                    row.child(button("setup-retry", "Retry").on_click(
                        cx.listener(move |this, _, window, cx| this.setup_retry(ix, window, cx)),
                    ))
                })
                .when(failed, |row| {
                    row.child(button("setup-send-anyway", "Send anyway").on_click(cx.listener(
                        move |this, _, window, cx| this.setup_send_anyway(ix, window, cx),
                    )))
                })
                .when(rerun, |row| {
                    row.child(button("setup-show-output", "Show output").on_click(
                        cx.listener(move |this, _, window, cx| {
                            this.show_command(&name, window, cx)
                        }),
                    ))
                })
                .child(button("setup-cancel", "Cancel").on_click(
                    cx.listener(move |this, _, window, cx| this.setup_cancel(ix, window, cx)),
                ))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/worktree/setup.rs"]
mod tests;
