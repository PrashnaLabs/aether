use super::*;

impl ChatApp {
    pub(super) fn bind_session_widgets(
        &mut self,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let composer = self.composer.clone();
        let scroller = self.scroller.clone();
        let selector = self.selector.clone();
        let access = self.access_menu.clone();
        let project = self.project_menu.clone();
        let headline = self.headline_project_menu.clone();
        let worktree = self.worktree_menu.clone();
        let palette_query = self.palette_query.clone();
        let subscriptions = vec![
            cx.subscribe_in(
                &palette_query,
                window,
                move |app, input, ev: &InputEvent, window, cx| {
                    if let InputEvent::Change = ev {
                        let query = input.read(cx).value();
                        app.in_session(id, cx, |app, cx| {
                            app.palette.update(cx, |s, cx| s.set_query(query, window, cx));
                            cx.notify();
                        });
                    }
                },
            ),
            cx.subscribe_in(&composer, window, move |app, _, ev: &InputEvent, window, cx| {
                app.in_session(id, cx, |app, cx| match ev {
                    InputEvent::Change => {
                        app.chats[app.active].draft = app.composer.read(cx).value().to_string();
                        app.composer_history_index = None;
                        app.composer_draft = None;
                        app.attach_error = None;
                        app.update_suggestions(cx);
                    }
                    InputEvent::PressEnter { shift: false, .. } => app.send(window, cx),
                    InputEvent::Focus | InputEvent::Blur => app.sync_fold(window, cx),
                    _ => {}
                });
            }),
            cx.observe_in(&scroller, window, move |app, _, window, cx| {
                app.in_session(id, cx, |app, cx| app.sync_fold(window, cx));
            }),
            cx.subscribe_in(
                &selector,
                window,
                move |app, _, ev: &AgentSelectorEvent, window, cx| {
                    app.in_session(id, cx, |app, cx| match ev {
                        AgentSelectorEvent::Pick { provider, model } => {
                            app.pick_agent(*provider, model.to_string(), window, cx)
                        }
                        AgentSelectorEvent::ToggleFavorite { provider, model } => {
                            app.toggle_favorite(*provider, model, cx)
                        }
                        AgentSelectorEvent::Effort(level) => app.set_effort(level.clone(), cx),
                        AgentSelectorEvent::OpenSettings => {
                            app.settings_tab = SettingsTab::Providers;
                            app.open_page(Page::Settings, cx);
                        }
                    });
                },
            ),
            cx.observe(&selector, |_, _, cx| cx.notify()),
            cx.subscribe(&access, move |app, _, ev: &ChoiceMenuEvent, cx| {
                app.in_session(id, cx, |app, cx| {
                    let ChoiceMenuEvent::Pick(i) = ev;
                    if let Some(mode) =
                        AccessMode::for_provider(app.chats[app.active].provider).get(*i)
                    {
                        app.set_access(*mode, cx);
                    }
                });
            }),
            cx.subscribe_in(&project, window, move |app, _, ev: &ChoiceMenuEvent, window, cx| {
                app.in_session(id, cx, |app, cx| {
                    let ChoiceMenuEvent::Pick(i) = ev;
                    app.pick_project(*i, window, cx);
                });
            }),
            cx.subscribe_in(&headline, window, move |app, _, ev: &ChoiceMenuEvent, window, cx| {
                app.in_session(id, cx, |app, cx| {
                    let ChoiceMenuEvent::Pick(i) = ev;
                    app.pick_project(*i, window, cx);
                });
            }),
            cx.subscribe_in(
                &worktree,
                window,
                move |app, _, ev: &WorkspaceSelectEvent, window, cx| {
                    app.in_session(id, cx, |app, cx| {
                        if !app.workspace_can_change(app.active) {
                            return;
                        }
                        match ev {
                            WorkspaceSelectEvent::NewBranch => {
                                app.open_create_branch_picker(window, cx)
                            }
                            WorkspaceSelectEvent::Local => app.choose_worktree(None, cx),
                            WorkspaceSelectEvent::SwitchBranch(branch) => {
                                app.confirm_switch_workspace_branch(branch.clone(), window, cx)
                            }
                            WorkspaceSelectEvent::NewWorktree(b) => {
                                app.pending_worktree_dialog_base = Some(b.clone());
                                app.pending_worktree_dialog = true;
                                cx.notify();
                            }
                            WorkspaceSelectEvent::UseWorktree(p) => {
                                app.choose_existing_worktree(p.clone(), cx)
                            }
                        }
                    });
                },
            ),
        ];
        self.session_subscriptions.insert(id, subscriptions);
        self.session_windows.insert(id, window.window_handle().window_id());
    }
}
