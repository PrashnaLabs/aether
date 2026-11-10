use std::collections::HashMap;

use crate::agents_view::externally_locked;
use crate::settings::{self, KeySource};
use aether_agent::ModelInfo;
use aether_agent::usage::{
    PlanUsage, fetch_antigravity_plan_usage, fetch_claude_plan_usage, fetch_codex_plan_usage,
    fetch_opencode_plan_usage, fetch_openrouter_plan_usage, read_claude_credentials,
    read_codex_token, read_openrouter_key,
};
use aether_core::{Appearance, CornerStyle, Provider, Role, Settings, Skill, effort_levels};
use aether_driver::{CliModel, catalog::provider_label};
use aether_ui::{Busy, CLI_DEFAULT, ModelItem, ProviderModels, busy_placeholder, favorite_key};
use gpui_kit::{Context, SharedString, Window};

use super::{Chat, ChatApp, apply_font_size, apply_look, chat_cwd, now_ms};

// ponytail: fixed 10 min usage poll, agy spawns a process per fetch; make it a setting if users want fresher
pub(super) const USAGE_STALE_MS: i64 = 10 * 60_000;
// auto-spawn wants numbers under a minute old, but fetches are costly: one burst per 10 s
const SPAWN_USAGE_TTL_MS: i64 = 60_000;
const SPAWN_USAGE_RETRY_MS: i64 = 10_000;

// gated on last attempt too: a failing fetch would otherwise respawn agy every tick
pub(super) fn usage_due(newest: Option<i64>, attempted: i64, configured: bool, now: i64) -> bool {
    let old = |at: i64| now - at >= USAGE_STALE_MS;
    old(attempted) && newest.map_or(configured, old)
}

impl ChatApp {
    pub(super) fn refresh_provider_usage(&mut self, provider: Provider, cx: &mut Context<Self>) {
        let bg = cx.background_executor().clone();
        match provider {
            Provider::Claude => {
                if let Some((tok, sub)) = read_claude_credentials() {
                    cx.spawn(async move |this, cx| {
                        let usage = bg
                            .spawn(async move { fetch_claude_plan_usage(&tok, sub.as_deref()) })
                            .await;
                        if let Some(usage) = usage {
                            let _ = this.update(cx, |this, cx| {
                                this.claude_plan_usage = Some(usage.clone());
                                this.store_plan_usage(Provider::Claude, usage);
                                cx.notify();
                            });
                        }
                    })
                    .detach();
                }
            }
            Provider::Codex => {
                if let Some(tok) = read_codex_token() {
                    cx.spawn(async move |this, cx| {
                        let usage = bg.spawn(async move { fetch_codex_plan_usage(&tok) }).await;
                        if let Some(usage) = usage {
                            let _ = this.update(cx, |this, cx| {
                                this.store_plan_usage(Provider::Codex, usage);
                                cx.notify();
                            });
                        }
                    })
                    .detach();
                }
            }
            Provider::Antigravity => {
                cx.spawn(async move |this, cx| {
                    let usage = bg.spawn(async move { fetch_antigravity_plan_usage() }).await;
                    if let Some(usage) = usage {
                        let _ = this.update(cx, |this, cx| {
                            this.store_plan_usage(Provider::Antigravity, usage);
                            cx.notify();
                        });
                    }
                })
                .detach();
            }
            Provider::OpenRouter => {
                if let Some(key) = read_openrouter_key() {
                    cx.spawn(async move |this, cx| {
                        let usage =
                            bg.spawn(async move { fetch_openrouter_plan_usage(&key) }).await;
                        if let Some(usage) = usage {
                            let _ = this.update(cx, |this, cx| {
                                this.store_plan_usage(Provider::OpenRouter, usage);
                                cx.notify();
                            });
                        }
                    })
                    .detach();
                }
            }
            Provider::OpenCode => {
                cx.spawn(async move |this, cx| {
                    let usage = bg.spawn(async move { fetch_opencode_plan_usage() }).await;
                    if let Some(usage) = usage {
                        let _ = this.update(cx, |this, cx| {
                            this.store_plan_usage(Provider::OpenCode, usage);
                            cx.notify();
                        });
                    }
                })
                .detach();
            }
            _ => {}
        }
    }

    fn store_plan_usage(&mut self, provider: Provider, usage: PlanUsage) {
        self.plan_usage_at.insert(provider, chrono::Utc::now().timestamp_millis());
        self.provider_plan_usages.insert(provider, usage);
    }

    pub(super) fn refresh_stale_usage(&mut self, cx: &mut Context<Self>) {
        let newest = self.plan_usage_at.values().max().copied();
        let configured = self.api_key.is_some()
            || [Provider::Claude, Provider::Codex, Provider::Antigravity, Provider::OpenCode]
                .iter()
                .any(|p| self.probes.get(p).is_some_and(|probe| probe.installed));
        if usage_due(newest, self.usage_attempt_at, configured, now_ms()) {
            self.refresh_all_provider_usages(cx);
        }
    }

    /// Fire and forget: the spawn in progress ranks the snapshot it has, the next one gets fresh numbers.
    pub(super) fn refresh_usage_for_spawn(&mut self, cx: &mut Context<Self>) {
        let now = now_ms();
        let stale = [Provider::Claude, Provider::Codex, Provider::Antigravity, Provider::OpenCode]
            .iter()
            .filter(|p| self.probes.get(p).is_some_and(|probe| probe.installed))
            .any(|p| self.plan_usage_at.get(p).is_none_or(|&at| now - at >= SPAWN_USAGE_TTL_MS));
        if stale && now - self.usage_attempt_at >= SPAWN_USAGE_RETRY_MS {
            self.refresh_all_provider_usages(cx);
        }
    }

    pub(super) fn refresh_all_provider_usages(&mut self, cx: &mut Context<Self>) {
        self.usage_attempt_at = now_ms();
        for p in [
            Provider::Claude,
            Provider::Codex,
            Provider::Antigravity,
            Provider::OpenRouter,
            Provider::OpenCode,
        ] {
            self.refresh_provider_usage(p, cx);
        }
    }

    pub(super) fn load_models(&self, window: &mut Window, cx: &mut Context<Self>) {
        let models = self.agent.list_models(self.api_key.as_deref().unwrap_or_default());
        cx.spawn_in(window, async move |this, cx| {
            let models = models.await;
            if models.is_empty() {
                return;
            }
            this.update(cx, |this, cx| {
                this.context_lengths =
                    models.iter().filter_map(|m| Some((m.id.clone(), m.context_length?))).collect();
                this.model_prices = models
                    .iter()
                    .filter_map(|m| Some((m.id.clone(), (m.prompt_price?, m.completion_price?))))
                    .collect();
                this.models = models.iter().map(openrouter_item).collect();
                this.sync_selector(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Rescans for agents outside aether; skipped while a scan is running.
    pub(crate) fn scan_agents(&mut self, cx: &mut Context<Self>) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        self.scan_run += 1;
        let run = self.scan_run;
        let own = self
            .chats
            .iter()
            .filter(|c| c.external_pid.is_none())
            .map(|c| c.cli_session.clone())
            .filter(|s| !s.is_empty())
            .collect();
        let scan = self.agent.scan_external(own);
        cx.spawn(async move |this, cx| {
            let found = scan.await;
            this.update(cx, |this, cx| {
                if this.scan_run == run {
                    // failed scan: keep the last list so locks hold
                    if let Some(found) = found {
                        for c in &mut this.chats {
                            if c.external_pid.is_some()
                                && externally_locked(c.external_pid, &c.cli_session, &found)
                                    .is_none()
                            {
                                c.external_pid = None;
                            }
                        }
                        this.external = found;
                    }
                    this.scanning = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Rechecks each CLI in `providers` in the background; a newer run
    /// supersedes an older one still in flight.
    pub(crate) fn probe_providers(
        &mut self,
        providers: &[Provider],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for &provider in providers {
            let run = self.probe_runs.entry(provider).or_default();
            *run += 1;
            let run = *run;
            self.probes.remove(&provider);
            let probe = self.agent.probe_provider(provider, &self.settings);
            cx.spawn_in(window, async move |this, cx| {
                let probe = probe.await;
                this.update_in(cx, |this, _, cx| {
                    if this.probe_runs.get(&provider) != Some(&run) {
                        return;
                    }
                    this.probes.insert(provider, probe);
                    this.sync_selector(cx);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    /// Rows the picker offers for `provider`, before the chat's own model.
    pub(crate) fn model_items(&self, provider: Provider) -> Vec<ModelItem> {
        match provider {
            Provider::OpenRouter => self.models.clone(),
            p => cli_items(
                p,
                self.probes.get(&p).map_or(&[], |probe| &probe.models),
                &initial_model(&self.settings, p),
            ),
        }
    }

    pub(crate) fn sync_placeholder(&self, window: &mut Window, cx: &mut Context<Self>) {
        let chat = &self.chats[self.active];
        let busy = match chat.busy() {
            false => Busy::Idle,
            true if chat.messages.last().is_some_and(|m| m.pending_request().is_some()) => {
                Busy::Waiting
            }
            true => Busy::Working,
        };
        let tips = [
            "Tip: @tag a file to include it as context.",
            "Tip: Type / to browse available commands.",
            "Tip: Press Ctrl+G to edit a longer draft.",
            "Tip: Use the attachment button to add images and files.",
            "Tip: Change your chat layout from the workspace menu.",
            "Tip: Customize visible items in Settings → Bottom Bar.",
        ];
        let phase = (super::now_ms() / 6_000) as usize;
        let text = if busy == Busy::Waiting || phase % 2 == 0 {
            busy_placeholder(chat.provider.short_label(), busy)
        } else {
            tips[(phase / 2) % tips.len()].to_string()
        };
        if self.composer.read(cx).presentation().placeholder().as_ref() != text.as_str() {
            self.composer.update(cx, |s, cx| s.set_placeholder(text, window, cx));
        }
    }

    /// Pushes every provider's rows, favorites and the active chat's choice to the selector.
    pub(crate) fn sync_selector(&self, cx: &mut Context<Self>) {
        let chat = &self.chats[self.active];
        let current = chat.provider;
        let mut model = SharedString::default();
        let providers = Provider::ALL
            .map(|provider| {
                let mut items = self.model_items(provider);
                if provider == current {
                    let value = picker_value(provider, &chat.model(&self.settings), &items);
                    if !value.is_empty() && !items.iter().any(|item| item.id == value) {
                        items.insert(0, ModelItem::custom(value.clone()));
                    }
                    model = value.into();
                }
                let installed = self.probes.get(&provider).map(|probe| probe.installed);
                ProviderModels {
                    provider,
                    items,
                    hidden: rail_hidden(provider, current, installed),
                }
            })
            .to_vec();
        let favorites = self.settings.favorite_models.clone();
        let efforts = self.effort_options(chat);
        let effort = chat.effort.clone();
        self.selector.update(cx, |selector, cx| {
            selector.sync(providers, favorites.clone(), current, model, efforts, effort, cx)
        });
        let default = self.settings.model.clone();
        let mut items = self.models.clone();
        if !default.is_empty() && !items.iter().any(|item| item.id == default) {
            items.insert(0, ModelItem::custom(default.clone()));
        }
        let openrouter =
            vec![ProviderModels { provider: Provider::OpenRouter, items, hidden: false }];
        self.default_model.update(cx, |selector, cx| {
            selector.sync(
                openrouter,
                favorites,
                Provider::OpenRouter,
                default.into(),
                Vec::new(),
                String::new(),
                cx,
            )
        });
    }

    pub(super) fn toggle_favorite(
        &mut self,
        provider: Provider,
        model: &str,
        cx: &mut Context<Self>,
    ) {
        let key = favorite_key(provider, model);
        self.update_settings(cx, |s| {
            if let Some(ix) = s.favorite_models.iter().position(|f| *f == key) {
                s.favorite_models.remove(ix);
            } else {
                s.favorite_models.push(key);
            }
        });
        self.sync_selector(cx);
    }

    /// Selector pick: switches provider first when the chat still allows it.
    pub(super) fn pick_agent(
        &mut self,
        provider: Provider,
        model: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.switching_folder(&chat_cwd(&self.chats[self.active], &self.settings.workspace_root))
        {
            return;
        }
        let ix = self.active;
        if !self.chats[ix].messages.is_empty() && self.chats[ix].provider != provider {
            self.switch_agent(ix, provider, stored_model(provider, &model), window, cx);
        } else {
            self.pick_provider(provider, cx);
        }
        if self.chats[ix].provider == provider {
            self.pick_model(model, window, cx);
        }
        self.sync_placeholder(window, cx);
    }

    /// Moves a chat with history to another agent in place: the next message starts a fresh
    /// session there, seeded with the conversation, after a "Context handoff" divider.
    pub(crate) fn switch_agent(
        &mut self,
        ix: usize,
        provider: Provider,
        model: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chat = &self.chats[ix];
        if chat.provider == provider
            || chat.busy()
            || chat.worktree_job.is_some()
            || self.locked(ix)
        {
            return;
        }
        if super::CLIS.contains(&provider) && chat.cwd.is_empty() {
            if self.settings.workspace_root.is_empty() {
                use gpui_kit::component::WindowExt;
                let note = gpui_kit::component::notification::Notification::error(
                    "Pick a project folder or set a workspace root first.",
                );
                return window.push_notification(note, cx);
            }
            self.chats[ix].cwd = self.settings.workspace_root.clone();
        }
        let label = |p: Provider, m: &str| {
            let models = self.probes.get(&p).map_or(&[][..], |probe| &probe.models);
            super::sidebar::model_label(models, m)
        };
        let chat = &self.chats[ix];
        let (from, from_model) = match chat.messages.last().and_then(|m| m.handoff.clone()) {
            Some(h) => (h.from, h.from_model),
            None => (chat.provider, label(chat.provider, &chat.provider_model)),
        };
        let handoff = aether_core::Handoff {
            from,
            from_model,
            to: provider,
            to_model: label(provider, &model),
        };
        let id = chat.id;
        self.agent.close_session(&id.to_string());
        self.drop_btw(id);
        let session = (provider == Provider::OpenRouter).then(|| self.next_session_id());
        let chat = &mut self.chats[ix];
        if let Some(last) = chat.messages.last_mut() {
            last.handoff = Some(handoff);
        }
        chat.provider = provider;
        chat.provider_model = model;
        chat.access = super::chat::fit_access(provider, chat.access);
        chat.effort.clear();
        chat.cli_session.clear();
        chat.external_pid = None;
        if let Some(session) = session {
            chat.session = session.clone();
            let hist = super::chat::history(&chat.messages);
            let seed = aether_agent::budget_history(&hist, aether_agent::HISTORY_BUDGET).0;
            if let Err(e) = self.agent.fork_session(&session, &seed) {
                eprintln!("aether: couldn't seed the OpenRouter session: {e}");
            }
        }
        let lead = chat.lead;
        self.persist(ix);
        self.update_settings(cx, |s| s.provider = provider);
        self.sync_selector(cx);
        self.reset_rows(cx);
        if lead && provider == Provider::Antigravity {
            self.agy_lead_check(id, false, window, cx);
        }
        cx.notify();
    }

    /// Sets the active chat's model and makes it its provider's default.
    pub(crate) fn pick_model(
        &mut self,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ix = self.active;
        if self.switching_folder(&chat_cwd(&self.chats[ix], &self.settings.workspace_root)) {
            return;
        }
        if self.chats[ix].cancel.is_some() {
            return self.sync_selector(cx);
        }
        let provider = self.chats[ix].provider;
        let model = stored_model(provider, &value);
        self.chats[ix].provider_model = model.clone();
        if !self.effort_options(&self.chats[ix]).contains(&self.chats[ix].effort) {
            self.chats[ix].effort.clear();
        }
        if !self.chats[ix].messages.is_empty() {
            self.persist(ix);
        }
        if let Some(input) = self.model_inputs.get(&provider) {
            let value = model.clone();
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        self.update_settings(cx, |s| set_default_model(s, provider, model));
        self.sync_selector(cx);
    }

    /// Switches the active chat's provider; only before its first message.
    pub(in crate::app) fn pick_provider(&mut self, provider: Provider, cx: &mut Context<Self>) {
        if self.switching_folder(&chat_cwd(&self.chats[self.active], &self.settings.workspace_root))
        {
            return;
        }
        let chat = &mut self.chats[self.active];
        if !chat.messages.is_empty() || chat.provider == provider {
            return;
        }
        chat.provider = provider;
        chat.provider_model = initial_model(&self.settings, provider);
        chat.access = self.settings.default_access(provider);
        chat.effort.clear();
        self.update_settings(cx, |s| s.provider = provider);
        self.sync_selector(cx);
        self.load_bases(cx);
        self.refresh_git(false, cx);
    }

    pub(crate) fn update_settings(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Settings),
    ) {
        let detail = self.settings.chat_detail;
        f(&mut self.settings);
        settings::save(&self.settings);
        if self.settings.chat_detail != detail {
            self.reset_rows(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_appearance(
        &mut self,
        appearance: Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_settings(cx, |s| s.appearance = appearance);
        apply_look(&self.settings, window, cx);
    }

    pub(crate) fn set_corner_style(
        &mut self,
        style: CornerStyle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_settings(cx, |s| s.corner_style = style);
        apply_look(&self.settings, window, cx);
    }

    pub(crate) fn set_window_opacity(
        &mut self,
        opacity: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings.window_opacity == opacity {
            return;
        }
        self.update_settings(cx, |s| s.window_opacity = opacity);
        apply_look(&self.settings, window, cx);
    }

    pub(crate) fn set_window_blur(
        &mut self,
        blur: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_settings(cx, |s| s.window_blur = blur);
        apply_look(&self.settings, window, cx);
    }

    pub(crate) fn set_font_size(&mut self, size: f32, window: &mut Window, cx: &mut Context<Self>) {
        apply_font_size(size, window);
        self.update_settings(cx, |s| s.font_size = size);
    }

    pub(crate) fn save_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.key_input.read(cx).value().trim().to_string();
        if key.is_empty() {
            return;
        }
        self.key_status = match settings::store_api_key(&key) {
            Ok(()) => key_status(Some(KeySource::Keychain)),
            Err(e) => format!(
                "Couldn't save to the system keychain ({e}). Using it for this session only."
            )
            .into(),
        };
        self.api_key = Some(key);
        self.key_input.update(cx, |input, cx| {
            input.clean(window, cx);
            input.set_placeholder("Paste a new key to replace the current one", window, cx);
        });
        self.load_models(window, cx);
        cx.notify();
    }

    pub(crate) fn model_prices_snapshot(&self) -> HashMap<String, (f64, f64)> {
        self.model_prices.clone()
    }

    /// Window the latest reply's provider reported for this model, else the catalog's, else a guess.
    pub(in crate::app) fn context_limit(&self, chat: &Chat) -> u64 {
        let model = chat.model(&self.settings);
        let sent_with = (!model.is_empty()).then(|| model.clone());
        chat.messages
            .iter()
            .rev()
            .find(|m| m.role == Role::Assistant && m.context_window.is_some())
            .filter(|m| m.model == sent_with)
            .and_then(|m| m.context_window)
            .or_else(|| match chat.provider {
                Provider::OpenRouter => self.context_lengths.get(&model).copied(),
                p => {
                    catalog_context(self.probes.get(&p).map_or(&[], |probe| &probe.models), &model)
                }
            })
            .unwrap_or_else(|| default_context_limit(chat.provider, &model))
    }

    /// Effort levels the chat's model takes: the catalog's per-model list, else the CLI's.
    pub(in crate::app) fn effort_options(&self, chat: &Chat) -> Vec<String> {
        let model = chat.model(&self.settings);
        self.probes
            .get(&chat.provider)
            .and_then(|p| {
                p.models.iter().find(|m| if model.is_empty() { m.default } else { m.id == model })
            })
            .and_then(|m| m.efforts.clone())
            .unwrap_or_else(|| effort_levels(chat.provider).iter().map(|e| e.to_string()).collect())
    }

    pub(crate) fn enabled_skills(&self) -> Vec<Skill> {
        self.skills.iter().filter(|skill| self.settings.skill_enabled(skill)).cloned().collect()
    }

    /// Skills available for the agent or chat.
    /// Returns all installed skills across sources that have not been explicitly disabled.
    pub(crate) fn agent_skills(&self, _provider: Provider) -> Vec<Skill> {
        let skills = if self.skills.is_empty() {
            aether_agent::skills::load_all()
        } else {
            self.skills.clone()
        };
        skills
            .into_iter()
            .filter(|skill| self.settings.skills.get(&skill.id()) != Some(&false))
            .collect()
    }
}

pub(super) fn key_status(source: Option<KeySource>) -> SharedString {
    match source {
        Some(KeySource::Keychain) => "Saved in your system keychain.",
        Some(KeySource::Env) => "Using OPENROUTER_API_KEY from the environment. Save one here to keep it in your keychain.",
        None => "No key yet. Create one at openrouter.ai/keys.",
    }
    .into()
}

/// Model a fresh chat on `provider` starts with: the last one picked there.
pub(super) fn initial_model(settings: &Settings, provider: Provider) -> String {
    match provider {
        Provider::OpenRouter => settings.model.clone(),
        Provider::OpenCode => settings
            .provider_models
            .get(provider.key())
            .filter(|m| !m.starts_with("openrouter/"))
            .cloned()
            .unwrap_or_default(),
        p => settings.provider_models.get(p.key()).cloned().unwrap_or_default(),
    }
}

pub(super) fn binary_override(settings: &mut Settings, provider: Provider) -> Option<&mut String> {
    match provider {
        Provider::Claude => Some(&mut settings.claude_binary),
        Provider::Codex => Some(&mut settings.codex_binary),
        Provider::Pi => Some(&mut settings.pi_binary),
        Provider::OpenCode => Some(&mut settings.opencode_binary),
        Provider::Antigravity => Some(&mut settings.antigravity_binary),
        Provider::OpenRouter => None,
    }
}

pub(super) fn set_default_model(settings: &mut Settings, provider: Provider, model: String) {
    match provider {
        Provider::OpenRouter => settings.model = model,
        p => {
            settings.provider_models.insert(p.key().into(), model);
        }
    }
}

/// Picker row for a stored model; a CLI's empty model shows as its flagged default, else `Default`.
pub(crate) fn picker_value(provider: Provider, model: &str, items: &[ModelItem]) -> String {
    if provider == Provider::OpenRouter || !model.is_empty() {
        return model.into();
    }
    items
        .iter()
        .find(|item| item.default)
        .map_or_else(|| CLI_DEFAULT.into(), |item| item.id.to_string())
}

/// A CLI's catalog rows, plus its last-picked model and a `Default` row when none is flagged.
fn cli_items(provider: Provider, models: &[CliModel], custom: &str) -> Vec<ModelItem> {
    let mut items: Vec<ModelItem> = models
        .iter()
        .map(|m| ModelItem {
            id: m.id.clone().into(),
            name: m.name.clone().into(),
            detail: detail(&m.provider, provider.label()).into(),
            default: m.default,
            description: m.description.clone().into(),
        })
        .collect();
    if !custom.is_empty() && !items.iter().any(|item| item.id == custom) {
        items.insert(0, ModelItem::custom(custom.to_string()));
    }
    if !items.iter().any(|item| item.default) {
        let detail = format!("{} config", provider.label()).into();
        let row = ModelItem {
            id: CLI_DEFAULT.into(),
            name: CLI_DEFAULT.into(),
            detail,
            default: false,
            description: "".into(),
        };
        items.insert(0, row);
    }
    items
}

/// `"{upstream} · {source}"`, or just one when they're the same name.
fn detail(upstream: &str, source: &str) -> String {
    if upstream.eq_ignore_ascii_case(source) {
        upstream.to_string()
    } else {
        format!("{upstream} · {source}")
    }
}

fn openrouter_item(m: &ModelInfo) -> ModelItem {
    let provider = m.id.split_once('/').map_or("Other", |(provider, _)| provider);
    ModelItem {
        id: m.id.clone().into(),
        name: m.name.clone().into(),
        detail: detail(&provider_label(provider), "OpenRouter").into(),
        default: false,
        description: "".into(),
    }
}

fn stored_model(provider: Provider, value: &str) -> String {
    if provider != Provider::OpenRouter && value == CLI_DEFAULT {
        String::new()
    } else {
        value.into()
    }
}

/// Whether the selector drops `provider`'s tab: a missing CLI, unless the chat is on it.
/// `installed` is `None` while its probe runs.
fn rail_hidden(provider: Provider, current: Provider, installed: Option<bool>) -> bool {
    provider != current && installed == Some(false)
}

/// Catalog window of `model`; an empty model is the CLI's flagged default.
fn catalog_context(models: &[CliModel], model: &str) -> Option<u64> {
    models.iter().find(|m| if model.is_empty() { m.default } else { m.id == model })?.context
}

/// Last resort before a provider reports a window or its catalog loads.
fn default_context_limit(provider: Provider, model: &str) -> u64 {
    let lower = model.to_lowercase();
    match provider {
        Provider::Claude if lower.contains("[1m]") => 1_000_000,
        Provider::Claude if ["claude-3", "-4-", "-4."].iter().any(|old| lower.contains(old)) => {
            200_000
        }
        Provider::Claude => 1_000_000,
        Provider::Codex => 258_400,
        Provider::Antigravity if lower.starts_with("gpt-oss") => 131_072,
        Provider::Antigravity => 1_048_576,
        Provider::OpenCode | Provider::Pi => 200_000,
        Provider::OpenRouter if lower.contains("gemini") => 1_000_000,
        Provider::OpenRouter if lower.contains("gpt-4o") => 128_000,
        Provider::OpenRouter => 200_000,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/providers.rs"]
mod tests;
