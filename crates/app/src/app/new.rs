use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use crate::actions::{CommitChanges, SearchWorkspace, key_hint};
use crate::agents_view::poll_interval;
use crate::attachments;
use crate::font_picker::{FontItem, fill_font_select, font_choices, font_row};
use crate::git_panel::GitPanel;
use crate::open_in;
use crate::settings;
use crate::settings_view::SettingsTab;
use crate::usage_view::{UsageBreakdown, UsageMetric, UsageViewMode};
use crate::workspace_popover::WorkspacePopover;
use aether_agent::{Agent, SideUpdate, Store};
use aether_core::{Appearance, SkillSource, attachments_dir, create_private_dir};
use aether_ui::{
    AgentSelectorEvent, AgentSelectorState, ChatScrollerState, ChoiceMenuEvent, ChoiceMenuState,
    WorkspaceSelectState, composer_placeholder,
};
use futures::StreamExt;
use gpui_kit::component::{
    command::CommandState,
    input::{InputEvent, InputState, TextareaState},
    select::{SearchableVec, SelectEvent, SelectState},
    slider::{SliderEvent, SliderState},
};
use gpui_kit::{Context, Window, prelude::*};

use super::providers::{binary_override, initial_model, key_status};
use super::{CLIS, ChatApp, Page, SidebarView, apply_font_size, apply_look};

impl ChatApp {
    pub fn new(agent: Agent, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = settings::load();
        crate::assets::set_active_icon_pack(settings.icon_pack);
        let key = settings::api_key();
        let store = open_store(&agent);
        Self::with_state(agent, settings, key, store, true, window, cx)
    }

    pub(crate) fn with_state(
        agent: Agent,
        settings: aether_core::Settings,
        key: Option<(String, settings::KeySource)>,
        store: Option<Store>,
        background_services: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        apply_look(&settings, window, cx);
        apply_font_size(settings.font_size, window);

        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 10)
                .submit_on_enter(true)
                .placeholder(composer_placeholder(None))
        });
        let instructions = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx)
                .auto_grow(4, 12)
                .placeholder("e.g. Answer briefly. Use metric units.");
            state.set_value(settings.instructions.clone(), window, cx);
            state
        });
        let key_input = cx.new(|cx| {
            InputState::new(window, cx).masked(true).placeholder(if key.is_some() {
                "Paste a new key to replace the current one"
            } else {
                "sk-or-v1-…"
            })
        });
        let rename_input = cx.new(|cx| InputState::new(window, cx).placeholder("Chat name"));
        let btw_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 5)
                .submit_on_enter(true)
                .placeholder("Ask a follow-up…")
        });
        let board_title = cx.new(|cx| InputState::new(window, cx).placeholder("Title"));
        let board_body =
            cx.new(|cx| TextareaState::new(window, cx).auto_grow(4, 16).placeholder("Description"));
        let worktree_name_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Custom name, empty = auto from message")
        });
        let terminal_font_value = settings.terminal_font_family.clone();
        let terminal_font_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder("System default");
            s.set_value(terminal_font_value, window, cx);
            s
        });
        let commit_tip = key_hint(&CommitChanges, cx)
            .map_or("Commit message".into(), |k| format!("Commit message · {k}"));
        let commit_message =
            cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 8).placeholder(commit_tip));
        let scroller = cx.new(|cx| ChatScrollerState::new(0, cx));
        let palette = cx.new(|cx| CommandState::new(window, cx));
        let palette_query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Type a command or search…"));
        let settings_search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search settings…"));
        let font_select = cx.new(|cx| {
            let saved = font_choices(std::slice::from_ref(&settings.ui_font_family));
            let row = font_row(&saved, &settings.ui_font_family);
            SelectState::new(SearchableVec::new(saved), Some(row), window, cx).searchable(true)
        });
        cx.spawn_in(window, {
            let select = font_select.clone();
            let text_system = cx.text_system().clone();
            async move |this, cx| {
                let choices = cx
                    .background_spawn(async move { font_choices(&text_system.all_font_names()) })
                    .await;
                this.update_in(cx, |this, window, cx| {
                    fill_font_select(&select, choices, &this.settings.ui_font_family, window, cx);
                })
            }
        })
        .detach();
        let opacity_slider = cx.new(|_| {
            SliderState::new()
                .min(60.)
                .max(100.)
                .step(5.)
                .default_value(f32::from(settings.clamped_window_opacity()))
        });
        let hn_search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search stories..."));
        let skill_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search skills"));
        let board_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search cards"));
        let explorer_quick_open_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search files by name..."));
        let grep_tip = key_hint(&SearchWorkspace, cx)
            .map_or("Search text in workspace...".into(), |k| {
                format!("Search text in workspace ({k})...")
            });
        let explorer_grep_input = cx.new(|cx| InputState::new(window, cx).placeholder(grep_tip));
        let usage_project_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter projects…"));
        let usage_window_menu = cx.new(|cx| ChoiceMenuState::new(cx).below());
        let project_open_menu = cx.new(|cx| ChoiceMenuState::new(cx).below());
        let selector = cx.new(|cx| AgentSelectorState::new(window, cx));
        let access_menu = cx.new(ChoiceMenuState::new);
        let project_menu = cx.new(ChoiceMenuState::new);
        let headline_project_menu = cx.new(|cx| ChoiceMenuState::new(cx).below());
        let worktree_menu = cx.new(|cx| WorkspaceSelectState::new(window, cx));
        let default_model = cx.new(|cx| AgentSelectorState::new(window, cx).form());
        let mut binary_inputs = HashMap::new();
        let mut model_inputs = HashMap::new();
        for p in CLIS {
            let binary = aether_driver::DriverStartOptions::for_provider(&settings, p, "").binary;
            let model = initial_model(&settings, p);
            binary_inputs.insert(
                p,
                cx.new(|cx| {
                    let mut s = InputState::new(window, cx).placeholder("Search PATH");
                    s.set_value(binary, window, cx);
                    s
                }),
            );
            model_inputs.insert(
                p,
                cx.new(|cx| {
                    let mut s = InputState::new(window, cx).placeholder("CLI default");
                    s.set_value(model, window, cx);
                    s
                }),
            );
        }

        let browser = cx.new(|cx| crate::browser::Browser::new(window, cx));
        let hn_browser = cx.new(|cx| crate::browser::Browser::new(window, cx));
        let app = cx.weak_entity();
        hn_browser.update(cx, |browser, _| {
            browser.home(gpui_kit::assets::IconName::Flame, "Back to stories", move |_, cx| {
                app.update(cx, |this, cx| this.close_hn_reader(cx)).ok();
            })
        });
        let notes = crate::notes::NotesModel::new(window, cx);
        let mut notes_setting_inputs = std::collections::BTreeMap::new();
        for (key, value) in [
            ("directory", &settings.notes.directory),
            ("nvim", &settings.notes.nvim),
            ("tmux", &settings.notes.tmux),
            ("session", &settings.notes.session),
            ("socket", &settings.notes.socket),
        ] {
            notes_setting_inputs.insert(
                key,
                cx.new(|cx| {
                    let mut input = InputState::new(window, cx);
                    input.set_value(value.clone(), window, cx);
                    input
                }),
            );
        }
        let mut subscriptions = vec![
            cx.subscribe_in(
                &browser,
                window,
                |this, _, event: &crate::browser::CycleBrowserTab, window, cx| {
                    if this.side_open()
                        && this.side_tab_in(window) == crate::side_panel::SideTab::Browser
                    {
                        this.cycle_side_tab(event.0, window, cx);
                    }
                },
            ),
            // a page swap unmounts the focused field; keep keys and menu items on the app
            cx.on_focus_lost(window, |this, window, cx| this.focus.focus(window, cx)),
            cx.subscribe(&default_model, |this, _, ev: &AgentSelectorEvent, cx| match ev {
                AgentSelectorEvent::Pick { model, .. } => {
                    let model = model.to_string();
                    this.update_settings(cx, |s| s.model = model);
                    this.sync_selector(cx);
                }
                AgentSelectorEvent::ToggleFavorite { provider, model } => {
                    this.toggle_favorite(*provider, model, cx)
                }
                AgentSelectorEvent::Effort(_) | AgentSelectorEvent::OpenSettings => {}
            }),
            cx.observe(&default_model, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &explorer_quick_open_input,
                window,
                |this, input, ev: &InputEvent, _, cx| {
                    if let InputEvent::Change = ev {
                        let query = input.read(cx).value().to_string();
                        let exp = this.active_project_explorer(cx);
                        exp.update(cx, |exp, _| exp.set_quick_open_query(query));
                        this.quick_open_scroll.scroll_to_item(0);
                        cx.notify();
                    }
                },
            ),
            cx.subscribe_in(&explorer_grep_input, window, |this, input, ev: &InputEvent, _, cx| {
                match ev {
                    InputEvent::Change => {
                        let query = input.read(cx).value().to_string();
                        this.queue_explorer_grep(query, cx);
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => {
                        this.run_explorer_grep(cx);
                    }
                    _ => {}
                }
            }),
            cx.subscribe_in(&instructions, window, |this, input, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    this.settings.instructions = input.read(cx).value().to_string();
                    settings::save(&this.settings);
                }
            }),
            cx.subscribe_in(&key_input, window, |this, _, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.save_key(window, cx);
                }
            }),
            cx.subscribe_in(
                &rename_input,
                window,
                |this, _, ev: &InputEvent, window, cx| match ev {
                    InputEvent::PressEnter { .. } => {
                        this.commit_rename(cx);
                        this.composer.update(cx, |s, cx| s.focus(window, cx));
                    }
                    InputEvent::Blur => this.commit_rename(cx),
                    _ => {}
                },
            ),
            cx.subscribe_in(&btw_input, window, |this, _, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.submit_btw(window, cx);
                }
            }),
            cx.subscribe_in(&terminal_font_input, window, |this, input, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    let value = input.read(cx).value().trim().to_string();
                    this.update_settings(cx, |s| s.terminal_font_family = value);
                }
            }),
            cx.subscribe_in(
                &font_select,
                window,
                |this, _, ev: &SelectEvent<SearchableVec<FontItem>>, window, cx| {
                    let SelectEvent::Confirm(Some(family)) = ev else { return };
                    let family = family.to_string();
                    this.update_settings(cx, |s| s.ui_font_family = family);
                    apply_look(&this.settings, window, cx);
                },
            ),
            cx.subscribe_in(&opacity_slider, window, |this, _, ev: &SliderEvent, window, cx| {
                if let SliderEvent::Change(value) = ev {
                    this.set_window_opacity(value.start().round() as u8, window, cx);
                }
            }),
            cx.subscribe_in(
                &settings_search_input,
                window,
                |this, input, ev: &InputEvent, _, cx| {
                    if let InputEvent::Change = ev {
                        let query = input.read(cx).value();
                        if !query.trim().is_empty()
                            && crate::settings_search::search_settings(&query)
                                .iter()
                                .any(|item| item.tab == SettingsTab::Worktrees)
                            && this.worktrees.is_none()
                            && !this.listing_worktrees
                        {
                            this.load_worktrees(cx);
                        }
                        cx.notify();
                    }
                },
            ),
            cx.subscribe_in(&skill_search, window, |_, _, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    cx.notify();
                }
            }),
            cx.subscribe_in(&board_search, window, |_, _, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    cx.notify();
                }
            }),
            cx.observe_window_appearance(window, |this, window, cx| {
                if this.settings.appearance == Appearance::System {
                    apply_look(&this.settings, window, cx);
                }
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    if let Some(exp) = this.notes.explorer.clone() {
                        let root = exp.read(cx).root.clone();
                        this.check_explorer_disk(&root, window, cx);
                    }
                    this.refresh_branches(None, cx);
                    this.refresh_git(false, cx);
                }
            }),
            cx.subscribe_in(&board_title, window, |this, _, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } | InputEvent::Blur = ev {
                    this.board_commit_in(window, cx);
                }
            }),
            cx.subscribe_in(&board_body, window, |this, _, ev: &InputEvent, window, cx| {
                if let InputEvent::Blur = ev {
                    this.board_commit_in(window, cx);
                }
            }),
            cx.subscribe(&commit_message, |this, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    this.git_message_changed(cx);
                }
            }),
            cx.subscribe_in(&usage_project_filter, window, |_, _, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    cx.notify();
                }
            }),
            cx.subscribe(&usage_window_menu, |this, _, ev: &ChoiceMenuEvent, cx| {
                let ChoiceMenuEvent::Pick(ix) = ev;
                if let Some(&window) = aether_agent::usage_history::WINDOW_CHOICES.get(*ix) {
                    this.set_usage_window(window, cx);
                }
            }),
            cx.subscribe(&project_open_menu, |this, _, ev: &ChoiceMenuEvent, cx| {
                let ChoiceMenuEvent::Pick(ix) = ev;
                let pick = this.project_page.clone().zip(this.open_apps.get(*ix).map(|a| a.id));
                if let Some((folder, id)) = pick {
                    this.update_settings(cx, |s| open_in::pick_app(s, &folder, id));
                }
            }),
        ];
        subscriptions.push(cx.subscribe_in(
            &notes.search,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    if this.notes.search_ui.open {
                        this.queue_notes_search(cx);
                    } else {
                        this.refresh_notes(cx);
                    }
                }
                InputEvent::PressEnter { .. } => {
                    if this.notes.search_ui.open {
                        this.open_note_result(window, cx);
                        return;
                    }
                    if let Some((path, _)) = this.notes.entries.first().cloned() {
                        if this.notes.listing == crate::notes::Listing::Trash {
                            this.note_job(window, cx, move |store| {
                                store.restore(&path).map(crate::notes::NoteResult::Open)
                            });
                        } else {
                            this.open_note(path, None, window, cx);
                        }
                    }
                }
                _ => {}
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &notes.name,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_note_form(window, cx);
                }
            },
        ));
        for (&key, input) in &notes_setting_inputs {
            subscriptions.push(cx.subscribe(input, move |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let value = input.read(cx).value().trim().to_string();
                    let mut notes = this.settings.notes.clone();
                    match key {
                        "directory" => notes.directory = value,
                        "nvim" => notes.nvim = value,
                        "tmux" => notes.tmux = value,
                        "session" => notes.session = value,
                        "socket" => notes.socket = value,
                        _ => {}
                    }
                    this.update_notes_settings(notes, cx);
                }
            }));
        }
        for (&provider, input) in &binary_inputs {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                move |this, input, ev: &InputEvent, window, cx| match ev {
                    InputEvent::Change => {
                        let value = input.read(cx).value().trim().to_string();
                        this.update_settings(cx, |s| {
                            if let Some(binary) = binary_override(s, provider) {
                                *binary = value;
                            }
                        });
                    }
                    InputEvent::PressEnter { .. } | InputEvent::Blur => {
                        this.probe_providers(&[provider], window, cx)
                    }
                    _ => {}
                },
            ));
        }
        for (&provider, input) in &model_inputs {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                move |this, input, ev: &InputEvent, _, cx| {
                    if let InputEvent::Change = ev {
                        let value = input.read(cx).value().trim().to_string();
                        this.update_settings(cx, |s| {
                            s.provider_models.insert(provider.key().into(), value);
                        });
                        this.sync_selector(cx);
                    }
                },
            ));
        }

        subscriptions.push(cx.on_app_quit(move |this, cx| {
            if background_services {
                this.flush_workspace(cx);
            }
            for ix in 0..this.chats.len() {
                if this.chats[ix].cancel.is_some() {
                    this.persist(ix);
                }
            }
            this.board_commit(cx);
            if let Some(store) = &this.store {
                store.flush();
            }
            this.shutdown_terminals(cx);
            aether_driver::opencode::kill_lead_servers();
            async {}
        }));
        let key_status = key_status(key.as_ref().map(|(_, source)| *source));
        let mut app = Self {
            image_viewers: HashMap::new(),
            chat_id_highwater: 0,
            stopped_by_lead: HashSet::new(),
            spawn_nonces: HashMap::new(),
            spawn_queue: Vec::new(),
            session_views: HashMap::new(),
            session_subscriptions: HashMap::new(),
            session_windows: HashMap::new(),
            ui_owner: None,
            workspace: Default::default(),
            agent,
            cards: store
                .as_ref()
                .and_then(|s| s.load_cards().ok())
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect(),
            board_selected: None,
            board_title,
            board_body,
            store,
            chats: Vec::new(),
            active: 0,
            draft_owner: None,
            composer,
            draft_editor: None,
            scroller,
            rows: Vec::new(),
            folded: false,
            selector,
            access_menu: access_menu.clone(),
            project_menu: project_menu.clone(),
            headline_project_menu: headline_project_menu.clone(),
            switcher_folders: Default::default(),
            worktree_menu: worktree_menu.clone(),
            open_apps: Vec::new(),
            open_error: None,
            default_model,
            binary_inputs,
            model_inputs,
            context_lengths: HashMap::new(),
            claude_plan_usage: None,
            provider_plan_usages: HashMap::new(),
            plan_usage_at: HashMap::new(),
            usage_attempt_at: 0,
            detected_provider: None,
            sys_sample: None,
            usage_pop: super::status_bar::UsagePop::new(cx),
            crew_pop: false,
            crew_pop_trigger: Default::default(),
            show_usage_limits: false,
            model_prices: HashMap::new(),
            settings,
            api_key: key.map(|(key, _)| key),
            key_status,
            page: Page::Chat,
            instructions,
            key_input,
            terminal_font_input,
            renaming: None,
            rename_in_tab: false,
            rename_input,
            worktree_name_input,
            worktree_base_select: None,
            pending_worktree_dialog: false,
            pending_worktree_dialog_base: None,
            models: Vec::new(),
            probes: HashMap::new(),
            probe_runs: HashMap::new(),
            branches: HashMap::new(),
            missing_folders: HashSet::new(),
            missing_attachments: HashSet::new(),
            refreshing_branches: false,
            branches_stale: false,
            bases: HashMap::new(),
            loading_bases: HashSet::new(),
            background_hidden: HashSet::new(),
            inspect: None,
            inspect_queued: None,
            inspect_steer: None,
            background_resizing: false,
            removing: HashSet::new(),
            worktrees: None,
            // tests: no real ~/.aether worktree scan; its tokio wakeup aborts gpui tests
            listing_worktrees: cfg!(test),
            external: Vec::new(),
            scan_run: 0,
            scanning: false,
            agents_status: None,
            palette,
            palette_query,
            palette_open: false,
            watch_open: false,
            watch_peek: None,
            watch_peek_live: None,
            palette_restore: None,
            palette_rows: Vec::new(),
            skills: aether_agent::skills::load_all(),
            skill_tab: SkillSource::Aether,
            settings_tab: SettingsTab::default(),
            settings_provider: aether_core::Provider::Claude,
            notes_advanced_open: false,
            settings_search_input,
            font_select,
            opacity_slider,
            settings_sidebar_focus: cx.focus_handle().tab_stop(true),
            skill_search,
            skill_editor: None,
            suggestions: None,
            suggestions_dismissed: None,
            request_inputs: HashMap::new(),
            request_subscriptions: HashMap::new(),
            request_picks: HashMap::new(),
            request_errors: HashMap::new(),
            editing_plans: HashSet::new(),
            editing: None,
            composer_history_index: None,
            composer_draft: None,
            attach_error: None,
            attaching: HashMap::new(),
            quotes: HashMap::new(),
            btw: HashMap::new(),
            btw_open: false,
            tab_bar_controls: false,
            btw_input,
            file_selection_bar: None,
            selection_bar: None,
            copied: None,
            sessions_made: 0,
            focus: cx.focus_handle(),
            sidebar_resizing: false,
            explorer_tree_size: 240.,
            explorer_tree_collapsed: false,
            sidebar_view: SidebarView::default(),
            terminals: HashMap::new(),
            global_terminal: None,
            active_terminal_tab: aether_terminal::TerminalTab::Global,
            git: GitPanel::new(commit_message, cx),
            browser,
            browser_open: false,
            diff_open: false,
            explorer_view: crate::explorer::ExplorerView::Files,
            side_tab: crate::side_panel::SideTab::Agent,
            right_surface: None,
            right_review_open: false,
            right_browser_open: false,
            board_tab: false,
            board_all: false,
            board_search,
            board_by: None,
            board_picks: HashSet::new(),
            archive_picks: Default::default(),
            project_picks: Default::default(),
            right_panel_resizing: false,
            side_tabs: HashMap::new(),
            side_back: crate::side_panel::SideTab::Agent,
            side_full: true,
            terminal_tab: true,
            diff_targets: HashMap::new(),
            diff_focus: cx.focus_handle(),
            project_explorers: HashMap::new(),
            explorer_prev_page: Page::Chat,
            project_page: None,
            project_view: Default::default(),
            project_open_menu,
            explorer_quick_open_input,
            quick_open_restore: None,
            quick_open_scroll: gpui_kit::ScrollHandle::new(),
            explorer_grep_input,
            explorer_markdown_source: HashSet::new(),
            explorer_editors: HashMap::new(),
            explorer_diff_key: None,
            explorer_diff: None,
            usage_view: UsageViewMode::default(),
            usage_window: aether_agent::usage_history::UsageWindow::TrailingDays(30),
            usage_metric: UsageMetric::default(),
            usage_breakdown: UsageBreakdown::default(),
            usage_history: None,
            usage_history_scanned_at: None,
            usage_history_pending_for: None,
            usage_history_generation: 0,
            usage_chart_bounds: Rc::new(Cell::new(None)),
            usage_chart_hover: None,
            usage_project_filter,
            usage_window_menu,
            usage_expanded_models: None,
            workspace_pop: WorkspacePopover::new(window, cx),
            remotes: HashMap::new(),
            gh: None,
            script_runs: Default::default(),
            voice: Default::default(),
            voice_note: None,
            notes,
            notes_setting_inputs,
            hackernews: crate::hackernews::HackerNewsModel::new(),
            hn_browser,
            hn_reader_open: false,
            hn_search_input,
            _subscriptions: subscriptions,
        };
        app.workspace.persistence_enabled = background_services;
        app.workspace.windows.insert(0, window.window_handle());
        super::links::install_file_links(cx);
        if !background_services {
            app.new_chat(window, cx);
            return app;
        }
        app.load_models(window, cx);
        if app.restore_chats() {
            app.prune_watchlist(cx);
            let homes = app
                .chats
                .iter()
                .filter(|c| !c.project.is_empty() && !c.cwd.is_empty() && c.cwd != c.project)
                .map(|c| (c.cwd.clone(), c.project.clone()))
                .collect();
            app.board_tidy(&homes);
        }
        let restored_workspace = app.load_session_workspace(window, cx);
        app.refresh_missing_attachments(true, cx);
        if let (Some(store), Some(root)) = (&app.store, attachments_dir())
            && let Ok(ids) = store.ids()
        {
            let live = ids.into_iter().collect::<HashSet<_>>();
            let loaded = app.chats.iter().map(|c| c.id).collect::<HashSet<_>>();
            let kept = app
                .chats
                .iter()
                .flat_map(|c| &c.messages)
                .flat_map(|m| &m.attachments)
                .map(|a| a.path.clone())
                .chain(app.chats.iter().flat_map(|c| &c.pending).map(|a| a.path.clone()))
                .chain(
                    app.chats
                        .iter()
                        .filter_map(|c| c.edit_stash.as_ref())
                        .flat_map(|(_, files)| files)
                        .map(|a| a.path.clone()),
                )
                .collect::<HashSet<_>>();
            cx.background_executor()
                .spawn(async move {
                    // CLIs are pointed at this folder (`--add-dir`) before anything is attached
                    let _ = create_private_dir(&root);
                    attachments::sweep(&root, &live, &loaded, &kept)
                })
                .detach();
        }
        if !restored_workspace || app.chats.is_empty() {
            app.new_chat(window, cx);
        }
        app.probe_providers(&CLIS, window, cx);
        app.scan_agents(cx);
        app.load_open_apps(cx);
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(6)).await;
                if this
                    .update_in(cx, |this, window, cx| {
                        if window.is_window_active() && this.composer.read(cx).value().is_empty() {
                            this.sync_placeholder(window, cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(30)).await;
                let tick = this.update_in(cx, |this, window, cx| {
                    if window.is_window_active() {
                        this.refresh_branches(None, cx);
                        this.refresh_project_page(cx);
                        this.refresh_stale_usage(cx);
                    }
                });
                if tick.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            let mut fails = 0u32;
            loop {
                let wait = if fails >= crate::sysinfo::BACKOFF_AFTER {
                    crate::sysinfo::BACKOFF_SECS
                } else {
                    crate::sysinfo::INTERVAL_SECS
                };
                cx.background_executor().timer(Duration::from_secs(wait)).await;
                let Ok(active) = this.update_in(cx, |_, window, _| window.is_window_active())
                else {
                    break;
                };
                if !active {
                    continue;
                }
                let sample =
                    cx.background_executor().spawn(async { crate::sysinfo::sample() }).await;
                fails = if sample.is_some() { 0 } else { fails + 1 };
                let set = this.update(cx, |this, cx| {
                    let next = crate::sysinfo::next_sample(this.sys_sample, sample, fails);
                    let shown = |s: Option<crate::sysinfo::Sample>| {
                        s.as_ref().map(super::status_bar::resource_text)
                    };
                    if shown(this.sys_sample) != shown(next) {
                        cx.notify();
                    }
                    this.sys_sample = next;
                });
                if set.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let tick = this.update(cx, |this, cx| {
                    this.tick_live(cx);
                    #[cfg(target_os = "macos")]
                    {
                        let now = super::now_ms();
                        crate::notify::dock_badge(
                            this.chats
                                .iter()
                                .filter(|c| {
                                    !c.archived && c.status(now) == aether_ui::ChatStatus::Waiting
                                })
                                .count(),
                        );
                    }
                });
                if tick.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                let Ok(visible) = this.update(cx, |this, _| this.page == Page::Agents) else {
                    break;
                };
                cx.background_executor().timer(poll_interval(visible)).await;
                if this.update(cx, |this, cx| this.scan_agents(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        if app.settings.notifications {
            crate::notify::authorize();
        }
        let view = cx.entity().downgrade();
        let handle = window.window_handle();
        cx.on_system_notification_response(move |response, cx| {
            let _ = handle.update(cx, |_, window, cx| {
                window.activate_window();
                view.update(cx, |this, cx| {
                    let tag = response.tag.as_ref();
                    if let Some(ix) = this.chats.iter().position(|c| c.id.to_string() == tag) {
                        this.select(ix, window, cx);
                    }
                })
            });
        });
        if let Some(mut side) = app.agent.side_updates() {
            cx.spawn_in(window, async move |this, cx| {
                while let Some(update) = side.next().await {
                    let applied = this.update_in(cx, |this, window, cx| {
                        let key = match &update {
                            SideUpdate::Meta(key, _) | SideUpdate::Turn(key, _) => key,
                        };
                        let Some(ix) = this.chats.iter().position(|c| c.id.to_string() == *key)
                        else {
                            return;
                        };
                        match update {
                            SideUpdate::Meta(_, update) => {
                                this.apply_cli_meta(ix, *update, window, cx)
                            }
                            SideUpdate::Turn(_, reply) => {
                                this.take_unprompted(ix, reply, window, cx)
                            }
                        }
                    });
                    if applied.is_err() {
                        break;
                    }
                }
            })
            .detach();
        }
        if let Some((mut calls, pending)) = app.agent.orchestrator_calls() {
            cx.spawn_in(window, async move |this, cx| {
                while let Some(call) = calls.next().await {
                    let done = this
                        .update_in(cx, |this, window, cx| this.orchestrate(call, window, cx))
                        .is_err();
                    aether_agent::orchestrator::done_pending(&pending);
                    if done {
                        break;
                    }
                }
            })
            .detach();
        }
        app.refresh_all_provider_usages(cx);
        app.detect_subscription(cx);
        app
    }
}

/// Chats live next to settings; without a usable file they just aren't kept.
fn open_store(agent: &Agent) -> Option<Store> {
    let dir = aether_core::config_dir()?;
    let opened = create_private_dir(&dir)
        .map_err(|e| e.to_string())
        .and_then(|()| agent.open_store(&dir.join("aether.db")).map_err(|e| e.to_string()));
    opened.inspect_err(|e| eprintln!("aether: chats won't be saved: {e}")).ok()
}
