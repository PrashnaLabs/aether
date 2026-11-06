use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use aether_voice::{Heard, MODEL_BYTES, Recording, VoiceError};
use futures::StreamExt;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme, Disableable, IconName, Sizable, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, AppContext, Context, SharedString, Task, Window, div};

use crate::app::ChatApp;

const MAX_RECORDING: Duration = Duration::from_secs(600);
const MUTED: &str = if cfg!(target_os = "macos") {
    "Aether can't hear your microphone. Check System Settings > Privacy & Security > Microphone."
} else {
    "Aether can't hear your microphone. Check your sound settings."
};
const SILENCE: &str = "Didn't catch anything. Try again a little closer to the mic.";

#[derive(Default)]
pub(crate) enum VoiceState {
    #[default]
    Idle,
    NeedsModel {
        chat: i64,
    },
    Downloading {
        chat: i64,
        done: u64,
        cancel: Arc<AtomicBool>,
    },
    Opening {
        chat: i64,
        started: Instant,
    },
    Recording {
        chat: i64,
        recording: Recording,
        started: Instant,
    },
    Transcribing {
        chat: i64,
    },
}

pub(crate) fn dictated(draft: &str, cursor: usize, text: &str) -> String {
    let lead = draft[..cursor].chars().next_back().is_some_and(|c| !c.is_whitespace());
    let trail = draft[cursor..].chars().next().is_some_and(char::is_alphanumeric);
    format!("{}{text}{}", if lead { " " } else { "" }, if trail { " " } else { "" })
}

pub(crate) fn no_speech_copy(heard: Heard) -> Option<&'static str> {
    match heard {
        Heard::Muted => Some(MUTED),
        Heard::Silence => Some(SILENCE),
        Heard::Speech => None,
    }
}

pub(crate) fn model_status(installed: bool) -> &'static str {
    if installed {
        "Installed. Takes 141 MB and runs on this device. English only."
    } else {
        "Not installed. A 141 MB download that runs on this device. English only."
    }
}

pub(crate) fn voice_copy(state: &VoiceState, seconds: u64) -> Option<String> {
    match state {
        VoiceState::Idle => None,
        VoiceState::NeedsModel { .. } => {
            Some("Dictation needs a speech model (141 MB). It runs on this device.".into())
        }
        VoiceState::Downloading { done, .. } => Some(format!(
            "Downloading speech model, {}%",
            (*done as f64 * 100. / MODEL_BYTES as f64).round()
        )),
        VoiceState::Opening { .. } => Some("Starting microphone…".into()),
        VoiceState::Recording { .. } => {
            Some(format!("Listening {}:{:02}", seconds / 60, seconds % 60))
        }
        VoiceState::Transcribing { .. } => Some("Transcribing…".into()),
    }
}

fn goes_to(copy: &str, title: &str) -> String {
    format!("{copy} · goes to \"{title}\"")
}

fn dictated_over(draft: &str, selected: Range<usize>, text: &str) -> String {
    let kept = format!("{}{}", &draft[..selected.start], &draft[selected.end..]);
    dictated(&kept, selected.start, text)
}

fn transcript_of(samples: &[f32]) -> Result<String, String> {
    if let Some(copy) = no_speech_copy(aether_voice::heard(samples)) {
        return Err(copy.to_string());
    }
    let Some(model) = aether_voice::model_path() else {
        return Err(VoiceError::Model("no config directory".into()).to_string());
    };
    transcript_outcome(aether_voice::transcribe(&model, samples), &model)
}

fn transcript_outcome(result: Result<String, VoiceError>, model: &Path) -> Result<String, String> {
    match result {
        Ok(text) if text.is_empty() => Err(SILENCE.to_string()),
        Ok(text) => Ok(text),
        Err(error) => {
            if matches!(error, VoiceError::Model(_)) {
                std::fs::remove_file(model).ok();
            }
            Err(error.to_string())
        }
    }
}

impl ChatApp {
    pub(crate) fn recording_voice(&self) -> bool {
        matches!(self.voice, VoiceState::Recording { .. })
    }

    pub(in crate::app) fn voice_line(&self) -> Option<SharedString> {
        let seconds = match &self.voice {
            VoiceState::Recording { started, .. } => started.elapsed().as_secs(),
            _ => 0,
        };
        let Some(copy) = voice_copy(&self.voice, seconds) else {
            return self.voice_note.clone();
        };
        let named = match self.voice {
            VoiceState::Opening { chat, .. }
            | VoiceState::Recording { chat, .. }
            | VoiceState::Transcribing { chat }
                if chat != self.chats[self.active].id =>
            {
                self.chats.iter().find(|c| c.id == chat).map(|c| goes_to(&copy, &c.title))
            }
            _ => None,
        };
        Some(named.unwrap_or(copy).into())
    }

    pub(crate) fn voice_banner(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let copy = self.voice_line()?;
        let button = |id: &'static str, label: &'static str| {
            aether_ui::button(id).ghost().xsmall().label(label).debug_selector(|| id.into())
        };
        let cancel = button("voice-cancel", "Cancel")
            .on_click(self.session_listener(cx, |this, _, _, cx| this.cancel_voice(cx)));
        let actions = match &self.voice {
            VoiceState::Idle => h_flex().child(button("voice-dismiss", "Dismiss").on_click(
                self.session_listener(cx, |this, _, _, cx| {
                    this.voice_note = None;
                    cx.notify();
                }),
            )),
            VoiceState::NeedsModel { .. } => h_flex()
                .child(button("voice-download", "Download").on_click(
                    self.session_listener(cx, |this, _, window, cx| {
                        this.download_voice_model(window, cx)
                    }),
                ))
                .child(cancel),
            VoiceState::Opening { .. } | VoiceState::Downloading { .. } => h_flex().child(cancel),
            VoiceState::Recording { .. } => h_flex()
                .child(button("voice-done", "Done").on_click(
                    self.session_listener(cx, |this, _, window, cx| this.stop_voice(window, cx)),
                ))
                .child(cancel),
            VoiceState::Transcribing { .. } => h_flex(),
        };
        let transcribing = matches!(self.voice, VoiceState::Transcribing { .. });
        Some(
            h_flex()
                .px_2()
                .gap_2()
                .justify_between()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(
                    h_flex().flex_1().min_w_0().gap_1p5().child(div().min_w_0().child(copy)).when(
                        transcribing,
                        |row| {
                            row.child(
                                div().flex_none().child(
                                    Spinner::new()
                                        .icon(IconName::LoaderCircle)
                                        .xsmall()
                                        .color(cx.theme().muted_foreground),
                                ),
                            )
                        },
                    ),
                )
                .child(div().flex_none().child(actions)),
        )
    }

    pub(crate) fn set_voice_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.update_settings(cx, |s| s.voice_enabled = enabled);
        if !enabled {
            self.cancel_voice(cx);
        }
    }

    pub(crate) fn voice_model_status(&self) -> String {
        match &self.voice {
            VoiceState::Downloading { .. } => voice_copy(&self.voice, 0).unwrap_or_default(),
            _ => model_status(aether_voice::installed()).into(),
        }
    }

    pub(crate) fn voice_model_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let button = |id: &'static str, label: &'static str| {
            aether_ui::button(id).ghost().small().label(label)
        };
        if matches!(self.voice, VoiceState::Downloading { .. }) {
            button("voice-model-cancel", "Cancel")
                .on_click(cx.listener(|this, _, _, cx| this.cancel_voice(cx)))
                .into_any_element()
        } else if aether_voice::installed() {
            let in_use = matches!(
                self.voice,
                VoiceState::Opening { .. }
                    | VoiceState::Recording { .. }
                    | VoiceState::Transcribing { .. }
            );
            button("voice-model-remove", "Remove")
                .disabled(in_use)
                .on_click(cx.listener(|this, _, _, cx| this.remove_voice_model(cx)))
                .into_any_element()
        } else {
            button("voice-model-download", "Download")
                .on_click(cx.listener(|this, _, window, cx| this.download_voice_model(window, cx)))
                .into_any_element()
        }
    }

    fn remove_voice_model(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = aether_voice::remove_model() {
            self.voice_note = Some(format!("Could not remove the speech model: {e}").into());
        }
        cx.notify();
    }

    pub(crate) fn voice_active(&self) -> bool {
        let id = self.chats[self.active].id;
        matches!(
            self.voice,
            VoiceState::Opening { chat, .. }
                | VoiceState::Recording { chat, .. }
                | VoiceState::Transcribing { chat } if chat == id
        )
    }

    pub(crate) fn toggle_voice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.voice_note = None;
        match self.voice {
            VoiceState::Idle if self.settings.voice_enabled => self.start_voice(window, cx),
            VoiceState::NeedsModel { .. } => self.voice = VoiceState::Idle,
            VoiceState::Recording { .. } => self.stop_voice(window, cx),
            VoiceState::Idle
            | VoiceState::Downloading { .. }
            | VoiceState::Opening { .. }
            | VoiceState::Transcribing { .. } => {}
        }
        cx.notify();
    }

    fn start_voice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chat = self.chats[self.active].id;
        if !aether_voice::installed() {
            self.voice = VoiceState::NeedsModel { chat };
            return;
        }
        let started = Instant::now();
        self.voice = VoiceState::Opening { chat, started };
        let opening = cx.background_spawn(async { Recording::start() });
        self.settle_opening(started, opening, window, cx);
    }

    pub(in crate::app) fn settle_opening(
        &self,
        started: Instant,
        opening: Task<Result<Recording, VoiceError>>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            let result = opening.await;
            let applied = this
                .update_in(cx, |this, window, cx| this.finish_opening(started, result, window, cx));
            if applied.is_err() {
                // no window left: the closure took the mic with it, only the state remains
                this.update(cx, |this, cx| {
                    if matches!(this.voice, VoiceState::Opening { started: s, .. } if s == started)
                    {
                        this.cancel_voice(cx);
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    pub(in crate::app) fn finish_opening(
        &mut self,
        started: Instant,
        result: Result<Recording, VoiceError>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let chat = match self.voice {
            VoiceState::Opening { chat, started: current } if current == started => chat,
            _ => return,
        };
        match result {
            Ok(recording) => {
                let started = Instant::now();
                self.voice = VoiceState::Recording { chat, recording, started };
                self.watch_recording(started, window, cx);
            }
            Err(error) => {
                self.voice_note = Some(error.to_string().into());
                self.voice = VoiceState::Idle;
            }
        }
        cx.notify();
    }

    fn watch_recording(&self, started: Instant, window: &Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let ticked =
                    this.update_in(cx, |this, window, cx| this.tick_recording(started, window, cx));
                if !ticked.unwrap_or(false) {
                    break;
                }
            }
            this.update(cx, |this, cx| {
                // no window left to show or stop it: release the mic
                if matches!(this.voice, VoiceState::Recording { started: s, .. } if s == started) {
                    this.cancel_voice(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(in crate::app) fn tick_recording(
        &mut self,
        started: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let VoiceState::Recording { recording, started: current, .. } = &self.voice else {
            return false;
        };
        if *current != started {
            return false;
        }
        let failure = recording.failure();
        if failure.is_none() && started.elapsed() < MAX_RECORDING {
            cx.notify();
            return true;
        }
        if let Some(failure) = failure {
            self.voice_note = Some(VoiceError::Capture(failure).to_string().into());
        }
        self.stop_voice(window, cx);
        false
    }

    pub(crate) fn stop_voice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (chat, recording) = match std::mem::take(&mut self.voice) {
            VoiceState::Recording { chat, recording, .. } => (chat, recording),
            other => return self.voice = other,
        };
        self.voice = VoiceState::Transcribing { chat };
        cx.notify();
        let work = cx.background_spawn(async move { transcript_of(&recording.finish()) });
        self.settle_transcript(chat, work, window, cx);
    }

    pub(in crate::app) fn settle_transcript(
        &self,
        chat: i64,
        work: Task<Result<String, String>>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            let outcome = work.await;
            this.update(cx, |this, cx| {
                this.voice = VoiceState::Idle;
                if let Err(note) = &outcome {
                    // a device failure noted while recording says more than what followed it
                    this.voice_note.get_or_insert_with(|| note.clone().into());
                }
                cx.notify();
            })
            .ok();
            let Ok(text) = outcome else { return };
            let delivered = this
                .update_in(cx, |this, window, cx| this.deliver_transcript(chat, &text, window, cx));
            if delivered.is_err() {
                this.update(cx, |this, cx| {
                    this.append_to_draft(chat, &text);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    pub(crate) fn download_voice_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chat = match self.voice {
            VoiceState::NeedsModel { chat } => chat,
            VoiceState::Idle => self.chats[self.active].id,
            _ => return,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.voice = VoiceState::Downloading { chat, done: 0, cancel: cancel.clone() };
        cx.notify();
        let (progress, mut updates) = futures::channel::mpsc::unbounded();
        let flag = cancel.clone();
        let download = cx.background_spawn(async move {
            let mut sent = 0;
            aether_voice::download(&flag, |done| {
                // whole percents: each message re-renders the app
                if done - sent >= MODEL_BYTES / 100 {
                    sent = done;
                    progress.unbounded_send(done).ok();
                }
            })
        });
        cx.spawn_in(window, async move |this, cx| {
            while let Some(done) = updates.next().await {
                this.update(cx, |this, cx| {
                    if let VoiceState::Downloading { done: shown, cancel: current, .. } =
                        &mut this.voice
                        && Arc::ptr_eq(current, &cancel)
                    {
                        *shown = done;
                        cx.notify();
                    }
                })
                .ok();
            }
            let result = download.await;
            this.update(cx, |this, cx| this.finish_download(&cancel, result, cx)).ok();
        })
        .detach();
    }

    pub(in crate::app) fn finish_download(
        &mut self,
        cancel: &Arc<AtomicBool>,
        result: Result<std::path::PathBuf, VoiceError>,
        cx: &mut Context<Self>,
    ) {
        if !matches!(
            &self.voice,
            VoiceState::Downloading { cancel: current, .. } if Arc::ptr_eq(current, cancel)
        ) {
            return;
        }
        let note = match result {
            Ok(_) => "Speech model ready".into(),
            Err(error) => error.to_string().into(),
        };
        self.voice_note = Some(note);
        self.voice = VoiceState::Idle;
        cx.notify();
    }

    pub(crate) fn cancel_voice(&mut self, cx: &mut Context<Self>) {
        match &self.voice {
            VoiceState::Transcribing { .. } => return,
            VoiceState::Downloading { cancel, .. } => cancel.store(true, Ordering::Relaxed),
            _ => {}
        }
        self.voice = VoiceState::Idle;
        cx.notify();
    }

    pub(crate) fn end_hidden_voice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.voice {
            VoiceState::Opening { .. } => self.cancel_voice(cx),
            VoiceState::Recording { .. } => self.stop_voice(window, cx),
            _ => {}
        }
    }

    pub(crate) fn cancel_voice_of(&mut self, owner: i64, cx: &mut Context<Self>) {
        if matches!(
            self.voice,
            VoiceState::NeedsModel { chat }
                | VoiceState::Downloading { chat, .. }
                | VoiceState::Opening { chat, .. }
                | VoiceState::Recording { chat, .. } if chat == owner
        ) {
            self.cancel_voice(cx);
        }
    }

    pub(crate) fn deliver_transcript(
        &mut self,
        chat: i64,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if text.is_empty() {
            return;
        }
        // in_session retargets self.active even with no view to swap in, so decide first
        if self.chats[self.active].id == chat {
            self.insert_dictation(text, window, cx);
        } else if self.session_views.contains_key(&chat) {
            self.in_session(chat, cx, |app, cx| app.insert_dictation(text, window, cx));
        } else {
            self.append_to_draft(chat, text);
        }
        cx.notify();
    }

    fn insert_dictation(&self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        // applying the open editor overwrites the composer, so the text must go in the editor
        if let Some(editor) = &self.draft_editor {
            editor.update(cx, |input, cx| {
                let spaced = dictated_over(&input.value(), input.selected_range(), text);
                input.replace(spaced, window, cx);
            });
        } else {
            self.composer.update(cx, |input, cx| {
                let spaced = dictated_over(&input.value(), input.selected_range(), text);
                input.replace(spaced, window, cx);
            });
        }
    }

    fn append_to_draft(&mut self, chat: i64, text: &str) {
        if let Some(chat) = self.chats.iter_mut().find(|c| c.id == chat) {
            let spaced = dictated(&chat.draft, chat.draft.len(), text);
            chat.draft.push_str(&spaced);
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/composer/voice.rs"]
mod tests;
