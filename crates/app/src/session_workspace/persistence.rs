use super::*;
use crate::app::ChatApp;
use gpui_kit::{App, Context, Window};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::ops::Range;
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct SessionSnapshot {
    pub text: String,
    pub selection: Range<usize>,
    pub editor: Option<String>,
    pub editor_selection: Range<usize>,
    pub editing: Option<(i64, usize)>,
    pub history: Option<usize>,
    pub history_draft: Option<String>,
    pub pending: Vec<aether_core::Attachment>,
    pub edit_stash: Option<(String, Vec<aether_core::Attachment>)>,
    pub scroll: (usize, f32, bool),
}
#[derive(Serialize, Deserialize)]
pub(crate) struct SavedWorkspace {
    pub version: u32,
    pub model: WorkspaceModel,
}
impl Default for SavedWorkspace {
    fn default() -> Self {
        Self { version: 1, model: WorkspaceModel::default() }
    }
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SavedDrafts {
    pub version: u32,
    pub drafts: BTreeMap<i64, SessionSnapshot>,
}

pub(crate) fn decode_layout(
    json: &str,
    known: &HashSet<i64>,
    size: [f32; 2],
) -> Result<WorkspaceModel, String> {
    let mut saved: SavedWorkspace = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if saved.version != 1 {
        return Err("Unsupported session layout version".into());
    }
    saved.model.repair(known, size);
    Ok(saved.model)
}
fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    let mut file = std::fs::File::create(&temp)?;
    file.write_all(data)?;
    file.sync_all()?;
    std::fs::rename(temp, path)
}
#[derive(Default)]
pub(crate) struct SaveState {
    inputs: Option<(WorkspaceModel, BTreeMap<i64, SessionSnapshot>)>,
    pub generation: Arc<AtomicU64>,
    pub lock: Arc<Mutex<()>>,
}
impl SaveState {
    fn payload(
        inputs: &mut Option<(WorkspaceModel, BTreeMap<i64, SessionSnapshot>)>,
        model: &WorkspaceModel,
        snapshots: BTreeMap<i64, SessionSnapshot>,
    ) -> Option<(String, String)> {
        if inputs.as_ref().is_some_and(|(saved, drafts)| saved == model && *drafts == snapshots) {
            return None;
        }
        let saved = SavedWorkspace { version: 1, model: model.clone() };
        let drafts = SavedDrafts { version: 1, drafts: snapshots };
        let payload = (serde_json::to_string(&saved).ok()?, serde_json::to_string(&drafts).ok()?);
        *inputs = Some((saved.model, drafts.drafts));
        Some(payload)
    }
}
impl ChatApp {
    pub(crate) fn load_session_workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(dir) = aether_core::config_dir() else { return false };
        if let Ok(json) = std::fs::read_to_string(dir.join("session-drafts.json"))
            && let Ok(saved) = serde_json::from_str::<SavedDrafts>(&json)
            && saved.version == 1
        {
            for (id, snapshot) in saved.drafts {
                if let Some(chat) = self.chats.iter_mut().find(|c| c.id == id) {
                    chat.draft = snapshot.text.clone();
                    chat.pending = snapshot.pending.clone();
                    chat.edit_stash = snapshot.edit_stash.clone();
                    self.workspace.restored_drafts.insert(id, snapshot);
                }
            }
        }
        let known = self.chats.iter().filter(|c| !c.archived).map(|c| c.id).collect();
        let Ok(json) = std::fs::read_to_string(dir.join("session-layout.json")) else {
            return false;
        };
        let size =
            [f32::from(window.viewport_size().width), f32::from(window.viewport_size().height)];
        match decode_layout(&json, &known, size) {
            Ok(model) => {
                self.workspace.model = model;
                let focused = self.workspace.model.windows[&0]
                    .focused
                    .and_then(|p| self.workspace.model.panes.get(&p).copied());
                if let Some(id) = focused
                    && let Some(ix) = self.chats.iter().position(|c| c.id == id)
                {
                    self.select(ix, window, cx);
                } else if !self.chats.is_empty() {
                    self.activate_session_ui(self.chats[0].id, window, cx);
                }
                self.restore_session_windows(cx);
                true
            }
            Err(error) => {
                self.workspace.error = Some(format!("Couldn't restore session layout: {error}"));
                false
            }
        }
    }
    fn workspace_payload(&self, cx: &App) -> Option<(String, String)> {
        let layout = serde_json::to_string(&SavedWorkspace {
            version: 1,
            model: self.workspace.model.clone(),
        })
        .ok()?;
        let drafts =
            serde_json::to_string(&SavedDrafts { version: 1, drafts: self.session_snapshots(cx) })
                .ok()?;
        Some((layout, drafts))
    }
    pub(crate) fn schedule_workspace_save(&mut self, cx: &mut Context<Self>) {
        if !self.workspace.persistence_enabled {
            return;
        }
        let mut inputs = self.workspace.save.inputs.take();
        let payload =
            SaveState::payload(&mut inputs, &self.workspace.model, self.session_snapshots(cx));
        self.workspace.save.inputs = inputs;
        let Some((layout, drafts)) = payload else { return };
        let Some(dir) = aether_core::config_dir() else { return };
        let generation = self.workspace.save.generation.clone();
        let n = generation.fetch_add(1, Ordering::SeqCst) + 1;
        let lock = self.workspace.save.lock.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(300)).await;
            let result = cx
                .background_executor()
                .spawn(async move {
                    let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
                    if generation.load(Ordering::SeqCst) != n {
                        return Ok(());
                    }
                    atomic_write(&dir.join("session-drafts.json"), drafts.as_bytes())?;
                    atomic_write(&dir.join("session-layout.json"), layout.as_bytes())
                })
                .await;
            this.update(cx, |app, cx| {
                if app.workspace.save.generation.load(Ordering::SeqCst) != n {
                    return;
                }
                match result {
                    Err(error) => {
                        app.workspace.save_error =
                            Some(format!("Couldn't save session layout: {error}"));
                        cx.notify();
                    }
                    Ok(()) => {
                        if app.workspace.save_error.take().is_some() {
                            cx.notify();
                        }
                    }
                }
            })
            .ok();
        })
        .detach();
    }
    pub(crate) fn flush_workspace(&mut self, cx: &App) {
        let Some((layout, drafts)) = self.workspace_payload(cx) else { return };
        let Some(dir) = aether_core::config_dir() else { return };
        self.workspace.save.generation.fetch_add(1, Ordering::SeqCst);
        let _guard = self.workspace.save.lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = atomic_write(&dir.join("session-drafts.json"), drafts.as_bytes())
            .and_then(|_| atomic_write(&dir.join("session-layout.json"), layout.as_bytes()))
        {
            eprintln!("aether: couldn't save session workspace: {e}");
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/session_workspace/persistence.rs"]
mod tests;
